// SPDX-License-Identifier: Apache-2.0
//! Notes.
//!
//! A note is a text callout anchored to a point of the design. It decodes
//! to a text annotation at that point; a hidden note keeps a `hidden`
//! parameter. The whole `notes` section also stays a retained record.

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::semantic_annotations::{
    SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind,
};
use serde_json::{json, Value};

use crate::loss::SolidcraftLossCode;

/// The runtime type a note annotation names.
const RUNTIME_TYPE: &str = "solidcraft::note";

fn anchor(value: Option<&Value>) -> Option<[f64; 3]> {
    let value = value?;
    if let Some(array) = value.as_array() {
        return Some([
            array.first()?.as_f64()?,
            array.get(1)?.as_f64()?,
            array.get(2)?.as_f64()?,
        ]);
    }
    Some([
        value.get("x")?.as_f64()?,
        value.get("y")?.as_f64()?,
        value.get("z")?.as_f64()?,
    ])
}

/// The notes of `document`, as text annotations.
pub(crate) fn notes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    document: &serde_json::Map<String, Value>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    let Some(records) = document.get("notes").and_then(Value::as_array) else {
        return Ok(losses);
    };
    ctx.charge_entities(u64_from_index(records.len()), "decode notes")?;
    let namespace = cadmpeg_ir::identity_namespace!("solidcraft", "design", "note");
    let section = cadmpeg_ir::ids::UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("solidcraft", "native", "section"),
        IdentityKey::encode_segment("notes"),
    )
    .into_string();
    for (index, record) in records.iter().enumerate() {
        let key = record
            .get("id")
            .and_then(Value::as_u64)
            .map_or_else(|| index.to_string(), |id| id.to_string());
        let (Some(text), Some(at)) = (
            record.get("text").and_then(Value::as_str),
            anchor(record.get("anchor")),
        ) else {
            losses.push(SolidcraftLossCode::DocumentSectionUndecoded.note(format!(
                "note {key} has no text or anchor, so it stays in the retained section"
            )));
            continue;
        };
        let id = SemanticAnnotationId::compose(&namespace, IdentityKey::encode_segment(&key));
        let mut annotation = SemanticAnnotation::new(
            id,
            format!("note:{key}"),
            SemanticAnnotationKind::Text,
            RUNTIME_TYPE.to_owned(),
            u32::try_from(index)
                .map_err(|_| CodecError::malformed(format_args!("too many notes")))?,
            section.clone(),
        );
        annotation.text = text.lines().map(str::to_owned).collect();
        match serde_json::from_value(json!([at[0], at[1], at[2]])) {
            Ok(position) => annotation.position = Some(position),
            Err(error) => {
                losses.push(SolidcraftLossCode::DocumentSectionUndecoded.note(format!(
                    "note {key} has an anchor with no neutral form: {error}"
                )));
                continue;
            }
        }
        if record.get("hidden").and_then(Value::as_bool) == Some(true) {
            annotation
                .parameters
                .insert(cadmpeg_core::nonblank_const!("hidden"), "true".to_owned());
        }
        ir.model.semantic_annotations.push(annotation);
    }
    Ok(losses)
}
