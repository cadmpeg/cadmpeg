// SPDX-License-Identifier: Apache-2.0
//! Document metadata has neutral attribute owners and typed native links.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::{AttributeId, Identity, IdentityKey};
use cadmpeg_ir::native::NativeRecord;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::CadIr;
use serde_json::{Map, Value};

fn values(value: &Value) -> Option<Vec<AttributeValue>> {
    Some(match value {
        Value::String(value) => vec![AttributeValue::String(value.clone())],
        Value::Bool(value) => vec![AttributeValue::Boolean(*value)],
        Value::Number(value) => vec![if let Some(value) = value.as_i64() {
            AttributeValue::Integer(value)
        } else {
            AttributeValue::Float(FiniteReal::new(value.as_f64()?)?)
        }],
        Value::Array(values) => vec![AttributeValue::Vector(
            values
                .iter()
                .map(|value| FiniteReal::new(value.as_f64()?))
                .collect::<Option<Vec<_>>>()?,
        )],
        _ => return None,
    })
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    for (arena, consumed) in [
        (
            "document_notes",
            &["html", "text", "visible", "window_rectangle", "locked"][..],
        ),
        (
            "revisions",
            &[
                "created_by",
                "created_utc_fields",
                "last_edited_by",
                "last_edited_utc_fields",
                "revision_count",
            ][..],
        ),
        ("applications", &["name", "url", "details"][..]),
    ] {
        let mut projected = Vec::new();
        if let Some(records) = ir
            .native
            .namespace("rhino")
            .and_then(|namespace| namespace.arenas().get(arena))
        {
            for record in records {
                projected.push((record.id().to_owned(), record.copy_fields(ctx)?));
            }
        }
        let mut replacements = Vec::new();
        for (ordinal, (native_id, mut fields)) in projected.into_iter().enumerate() {
            let mut links = Map::new();
            for field in consumed {
                let Some(value) = fields.get(*field) else {
                    continue;
                };
                if value.is_null() {
                    continue;
                }
                let Some(values) = values(value) else {
                    losses.push(crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!("{native_id}: metadata field {field} has no admitted neutral attribute value")));
                    continue;
                };
                let key = IdentityKey::try_new(format!("{arena}-{ordinal}-{field}"))
                    .map_err(CodecError::malformed)?;
                let id = AttributeId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "model", "document-attribute"),
                    key,
                );
                ctx.charge_entities(1, "Rhino document attributes")?;
                ir.model.attributes.push(SourceAttribute {
                    id: id.clone(),
                    target: AttributeTarget::Document,
                    name: format!("rhino.{arena}.{field}"),
                    values,
                });
                links.insert((*field).to_owned(), Value::String(id.to_string()));
            }
            fields.insert("neutral_attributes".to_owned(), Value::Object(links));
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
