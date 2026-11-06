// SPDX-License-Identifier: Apache-2.0
//! Opaque side members are source bytes, never typed property values.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::{features::FeatureOperation, ArenaClass, ArenaClasses, CadIr};

use crate::native::{
    EntryRecord, ExtensionRecord, GuiPropertyRecord, GuiViewProviderRecord, ObjectRecord,
    ProductNodeRecord, PropertyRecord, StringTableRecord,
};

/// One source payload whose semantic content remains untyped.
pub(crate) struct OpaquePayload {
    pub(crate) name: String,
    pub(crate) classes: ArenaClasses,
}

pub(crate) fn retain_untyped(
    ir: &CadIr,
    sources: &crate::entry_sources::EntrySources<'_>,
    fidelity: &mut cadmpeg_ir::SourceFidelity,
    failed_sidecars: &BTreeSet<String>,
) -> Result<Vec<OpaquePayload>, CodecError> {
    let Some(native) = ir.native.namespace("fcstd") else {
        return Ok(Vec::new());
    };
    let entries = native.arena_as::<EntryRecord>("entries")?;
    let objects = native.arena_as::<ObjectRecord>("objects")?;
    let properties = native.arena_as::<PropertyRecord>("properties")?;
    let extensions = native.arena_as::<ExtensionRecord>("extensions")?;
    let products = native.arena_as::<ProductNodeRecord>("product_nodes")?;
    let gui_properties = native.arena_as::<GuiPropertyRecord>("gui_properties")?;
    let providers = native.arena_as::<GuiViewProviderRecord>("gui_view_providers")?;
    let mut represented = BTreeSet::from(["Document.xml".to_owned(), "GuiDocument.xml".to_owned()]);
    for payload in native.arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")? {
        if let Some(entry) = entries.iter().find(|entry| entry.id() == payload.entry) {
            represented.insert(entry.name().to_owned());
        }
    }
    for table in native.arena_as::<StringTableRecord>("string_tables")? {
        if table.entries().is_some() {
            represented.extend(table.source_entry);
        }
    }
    for map in native.arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")? {
        represented.extend(map.source_entry);
    }
    for property in &properties {
        let owner = extensions
            .iter()
            .find(|extension| extension.id == property.owner)
            .map_or(property.owner.as_str(), |extension| {
                extension.owner.as_str()
            });
        let attribute_holds = ir
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.native_ref.as_deref() == Some(property.id.as_str()));
        let typed = attribute_holds || match property.type_name.as_str() {
            "App::PropertyPlacementList" | "App::PropertyVectorList"
                if matches!(property.name.as_str(), "PlacementList" | "ScaleList") =>
            {
                products.iter().any(|product| product.object == owner)
            }
            "Mesh::PropertyMeshKernel" | "Points::PropertyPointKernel" => {
                crate::application_geometry::holds_property(ir, &property.type_name, owner)
            }
            "App::PropertyVectorList" | "App::PropertyFloatList"
                if matches!(
                    property.name.as_str(),
                    "Nodes" | "Spacings" | "Spacings2" | "SpacingPattern" | "SpacingPattern2"
                ) =>
            {
                ir.scope.design
                    && ir.model.features.iter().any(|feature| {
                        feature.native_ref.as_deref() == Some(owner)
                            && !matches!(
                                feature.evaluation.definition().operation(),
                                FeatureOperation::Native { .. }
                            )
                    })
            }
            "Part::PropertyGeometryList" | "Sketcher::PropertyConstraintList" => {
                ir.scope.design
                    && ir
                        .model
                        .sketches
                        .iter()
                        .any(|sketch| sketch.native_ref.as_deref() == Some(owner))
                    && ir.model.sketches.iter().filter(|sketch|sketch.native_ref.as_deref()==Some(owner)).all(|sketch| {
                        if property.type_name=="Part::PropertyGeometryList" {
                            ir.model.sketch_entities.iter().filter(|entity|entity.sketch==sketch.id).all(|entity| !matches!(entity.geometry.definition(),cadmpeg_ir::sketches::SketchGeometryDefinition::Native{..}))
                        } else {
                            ir.model.sketch_constraints.iter().filter(|constraint|constraint.sketch==sketch.id).all(|constraint| !matches!(constraint.definition.kind(),cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native{..}))
                        }
                    })
            }
            _ => false,
        };
        if typed {
            represented.extend(property.side_entries().iter().cloned());
        }
    }
    for property in &gui_properties {
        let family = match property.name.as_str() {
            "DiffuseColor" => "face",
            "LineColorArray" => "edge",
            "PointColorArray" => "vertex",
            "ShapeAppearance" => "shape-material",
            _ => continue,
        };
        let Some(provider) = providers
            .iter()
            .find(|provider| provider.id == property.owner)
        else {
            continue;
        };
        let key = cadmpeg_ir::ids::IdentityKey::encode_segment(&provider.name);
        let prefix = format!("fcstd:appearance:{family}#{key}:");
        let decoded_empty = if property.values.iter().any(|value| {
            if property.name == "ShapeAppearance" {
                value.tag == "MaterialList"
                    && value
                        .attributes
                        .get("version")
                        .map_or(Some(0), |value| value.parse::<u32>().ok())
                        .is_some_and(|version| version <= 3)
            } else {
                value.tag == "ColorList"
            }
        }) {
            match property.side_entries.as_slice() {
                [name] => match entries.iter().find(|entry| entry.name() == name) {
                    Some(entry) => sources.view(entry)?.window() == [0, 0, 0, 0],
                    None => false,
                },
                _ => false,
            }
        } else {
            false
        };
        if decoded_empty
            || ir
                .model
                .appearances
                .iter()
                .any(|appearance| appearance.id.as_str().starts_with(&prefix))
        {
            represented.extend(property.side_entries.iter().cloned());
        }
    }
    let mut opaque = BTreeMap::<(String, String), ArenaClasses>::new();
    for property in properties
        .iter()
        .filter(|property| crate::fidelity::opaque_property_payload(property))
    {
        let owner = extensions
            .iter()
            .find(|extension| extension.id == property.owner)
            .map_or(property.owner.as_str(), |extension| {
                extension.owner.as_str()
            });
        let class = if owner == crate::native::native_id("document", "0")
            || objects.iter().any(|object| {
                object.id().as_str() == owner && crate::design::is_design_kind(&object.type_name)
            }) {
            ArenaClass::Design
        } else {
            ArenaClass::Model
        };
        opaque.insert(
            (
                property.id.clone(),
                format!("inline property {}", property.id),
            ),
            ArenaClasses::from([class]),
        );
    }
    for entry in entries.iter().filter(|entry| {
        !represented.contains(entry.name()) && !failed_sidecars.contains(entry.name())
    }) {
        let mut classes = ArenaClasses::new();
        for property in properties.iter().filter(|property| {
            property
                .side_entries()
                .iter()
                .any(|name| name == entry.name())
        }) {
            let owner = extensions
                .iter()
                .find(|extension| extension.id == property.owner)
                .map_or(property.owner.as_str(), |extension| {
                    extension.owner.as_str()
                });
            let class = if property.type_name == "Part::PropertyPartShape" {
                ArenaClass::Model
            } else if owner == crate::native::native_id("document", "0")
                || objects.iter().any(|object| {
                    object.id().as_str() == owner
                        && crate::design::is_design_kind(&object.type_name)
                })
            {
                ArenaClass::Design
            } else {
                ArenaClass::Model
            };
            classes.insert(class);
        }
        if gui_properties.iter().any(|property| {
            property
                .side_entries
                .iter()
                .any(|name| name == entry.name())
        }) || std::path::Path::new(entry.name())
            .extension()
            .is_some_and(|extension| extension.as_encoded_bytes().eq_ignore_ascii_case(b"png"))
        {
            classes.insert(ArenaClass::Model);
        }
        if classes.is_empty() {
            classes.extend([ArenaClass::Model, ArenaClass::Design]);
        }
        if fidelity.retained_record(entry.id()).is_none() {
            let source = sources.view(entry)?;
            crate::fidelity::evidence(
                fidelity,
                entry.id(),
                entry.name(),
                cadmpeg_core::decode::u64_from_index(source.start()),
                source.window(),
                classes.clone(),
            )?;
        } else {
            fidelity
                .mark_untyped(entry.id(), classes.iter().copied())
                .map_err(CodecError::malformed)?;
        }
        opaque.insert((entry.id().to_owned(), entry.name().to_owned()), classes);
    }
    Ok(opaque
        .into_iter()
        .map(|((_id, name), classes)| OpaquePayload { name, classes })
        .collect())
}

#[cfg(test)]
mod tests;
