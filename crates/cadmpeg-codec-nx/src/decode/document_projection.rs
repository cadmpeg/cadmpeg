// SPDX-License-Identifier: Apache-2.0
//! Neutral homes for independently typed NX document records.

use std::collections::BTreeMap;

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    attributes::{AttributeValue, SourceAttribute},
    products::{ProductDefinition, ProductDefinitionKind},
    report::loss::LossNote,
    CadIr,
};
use serde_json::{Map, Value};

use crate::loss::NxLossCode;

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(namespace) = ir.native.namespace("nx") else {
        return Ok(());
    };
    let before = ir.model.entity_count();
    let mut products = Vec::new();
    let mut attributes = Vec::new();
    let mut appearances = Vec::new();
    for (arena, records) in namespace.arenas() {
        for record in records {
            let fields = record.fields()?;
            match arena.as_str() {
                "fast_load_component_prototypes" => {
                    let Some(name) = fields
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|name| !name.trim().is_empty())
                    else {
                        losses.push(NxLossCode::NeutralRecordUnprojected.note(format!(
                            "Component prototype {} has no decoded name.",
                            record.id()
                        )));
                        continue;
                    };
                    let product_id = cadmpeg_ir::ids::ProductDefinitionId::compose(
                        &cadmpeg_ir::identity_namespace!("nx", "document", "product"),
                        cadmpeg_ir::identity_key!("prototype-").then(products.len()),
                    );
                    products.push(ProductDefinition {
                        id: product_id,
                        kind: ProductDefinitionKind::Part,
                        source_name: Some(name.to_owned()),
                        label: None,
                        description: None,
                        part_number: None,
                        bom_properties: BTreeMap::new(),
                        bodies: Vec::new(),
                        sketches: Vec::new(),
                        native_ref: Some(record.id().to_owned()),
                    });
                }
                "fast_load_component_occurrences" => {
                    // The available fast-load grammar contains a prototype
                    // roster, not placement matrices. Unknown placement is not
                    // an identity transform.
                    losses.push(NxLossCode::NeutralRecordUnprojected.note(format!(
                        "Component occurrence {} names prototype {}, but its placement is not decoded; its typed roster record is held without inventing a neutral placement.",
                        record.id(), fields.get("prototype").and_then(Value::as_str).unwrap_or("<unresolved>")
                    )));
                }
                "display_jt_string_property_atoms" => {
                    if let Some(value) = fields.get("value").and_then(Value::as_str) {
                        let Some(target) =
                            cadmpeg_ir::attributes::AttributeTarget::record_for(
                                &ir.model,
                                record.id(),
                            )
                        else {
                            continue;
                        };
                        let attribute_id = cadmpeg_ir::ids::AttributeId::compose(
                            &cadmpeg_ir::identity_namespace!("nx", "document", "attribute"),
                            cadmpeg_ir::identity_key!("jt-string-").then(attributes.len()),
                        );
                        attributes.push(SourceAttribute::new(
                            ctx,
                            attribute_id,
                            Some(record.id().to_owned()),
                            target,
                            "JT string property".into(),
                            vec![AttributeValue::String(value.to_owned())],
                        )?);
                    } else {
                        losses.push(NxLossCode::NeutralRecordUnprojected.note(format!(
                            "JT property atom {} has no decoded string value.",
                            record.id()
                        )));
                    }
                }
                "material_texture_assets" | "material_texture_catalog_entries" => {
                    losses.push(NxLossCode::NeutralRecordUnprojected.note(format!(
                        "Material texture record {} ({arena}) retains decoded resource/catalog fields, but no decoded shader slot, material assignment or texture mapping supplies the required neutral appearance fields; no default mapping was invented.", record.id()
                    )));
                }
                "display_jt_material_attributes" => {
                    let appearance = material(record.id(), &fields)?;
                    appearances.push(appearance);
                }
                "external_references"
                | "external_reference_records"
                | "external_reference_indexed_records"
                | "external_reference_empty_records"
                | "external_reference_tail_reference_pairs"
                | "external_reference_record_string_uses"
                | "external_reference_record_children" => {
                    losses.push(NxLossCode::NeutralRecordUnprojected.note(format!(
                        "External-reference record {} ({arena}) has no decoded occurrence placement; its typed reference fields remain held.", record.id()
                    )));
                }
                _ => {}
            }
        }
    }
    ir.model.product_definitions.extend(products);
    ir.model.attributes.extend(attributes);
    ir.model.appearances.extend(appearances);
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count() - before),
        "admit NX document projections",
    )?;
    Ok(())
}

fn material(
    id: &str,
    fields: &Map<String, Value>,
) -> Result<cadmpeg_ir::appearance::Appearance, CodecError> {
    let mut properties = BTreeMap::new();
    for field in ["ambient", "specular", "emission"] {
        if let Some(components) = fields.get(field).and_then(Value::as_array) {
            for (ordinal, component) in components.iter().enumerate() {
                if let Some(component) = component
                    .as_f64()
                    .and_then(cadmpeg_ir::scalar::FiniteReal::new)
                {
                    properties.insert(
                        cadmpeg_core::text::NonBlankString::new(format!("{field}.{ordinal}"))
                            .ok_or_else(|| {
                                CodecError::malformed("invalid JT material property name")
                            })?,
                        component,
                    );
                }
            }
        }
    }
    if let Some(shininess) = fields
        .get("shininess")
        .and_then(Value::as_f64)
        .and_then(cadmpeg_ir::scalar::FiniteReal::new)
    {
        properties.insert(
            cadmpeg_core::text::NonBlankString::new("shininess")
                .ok_or_else(|| CodecError::malformed("invalid JT shininess property name"))?,
            shininess,
        );
    }
    let diffuse = fields
        .get("diffuse")
        .and_then(Value::as_array)
        .and_then(|values| {
            let [r, g, b, a] = values.as_slice() else {
                return None;
            };
            cadmpeg_ir::topology::Color::new(
                cadmpeg_core::convert::f32_from_f64(r.as_f64()?)?,
                cadmpeg_core::convert::f32_from_f64(g.as_f64()?)?,
                cadmpeg_core::convert::f32_from_f64(b.as_f64()?)?,
                cadmpeg_core::convert::f32_from_f64(a.as_f64()?)?,
            )
        });
    Ok(cadmpeg_ir::appearance::Appearance {
        native_ref: Some(id.to_owned()),
        id: cadmpeg_ir::ids::AppearanceId::mint(format!("{id}:appearance"))
            .map_err(CodecError::malformed)?,
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some("JTMaterial".into()),
        category: None,
        base_color: diffuse,
        properties,
        textures: Vec::new(),
    })
}

#[cfg(test)]
mod tests;
