//! Find every HII formset in a firmware walk and decode it with its own English strings.
//!
//! A module can carry several formsets, each registered with its own string package (AMD's
//! AodDxe has four). Pairing uses, in order: membership of the same HII package list, a string
//! package laid out directly next to the forms package, then the only English string package
//! that defines every string id the forms reference. A formset that cannot be paired this way
//! is skipped with a diagnostic rather than decoded with somebody else's strings.

use super::{SetupModule, hii, ifr, volume};
use crate::bytes::sha256_hex;
use crate::error::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};

/// Largest gap between adjacent forms and string arrays (length prefix plus alignment).
const ADJACENT: usize = 16;

pub struct ScannedFormset {
    pub module: SetupModule,
    pub pkg: ifr::FormsPackage,
}

type English = (usize, usize, String);

pub fn scan_formsets(
    walk: &volume::Walk<'_>,
    limits: volume::Limits,
    diagnostics: &mut Vec<String>,
) -> Result<Vec<ScannedFormset>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    // Non-decoding byte matches in modules that hold no formset at all (TLS/crypto code):
    // reported once as a summary, not per match.
    let (mut stray, mut stray_modules) = (0usize, BTreeSet::new());
    for f in &walk.files {
        for r in &f.pe32 {
            let pe = walk.bytes_of(r);
            let forms = ifr::find_forms_packages(pe);
            if forms.is_empty() {
                continue;
            }
            let pe_hash = sha256_hex(pe);
            if !seen.insert(pe_hash.clone()) {
                continue; // identical copy (e.g. recovery volume)
            }
            let english: Vec<English> = hii::find_string_packages(pe)
                .into_iter()
                .filter(|(_, _, lang)| lang == "en-US" || lang == "en")
                .collect();
            let lists = if english.len() > 1 {
                hii::package_lists(pe)
            } else {
                Vec::new()
            };
            let mut tables: BTreeMap<usize, BTreeMap<u16, String>> = BTreeMap::new();
            // End of the last package that decoded; candidates inside it are its own bytes.
            let mut covered = 0usize;
            let mut false_matches: Vec<String> = Vec::new();
            let before = out.len();
            for (fo, fl) in forms {
                if fo < covered {
                    continue;
                }
                let label = || {
                    format!(
                        "module {} ({})",
                        f.guid,
                        f.name.as_deref().unwrap_or("unnamed")
                    )
                };
                let decoded = pair_and_parse(pe, fo, fl, &english, &lists, &mut tables, limits);
                let (pkg, (so, _, lang)) = match decoded {
                    Ok(Some(found)) => found,
                    Ok(None) => {
                        let decodes = ifr::parse_forms_package(
                            pe,
                            fo,
                            fl,
                            &BTreeMap::new(),
                            limits.max_ifr_ops,
                        )
                        .is_ok();
                        if decodes {
                            diagnostics.push(format!(
                                "{}: forms package at 0x{fo:x} has no identifiable English strings; skipped",
                                label()
                            ));
                        } else {
                            false_matches.push(format!(
                                "{}: bytes at 0x{fo:x} look like a forms package but do not decode; skipped",
                                label()
                            ));
                        }
                        continue;
                    }
                    // The package scan is a byte-pattern search, so code or data can match it.
                    // A candidate that does not decode is not a forms package.
                    Err(Error::Malformed { .. }) => {
                        false_matches.push(format!(
                            "{}: bytes at 0x{fo:x} look like a forms package but do not decode; skipped",
                            label()
                        ));
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                covered = fo + fl;
                if pkg.missing_strings > 0 {
                    diagnostics.push(format!(
                        "{}: formset \"{}\" references {} string ids its strings do not define",
                        label(),
                        pkg.formset_title,
                        pkg.missing_strings
                    ));
                }
                out.push(ScannedFormset {
                    module: SetupModule {
                        file_guid: f.guid.clone(),
                        name: f.name.clone(),
                        container_path: f.path.clone(),
                        pe32_sha256: pe_hash.clone(),
                        forms_package_offset: fo,
                        strings_package_offset: so,
                        language: lang,
                    },
                    pkg,
                });
            }
            if out.len() > before {
                diagnostics.append(&mut false_matches);
            } else if !false_matches.is_empty() {
                stray += false_matches.len();
                stray_modules.insert(f.name.clone().unwrap_or_else(|| f.guid.clone()));
            }
        }
    }
    if stray > 0 {
        diagnostics.push(format!(
            "{stray} byte pattern match(es) in modules without any formset ({}) are not forms packages; ignored",
            stray_modules.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(out)
}

fn strings_of<'t>(
    pe: &[u8],
    e: &English,
    tables: &'t mut BTreeMap<usize, BTreeMap<u16, String>>,
) -> Result<&'t BTreeMap<u16, String>> {
    Ok(match tables.entry(e.0) {
        std::collections::btree_map::Entry::Occupied(o) => o.into_mut(),
        std::collections::btree_map::Entry::Vacant(v) => {
            v.insert(hii::parse_string_package(pe, e.0, e.1, &e.2)?.strings)
        }
    })
}

fn pair_and_parse(
    pe: &[u8],
    fo: usize,
    fl: usize,
    english: &[English],
    lists: &[(usize, usize)],
    tables: &mut BTreeMap<usize, BTreeMap<u16, String>>,
    limits: volume::Limits,
) -> Result<Option<(ifr::FormsPackage, English)>> {
    let parse = |strings: &BTreeMap<u16, String>| {
        ifr::parse_forms_package(pe, fo, fl, strings, limits.max_ifr_ops)
    };
    let chosen: Option<&English> = if english.len() <= 1 {
        english.first()
    } else {
        let in_list: Vec<&English> = lists
            .iter()
            .find(|(s, e)| *s <= fo && fo < *e)
            .map(|(s, e)| english.iter().filter(|x| *s <= x.0 && x.0 < *e).collect())
            .unwrap_or_default();
        let adjacent: Vec<&English> = english
            .iter()
            .filter(|(so, sl, _)| {
                let s_end = so + sl;
                (s_end <= fo && fo - s_end <= ADJACENT)
                    || (*so >= fo + fl && *so - (fo + fl) <= ADJACENT)
            })
            .collect();
        match (in_list.as_slice(), adjacent.as_slice()) {
            ([one], _) => Some(*one),
            ([], [one]) => Some(*one),
            _ => None,
        }
    };
    if let Some(e) = chosen {
        let pkg = parse(strings_of(pe, e, tables)?)?;
        return Ok(Some((pkg, e.clone())));
    }
    // Last resort: the single English table that defines every referenced string id.
    let mut complete = Vec::new();
    for e in english {
        let pkg = parse(strings_of(pe, e, tables)?)?;
        if pkg.missing_strings == 0 {
            complete.push((pkg, e.clone()));
        }
    }
    Ok(if complete.len() == 1 {
        complete.pop()
    } else {
        None
    })
}
