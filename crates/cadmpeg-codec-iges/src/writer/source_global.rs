//! The Global fields a semantic write restates from the source Global cards.

use std::collections::BTreeMap;

use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::global::{GlobalTable, SourceField, MAXIMUM_V4_LINE_WEIGHT_GRADATIONS};
use crate::loss::IgesLossCode;

/// The Global field numbers (IGES 5.3 Table 3, from 1) whose source value
/// the writer states, with the name a loss gives each. Fields 13 to 15 hold
/// the writer's millimetres at model scale 1, the unit of every written
/// length, and are charged when the source states another scale or unit.
const RESTATED_FIELDS: &[(usize, &str)] = &[
    (3, "sending product id"),
    (4, "file name"),
    (12, "receiving product id"),
    (13, "model space scale"),
    (14, "units flag"),
    (15, "units name"),
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
    /// The source fields the writer states with its own value.
    unrestated: Vec<Unrestated>,
}

/// A source Global field the writer states with its own value.
struct Unrestated {
    number: usize,
    name: &'static str,
    /// The source value, as Global text.
    source: String,
}

impl GlobalRestatement {
    /// Reads the source Global cards of `ir` for a write of `version`. A
    /// document with no source Global cards restates nothing.
    pub(super) fn of(ir: &CadIr, version: crate::IgesVersion) -> Self {
        let Some(bytes) = source_global_bytes(ir) else {
            return Self::default();
        };
        let source = crate::global::source_fields(&bytes);
        let resolved = crate::global::resolved(&bytes);
        let table = crate::version::VersionFlag::from_write_version(version).global_table();
        let field_count = match version {
            crate::IgesVersion::V4_0 => 24,
            crate::IgesVersion::V5_0 => 25,
            crate::IgesVersion::V5_1 | crate::IgesVersion::V5_2 | crate::IgesVersion::V5_3 => 26,
        };
        let length_factor_mm = resolved
            .as_ref()
            .and_then(crate::global::ResolvedGlobal::length_context)
            .map(|context| context.length_factor_mm());
        let mut restatement = Self::default();
        for &(number, name) in RESTATED_FIELDS {
            let value = source.as_ref().map_or(&SourceField::Unreadable, |fields| {
                fields.get(number - 1).unwrap_or(&SourceField::Omitted)
            });
            let statement = match (number, value) {
                (13, _) => Statement::writer_value_when(
                    resolved
                        .as_ref()
                        .and_then(crate::global::ResolvedGlobal::model_scale)
                        .is_some_and(|scale| scale.get() == 1.0),
                ),
                (14, _) => Statement::writer_value_when(
                    resolved
                        .as_ref()
                        .and_then(crate::global::ResolvedGlobal::units_flag)
                        == Some(super::WRITER_UNITS_FLAG),
                ),
                (15, _) => {
                    Statement::writer_value_when(resolved.as_ref().is_some_and(|resolved| {
                        resolved
                            .units_name()
                            .is_none_or(|units| units == super::WRITER_UNITS_NAME)
                    }))
                }
                (_, SourceField::Omitted) => match number {
                    3 | 4 | 12 | 21 | 22 | 25 | 26 => Statement::Source(String::new()),
                    _ => Statement::WriterValue,
                },
                (number, _) if number > field_count => Statement::Unrestated,
                (3 | 4 | 12 | 21 | 22 | 26, SourceField::Text(text)) => {
                    Statement::source(hollerith(text))
                }
                (25, SourceField::Text(text)) if text.is_empty() => {
                    Statement::source(hollerith(text))
                }
                (18 | 25, SourceField::Text(text)) => Statement::source(
                    crate::global::date_is_valid(text, table)
                        .then(|| hollerith(text))
                        .flatten(),
                ),
                (16, SourceField::Integer(gradations))
                    if *gradations >= 1
                        && (table != GlobalTable::V4_0
                            || *gradations <= MAXIMUM_V4_LINE_WEIGHT_GRADATIONS) =>
                {
                    Statement::Source(gradations.to_string())
                }
                (17, value) => {
                    let width = match value {
                        SourceField::Real(value) => Some(value.get()),
                        SourceField::Integer(value) => i32::try_from(*value).ok().map(f64::from),
                        SourceField::Omitted | SourceField::Text(_) | SourceField::Unreadable => {
                            None
                        }
                    };
                    match (width, length_factor_mm) {
                        (Some(width), _)
                            if width == 0.0
                                && table == GlobalTable::V5_0
                                && resolved.as_ref().is_some_and(|resolved| {
                                    resolved.global_table() == GlobalTable::V5_0
                                }) =>
                        {
                            Statement::Source("0".to_owned())
                        }
                        (Some(width), Some(factor)) if width > 0.0 => Statement::source(
                            cadmpeg_ir::scalar::FiniteReal::new(width * factor).map(super::number),
                        ),
                        _ => Statement::Unrestated,
                    }
                }
                (24, SourceField::Integer(standard)) if (0..=7).contains(standard) => {
                    Statement::Source(standard.to_string())
                }
                _ => Statement::Unrestated,
            };
            match statement {
                Statement::Source(text) => {
                    restatement.fields.insert(number, text);
                }
                Statement::WriterValue => {}
                Statement::Unrestated => restatement.unrestated.push(Unrestated {
                    number,
                    name,
                    source: source_text(value),
                }),
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

    /// A loss for each source field the written Global `fields` states with
    /// the writer's value, naming both values.
    pub(super) fn losses<'a>(
        &'a self,
        fields: &'a [String],
    ) -> impl Iterator<Item = LossNote> + 'a {
        self.unrestated.iter().map(|field| {
            let written = fields.get(field.number - 1).map_or_else(
                || format!("no value: the written Global has no field {}", field.number),
                Clone::clone,
            );
            IgesLossCode::WriterGlobalFieldNotRestated.note(format!(
                "IGES Global {} (field {}) changed from {} to {written}",
                field.name, field.number, field.source
            ))
        })
    }
}

/// What the writer states in one Global field of a source.
enum Statement {
    /// The source value, as Global text.
    Source(String),
    /// The writer's own value, which states the source value.
    WriterValue,
    /// The writer's own value, which does not state the source value.
    Unrestated,
}

impl Statement {
    fn source(text: Option<String>) -> Self {
        text.map_or(Self::Unrestated, Self::Source)
    }

    fn writer_value_when(restates: bool) -> Self {
        if restates {
            Self::WriterValue
        } else {
            Self::Unrestated
        }
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

/// `value` as Global text, or a word for a value the source does not state.
fn source_text(value: &SourceField) -> String {
    match value {
        SourceField::Omitted => "an omitted value".to_owned(),
        SourceField::Unreadable => "an unreadable value".to_owned(),
        SourceField::Text(text) => format!("{}H{}", text.len(), text.escape_ascii()),
        SourceField::Integer(value) => value.to_string(),
        SourceField::Real(value) => super::number(*value),
    }
}

/// What the source Global field 19 states, when the source has Global cards
/// and the field resolves to no positive length in millimetres.
pub(super) fn unresolved_minimum_resolution(ir: &CadIr) -> Option<String> {
    let bytes = source_global_bytes(ir)?;
    let value = crate::global::source_fields(&bytes).map_or(SourceField::Unreadable, |fields| {
        fields.get(18).cloned().unwrap_or(SourceField::Omitted)
    });
    let stated = match &value {
        SourceField::Omitted => "no value: the source omits field 19".to_owned(),
        SourceField::Unreadable => "an unreadable value".to_owned(),
        value => format!("the invalid value {}", source_text(value)),
    };
    match crate::global::resolved(&bytes)
        .as_ref()
        .and_then(crate::global::ResolvedGlobal::length_context)
    {
        None => Some(match value {
            SourceField::Omitted | SourceField::Unreadable => stated,
            value => format!(
                "{} in a length unit the decoder does not resolve",
                source_text(&value)
            ),
        }),
        Some(context) => cadmpeg_ir::scalar::PositiveLength::new(context.minimum_resolution_mm())
            .is_none()
            .then_some(stated),
    }
}
