//! The Global fields a semantic write restates from the source Global cards.

use std::collections::BTreeMap;

use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::CadIr;

use crate::global::SourceField;

/// The Global field numbers (IGES 5.3 Table 3, from 1) the writer restates
/// from the source, with the name a loss reason gives each.
const RESTATED_FIELDS: &[(usize, &str)] = &[
    (3, "sending product id"),
    (4, "file name"),
    (5, "native system id"),
    (6, "preprocessor version"),
    (12, "receiving product id"),
    (13, "model space scale"),
    (16, "maximum line weight gradations"),
    (17, "maximum line width"),
    (18, "file generation date"),
    (21, "author"),
    (22, "organization"),
    (24, "drafting standard"),
    (25, "model modification date"),
    (26, "application protocol"),
];

/// The columns of a Global card that hold section text.
const GLOBAL_TEXT_COLUMNS: usize = 72;

/// What the writer states in each restated Global field.
#[derive(Default)]
pub(super) struct GlobalRestatement {
    /// The text of each field the writer takes from the source, by field
    /// number. An empty text is an omitted field.
    fields: BTreeMap<usize, String>,
    /// The names of the source fields the writer cannot restate.
    unrestated: Vec<&'static str>,
}

impl GlobalRestatement {
    /// Reads the source Global cards of `ir` for a write of `version`. A
    /// document with no source Global cards restates nothing.
    pub(super) fn of(ir: &CadIr, version: crate::IgesVersion) -> Self {
        let Some(bytes) = source_global_bytes(ir) else {
            return Self::default();
        };
        let Some(source) = crate::global::source_fields(&bytes) else {
            return Self {
                fields: BTreeMap::new(),
                unrestated: RESTATED_FIELDS.iter().map(|(_, name)| *name).collect(),
            };
        };
        let table = crate::version::VersionFlag::from_write_version(version).global_table();
        let field_count = match version {
            crate::IgesVersion::V4_0 => 24,
            crate::IgesVersion::V5_0 => 25,
            crate::IgesVersion::V5_1 | crate::IgesVersion::V5_2 | crate::IgesVersion::V5_3 => 26,
        };
        let mut restatement = Self::default();
        for &(number, name) in RESTATED_FIELDS {
            let value = source.get(number - 1).unwrap_or(&SourceField::Omitted);
            let text = match (number, value) {
                (_, SourceField::Omitted) => None,
                (number, _) if number > field_count => {
                    restatement.unrestated.push(name);
                    continue;
                }
                (3 | 4 | 5 | 6 | 12 | 21 | 22 | 26, SourceField::Text(text)) => hollerith(text),
                (25, SourceField::Text(text)) if text.is_empty() => hollerith(text),
                (18 | 25, SourceField::Text(text)) => (crate::global::date_is_valid(text, table))
                    .then(|| hollerith(text))
                    .flatten(),
                (13, value) => real(value)
                    .filter(|scale| *scale > 0.0)
                    .and_then(cadmpeg_ir::scalar::FiniteReal::new)
                    .map(super::number),
                (17, value) => real(value)
                    .filter(|width| *width >= 0.0)
                    .and_then(cadmpeg_ir::scalar::FiniteReal::new)
                    .map(super::number),
                (16, SourceField::Integer(gradations)) if *gradations >= 1 => {
                    Some(gradations.to_string())
                }
                (24, SourceField::Integer(standard)) => Some(standard.to_string()),
                _ => {
                    restatement.unrestated.push(name);
                    continue;
                }
            };
            match text {
                Some(text) => {
                    restatement.fields.insert(number, text);
                }
                None if matches!(value, SourceField::Omitted) => {
                    if matches!(number, 3 | 4 | 5 | 6 | 12 | 21 | 22 | 25 | 26) {
                        restatement.fields.insert(number, String::new());
                    }
                }
                None => restatement.unrestated.push(name),
            }
        }
        restatement
    }

    /// Each Global field number the writer restates from the source, with
    /// the text it states.
    pub(super) fn fields(&self) -> impl Iterator<Item = (usize, &str)> {
        self.fields
            .iter()
            .map(|(number, text)| (*number, text.as_str()))
    }

    /// The reason the source Global cards are dropped, or `None` when the
    /// writer restates every field it takes from the source.
    pub(super) fn drop_reason(&self) -> Option<String> {
        (!self.unrestated.is_empty()).then(|| {
            format!(
                "that are Global cards whose fields the semantic writer cannot restate: {}",
                self.unrestated.join(", ")
            )
        })
    }
}

/// Columns 1 to 72 of the source Global cards, in sequence order.
pub(super) fn source_global_bytes(ir: &CadIr) -> Option<Vec<u8>> {
    let cards = ir.native.namespace("iges")?.arenas().get("cards")?;
    let mut global = cards
        .iter()
        .filter(|card| {
            card.field("section")
                .is_some_and(|section| section.as_str() == Some("global"))
        })
        .map(|card| {
            let sequence = card.field("sequence").and_then(|value| value.as_u64());
            let payload = card
                .field("payload")
                .and_then(|value| <NativeBytes as serde::Deserialize>::deserialize(value).ok());
            (sequence, payload)
        })
        .collect::<Vec<_>>();
    if global.is_empty() {
        return None;
    }
    global.sort_by_key(|(sequence, _)| *sequence);
    let mut bytes = Vec::new();
    for (_, payload) in global {
        let payload = payload?;
        bytes.extend_from_slice(payload.get(..GLOBAL_TEXT_COLUMNS).unwrap_or(&payload));
    }
    Some(bytes)
}

/// `text` as a Global Hollerith string, when every byte is printable ASCII.
fn hollerith(text: &[u8]) -> Option<String> {
    text.iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        .then(|| format!("{}H{}", text.len(), String::from_utf8_lossy(text)))
}

fn real(value: &SourceField) -> Option<f64> {
    match value {
        SourceField::Real(value) => Some(value.get()),
        SourceField::Integer(value) => i32::try_from(*value).ok().map(f64::from),
        SourceField::Omitted | SourceField::Text(_) | SourceField::Unreadable => None,
    }
}
