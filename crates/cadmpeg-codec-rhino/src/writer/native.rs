// SPDX-License-Identifier: Apache-2.0
//! What the Rhino write does with each record of the `rhino` native namespace.

use std::collections::BTreeSet;

use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{NativeNamespace, NativeRecord};

use crate::loss::RhinoLossCode;

/// What the write does with one native record (§3.4, Rule W3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeDisposition {
    /// The writer regenerates the record from neutral records and its own
    /// defaults.
    Regenerated,
    /// The writer has no encoder for the record, or the record states content
    /// the regenerated record does not hold.
    Dropped,
}

/// Opaque typecodes the writer emits itself: the OpenNURBS version property
/// and the units-and-tolerances setting.
const REGENERATED_OPAQUE_TYPECODES: &[&str] = &["0xa0000026", "0x20008031"];

/// Document settings fields the writer states itself: the record identity and
/// the OpenNURBS writer version property.
const REGENERATED_SETTINGS: &[&str] = &["id", "writer_version"];

/// The disposition of `record` in native arena `arena`.
pub(crate) fn disposition(arena: &str, record: &NativeRecord) -> NativeDisposition {
    use NativeDisposition::{Dropped, Regenerated};
    let regenerated = match arena {
        "byte_spans" | "unknowns" => true,
        "document_settings" => record.fields().is_ok_and(|fields| {
            fields
                .iter()
                .all(|(name, value)| REGENERATED_SETTINGS.contains(&name.as_str()) || value.is_null())
        }),
        "layers" => super::default_native_layer(record),
        "object_presentation" => super::default_native_presentation(record),
        "opaque_records" => {
            let typecode = record.field("typecode");
            let typecode = typecode.as_ref().and_then(serde_json::Value::as_str);
            typecode.is_some_and(|typecode| REGENERATED_OPAQUE_TYPECODES.contains(&typecode))
                || generated_comment(record)
        }
        _ => false,
    };
    if regenerated {
        Regenerated
    } else {
        Dropped
    }
}

/// Whether `record` is the start-section comment this writer emits.
fn generated_comment(record: &NativeRecord) -> bool {
    let Ok(fields) = record.fields() else {
        return false;
    };
    record.id() == "rhino:source:opaque#comment"
        && fields.get("typecode").and_then(serde_json::Value::as_str) == Some("0x00000001")
        && fields.get("data").and_then(serde_json::Value::as_str)
            == Some("AQAAAAcAAAAAAAAAY2FkbXBlZw==")
}

/// The source UUIDs of the layers the write regenerates: an object on one of
/// them keeps its layer.
pub(crate) fn regenerated_layers(namespace: Option<&NativeNamespace>) -> BTreeSet<String> {
    namespace
        .and_then(|namespace| namespace.arenas().get("layers"))
        .into_iter()
        .flatten()
        .filter(|record| disposition("layers", record) == NativeDisposition::Regenerated)
        .filter_map(|record| {
            record
                .field("source_uuid")
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// One `native.records-omitted` loss per arena that holds a dropped record,
/// naming every dropped id in sorted order.
pub(crate) fn omission_losses(namespace: Option<&NativeNamespace>) -> Vec<LossNote> {
    let Some(namespace) = namespace else {
        return Vec::new();
    };
    namespace
        .arenas()
        .iter()
        .filter_map(|(arena, records)| {
            let mut dropped = records
                .iter()
                .filter(|record| disposition(arena, record) == NativeDisposition::Dropped)
                .map(NativeRecord::id)
                .collect::<Vec<_>>();
            if dropped.is_empty() {
                return None;
            }
            dropped.sort_unstable();
            Some(RhinoLossCode::NativeRecordsOmitted.note(format!(
                "the rhino write has no encoder for native arena `{arena}`: {} record(s) omitted: {}",
                dropped.len(),
                dropped.join(", ")
            )))
        })
        .collect()
}
