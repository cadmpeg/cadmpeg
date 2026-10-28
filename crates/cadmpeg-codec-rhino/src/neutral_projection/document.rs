// SPDX-License-Identifier: Apache-2.0
//! Document metadata has neutral attribute owners and typed native links.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::{AttributeId, IdentityKey};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::container::Scan;

enum FieldValue<'a> {
    Text(&'a str),
    Boolean(bool),
    Integer(i32),
    Vector(&'a [i32]),
}

impl FieldValue<'_> {
    fn value(&self, ctx: &DecodeContext<'_>) -> Result<Option<AttributeValue>, CodecError> {
        Ok(match self {
            Self::Text(value) => Some(AttributeValue::String((*value).to_owned())),
            Self::Boolean(value) => Some(AttributeValue::Boolean(*value)),
            Self::Integer(value) => Some(AttributeValue::Integer(i64::from(*value))),
            Self::Vector(values) => {
                let _depth = ctx.enter_nested("copy native field sequence")?;
                AttributeValue::vector(values.iter().copied().map(f64::from))
            }
        })
    }
}

fn project_record(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    arena: &str,
    native_id: &str,
    fields: &[(&str, FieldValue<'_>)],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(ordinal) = ir
        .native
        .namespace("rhino")
        .and_then(|namespace| namespace.arenas().get(arena))
        .and_then(|records| records.iter().position(|record| record.id() == native_id))
    else {
        return Ok(());
    };
    let projected = {
        let _depth = ctx.enter_nested("copy native field object")?;
        fields
            .iter()
            .map(|(field, value)| Ok((*field, value.value(ctx)?)))
            .collect::<Result<Vec<_>, CodecError>>()?
    };
    for (field, value) in projected {
        let Some(value) = value else {
            losses.push(
                crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!(
                    "{native_id}: metadata field {field} has no admitted neutral attribute value"
                )),
            );
            continue;
        };
        let key = IdentityKey::try_new(format!("{arena}-{ordinal}-{field}"))
            .map_err(CodecError::malformed)?;
        let id = AttributeId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "model", "document-attribute"),
            key,
        );
        ctx.charge_entities(1, "Rhino document attributes")?;
        ir.model.attributes.push(SourceAttribute::new(
            ctx,
            id,
            Some(native_id.to_owned()),
            AttributeTarget::Document,
            format!("rhino.{arena}.{field}"),
            vec![value],
        )?);
    }
    Ok(())
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    let properties = &scan.metadata.properties;
    if let Some(notes) = &properties.notes {
        project_record(
            ctx,
            ir,
            "document_notes",
            "rhino:document:notes#current",
            &[
                ("html", FieldValue::Boolean(notes.html)),
                ("text", FieldValue::Text(&notes.text)),
                ("visible", FieldValue::Boolean(notes.visible)),
                ("window_rectangle", FieldValue::Vector(&notes.rectangle)),
                ("locked", FieldValue::Boolean(notes.locked)),
            ],
            &mut losses,
        )?;
    }
    if let Some(revision) = &properties.revision_history {
        project_record(
            ctx,
            ir,
            "revisions",
            "rhino:document:revision#current",
            &[
                ("created_by", FieldValue::Text(&revision.created_by)),
                (
                    "created_utc_fields",
                    FieldValue::Vector(&revision.created.fields),
                ),
                ("last_edited_by", FieldValue::Text(&revision.last_edited_by)),
                (
                    "last_edited_utc_fields",
                    FieldValue::Vector(&revision.last_edited.fields),
                ),
                (
                    "revision_count",
                    FieldValue::Integer(revision.revision_count),
                ),
            ],
            &mut losses,
        )?;
    }
    if let Some(application) = &properties.application {
        project_record(
            ctx,
            ir,
            "applications",
            "rhino:document:application#writer",
            &[
                ("name", FieldValue::Text(&application.name)),
                ("url", FieldValue::Text(&application.url)),
                ("details", FieldValue::Text(&application.details)),
            ],
            &mut losses,
        )?;
    }
    Ok(losses)
}
