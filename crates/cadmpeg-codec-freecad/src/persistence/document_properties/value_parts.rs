// SPDX-License-Identifier: Apache-2.0
//! Value fields are produced inside their actual child-element transaction.

use crate::native::PropertyRecord;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

pub(super) fn decode<'source>(
    ctx: &DecodeContext<'source>,
    view: View<'source>,
    source: &crate::persistence::source::Source<'source>,
    node: roxmltree::Node<'_, '_>,
    owner: &str,
    order: usize,
    stage: &mut PartOutput,
    objects: &[crate::native::ObjectRecord],
) -> Result<PropertyRecord, CodecError> {
    let mut property = crate::persistence::property_header(node, owner, order, Some(source))?;
    property.dynamic = crate::persistence::dynamic_property_metadata(node, Some(source))?;
    let value = node
        .children()
        .find(roxmltree::Node::is_element)
        .ok_or_else(|| CodecError::malformed("selected property has no value element"))?;
    let extent = crate::fidelity::relative_xml_extent(view, value.range())?;
    let mut spec = crate::persistence::document_part("freecad.property_value");
    let type_name = property.type_name.clone();
    if let Some(class) = crate::fidelity::property_value_class(objects, owner, &type_name) {
        spec = spec.holding([class]);
    }
    let name = property.name.clone();
    let type_name = property.type_name.clone();
    let body = ctx.decode_part(stage, extent, spec, |value_stage| {
        let body = crate::persistence::property_body(node, owner, &name, &type_name)?;
        let mut complete = property.clone();
        complete.body = Some(body.clone());
        if owner == crate::native::native_id("document", "0") {
        if let Some(attribute) = crate::attributes::document_property(ctx, &complete)? {
            value_stage.insert_model(ctx, attribute)?;
        } else if (complete.type_name == "App::PropertyIntegerConstraint"
            || !matches!(complete.name.as_str(), "Shape" | "Placement"))
            && !crate::property_policy::is_projection_metadata(&complete)
        {
            value_stage.push_loss(ctx, crate::loss::FreecadLossCode::AttributePropertyUnprojected.note(format!(
                "property {} ({}) has no neutral attribute encoding; its admitted typed native values remain held",
                complete.id, complete.type_name,
            )))?;
        }
        }
        Ok(body)
    })?;
    property.body = body;
    if property.type_name == "App::PropertyString" {
        stage.insert_native(
            ctx,
            "properties",
            crate::native::document_string::DocumentString::new(property.clone())?,
        )?;
    } else {
        stage.insert_native(
            ctx,
            "properties",
            crate::native::document_value::DocumentValue::new(property.clone())?,
        )?;
    }
    Ok(property)
}
