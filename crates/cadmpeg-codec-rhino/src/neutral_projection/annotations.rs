// SPDX-License-Identifier: Apache-2.0
//! General text and leaders have semantic annotation owners.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::native::NativeRecord;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::semantic_annotations::{
    SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind,
};
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

fn position(record: &NativeRecord, key: &str) -> Option<FiniteVector<3>> {
    let value = record.field(key)?;
    let values = value.as_array()?;
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
        let Some(records) = ir
            .native
            .namespace("rhino")
            .and_then(|namespace| namespace.arenas().get(arena))
        else {
            continue;
        };
        for (ordinal, record) in records.iter().enumerate() {
            let native_id = record.id();
            let (kind, position_key, text_keys) = match arena {
                "text_dots" => (
                    SemanticAnnotationKind::Text,
                    "center",
                    &["primary_text", "secondary_text"][..],
                ),
                "annotation_arrows" => (SemanticAnnotationKind::Leader, "tail", &[][..]),
                _ => (
                    if record
                        .field("kind")
                        .is_some_and(|kind| kind.as_str() == Some("leader"))
                    {
                        SemanticAnnotationKind::Leader
                    } else {
                        SemanticAnnotationKind::Text
                    },
                    "plane_origin",
                    &["rich_text"][..],
                ),
            };
            let Some(position) = position(record, position_key) else {
                losses.push(
                    crate::loss::RhinoLossCode::AnnotationRecordUnprojected.note(format!(
                        "{native_id}: annotation placement is not a finite neutral position"
                    )),
                );
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
                .filter_map(|key| {
                    record
                        .field(key)
                        .and_then(|value| value.as_str().map(str::to_owned))
                })
                .collect();
            ctx.charge_entities(1, "Rhino semantic annotations")?;
            let order = u32::try_from(ir.model.semantic_annotations.len())
                .map_err(CodecError::malformed)?;
            ir.model.semantic_annotations.push(SemanticAnnotation {
                id: id.clone(),
                object: native_id.to_owned(),
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
                native_ref: native_id.to_owned(),
            });
        }
    }
    Ok(losses)
}
