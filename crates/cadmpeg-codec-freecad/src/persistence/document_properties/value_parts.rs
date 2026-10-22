// SPDX-License-Identifier: Apache-2.0
//! Value fields are produced inside their actual child-element transaction.

use crate::native::{PropertyBody, PropertyRecord};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

pub(super) fn decode<'source>(
    ctx: &DecodeContext<'source>,
    view: View<'source>,
    source: &crate::persistence::source::Source<'_, 'source>,
    node: roxmltree::Node<'_, '_>,
    owner: &str,
    order: usize,
    stage: &mut PartOutput,
) -> Result<PropertyRecord, CodecError> {
    let mut property = crate::persistence::property_header(node, owner, order, Some(source))?;
    property.dynamic = crate::persistence::dynamic_property_metadata(node, Some(source))?;
    let value = node
        .children()
        .find(roxmltree::Node::is_element)
        .ok_or_else(|| CodecError::malformed("selected property has no value element"))?;
    let (extent, fields) = crate::xml_framing::element_fields(ctx, view, value)?;
    let level = PartLevel::parse("freecad.property_value")
        .ok_or_else(|| CodecError::malformed("property value level is absent from Appendix C"))?;
    let body = ctx.decode_part(stage, extent, PartSpec::new(level, fields), |value_stage| {
        let mut losses = Vec::new();
        let body = crate::persistence::property_body(
            node, owner, &property.name, &property.type_name, &mut losses, Some(source), true,
        )?;
        if let PropertyBody::Unreadable(detail) = &body {
            return Err(CodecError::malformed(detail));
        }
        let mut complete = property.clone();
        complete.body = Some(body.clone());
        if let Some(attribute) = crate::attributes::document_property(&complete)? {
            value_stage.insert_model(ctx, attribute)?;
        } else if !crate::property_policy::is_projection_metadata(&complete) {
            value_stage.push_loss(ctx, crate::loss::FreecadLossCode::AttributePropertyUnprojected.note(format!(
                "property {} ({}) has no neutral attribute encoding; its admitted typed native values remain held",
                complete.id, complete.type_name,
            )))?;
        }
        for loss in losses { value_stage.push_loss(ctx, loss)?; }
        Ok(body)
    })?;
    property.body = body;
    stage.insert_native(
        ctx,
        "properties",
        crate::native::document_value::DocumentValue::new(property.clone())?,
    )?;
    Ok(property)
}
