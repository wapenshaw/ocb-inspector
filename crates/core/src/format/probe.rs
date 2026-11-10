//! Bounded, read-only container probes for experimental profile analysis
//! (docs/EXPERIMENTAL_PROFILE_INSPECTION.md, stage B).
//!
//! The strict parser (`OcbFile::parse`) is not used or changed here. Each probe is a named,
//! versioned rule that states its assumptions; a probe that fits a file yields a *candidate*
//! container, never validated support. Nothing in this module writes or repairs bytes.

use crate::contracts::{Check, CheckStatus};
use crate::error::{Error, Result};
use std::time::{Duration, Instant};

use super::{MAGIC, OCI_MAGIC};

/// The known MS-7D91 layout: 12-byte identifier at 5, checksum at 0x11, count at 0x15,
/// records at 0x19, a 256-byte block, then `$OCI$` and a 24-byte tail.
pub const RULE_KNOWN: &str = "known_layout";
/// `$MOS$` + a printable identifier of 8..=32 bytes, then (hypothesis) the same checksum,
/// count and record header shapes directly after it.
pub const RULE_VARIABLE: &str = "variable_header";
pub const RULE_VERSION: u32 = 1;
/// Identifier length of the known layout.
pub const KNOWN_ID_LEN: usize = 12;
const VARIABLE_ID_LENS: std::ops::RangeInclusive<usize> = 8..=32;
/// Gap between record end and `$OCI$` in the known layout. Firmware code shows it is the
/// 256-byte CMOS image the save routine reads through ports 0x70-0x73.
pub const KNOWN_GAP: usize = 256;
/// The CMOS image after the records (see `KNOWN_GAP`); boards without the `$OCI$` trailer
/// module end the file right after it.
pub const CMOS_BLOCK_LEN: usize = KNOWN_GAP;
/// Bytes after the four `$OCI$` strings in the known layout.
pub const KNOWN_TAIL: usize = 24;
const OCI_FIELD_MAX: usize = 128;
const MAX_OCI_SIGNATURES: usize = 64;

/// Explicit caps for one analysis (spec §6). Exceeding one is a typed `Error::Limit`.
#[derive(Debug, Clone, Copy)]
pub struct ProbeLimits {
    pub max_candidates: usize,
    pub max_records: usize,
    pub max_header_visits: usize,
    pub max_edges: usize,
    pub max_fields: usize,
    pub max_observations: usize,
    pub max_preview_bytes: usize,
    pub max_json_bytes: usize,
    pub deadline: Duration,
}

impl Default for ProbeLimits {
    fn default() -> Self {
        ProbeLimits {
            max_candidates: 32,
            max_records: 100_000,
            max_header_visits: 1_000_000,
            max_edges: 1_000_000,
            max_fields: 250_000,
            max_observations: 10_000,
            max_preview_bytes: 1 << 20,
            max_json_bytes: 32 << 20,
            deadline: Duration::from_secs(30),
        }
    }
}

/// Work counters and the deadline, checked inside every loop of an analysis.
#[derive(Debug)]
pub struct Budget {
    pub limits: ProbeLimits,
    deadline: Instant,
    header_visits: usize,
    edges: usize,
    fields: usize,
    observations: usize,
    preview_bytes: usize,
}

impl Budget {
    pub fn new(limits: ProbeLimits) -> Self {
        Budget {
            limits,
            deadline: Instant::now() + limits.deadline,
            header_visits: 0,
            edges: 0,
            fields: 0,
            observations: 0,
            preview_bytes: 0,
        }
    }

    fn count(used: &mut usize, n: usize, cap: usize, what: &str) -> Result<()> {
        *used = used.saturating_add(n);
        if *used > cap {
            return Err(Error::Limit(format!(
                "experimental analysis: more than {cap} {what}"
            )));
        }
        Ok(())
    }

    /// Cooperative deadline check.
    pub fn tick(&self) -> Result<()> {
        if Instant::now() > self.deadline {
            return Err(Error::Limit(format!(
                "experimental analysis took longer than {} s",
                self.limits.deadline.as_secs()
            )));
        }
        Ok(())
    }

    pub fn header_visit(&mut self) -> Result<()> {
        Self::count(
            &mut self.header_visits,
            1,
            self.limits.max_header_visits,
            "record-header visits",
        )
    }

    pub fn edges(&mut self, n: usize) -> Result<()> {
        Self::count(&mut self.edges, n, self.limits.max_edges, "mapping edges")?;
        self.tick()
    }

    pub fn field(&mut self) -> Result<()> {
        Self::count(
            &mut self.fields,
            1,
            self.limits.max_fields,
            "field evaluations",
        )
    }

    pub fn observation(&mut self) -> Result<()> {
        Self::count(
            &mut self.observations,
            1,
            self.limits.max_observations,
            "diagnostic entries",
        )
    }

    /// How many preview bytes may still be embedded (previews are truncated, never the analysis).
    pub fn take_preview(&mut self, want: usize) -> usize {
        let left = self
            .limits
            .max_preview_bytes
            .saturating_sub(self.preview_bytes);
        let n = want.min(left);
        self.preview_bytes += n;
        n
    }
}

/// One record of a complete chain. Spans are `[start, end)` file offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainRecord {
    pub index: usize,
    pub id: u32,
    pub header_at: usize,
    pub payload_at: usize,
    pub size: usize,
}

impl ChainRecord {
    pub fn payload_end(&self) -> usize {
        self.payload_at + self.size
    }
}

/// A `$OCI$` signature after the record chain with four bounded NUL-terminated strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trailer {
    pub at: usize,
    /// Bytes between record end and the signature.
    pub gap: usize,
    /// Raw strings in order (board, revision, CPU brand, save time in the known layout).
    pub strings: Vec<String>,
    /// `[start, end)` of each string.
    pub spans: Vec<(usize, usize)>,
    /// First byte after the fourth string's terminator.
    pub end: usize,
    pub tail_len: usize,
    pub timestamp_valid: bool,
}

#[derive(Debug, Clone)]
pub struct ContainerCandidate {
    /// Stable id: `known_layout` or `variable_header/<id length>`.
    pub id: String,
    pub rule: &'static str,
    pub rule_version: u32,
    pub assumptions: Vec<String>,
    /// `[start, end)` of the embedded identifier and its printable text.
    pub id_span: (usize, usize),
    pub header_id: String,
    pub checksum_at: usize,
    pub count_at: usize,
    pub records_at: usize,
    pub count: Option<u32>,
    /// Complete chain only; empty when the chain is incomplete.
    pub records: Vec<ChainRecord>,
    pub chain_end: Option<usize>,
    /// Why the chain is incomplete.
    pub chain_error: Option<String>,
    /// Coherent `$OCI$` candidates in the suffix after the chain.
    pub trailers: Vec<Trailer>,
    pub checks: Vec<Check>,
}

impl ContainerCandidate {
    pub fn complete(&self) -> bool {
        self.chain_end.is_some()
    }

    /// The single coherent trailer, when exactly one exists.
    pub fn trailer(&self) -> Option<&Trailer> {
        match self.trailers.as_slice() {
            [one] => Some(one),
            _ => None,
        }
    }

    /// The file ends exactly one CMOS block after the records: the layout written when the
    /// firmware has no `$OCI$` trailer module.
    pub fn ends_after_cmos_block(&self, file_len: usize) -> bool {
        self.trailers.is_empty()
            && self.chain_end.and_then(|e| e.checked_add(CMOS_BLOCK_LEN)) == Some(file_len)
    }

    /// The independent checks of the known layout all passed.
    pub fn all_checks_pass(&self) -> bool {
        self.checks.iter().all(|c| c.status == CheckStatus::Pass)
    }
}

/// What the probes found in a file.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub size: usize,
    pub magic: bool,
    pub candidates: Vec<ContainerCandidate>,
    /// Every `$OCI$` signature in the file (bounded), whether or not it is a trailer.
    pub oci_signatures: Vec<usize>,
}

fn check(name: &str, ok: bool, detail: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        status: if ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: detail.into(),
    }
}

fn not_evaluated(name: &str, detail: &str) -> Check {
    Check {
        name: name.into(),
        status: CheckStatus::NotEvaluated,
        detail: detail.into(),
    }
}

/// `-(sum of every byte outside [at, at+4))` in 32-bit wrapping arithmetic (as the firmware's
/// save and load routines compute it), if the file is long enough. Established for the known
/// layout; elsewhere only a trial.
pub fn additive_checksum(bytes: &[u8], at: usize) -> Option<i32> {
    let end = at.checked_add(4)?;
    if end > bytes.len() {
        return None;
    }
    let sum = bytes[..at]
        .iter()
        .chain(&bytes[end..])
        .fold(0u32, |s, &b| s.wrapping_add(u32::from(b)));
    Some(sum.wrapping_neg() as i32)
}

/// The i32 stored at `at`, if present.
pub fn stored_i32(bytes: &[u8], at: usize) -> Option<i32> {
    let s = bytes.get(at..at.checked_add(4)?)?;
    Some(i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Run every registered probe. Deterministic: candidates in rule order, then identifier length.
pub fn probe(bytes: &[u8], budget: &mut Budget) -> Result<ProbeResult> {
    let oci_signatures = signatures(bytes, OCI_MAGIC, MAX_OCI_SIGNATURES);
    let magic = bytes.starts_with(MAGIC);
    let mut candidates = Vec::new();
    if magic {
        candidates.push(known(bytes, budget)?);
        for len in VARIABLE_ID_LENS {
            budget.tick()?;
            if len == KNOWN_ID_LEN {
                continue; // identical to the known layout's header shape
            }
            if let Some(c) = variable(bytes, len, budget)? {
                if candidates.len() >= budget.limits.max_candidates {
                    return Err(Error::Limit(format!(
                        "more than {} container candidates",
                        budget.limits.max_candidates
                    )));
                }
                candidates.push(c);
            }
        }
    }
    Ok(ProbeResult {
        size: bytes.len(),
        magic,
        candidates,
        oci_signatures,
    })
}

fn signatures(bytes: &[u8], sig: &[u8], max: usize) -> Vec<usize> {
    bytes
        .windows(sig.len())
        .enumerate()
        .filter(|(_, w)| *w == sig)
        .map(|(i, _)| i)
        .take(max)
        .collect()
}

fn printable(b: &[u8]) -> bool {
    !b.is_empty() && b.iter().all(|c| c.is_ascii_graphic())
}

/// Shared header + chain walk for an identifier of `len` bytes at offset 5.
fn header_and_chain(
    bytes: &[u8],
    len: usize,
    id: String,
    rule: &'static str,
    assumptions: Vec<String>,
    budget: &mut Budget,
) -> Result<ContainerCandidate> {
    let id_span = (MAGIC.len(), MAGIC.len() + len);
    let checksum_at = id_span.1;
    let count_at = checksum_at + 4;
    let records_at = count_at + 4;
    let mut c = ContainerCandidate {
        id: if rule == RULE_KNOWN {
            RULE_KNOWN.into()
        } else {
            format!("{rule}/{len}")
        },
        rule,
        rule_version: RULE_VERSION,
        assumptions,
        id_span,
        header_id: id,
        checksum_at,
        count_at,
        records_at,
        count: None,
        records: Vec::new(),
        chain_end: None,
        chain_error: None,
        trailers: Vec::new(),
        checks: Vec::new(),
    };
    match walk_chain(bytes, count_at, records_at, budget)? {
        Ok((count, records, end)) => {
            c.count = Some(count);
            c.records = records;
            c.chain_end = Some(end);
            c.trailers = trailers(bytes, end, budget)?;
        }
        Err((count, why)) => {
            c.count = count;
            c.chain_error = Some(why);
        }
    }
    Ok(c)
}

type Chain = std::result::Result<(u32, Vec<ChainRecord>, usize), (Option<u32>, String)>;

/// Walk `count` records of `u32 id, u16 size, payload` from `records_at`. Bounds and arithmetic
/// are checked before any allocation; a truncated chain is not a chain.
fn walk_chain(
    bytes: &[u8],
    count_at: usize,
    records_at: usize,
    budget: &mut Budget,
) -> Result<Chain> {
    let Some(count) = stored_i32(bytes, count_at).map(|v| v as u32) else {
        return Ok(Err((None, "file ends before the record count".into())));
    };
    let max = budget.limits.max_records.min(100_000);
    if count == 0 || count as usize > max {
        return Ok(Err((
            Some(count),
            format!("record count {count} is outside 1..={max}"),
        )));
    }
    let n = count as usize;
    let headers = n.checked_mul(6);
    let room = bytes.len().checked_sub(records_at);
    if headers.zip(room).is_none_or(|(h, r)| h > r) {
        return Ok(Err((
            Some(count),
            format!("{count} record headers do not fit in the file"),
        )));
    }
    let mut records = Vec::with_capacity(n);
    let mut pos = records_at;
    for index in 0..n {
        budget.header_visit()?;
        if index % 4096 == 0 {
            budget.tick()?;
        }
        let id = stored_i32(bytes, pos).map(|v| v as u32);
        let size = bytes
            .get(pos + 4..pos + 6)
            .map(|s| usize::from(u16::from_le_bytes([s[0], s[1]])));
        let (Some(id), Some(size)) = (id, size) else {
            return Ok(Err((
                Some(count),
                format!("record {index} header is truncated at 0x{pos:x}"),
            )));
        };
        let payload_at = pos + 6;
        let Some(end) = payload_at.checked_add(size).filter(|&e| e <= bytes.len()) else {
            return Ok(Err((
                Some(count),
                format!("record {index} (id {id}) runs past the end of the file"),
            )));
        };
        records.push(ChainRecord {
            index,
            id,
            header_at: pos,
            payload_at,
            size,
        });
        pos = end;
    }
    Ok(Ok((count, records, pos)))
}

/// Coherent `$OCI$` candidates in `[from, len)`: signature, one byte, then four NUL-terminated
/// strings of at most 128 bytes each. Signatures inside record payloads are never considered.
fn trailers(bytes: &[u8], from: usize, budget: &mut Budget) -> Result<Vec<Trailer>> {
    let mut out = Vec::new();
    let Some(suffix) = bytes.get(from..) else {
        return Ok(out);
    };
    for rel in signatures(suffix, OCI_MAGIC, MAX_OCI_SIGNATURES) {
        budget.observation()?;
        let at = from + rel;
        let mut p = at + OCI_MAGIC.len() + 1;
        let mut strings = Vec::new();
        let mut spans = Vec::new();
        for _ in 0..4 {
            let Some(rest) = bytes.get(p..) else { break };
            let Some(end) = rest.iter().take(OCI_FIELD_MAX).position(|&c| c == 0) else {
                break;
            };
            strings.push(String::from_utf8_lossy(&rest[..end]).into_owned());
            spans.push((p, p + end));
            p += end + 1;
        }
        if strings.len() != 4 {
            continue;
        }
        let ts = &strings[3];
        out.push(Trailer {
            at,
            gap: at - from,
            timestamp_valid: ts.len() == 14 && ts.bytes().all(|c| c.is_ascii_digit()),
            strings,
            spans,
            end: p,
            tail_len: bytes.len() - p,
        });
    }
    Ok(out)
}

fn known(bytes: &[u8], budget: &mut Budget) -> Result<ContainerCandidate> {
    let id_bytes = bytes.get(5..5 + KNOWN_ID_LEN).unwrap_or_default();
    let id = String::from_utf8_lossy(id_bytes).into_owned();
    let mut c = header_and_chain(
        bytes,
        KNOWN_ID_LEN,
        id,
        RULE_KNOWN,
        vec![
            "Layout established on the profiles examined so far; elsewhere a candidate only".into(),
            "12-byte identifier at 0x05, checksum at 0x11, record count at 0x15, records at 0x19"
                .into(),
            "256 bytes between record end and $OCI$, 24 bytes after its four strings".into(),
        ],
        budget,
    )?;
    let mut checks = vec![
        check("magic", true, "$MOS$ at 0x00"),
        check(
            "identifier_printable",
            printable(id_bytes) && id_bytes.len() == KNOWN_ID_LEN,
            format!("bytes 0x05..0x11: {:?}", c.header_id),
        ),
    ];
    // Set when the trailer at the known place is coherent: `$OCI$` bytes before it then lie
    // inside the CMOS image, which is data, not a second trailer.
    let mut trailer_settled = false;
    match (&c.chain_error, c.chain_end) {
        (None, Some(end)) => {
            checks.push(check(
                "record_chain",
                true,
                format!("{} records end at 0x{end:x}", c.records.len()),
            ));
            let increasing = c.records.windows(2).all(|w| w[0].id < w[1].id);
            checks.push(check(
                "record_ids_increasing",
                increasing,
                if increasing {
                    "record ids strictly increase".to_string()
                } else {
                    "record ids repeat or decrease".to_string()
                },
            ));
            let at_gap = c.trailers.iter().find(|t| t.gap == KNOWN_GAP);
            checks.push(check(
                "oci_after_256_bytes",
                at_gap.is_some(),
                format!("$OCI$ expected at 0x{:x}", end + KNOWN_GAP),
            ));
            match at_gap {
                Some(t) => {
                    checks.push(check(
                        "oci_timestamp",
                        t.timestamp_valid,
                        format!("save time {:?}", t.strings[3]),
                    ));
                    checks.push(check(
                        "oci_tail_24_bytes",
                        t.tail_len == KNOWN_TAIL,
                        format!("{} bytes after the four strings", t.tail_len),
                    ));
                    trailer_settled = t.timestamp_valid && t.tail_len == KNOWN_TAIL;
                }
                None => {
                    checks.push(not_evaluated(
                        "oci_timestamp",
                        "no $OCI$ at the known place",
                    ));
                    checks.push(not_evaluated(
                        "oci_tail_24_bytes",
                        "no $OCI$ at the known place",
                    ));
                }
            }
        }
        (why, _) => {
            checks.push(check(
                "record_chain",
                false,
                why.clone().unwrap_or_default(),
            ));
            for name in [
                "record_ids_increasing",
                "oci_after_256_bytes",
                "oci_timestamp",
                "oci_tail_24_bytes",
            ] {
                checks.push(not_evaluated(name, "record chain incomplete"));
            }
        }
    }
    if trailer_settled {
        let before = c.trailers.len();
        c.trailers.retain(|t| t.gap >= KNOWN_GAP);
        let inside = before - c.trailers.len();
        if inside > 0 {
            checks.push(check(
                "oci_bytes_inside_cmos_block",
                true,
                format!(
                    "{inside} $OCI$ signature(s) inside the 256-byte CMOS block are its data, not a trailer"
                ),
            ));
        }
    }
    c.checks = checks;
    Ok(c)
}

/// Variable-length identifier hypothesis. Returns a candidate only when the identifier is
/// printable and the record chain after it is complete; anything else is noise, not structure.
fn variable(bytes: &[u8], len: usize, budget: &mut Budget) -> Result<Option<ContainerCandidate>> {
    let Some(id_bytes) = bytes.get(5..5 + len) else {
        return Ok(None);
    };
    if !printable(id_bytes) {
        return Ok(None);
    }
    let c = header_and_chain(
        bytes,
        len,
        String::from_utf8_lossy(id_bytes).into_owned(),
        RULE_VARIABLE,
        vec![
            format!("Hypothesis: a {len}-byte identifier at 0x05"),
            "Hypothesis: checksum, record count and records follow it in the known order".into(),
            "Hypothesis: records are u32 id, u16 size, payload".into(),
        ],
        budget,
    )?;
    if !c.complete() {
        return Ok(None);
    }
    let mut c = c;
    c.checks = vec![
        check("magic", true, "$MOS$ at 0x00"),
        check("identifier_printable", true, format!("{:?}", c.header_id)),
        check(
            "record_chain",
            true,
            format!(
                "{} records end at 0x{:x}",
                c.records.len(),
                c.chain_end.unwrap_or_default()
            ),
        ),
    ];
    Ok(Some(c))
}

#[cfg(test)]
pub(crate) mod build {
    //! Synthetic profile builder for tests. Offsets are computed here, independently of the probe.

    pub struct Rec {
        pub id: u32,
        pub payload: Vec<u8>,
    }

    pub struct Spec {
        pub id: Vec<u8>,
        pub records: Vec<Rec>,
        pub gap: usize,
        pub oci: Option<[&'static str; 4]>,
        pub tail: usize,
        /// Fix the additive checksum at `5 + id.len()`.
        pub checksum: bool,
    }

    impl Spec {
        pub fn known(board: &'static str, rev: &'static str, records: Vec<Rec>) -> Spec {
            let id = format!("{board}.{rev}");
            Spec {
                id: id.into_bytes(),
                records,
                gap: 256,
                oci: Some([board, rev, "Test CPU", "20261001120000"]),
                tail: 24,
                checksum: true,
            }
        }
    }

    pub fn build(s: &Spec) -> Vec<u8> {
        let mut b = b"$MOS$".to_vec();
        b.extend_from_slice(&s.id);
        let checksum_at = b.len();
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&(s.records.len() as u32).to_le_bytes());
        for r in &s.records {
            b.extend_from_slice(&r.id.to_le_bytes());
            b.extend_from_slice(&(r.payload.len() as u16).to_le_bytes());
            b.extend_from_slice(&r.payload);
        }
        b.extend(std::iter::repeat_n(0x11u8, s.gap));
        if let Some(strings) = s.oci {
            b.extend_from_slice(b"$OCI$\0");
            for st in strings {
                b.extend_from_slice(st.as_bytes());
                b.push(0);
            }
        }
        b.extend(std::iter::repeat_n(0x22u8, s.tail));
        if s.checksum {
            let sum: i64 = b[..checksum_at]
                .iter()
                .chain(&b[checksum_at + 4..])
                .map(|&x| i64::from(x))
                .sum();
            let v = i32::try_from(-sum).unwrap_or(0);
            b[checksum_at..checksum_at + 4].copy_from_slice(&v.to_le_bytes());
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::build::{Rec, Spec, build};
    use super::*;

    fn run(b: &[u8]) -> ProbeResult {
        probe(b, &mut Budget::new(ProbeLimits::default())).unwrap()
    }

    fn recs() -> Vec<Rec> {
        vec![
            Rec {
                id: 0,
                payload: vec![1; 40],
            },
            Rec {
                id: 1,
                payload: vec![2; 9],
            },
        ]
    }

    #[test]
    fn the_known_layout_passes_every_check_with_its_offsets() {
        let b = build(&Spec::known("E7D91IMS", "HJ0", recs()));
        let r = run(&b);
        let k = &r.candidates[0];
        assert_eq!(k.id, "known_layout");
        assert!(k.all_checks_pass(), "{:?}", k.checks);
        // 5 + 12 + 4 + 4 = 0x19; then 6+40 and 6+9 bytes of records.
        assert_eq!(k.records_at, 0x19);
        assert_eq!(k.records[1].payload_at, 0x19 + 46 + 6);
        assert_eq!(k.chain_end, Some(0x19 + 46 + 15));
        let t = k.trailer().unwrap();
        assert_eq!(t.gap, 256);
        assert_eq!(t.strings[0], "E7D91IMS");
        assert_eq!(additive_checksum(&b, 0x11), stored_i32(&b, 0x11));
        // The variable probe never duplicates the known layout.
        assert!(r.candidates.iter().all(|c| c.id != "variable_header/12"));
    }

    #[test]
    fn a_longer_identifier_is_a_variable_header_candidate_with_explicit_offsets() {
        let b = build(&Spec::known("E7E34IMSI", "1AF0", recs()));
        let r = run(&b);
        assert!(!r.candidates[0].complete() || !r.candidates[0].all_checks_pass());
        let v: Vec<_> = r
            .candidates
            .iter()
            .filter(|c| c.complete() && c.rule == RULE_VARIABLE)
            .collect();
        assert_eq!(
            v.len(),
            1,
            "{:?}",
            r.candidates.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
        let v = v[0];
        assert_eq!(v.id, "variable_header/14");
        assert_eq!(v.header_id, "E7E34IMSI.1AF0");
        assert_eq!((v.checksum_at, v.count_at, v.records_at), (19, 23, 27));
        assert_eq!(v.trailer().unwrap().strings[1], "1AF0");
    }

    #[test]
    fn oci_bytes_inside_the_cmos_block_are_not_a_second_trailer() {
        // A real MS-7C77 profile has "$OCI$" at CMOS offset 250, six bytes before the trailer.
        let mut b = build(&Spec::known("E7C77IMS", "1C0", recs()));
        let end = run(&b).candidates[0].chain_end.unwrap();
        b[end + 250..end + 256].copy_from_slice(b"$OCI$ ");
        let stored = additive_checksum(&b, 0x11).unwrap();
        b[0x11..0x15].copy_from_slice(&stored.to_le_bytes());
        let r = run(&b);
        let k = &r.candidates[0];
        assert!(k.all_checks_pass(), "{:?}", k.checks);
        let t = k.trailer().expect("one trailer");
        assert_eq!((t.gap, t.strings[0].as_str()), (256, "E7C77IMS"));
        assert!(
            k.checks
                .iter()
                .any(|c| c.name == "oci_bytes_inside_cmos_block")
        );

        // Without a coherent trailer at the known place nothing is dropped: still ambiguous.
        let mut s = Spec::known("E7C77IMS", "1C0", recs());
        s.tail = 7;
        let mut b = build(&s);
        b[end + 250..end + 256].copy_from_slice(b"$OCI$ ");
        let k = &run(&b).candidates[0];
        assert!(k.trailer().is_none(), "{:?}", k.trailers);
    }

    #[test]
    fn a_different_gap_and_tail_are_reported_not_normalised() {
        let mut s = Spec::known("E7D91IMS", "HJ0", recs());
        s.gap = 100;
        s.tail = 7;
        let r = run(&build(&s));
        let k = &r.candidates[0];
        let t = k.trailer().unwrap();
        assert_eq!((t.gap, t.tail_len), (100, 7));
        let st = |n: &str| {
            k.checks
                .iter()
                .find(|c| c.name == n)
                .unwrap()
                .status
                .clone()
        };
        assert_eq!(st("oci_after_256_bytes"), CheckStatus::Fail);
        assert_eq!(st("record_chain"), CheckStatus::Pass);
    }

    #[test]
    fn oci_inside_a_record_is_not_a_trailer() {
        let mut payload = b"$OCI$\x00FAKE\x00X\x00Y\x0020200101000000\x00".to_vec();
        payload.resize(60, 0);
        let mut s = Spec::known("E7D91IMS", "HJ0", vec![Rec { id: 3, payload }]);
        s.oci = None;
        let r = run(&build(&s));
        assert_eq!(r.oci_signatures.len(), 1);
        assert!(r.candidates[0].trailers.is_empty());
    }

    #[test]
    fn hostile_counts_and_lengths_end_cleanly() {
        // Huge count, truncated payload, and a file cut inside a header.
        let mut b = build(&Spec::known("E7D91IMS", "HJ0", recs()));
        b[0x15..0x19].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!run(&b).candidates[0].complete());
        let mut b = build(&Spec::known("E7D91IMS", "HJ0", recs()));
        b[0x1D..0x1F].copy_from_slice(&u16::MAX.to_le_bytes());
        let k = &run(&b).candidates[0];
        assert!(k.chain_error.as_deref().unwrap().contains("runs past"));
        let b = build(&Spec::known("E7D91IMS", "HJ0", recs()));
        for cut in [0, 3, 5, 0x12, 0x18, 0x1B, 0x40] {
            let r = run(&b[..cut]);
            assert!(
                r.candidates
                    .iter()
                    .all(|c| !c.complete() || c.chain_end <= Some(cut))
            );
        }
        // Zero-size records are part of the chain.
        let r = run(&build(&Spec::known(
            "E7D91IMS",
            "HJ0",
            vec![Rec {
                id: 0,
                payload: vec![],
            }],
        )));
        assert_eq!(r.candidates[0].records[0].size, 0);
    }

    #[test]
    fn the_header_visit_cap_is_a_limit_error() {
        let b = build(&Spec::known("E7D91IMS", "HJ0", recs()));
        let mut budget = Budget::new(ProbeLimits {
            max_header_visits: 1,
            ..ProbeLimits::default()
        });
        assert!(matches!(probe(&b, &mut budget), Err(Error::Limit(_))));
    }

    #[test]
    fn a_file_ending_after_the_cmos_block_has_no_trailer_but_a_known_end() {
        let mut s = Spec::known("E7E34IMSI", "1AF0", recs());
        s.oci = None;
        s.tail = 0;
        let b = build(&s);
        let v = run(&b)
            .candidates
            .into_iter()
            .find(|c| c.id == "variable_header/14")
            .unwrap();
        assert!(v.trailers.is_empty());
        assert!(v.ends_after_cmos_block(b.len()));
        assert!(!v.ends_after_cmos_block(b.len() + 1));
    }

    #[test]
    fn the_checksum_is_the_negated_32_bit_sum() {
        // Hand-computed: 60 bytes of 0xFF outside the field sum to 15300; stored as -15300.
        let b = vec![0xFFu8; 64];
        assert_eq!(additive_checksum(&b, 0), Some(-15300));
    }

    #[test]
    fn checksum_relation_is_parameterised_by_offset() {
        let b = build(&Spec::known("E7E34IMSI", "1AF0", recs()));
        assert_eq!(additive_checksum(&b, 19), stored_i32(&b, 19));
        assert_ne!(additive_checksum(&b, 0x11), stored_i32(&b, 0x11));
        assert_eq!(additive_checksum(&b, b.len()), None);
    }
}
