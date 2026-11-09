//! A BIOS image on its own: every setting its menus define, where it appears and its optimized
//! default. Works for every family, with or without a profile. Display only: nothing here
//! feeds validation or writing.

use super::session::image_summary;
use crate::contracts::*;
use crate::firmware::ifr::{QuestionKind, StoreKind};
use crate::firmware::{BiosImage, BiosSchema, Placement};
use std::collections::BTreeMap;

pub const BROWSE_PAYLOAD_VERSION: &str = "1.1";

pub fn browse(
    img: &BiosImage,
    schema: Option<&BiosSchema>,
    path: Option<String>,
    set: usize,
) -> Report<BrowsePayload> {
    let mut fields = Vec::new();
    let mut formsets = Vec::new();
    let mut diagnostics = Vec::new();
    let set = if set < img.sets.len().max(1) {
        set
    } else {
        diagnostics.push(Diagnostic::new(
            "firmware_set_unknown",
            Severity::Warning,
            "bios_parse",
            format!("this image has no firmware set {}; showing set 1", set + 1),
        ));
        0
    };
    for (fi, fs) in img.formsets.iter().filter(|f| f.set == set).enumerate() {
        let kinds: BTreeMap<u16, &StoreKind> = fs.stores.iter().map(|s| (s.id, &s.kind)).collect();
        // One row per (store, offset, width, prompt); prefer a copy the BIOS actually shows.
        let mut rows: BTreeMap<(String, u16, u16, String), usize> = BTreeMap::new();
        let start = fields.len();
        for q in &fs.questions {
            let stored = matches!(
                kinds.get(&q.store_id),
                Some(StoreKind::Buffer | StoreKind::Efi)
            );
            if q.prompt.trim().is_empty() || q.store.is_empty() || !stored {
                continue;
            }
            let key = (q.store.clone(), q.offset, q.width_bits, q.prompt.clone());
            let hidden = q.never_shown();
            if let Some(&at) = rows.get(&key) {
                let prev: &mut BiosField = &mut fields[at];
                if prev.hidden && !hidden {
                    *prev = field(fi, fs, q);
                }
                continue;
            }
            rows.insert(key, fields.len());
            fields.push(field(fi, fs, q));
        }
        formsets.push(FormsetInfo {
            title: fs.title.clone(),
            guid: fs.guid.clone(),
            module: fs
                .module
                .name
                .clone()
                .unwrap_or_else(|| fs.module.file_guid.clone()),
            placed_under: fs.placed_under.clone(),
            field_count: fields.len() - start,
            defaults_missing: fs.defaults_missing.clone(),
            placement: match fs.placement {
                Placement::Main => "main",
                Placement::Linked => "linked",
                Placement::TitleMatch => "title_match",
                Placement::TopLevel => "top_level",
            }
            .into(),
            note: fs.note.clone(),
        });
    }
    diagnostics.extend(img.diagnostics.iter().map(|d| {
        Diagnostic::new(
            "firmware_parse_note",
            Severity::Info,
            "bios_parse",
            d.clone(),
        )
    }));
    let mut bios = image_summary(img, schema, path);
    if let Some(s) = img.sets.get(set) {
        bios.defaults.source = s.defaults_note.clone();
    }
    let mut r = Report::new(
        "bios_browse",
        BrowsePayload {
            bios,
            formsets,
            fields,
            sets: img
                .sets
                .iter()
                .map(|s| FirmwareSetInfo {
                    label: s.label.clone(),
                    note: s.note.clone(),
                    defaults_note: s.defaults_note.clone(),
                })
                .collect(),
            set,
        },
        diagnostics,
    );
    r.payload_version = BROWSE_PAYLOAD_VERSION.into();
    r
}

fn field(
    fi: usize,
    fs: &crate::firmware::Formset,
    q: &crate::firmware::ifr::Question,
) -> BiosField {
    let width = usize::from(q.width_bits / 8);
    let default_raw = fs.default_raw(&q.store, q.offset, width);
    let default_label = default_raw.and_then(|v| match q.kind {
        QuestionKind::OneOf => q
            .options
            .iter()
            .find(|o| o.value == v)
            .map(|o| o.label.clone()),
        QuestionKind::Checkbox => Some(if v != 0 { "Enabled" } else { "Disabled" }.into()),
        QuestionKind::Numeric => None,
    });
    BiosField {
        formset: fi,
        store: q.store.clone(),
        offset: q.offset,
        width_bits: q.width_bits,
        prompt: q.prompt.trim().to_string(),
        help: q.help.trim().to_string(),
        kind: match q.kind {
            QuestionKind::OneOf => "one_of",
            QuestionKind::Checkbox => "checkbox",
            QuestionKind::Numeric => "numeric",
        }
        .into(),
        options: q
            .options
            .iter()
            .map(|o| FieldOption {
                raw: o.value,
                label: o.label.clone(),
            })
            .collect(),
        min: q.min,
        max: q.max,
        step: q.step,
        menu_path: fs.menu_path(q),
        menu_order: fs.menu_order(q),
        section: q.section.clone(),
        default_raw,
        default_label,
        callback: q.is_callback(),
        reset_required: q.flags & 0x10 != 0,
        read_only: q.flags & 0x01 != 0,
        hidden: q.never_shown(),
        conditional: !q.conditions.is_empty(),
    }
}
