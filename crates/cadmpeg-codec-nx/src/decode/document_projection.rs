// SPDX-License-Identifier: Apache-2.0
//! Neutral homes for independently typed NX document records.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    attributes::{AttributeValue, SourceAttribute},
    products::{
        ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind,
        PrototypeReference,
    },
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
    let mut occurrences = Vec::new();
    let mut attributes = Vec::new();
    let mut appearances = Vec::new();
    let mut prototypes = BTreeMap::new();    if let Some(records) = namespace.arenas().get("fast_load_component_prototypes") {
        for record in records {
            let fields = record.fields()?;
            let name = fields
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty());
            let product_id = cadmpeg_ir::ids::ProductDefinitionId::compose(
                &cadmpeg_ir::identity_namespace!("nx", "document", "product"),
                cadmpeg_ir::identity_key!("prototype-").then(products.len()),
            );
            prototypes.insert(record.id().to_owned(), product_id.clone());
            products.push(ProductDefinition {
                id: product_id,
                kind: ProductDefinitionKind::Part,
                source_name: name.map(str::to_owned),
                label: None,
                description: None,
                part_number: None,
                bom_properties: BTreeMap::new(),
                bodies: Vec::new(),
                sketches: Vec::new(),
                native_ref: Some(record.id().to_owned()),
            });
        }
    }
    // The available fast-load grammar contains a prototype roster, not
    // placement matrices. Unknown placement is not an identity transform.
    if let Some(records) = namespace.arenas().get("fast_load_component_occurrences") {
        for record in records {
            let fields = record.fields()?;
            let prototype = fields
                .get("prototype")
                .and_then(Value::as_str)
                .and_then(|prototype| prototypes.get(prototype))
                .map(|definition| PrototypeReference::Local {
                    definition: definition.clone(),
                })
                .unwrap_or(PrototypeReference::Unresolved {});
            let ordinal = fields
                .get("ordinal")
                .and_then(Value::as_u64)
                .and_then(|ordinal| u32::try_from(ordinal).ok())
                .unwrap_or(0);
            let id = cadmpeg_ir::ids::OccurrenceId::compose(
                &cadmpeg_ir::identity_namespace!("nx", "document", "occurrence"),
                cadmpeg_ir::identity_key!("occurrence-").then(occurrences.len()),
            );
            occurrences.push(Occurrence {
                id,
                prototype,
                parent: OccurrenceParent::Root {},
                ordinal,
                transform: None,
                linked_prototype: None,
                name: None,
                visible: None,
                link: None,
                native_ref: Some(record.id().to_owned()),
            });
        }
    }
    occurrences.extend(external_occurrences(namespace)?);
    for (arena, records) in namespace.arenas() {
        for record in records {
            let fields = record.fields()?;
            match arena.as_str() {
                "fast_load_component_prototypes" | "fast_load_component_occurrences" => {}
                "display_jt_string_property_atoms" => {
                    if let Some(value) = fields.get("value").and_then(Value::as_str) {
                        let Some(target) = cadmpeg_ir::attributes::AttributeTarget::record_for(
                            &ir.model,
                            record.id(),
                        ) else {
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
                "display_jt_material_attributes" => {
                    let appearance = material(record.id(), &fields)?;
                    appearances.push(appearance);
                }
                _ => {}
            }
        }
    }
    ir.model.product_definitions.extend(products);
    ir.model.occurrences.extend(occurrences);
    ir.model.attributes.extend(attributes);
    ir.model.appearances.extend(appearances);
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count() - before),
        "admit NX document projections",
    )?;
    Ok(())
}

/// One neutral external occurrence per resolved child-part lane, or per
/// stored child path in an entry without a record lane. Strings selected by
/// a record lane are fragments of the joined child path, not children.
fn external_occurrences(
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<Vec<Occurrence>, CodecError> {
    let empty = Vec::new();
    let strings = namespace
        .arenas()
        .get("external_references")
        .unwrap_or(&empty);
    let records = namespace
        .arenas()
        .get("external_reference_records")
        .unwrap_or(&empty);
    let children = namespace
        .arenas()
        .get("external_reference_record_children")
        .unwrap_or(&empty);
    let mut record_entries = BTreeMap::new();
    for record in records {
        let fields = record.fields()?;
        if let Some(entry) = fields.get("source_entry").and_then(Value::as_str) {
            record_entries.insert(record.id().to_owned(), entry.to_owned());
        }
    }
    let mut paths = BTreeMap::new();
    let mut string_entries = BTreeMap::new();
    for record in strings {
        let fields = record.fields()?;
        if let Some(path) = fields.get("path").and_then(Value::as_str) {
            paths.insert(record.id().to_owned(), path.to_owned());
        }
        if let Some(entry) = fields.get("source_entry").and_then(Value::as_str) {
            string_entries.insert(record.id().to_owned(), entry.to_owned());
        }
    }
    let laned: BTreeSet<&str> = record_entries.values().map(String::as_str).collect();
    let mut occurrences = Vec::new();
    for child in children {
        let fields = child.fields()?;
        let (Some(record), Some(name), Some(directory)) = (
            fields.get("external_record").and_then(Value::as_str),
            fields
                .get("name_reference")
                .and_then(Value::as_str)
                .and_then(|id| paths.get(id)),
            fields
                .get("directory_reference")
                .and_then(Value::as_str)
                .and_then(|id| paths.get(id)),
        ) else {
            continue;
        };
        if !record_entries.contains_key(record) {
            continue;
        }
        let path = child_path(directory, name);
        occurrences.push(external_occurrence(
            occurrences.len(),
            path,
            Some(child.id().to_owned()),
        ));
    }
    if occurrences.is_empty() {
        for record in strings {
            let id = record.id().to_owned();
            let (Some(path), Some(entry)) = (paths.get(&id), string_entries.get(&id)) else {
                continue;
            };
            if laned.contains(entry.as_str()) {
                continue;
            }
            occurrences.push(external_occurrence(
                occurrences.len(),
                path.clone(),
                Some(id),
            ));
        }
    }
    Ok(occurrences)
}

/// Join a child directory and name into one stored path.
fn child_path(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_owned()
    } else if directory.ends_with('/') || directory.ends_with('\\') {
        format!("{directory}{name}")
    } else {
        format!("{directory}/{name}")
    }
}

fn external_occurrence(
    ordinal: usize,
    path: String,
    native_ref: Option<String>,
) -> Occurrence {
    let id = cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("nx", "document", "occurrence"),
        cadmpeg_ir::identity_key!("external-").then(ordinal),
    );
    Occurrence {
        id,
        prototype: PrototypeReference::External {
            document: ExternalDocument::path(path.clone()),
            object: None,
        },
        parent: OccurrenceParent::Root {},
        ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
        transform: None,
        linked_prototype: None,
        name: Some(path),
        visible: None,
        link: None,
        native_ref,
    }
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
