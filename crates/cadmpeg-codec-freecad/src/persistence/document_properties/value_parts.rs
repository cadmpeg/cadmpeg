// SPDX-License-Identifier: Apache-2.0
//! Value fields are produced inside their actual child-element transaction.

use crate::native::PropertyRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

pub(super) fn decode<'source>(
    ctx: &DecodeContext<'source>,
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
    let extent = source.extent(value.range())?;
    let identity =
        cadmpeg_ir::ids::Identity::try_from(property.id.as_str()).map_err(CodecError::malformed)?;
    let mut spec = crate::persistence::document_part("freecad.property_value")
        .contained_in(identity)
        .absent_field("values")
        .absent_field("links")
        .absent_field("side_entries");
    let type_name = property.type_name.clone();
    let name = property.name.clone();
    if let Some(classes) = crate::fidelity::property_value_classes(objects, owner, &type_name) {
        spec = spec.holding(classes);
    }
    let header = property.clone();
    let body = ctx.decode_part_with_failure_parent(
        stage,
        extent,
        spec,
        |stage| stage.insert_native(ctx, "properties", header),
        |value_stage| {
            let body = crate::persistence::property_body(node, owner, &name, &type_name)?;
            let mut complete = property.clone();
            complete.body = Some(body.clone());
            if owner == crate::native::native_id("document", "0") {
                if let Some(attribute) = crate::attributes::document_property(ctx, &complete)? {
                    value_stage.insert_model(ctx, attribute)?;
                }
            }
            Ok(body)
        },
    )?;
    property.body = body;
    if property.body.is_none() {
        return Ok(property);
    }
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
