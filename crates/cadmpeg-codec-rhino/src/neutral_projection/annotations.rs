// SPDX-License-Identifier: Apache-2.0
//! General text and leaders have semantic annotation owners.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::ids::{Identity, IdentityKey};
use cadmpeg_ir::native::NativeRecord;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::semantic_annotations::{
    SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind,
};
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

fn position(fields: &Map<String, Value>, key: &str) -> Option<FiniteVector<3>> {
    let values = fields.get(key)?.as_array()?;
    if values.len() != 3 {
        return None;
    }
    FiniteVector::new([
        values[0].as_f64()?,
        values[1].as_f64()?,
        values[2].as_f64()?,
    ])
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    for arena in ["annotations", "text_dots", "annotation_arrows"] {
        let mut sources = Vec::new();
        if let Some(records) = ir
            .native
            .namespace("rhino")
            .and_then(|namespace| namespace.arenas().get(arena))
        {
            for record in records {
                sources.push((record.id().to_owned(), record.copy_fields(ctx)?));
            }
        }
        let mut replacements = Vec::new();
        for (ordinal, (native_id, mut fields)) in sources.into_iter().enumerate() {
            let (kind, position_key, text_keys) = match arena {
                "text_dots" => (
                    SemanticAnnotationKind::Text,
                    "center",
                    &["primary_text", "secondary_text"][..],
                ),
                "annotation_arrows" => (SemanticAnnotationKind::Leader, "tail", &[][..]),
                _ => (
                    if fields.get("kind").and_then(Value::as_str) == Some("leader") {
                        SemanticAnnotationKind::Leader
                    } else {
                        SemanticAnnotationKind::Text
                    },
                    "plane_origin",
                    &["rich_text"][..],
                ),
            };
            let Some(position) = position(&fields, position_key) else {
                losses.push(
                    crate::loss::RhinoLossCode::AnnotationRecordUnprojected.note(format!(
                        "{native_id}: annotation placement is not a finite neutral position"
                    )),
                );
                replacements.push(NativeRecord::new(
                    Identity::new(native_id).map_err(CodecError::malformed)?,
                    fields,
                )?);
                continue;
            };
            let key = IdentityKey::try_new(format!("{arena}-{ordinal}"))
                .map_err(CodecError::malformed)?;
            let id = SemanticAnnotationId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "model", "annotation"),
                key,
            );
            let text = text_keys
                .iter()
                .filter_map(|key| fields.get(*key).and_then(Value::as_str).map(str::to_owned))
                .collect();
            ctx.charge_entities(1, "Rhino semantic annotations")?;
            let order = u32::try_from(ir.model.semantic_annotations.len())
                .map_err(CodecError::malformed)?;
            ir.model.semantic_annotations.push(SemanticAnnotation {
                id: id.clone(),
                object: native_id.clone(),
                kind,
                runtime_type: format!("rhino::{arena}"),
                order,
                text,
                references: BTreeMap::new(),
                value: None,
                format: None,
                position: Some(position),
                parameters: BTreeMap::new(),
                assets: Vec::new(),
                native_ref: native_id.clone(),
            });
            fields.insert("neutral_ref".to_owned(), Value::String(id.to_string()));
            replacements.push(NativeRecord::new(
                Identity::new(native_id).map_err(CodecError::malformed)?,
                fields,
            )?);
        }
        if !replacements.is_empty() {
            ir.native
                .namespace_mut("rhino")
                .arenas_mut()
                .insert(arena.to_owned(), replacements);
        }
    }
    Ok(losses)
}
