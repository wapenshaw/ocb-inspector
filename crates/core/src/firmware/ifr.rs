//! IFR opcode parsing (UEFI spec §33.3.8): variable stores, questions, options,
//! defaults and the references made by enclosing visibility conditions.
//!
//! Conditions are recorded as evidence only. They are not evaluated (SPEC §5).

use crate::bytes::{guid_string, slice, u8_at, u16_at, u24_at, uint_at};
use crate::error::{Error, Result};
use serde::Serialize;
use std::collections::BTreeMap;

const OP_FORM_SET: u8 = 0x0E;
const OP_FORM: u8 = 0x01;
const OP_SUBTITLE: u8 = 0x02;
const OP_REF: u8 = 0x0F;
const OP_ONE_OF: u8 = 0x05;
const OP_CHECKBOX: u8 = 0x06;
const OP_NUMERIC: u8 = 0x07;
const OP_ONE_OF_OPTION: u8 = 0x09;
const OP_SUPPRESS_IF: u8 = 0x0A;
const OP_GRAY_OUT_IF: u8 = 0x19;
const OP_DISABLE_IF: u8 = 0x1E;
const OP_EQ_ID_VAL: u8 = 0x12;
const OP_EQ_ID_ID: u8 = 0x13;
const OP_EQ_ID_VAL_LIST: u8 = 0x14;
const OP_VARSTORE: u8 = 0x24;
const OP_VARSTORE_NAME_VALUE: u8 = 0x25;
const OP_VARSTORE_EFI: u8 = 0x26;
const OP_END: u8 = 0x29;
const OP_TRUE: u8 = 0x46;
const OP_DEFAULT: u8 = 0x5B;

/// Expression opcodes that only compare question values with constants.
const SIMPLE_EXPR: &[u8] = &[0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x46, 0x47];
/// All opcodes treated as part of a condition expression (UEFI expression opcode set).
fn is_expression_op(op: u8) -> bool {
    matches!(op,
        0x12..=0x18 | 0x2B..=0x2F | 0x30..=0x34 | 0x35..=0x3A | 0x3B..=0x45 | 0x46..=0x4E |
        0x52..=0x54 | 0x56..=0x59 | 0x5A | 0x5F | 0x60 | 0x61 | 0x62 | 0x63 | 0x64 | 0x65)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StoreKind {
    Buffer,
    Efi,
    NameValue,
}

#[derive(Debug, Clone, Serialize)]
pub struct VarStore {
    pub id: u16,
    pub guid: String,
    pub name: String,
    pub size: u16,
    pub kind: StoreKind,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    OneOf,
    Checkbox,
    Numeric,
}

#[derive(Debug, Clone, Serialize)]
pub struct OptionItem {
    pub value: u64,
    pub label: String,
    pub default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Condition {
    pub kind: &'static str,
    /// Question IDs referenced by the condition expression.
    pub question_refs: Vec<u16>,
    /// True when the expression only uses equality/boolean opcodes.
    pub simple: bool,
    /// True when the whole expression is the constant TRUE (e.g. `suppressif TRUE`).
    pub constant_true: bool,
    #[serde(skip)]
    expr_ops: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct Question {
    pub question_id: u16,
    pub kind: QuestionKind,
    pub prompt: String,
    pub help: String,
    pub store_id: u16,
    pub store: String,
    pub offset: u16,
    pub width_bits: u16,
    /// Raw question flags. 0x04 = callback, 0x10 = reset required, 0x01 = read only.
    pub flags: u8,
    pub min: Option<u64>,
    pub max: Option<u64>,
    pub step: Option<u64>,
    pub options: Vec<OptionItem>,
    pub defaults: Vec<(u16, u64)>,
    pub conditions: Vec<Condition>,
    /// Byte offset of the question opcode within its forms package (provenance).
    pub ifr_offset: u32,
    /// Form (menu page) that contains the question, if any. Display only.
    pub form_id: Option<u16>,
    /// Nearest preceding section heading on that page. Display only (heuristic, see `section_heading`).
    pub section: Option<String>,
}

impl Question {
    /// Under `suppressif TRUE`: the BIOS never displays this copy of the question.
    pub fn never_shown(&self) -> bool {
        self.conditions
            .iter()
            .any(|c| c.kind == "suppress_if" && c.constant_true)
    }

    pub fn is_callback(&self) -> bool {
        self.flags & 0x04 != 0
    }
}

#[derive(Debug, Default)]
pub struct FormsPackage {
    pub stores: BTreeMap<u16, VarStore>,
    pub questions: Vec<Question>,
    /// Form id -> page title, in IFR order of first definition.
    pub forms: Vec<(u16, String)>,
    /// Menu links: (form containing the REF, form it opens, REF byte offset for ordering).
    pub refs: Vec<(u16, u16, u32)>,
    /// Links into another formset (REF3/REF4): (form containing the REF, formset GUID, form id,
    /// REF byte offset). Display only.
    pub xrefs: Vec<(u16, String, u16, u32)>,
    /// Title and GUID of the first FORM_SET in the package.
    pub formset_title: String,
    pub formset_guid: String,
    /// Distinct non-zero string ids referenced by the package that the string table lacks.
    /// Non-zero means the string package paired with these forms is probably the wrong one.
    pub missing_strings: usize,
    pub op_count: usize,
}

/// Turn a SUBTITLE text into a section heading, or None for layout markers.
///
/// Heuristic and display only: MSI pages use `TAB`/`SSPACE`/`ESPACE` as spacing markers and
/// column headers padded with runs of spaces. Nothing that decides values may depend on this.
fn section_heading(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || matches!(t, "TAB" | "SSPACE" | "ESPACE") || t.contains("  ") {
        return None;
    }
    Some(t.to_string())
}

/// Locate forms packages: a package header of type 0x02 whose first opcode is FORM_SET.
///
/// This is a byte-pattern search, so it also matches code and data. Candidates may overlap; the
/// caller decodes them in order and skips any that fall inside one that decoded.
pub fn find_forms_packages(pe: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut o = 0;
    while o + 8 < pe.len() {
        if pe[o + 3] == super::hii::PACKAGE_FORMS && pe[o + 4] == OP_FORM_SET {
            let len = u24_at(pe, o, "").unwrap_or(0) as usize;
            let op_len = usize::from(pe[o + 5] & 0x7F);
            if len > 8 && o + len <= pe.len() && op_len >= 23 && (op_len - 23) % 16 == 0 {
                out.push((o, len));
            }
        }
        o += 1;
    }
    out
}

struct Scope {
    condition: Option<usize>,
    question: bool,
    form: bool,
}

pub fn parse_forms_package(
    pe: &[u8],
    offset: usize,
    length: usize,
    strings: &BTreeMap<u16, String>,
    max_ops: usize,
) -> Result<FormsPackage> {
    let pkg = slice(pe, offset, length, "forms package")?;
    let mut fp = FormsPackage::default();
    let mut scopes: Vec<Scope> = Vec::new();
    let mut conditions: Vec<Condition> = Vec::new();
    let mut active: Vec<usize> = Vec::new(); // indices into `conditions` currently enclosing
    let mut capturing: Option<usize> = None;
    let mut current_q: Option<usize> = None;
    let mut current_form: Option<u16> = None;
    let mut section: Option<String> = None;
    let missing = std::cell::RefCell::new(std::collections::BTreeSet::new());
    let s = |id: u16| match strings.get(&id) {
        Some(t) => t.clone(),
        None => {
            if id != 0 {
                missing.borrow_mut().insert(id);
            }
            String::new()
        }
    };

    let mut o = 4;
    while o < pkg.len() {
        fp.op_count += 1;
        if fp.op_count > max_ops {
            return Err(Error::Limit(format!(
                "more than {max_ops} IFR operations in one package"
            )));
        }
        let op = u8_at(pkg, o, "ifr")?;
        let lb = u8_at(pkg, o + 1, "ifr")?;
        let len = usize::from(lb & 0x7F);
        let opens = lb & 0x80 != 0;
        if len < 2 || o + len > pkg.len() {
            return Err(Error::malformed(
                "ifr",
                format!("bad opcode length at 0x{:x}", offset + o),
            ));
        }
        let body = &pkg[o..o + len];

        if let Some(ci) = capturing {
            if is_expression_op(op) {
                let c = &mut conditions[ci];
                c.expr_ops = c.expr_ops.saturating_add(1);
                c.constant_true = c.expr_ops == 1 && op == OP_TRUE;
                if !SIMPLE_EXPR.contains(&op) {
                    c.simple = false;
                }
                if matches!(op, OP_EQ_ID_VAL | OP_EQ_ID_VAL_LIST) {
                    c.question_refs.push(u16_at(body, 2, "eq_id")?);
                } else if op == OP_EQ_ID_ID {
                    c.question_refs.push(u16_at(body, 2, "eq_id_id")?);
                    c.question_refs.push(u16_at(body, 4, "eq_id_id")?);
                }
            } else {
                capturing = None;
            }
        }

        let mut scope = Scope {
            condition: None,
            question: false,
            form: false,
        };
        match op {
            OP_FORM_SET if fp.formset_guid.is_empty() && len >= 23 => {
                fp.formset_guid = guid_string(slice(body, 2, 16, "formset")?);
                fp.formset_title = s(u16_at(body, 18, "formset")?).trim().to_string();
            }
            OP_FORM => {
                let id = u16_at(body, 2, "form")?;
                let title = s(u16_at(body, 4, "form")?).trim().to_string();
                if !fp.forms.iter().any(|(f, _)| *f == id) {
                    fp.forms.push((id, title));
                }
                current_form = Some(id);
                section = None;
                scope.form = true;
            }
            OP_SUBTITLE => {
                if current_form.is_some()
                    && let Some(h) = section_heading(&s(u16_at(body, 2, "subtitle")?))
                {
                    section = Some(h);
                }
            }
            OP_REF if len >= 15 => {
                if let Some(parent) = current_form {
                    let form = u16_at(body, 13, "ref")?;
                    let at = (offset + o) as u32;
                    // REF3/REF4 carry a formset GUID at 17; an all-zero GUID means this formset.
                    let other = if len >= 33 {
                        let g = slice(body, 17, 16, "ref")?;
                        g.iter().any(|&b| b != 0).then(|| guid_string(g))
                    } else {
                        None
                    };
                    match other {
                        Some(g) => fp.xrefs.push((parent, g, form, at)),
                        None => fp.refs.push((parent, form, at)),
                    }
                }
            }
            OP_VARSTORE => {
                let guid = guid_string(slice(body, 2, 16, "varstore")?);
                let id = u16_at(body, 18, "varstore")?;
                let size = u16_at(body, 20, "varstore")?;
                let name = cstr(&body[22..]);
                fp.stores.insert(
                    id,
                    VarStore {
                        id,
                        guid,
                        name,
                        size,
                        kind: StoreKind::Buffer,
                    },
                );
            }
            OP_VARSTORE_EFI => {
                let id = u16_at(body, 2, "varstore_efi")?;
                let guid = guid_string(slice(body, 4, 16, "varstore_efi")?);
                let size = u16_at(body, 24, "varstore_efi")?;
                let name = cstr(body.get(26..).unwrap_or(&[]));
                fp.stores.insert(
                    id,
                    VarStore {
                        id,
                        guid,
                        name,
                        size,
                        kind: StoreKind::Efi,
                    },
                );
            }
            OP_VARSTORE_NAME_VALUE => {
                let id = u16_at(body, 2, "varstore_nv")?;
                let guid = guid_string(slice(body, 4, 16, "varstore_nv")?);
                fp.stores.insert(
                    id,
                    VarStore {
                        id,
                        guid,
                        name: String::new(),
                        size: 0,
                        kind: StoreKind::NameValue,
                    },
                );
            }
            OP_SUPPRESS_IF | OP_GRAY_OUT_IF | OP_DISABLE_IF => {
                let kind = match op {
                    OP_SUPPRESS_IF => "suppress_if",
                    OP_GRAY_OUT_IF => "gray_out_if",
                    _ => "disable_if",
                };
                conditions.push(Condition {
                    kind,
                    question_refs: Vec::new(),
                    simple: true,
                    constant_true: false,
                    expr_ops: 0,
                });
                let ci = conditions.len() - 1;
                capturing = Some(ci);
                scope.condition = Some(ci);
            }
            OP_ONE_OF | OP_CHECKBOX | OP_NUMERIC => {
                let prompt = u16_at(body, 2, "question")?;
                let help = u16_at(body, 4, "question")?;
                let qid = u16_at(body, 6, "question")?;
                let store_id = u16_at(body, 8, "question")?;
                let voff = u16_at(body, 10, "question")?;
                let qflags = u8_at(body, 12, "question")?;
                let (kind, width, min, max, step) = if op == OP_CHECKBOX {
                    (QuestionKind::Checkbox, 1usize, None, None, None)
                } else {
                    let f = u8_at(body, 13, "question flags")?;
                    let w = 1usize << (f & 0x0F);
                    if w > 8 {
                        return Err(Error::malformed(
                            "ifr",
                            format!("bad numeric size at 0x{:x}", offset + o),
                        ));
                    }
                    let mm = |i: usize| uint_at(body, 14 + i * w, w, "min/max/step").ok();
                    let k = if op == OP_ONE_OF {
                        QuestionKind::OneOf
                    } else {
                        QuestionKind::Numeric
                    };
                    (k, w, mm(0), mm(1), mm(2))
                };
                fp.questions.push(Question {
                    question_id: qid,
                    kind,
                    prompt: s(prompt),
                    help: s(help),
                    store_id,
                    store: String::new(),
                    offset: voff,
                    width_bits: (width * 8) as u16,
                    flags: qflags,
                    min,
                    max,
                    step,
                    options: Vec::new(),
                    defaults: Vec::new(),
                    conditions: active.iter().map(|&i| conditions[i].clone()).collect(),
                    ifr_offset: (offset + o) as u32,
                    form_id: current_form,
                    section: section.clone(),
                });
                current_q = Some(fp.questions.len() - 1);
                scope.question = true;
            }
            OP_ONE_OF_OPTION => {
                if let Some(qi) = current_q {
                    let sid = u16_at(body, 2, "option")?;
                    let flags = u8_at(body, 4, "option")?;
                    let ty = u8_at(body, 5, "option")?;
                    let value = match ty {
                        0..=3 => Some(uint_at(body, 6, 1 << ty, "option value")?),
                        4 => Some(u64::from(u8_at(body, 6, "option value")?)),
                        _ => None,
                    };
                    if let Some(value) = value {
                        fp.questions[qi].options.push(OptionItem {
                            value,
                            label: s(sid),
                            default: flags & 0x30 != 0,
                        });
                    }
                }
            }
            OP_DEFAULT => {
                if let Some(qi) = current_q {
                    let did = u16_at(body, 2, "default")?;
                    let ty = u8_at(body, 4, "default")?;
                    if ty <= 4 {
                        let w = if ty == 4 { 1 } else { 1 << ty };
                        if let Ok(v) = uint_at(body, 5, w, "default value") {
                            fp.questions[qi].defaults.push((did, v));
                        }
                    }
                }
            }
            OP_END => {
                if let Some(sc) = scopes.pop() {
                    if sc.condition.is_some() {
                        active.pop();
                    }
                    if sc.question {
                        current_q = None;
                    }
                    if sc.form {
                        current_form = None;
                        section = None;
                    }
                }
            }
            _ => {}
        }
        if opens {
            if let Some(ci) = scope.condition {
                active.push(ci);
            }
            scopes.push(scope);
        }
        o += len;
    }
    for q in &mut fp.questions {
        if let Some(st) = fp.stores.get(&q.store_id) {
            q.store = st.name.clone();
        }
    }
    fp.missing_strings = missing.borrow().len();
    // A REF3 naming this formset's own GUID is an ordinary in-formset link.
    let own = fp.formset_guid.clone();
    let (same, other): (Vec<_>, Vec<_>) = std::mem::take(&mut fp.xrefs)
        .into_iter()
        .partition(|(_, g, _, _)| *g == own);
    fp.refs
        .extend(same.into_iter().map(|(p, _, f, at)| (p, f, at)));
    fp.xrefs = other;
    Ok(fp)
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}
