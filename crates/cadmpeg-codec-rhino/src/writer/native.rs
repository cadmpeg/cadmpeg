// SPDX-License-Identifier: Apache-2.0
//! What the Rhino write does with each arena of the `rhino` native namespace
//! (§3.4, Rules W1 and W3).

use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{NativeNamespace, NativeRecord};

use super::layers::LayerTable;

/// What the write does with one native record (§3.4, Rule W3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeDisposition {
    /// The archive states the record itself.
    Written,
    /// The writer states the record's content from neutral records and its
    /// own defaults.
    Regenerated,
    /// The archive does not state the record.
    Dropped,
}

/// What the write declares for one native arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArenaDeclaration {
    /// Every record of the arena has this disposition.
    Uniform(NativeDisposition),
    /// A mixed arena: each record has its own disposition.
    PerRecord,
}

/// The declaration of native arena `arena`. An arena not named here is one
/// this writer has no encoder for.
fn declaration(arena: &str) -> ArenaDeclaration {
    use ArenaDeclaration::{PerRecord, Uniform};
    match arena {
        "unknowns" => Uniform(NativeDisposition::Regenerated),
        "document_settings" | "layers" | "object_presentation" | "opaque_records" => PerRecord,
        _ => Uniform(NativeDisposition::Dropped),
    }
}

/// Opaque typecodes the writer emits itself: the OpenNURBS version property
/// and the units-and-tolerances setting.
const REGENERATED_OPAQUE_TYPECODES: &[&str] = &["0xa0000026", "0x20008031"];

/// Document settings fields the writer states itself: the record identity and
/// the OpenNURBS writer version property.
const REGENERATED_SETTINGS: &[&str] = &["id", "writer_version"];

/// The disposition of `record` in native arena `arena`.
pub(crate) fn disposition(
    arena: &str,
    record: &NativeRecord,
    layers: &LayerTable,
) -> NativeDisposition {
    use NativeDisposition::{Dropped, Regenerated, Written};
    match declaration(arena) {
        ArenaDeclaration::Uniform(disposition) => disposition,
        ArenaDeclaration::PerRecord => match arena {
            "layers" if layers.writes_native_layer(record.id()) => Written,
            "object_presentation" if layers.regenerates_presentation(record.id()) => Regenerated,
            "document_settings" if writer_settings(record) => Regenerated,
            "opaque_records" if writer_opaque_record(record) => Regenerated,
            _ => Dropped,
        },
    }
}

/// Whether a document settings record states only fields the writer states.
fn writer_settings(record: &NativeRecord) -> bool {
    record.fields().is_ok_and(|fields| {
        fields
            .iter()
            .all(|(name, value)| REGENERATED_SETTINGS.contains(&name.as_str()) || value.is_null())
    })
}

/// Whether an opaque record is one the writer emits: the version property,
/// the units setting, or the start-section comment.
fn writer_opaque_record(record: &NativeRecord) -> bool {
    let Ok(fields) = record.fields() else {
        return false;
    };
    let typecode = fields.get("typecode").and_then(serde_json::Value::as_str);
    typecode.is_some_and(|typecode| REGENERATED_OPAQUE_TYPECODES.contains(&typecode))
        || (record.id() == "rhino:source:opaque#comment"
            && typecode == Some("0x00000001")
            && fields.get("data").and_then(serde_json::Value::as_str)
                == Some("AQAAAAcAAAAAAAAAY2FkbXBlZw=="))
}

/// One `export:native.records-omitted` loss per arena that holds a dropped
/// record, naming every dropped id.
pub(crate) fn omission_losses(
    namespace: Option<&NativeNamespace>,
    layers: &LayerTable,
) -> Vec<LossNote> {
    let Some(namespace) = namespace else {
        return Vec::new();
    };
    namespace
        .arenas()
        .iter()
        .filter_map(|(arena, records)| {
            let reason = match declaration(arena) {
                ArenaDeclaration::Uniform(_) => {
                    format!("of arena `{arena}`, which it has no encoder for")
                }
                ArenaDeclaration::PerRecord => format!(
                    "of arena `{arena}` that state fields or references the written records do \
                     not hold"
                ),
            };
            cadmpeg_ir::codec::write::native_records_omitted(
                "rhino",
                &reason,
                records
                    .iter()
                    .filter(|record| {
                        disposition(arena, record, layers) == NativeDisposition::Dropped
                    })
                    .map(NativeRecord::id),
            )
        })
        .collect()
}
