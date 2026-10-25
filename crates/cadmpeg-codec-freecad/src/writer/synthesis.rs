// SPDX-License-Identifier: Apache-2.0
//! `FCStd` archive synthesis from the current document state.

use super::{brep, design, xml};
use crate::loss::FreecadLossCode;
use crate::native::{ObjectRecord, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{ArenaCoverage, ExportBody};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureOperation;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Cursor, Write};
use xml::Object;

pub(super) fn write(ir: &CadIr, schema: &str) -> Result<ExportBody, CodecError> {
    let native_only = !ir.scope.model;
    let projected = super::native_geometry::project(ir)?;
    let (ir, mut losses) = projected.as_ref().map_or_else(
        || (ir, Vec::new()),
        |(projected, losses)| (projected, losses.clone()),
    );
    let mut native_dispositions = super::dispositions::initial();
    let mut objects = Vec::new();
    let mut names;
    let mut native_names = HashSet::new();
    let mut document_properties = BTreeMap::new();
    let namespace = ir.native.namespace("fcstd");
    let native_objects = namespace
        .map(|namespace| namespace.arena_as::<ObjectRecord>("objects"))
        .transpose()?
        .unwrap_or_default();
    let native_properties = namespace
        .map(|namespace| namespace.arena_as::<PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
    let native_payloads = namespace
        .map(|namespace| crate::brep::records::payloads_untracked(namespace))
        .transpose()?
        .unwrap_or_default();
    let gui_properties = namespace
        .map(|namespace| namespace.arena_as::<crate::native::GuiPropertyRecord>("gui_properties"))
        .transpose()?
        .unwrap_or_default();
    if let Some(namespace) = namespace {
        let entries = namespace.arena_as::<crate::native::EntryRecord>("entries")?;
        super::validate_entry_names(&entries)?;
    }
    for property in &native_properties {
        xml::validate(property)?;
    }
    names = super::names::prepare(ir, &native_objects, &native_properties, &native_payloads);
    let mut entries = BTreeMap::<String, Vec<u8>>::new();
    // Source-only objects remain typed content. A neutral counterpart owns its
    // complete object and is regenerated below, so stale native properties can
    // never override a neutral edit.
    for original in &native_objects {
        if !super::native::retained(ir, original, &native_properties)? {
            continue;
        }
        let neutral_feature = ir
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(original.id().as_str()));
        let represented = neutral_feature.is_some_and(|feature| {
            !matches!(
                feature.evaluation.definition().operation(),
                FeatureOperation::Native { .. }
            )
        }) || ir
            .model
            .sketches
            .iter()
            .any(|sketch| sketch.native_ref.as_deref() == Some(original.id().as_str()))
            || ir.model.product_definitions.iter().any(|definition| {
                definition.native_ref.as_deref() == Some(original.id().as_str())
                    && definition.kind != cadmpeg_ir::products::ProductDefinitionKind::Object
            })
            || (matches!(
                original.type_name.as_str(),
                "App::Link" | "App::LinkElement"
            ) && ir.model.occurrences.iter().any(|occurrence| {
                occurrence.native_ref.as_deref() == Some(original.id().as_str())
            }));
        if represented
            || (ir.scope.design
                && crate::design::is_design_object(&original.type_name)
                && !crate::application_geometry::projects_object(original)
                && neutral_feature.is_none())
        {
            continue;
        }
        let name = names
            .get(original.id().as_str())
            .cloned()
            .unwrap_or_else(|| original.name().to_owned());
        native_names.insert(name.clone());
        names.insert(original.id().as_str().to_owned(), name.clone());
        let mut object = Object::from_native(name, &original.type_name);
        object
            .declaration_attributes
            .clone_from(&original.attributes);
        object.dependencies = original
            .dependencies
            .iter()
            .filter_map(|id| names.get(id).cloned())
            .collect();
        if let Some(feature) = neutral_feature {
            object.dependencies = feature
                .dependencies
                .iter()
                .filter_map(|dependency| names.get(dependency.as_str()).cloned())
                .collect();
            if let FeatureOperation::Native { kind, .. } =
                feature.evaluation.definition().operation()
            {
                kind.as_str().clone_into(&mut object.kind);
            }
        }
        objects.push(object);
    }
    for feature in &ir.model.features {
        if let Some(name) = names
            .get(feature.native_ref.as_deref().unwrap_or_default())
            .filter(|name| native_names.contains(*name))
            .cloned()
        {
            names.insert(feature.id.as_str().to_owned(), name);
            continue;
        }
        let name = names
            .get(feature.id.as_str())
            .cloned()
            .ok_or_else(|| CodecError::malformed("feature has no output identity"))?;
        names.insert(feature.id.as_str().to_owned(), name.clone());
        if let Some(native_ref) = &feature.native_ref {
            names.insert(native_ref.clone(), name.clone());
        }
        let mut object = Object::new(
            name,
            "PartDesign::Feature",
            feature.name.as_deref().unwrap_or(feature.id.as_str()),
        );
        match design::features::write(ir, feature, &mut object, &names) {
            Ok(output) => {
                losses.extend(output.losses);
                objects.extend(output.objects);
                entries.extend(output.entries);
            }
            Err(CodecError::UnsupportedContent(reason)) => {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "feature {} construction is omitted: {reason}",
                    feature.id
                )));
                continue;
            }
            Err(error) => return Err(error),
        }
        if let Some(suppressed) = feature.suppressed {
            object.set_boolean("Suppressed", suppressed);
        }
        objects.push(object);
    }
    let numeric_attributes =
        super::attributes::prepare_numeric(ir, &native_properties, &mut entries, &mut losses)?;
    // Native values are emitted from their typed tree. Retained XML and entry
    // bytes are not consulted. Every opaque payload has an explicit loss.
    let objects_names: HashSet<_> = objects.iter().map(|object| object.name.clone()).collect();
    for property in &native_properties {
        if numeric_attributes.claimed.contains(&property.id) {
            continue;
        }
        if !super::native::property_held(ir, property, &native_objects) {
            continue;
        }
        if crate::attributes::projection_held(ir)
            && crate::attributes::target(ir, &property.owner)?.is_some()
            && !crate::attributes::consumed(ir, property, &native_objects)
            && (crate::attributes::values(property).is_some()
                || crate::attributes::numeric_kind(&property.type_name))
        {
            // A deleted canonical attribute intentionally omits its native property.
            if ir
                .model
                .attributes
                .iter()
                .any(|attribute| attribute.native_ref.as_deref() == Some(property.id.as_str()))
            {
                note_omitted_value_parts(property, &mut losses)?;
            }
            continue;
        }
        if property.type_name == "Part::PropertyPartShape" {
            continue;
        }
        let destination = names
            .get(&property.owner)
            .and_then(|name| objects.iter_mut().find(|object| object.name == *name));
        if let Some(object) = destination {
            let neutral_owned = !native_names.contains(&object.name);
            if neutral_owned
                && (object.properties.contains_key(&property.name)
                    || crate::attributes::consumed(ir, property, &native_objects))
            {
                if object.properties.contains_key(&property.name) {
                    note_omitted_value_parts(property, &mut losses)?;
                }
                continue;
            }
            if property
                .links()
                .iter()
                .flatten()
                .filter(|link| link.document().is_none())
                .filter_map(|link| link.object())
                .any(|target| {
                    names
                        .get(target)
                        .is_none_or(|name| !objects_names.contains(name))
                })
            {
                losses.push(
                    FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                        "native property {} refers to an omitted object",
                        property.id
                    )),
                );
                continue;
            }
            let payload_members = super::property_payloads::write(
                namespace,
                &property.id,
                &property.type_name,
                property.values(),
                &mut entries,
                &mut losses,
                &mut native_dispositions,
            )?;
            match xml::native_named_payloads(property, &names, &payload_members)? {
                Some(property_xml) => {
                    note_omitted_value_parts(property, &mut losses)?;
                    object.set(&property.name, property_xml);
                }
                None => {
                    if property.body.is_none()
                        || xml::has_omitted_value_parts(property)
                        || has_unassociated_reference_parts(property)?
                    {
                        losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                            "property {} ({}) on object {} is omitted because its value is not held",
                            property.id, property.name, property.owner,
                        )));
                    } else {
                        losses.push(FreecadLossCode::ExportNativePropertyNotTransferred.note(
                            format!(
                                "native property {} has no independently writable typed payload",
                                property.id,
                            ),
                        ));
                    }
                }
            }
        } else if property.owner == crate::native::native_id("document", "0") {
            if property.body.is_none() {
                losses.push(
                    cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                        "document property {} has no decoded body and is omitted",
                        property.id
                    )),
                );
            } else if let Some(property_xml) = {
                let payload_members = super::property_payloads::write(
                    namespace,
                    &property.id,
                    &property.type_name,
                    property.values(),
                    &mut entries,
                    &mut losses,
                    &mut native_dispositions,
                )?;
                xml::native_named_payloads(property, &names, &payload_members)?
            } {
                note_omitted_value_parts(property, &mut losses)?;
                document_properties.insert(property.name.clone(), property_xml);
            } else {
                losses.push(if has_unassociated_reference_parts(property)? {
                    cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                        "document property {} is omitted because its reference carrier has no proven native slot", property.id
                    ))
                } else {
                    FreecadLossCode::ExportNativePropertyNotTransferred
                        .note(format!("document property {} is omitted", property.id))
                });
            }
        }
    }
    super::native::operands(ir, &mut objects, &names, &native_properties, &mut losses)?;
    // Neutral display names and suppression always win over typed source values.
    for feature in &ir.model.features {
        if let Some(object) = names
            .get(feature.id.as_str())
            .and_then(|name| objects.iter_mut().find(|object| object.name == *name))
        {
            if feature.needs_recompute {
                object
                    .declaration_attributes
                    .insert("Touched".into(), "1".into());
            } else {
                object.declaration_attributes.remove("Touched");
            }
            if let Some(label) = &feature.name {
                object.set_text("Label", label);
            }
            if let Some(suppressed) = feature.suppressed {
                object.set_boolean("Suppressed", suppressed);
            }
        }
    }
    let mut body_names = HashMap::new();
    let mut shape_groups =
        BTreeMap::<(String, String, Option<usize>), Vec<&cadmpeg_ir::topology::Body>>::new();
    for (index, body) in ir.model.bodies.iter().enumerate() {
        if body.native_ref.as_ref().is_some_and(|native| {
            native_payloads.iter().any(|payload| {
                &payload.id == native
                    && gui_properties
                        .iter()
                        .any(|property| property.id == payload.property)
            })
        }) {
            continue;
        }
        let proposed = names
            .get(body.id.as_str())
            .cloned()
            .unwrap_or_else(|| format!("Shape{index}"));
        let name = if objects.iter().any(|object| object.name == proposed) {
            proposed
        } else {
            format!("Shape{index}")
        };
        body_names.insert(body.id.as_str().to_owned(), name.clone());
        let payload = body
            .native_ref
            .as_ref()
            .and_then(|native| native_payloads.iter().find(|payload| &payload.id == native));
        let property = payload.and_then(|payload| {
            native_properties
                .iter()
                .find(|property| property.id == payload.property)
        });
        let property_name = property.map_or("Shape", |property| property.name.as_str());
        let list_ordinal = property
            .filter(|property| property.type_name == "Part::PropertyTopoShapeList")
            .and_then(|property| {
                native_payloads
                    .iter()
                    .filter(|payload| payload.property == property.id)
                    .position(|candidate| payload.is_some_and(|payload| payload.id == candidate.id))
            });
        shape_groups
            .entry((name, property_name.to_owned(), list_ordinal))
            .or_default()
            .push(body);
    }
    let mut emitted_generated_meshes = std::collections::HashSet::new();
    let mut emitted_polygon_homes = HashMap::new();
    for ((name, property_name, list_ordinal), bodies) in shape_groups {
        let Some(serialized) = brep::serialize(ir, &bodies)? else {
            for body in bodies {
                losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "body {} is omitted because a required carrier has no OCCT text encoding",
                    body.id
                )));
                body_names.remove(body.id.as_str());
            }
            continue;
        };
        if !objects.iter().any(|object| object.name == name) {
            objects.push(Object::new(
                name.clone(),
                "Part::Feature",
                bodies[0].name.as_deref().unwrap_or(bodies[0].id.as_str()),
            ));
        }
        let file = if let Some(ordinal) = list_ordinal {
            format!(
                "{name}.{}.{ordinal}.brp",
                cadmpeg_ir::ids::IdentityKey::encode_segment(&property_name)
            )
        } else if property_name == "Shape" {
            format!("{name}.brp")
        } else {
            format!(
                "{name}.{}.brp",
                cadmpeg_ir::ids::IdentityKey::encode_segment(&property_name)
            )
        };
        brep::occurrences::assign_names(&serialized.topology, &body_names, &mut names, &mut losses);
        emitted_generated_meshes.extend(serialized.meshes);
        emitted_polygon_homes.insert(file.clone(), serialized.polygon_homes);
        let bytes = serialized.bytes;
        entries.insert(file.clone(), bytes);
        let object = objects
            .iter_mut()
            .find(|object| object.name == name)
            .ok_or_else(|| CodecError::malformed("body owner is absent"))?;
        object.set(
            &property_name,
            xml::property(
                &property_name,
                "Part::PropertyPartShape",
                &xml::element("Part", &[("file", file)]),
            ),
        );
        object.mark_geometry_written();
        object.properties.remove("CadmpegBodyNames");
        if bodies.iter().any(|body| body.name.is_some()) {
            let values = bodies
                .iter()
                .map(|body| {
                    Ok(xml::element(
                        "String",
                        &[(
                            "value",
                            serde_json::to_string(&body.name).map_err(CodecError::malformed)?,
                        )],
                    ))
                })
                .collect::<Result<String, CodecError>>()?;
            object.set(
                "CadmpegBodyNames",
                xml::property(
                    "CadmpegBodyNames",
                    "App::PropertyStringList",
                    &format!(
                        "<StringList count=\"{}\">{values}</StringList>",
                        bodies.len()
                    ),
                )
                .replacen("<Property ", "<Property group=\"CADIR\" hide=\"1\" ", 1),
            );
        }
        super::body_styles::preserve(object, &bodies);
        // FreeCAD persists the application's placement and the BRep root
        // location as coupled lanes. Restoring the property does not compose
        // the same placement twice. Keep canonical construction placements and
        // typed native placements; body-only objects use their BRep root.
    }
    let payload_owners = native_payloads
        .iter()
        .filter(|payload| {
            gui_properties
                .iter()
                .any(|property| property.id == payload.property)
                || native_properties
                    .iter()
                    .find(|property| property.id == payload.property)
                    .and_then(|property| names.get(&property.owner))
                    .is_some_and(|name| objects.iter().any(|object| &object.name == name))
        })
        .map(|payload| payload.id.clone())
        .collect();
    if let Some(serialized) = brep::unowned(ir, &mut losses, &payload_owners)? {
        emitted_generated_meshes.extend(serialized.meshes);
        let bytes = serialized.bytes;
        let mut name = "FreeGeometry".to_owned();
        while objects.iter().any(|object| object.name == name) {
            name.push('_');
        }
        let file = format!("{name}.brp");
        emitted_polygon_homes.insert(file.clone(), serialized.polygon_homes);
        entries.insert(file.clone(), bytes);
        let mut object = Object::new(name, "Part::Feature", "Independent geometry");
        object.set(
            "Shape",
            xml::property(
                "Shape",
                "Part::PropertyPartShape",
                &xml::element("Part", &[("file", file)]),
            ),
        );
        object.mark_geometry_written();
        objects.push(object);
    }
    let mut emitted_meshes = brep::native_tables::write(
        ir,
        &mut objects,
        &names,
        &native_properties,
        &native_payloads,
        &mut entries,
        &emitted_polygon_homes,
        native_only,
        &mut native_dispositions,
        &mut losses,
    )?;
    emitted_meshes.extend(emitted_generated_meshes);
    design::sketches(ir, &mut objects, &mut names, &mut losses)?;
    super::parameters::prepare(ir, &mut objects, &mut names)?;
    design::spreadsheets(ir, &mut objects, &mut names)?;
    super::products::write(ir, &mut objects, &mut names, &body_names, &mut losses)?;
    super::parameters::write(ir, &mut objects, &names, &mut losses)?;
    super::attributes::write(
        ir,
        &names,
        &mut objects,
        &mut document_properties,
        &mut losses,
        &native_properties,
        &numeric_attributes,
        &mut entries,
        &mut native_dispositions,
    )?;
    emitted_meshes.extend(super::application_payloads::write(
        ir,
        &mut objects,
        &names,
        &native_properties,
        &mut entries,
        &mut native_dispositions,
        &mut losses,
    )?);
    // Product and feature labels are independent neutral fields even when
    // FreeCAD stores both on the same object. Preserve the product label in a
    // dynamic property while the visible object Label follows the feature.
    for feature in &ir.model.features {
        if let Some(label) = &feature.name {
            if let Some(object) = names
                .get(feature.id.as_str())
                .and_then(|name| objects.iter_mut().find(|object| object.name == *name))
            {
                if let Some(definition) = ir
                    .model
                    .product_definitions
                    .iter()
                    .find(|definition| names.get(definition.id.as_str()) == Some(&object.name))
                {
                    let value =
                        serde_json::to_string(&definition.label).map_err(CodecError::malformed)?;
                    object.set(
                        "CadmpegProductLabel",
                        xml::string("CadmpegProductLabel", &value).replacen(
                            "<Property ",
                            "<Property group=\"CADIR\" hide=\"1\" ",
                            1,
                        ),
                    );
                }
                object.set_text("Label", label);
            }
        }
    }
    super::ordinals::preserve(ir, &mut objects, &names)?;
    super::native::attachments(
        ir,
        &mut objects,
        &names,
        &body_names,
        &mut losses,
        &mut native_dispositions,
    )?;
    super::native::extensions(
        ir,
        &mut objects,
        &names,
        &native_properties,
        &mut losses,
        &mut native_dispositions,
    )?;
    let dictionaries = super::native::maps(
        ir,
        &mut objects,
        &names,
        &native_properties,
        native_only,
        &mut entries,
        &mut losses,
        &mut native_dispositions,
    )?;
    super::synthesis_graph::close(&mut objects, &mut losses)?;
    let document = xml::document(&objects, &document_properties, schema);
    let document = if dictionaries.is_empty() {
        document
    } else {
        document.replacen(
            "<StringHasher saveall=\"0\" threshold=\"0\" count=\"0\"></StringHasher>",
            &dictionaries,
            1,
        )
    };
    let document_reduced = xml::report_reductions(&document, "Document.xml", &mut losses)?;
    entries.insert("Document.xml".into(), document.into_bytes());
    let mut gui_has_geometry = false;
    let gui = super::presentation::write(
        ir,
        &names,
        &body_names,
        &objects,
        &mut entries,
        &mut losses,
        &mut native_dispositions,
        &mut emitted_meshes,
        &mut gui_has_geometry,
        native_only,
    )?;
    if !gui_has_geometry
        && objects.iter().all(Object::geometry_empty)
        && document_properties.values().all(|property| {
            roxmltree::Document::parse(property).is_ok_and(|xml| {
                let root = xml.root_element();
                let mut values = root.children().filter(roxmltree::Node::is_element);
                root.has_tag_name("Property")
                    && root.attribute("type") == Some("App::PropertyString")
                    && values.next().is_some_and(|value| {
                        value.has_tag_name("String")
                            && !value.children().any(|child| child.is_element())
                            && !value.attributes().any(|attribute| {
                                matches!(attribute.name(), "file" | "File")
                                    && !attribute.value().is_empty()
                            })
                    })
                    && values.next().is_none()
            })
        })
    {
        losses.push(
            cadmpeg_ir::codec::write::ExportLossCode::OutputEmpty
                .note("the emitted FCStd document has no geometry"),
        );
    }
    let gui_reduced = xml::report_reductions(&gui, "GuiDocument.xml", &mut losses)?;
    super::dispositions::gui(ir, &names, &gui, &mut native_dispositions)?;
    if !gui.is_empty() {
        entries.insert("GuiDocument.xml".into(), gui.into_bytes());
    }
    super::dispositions::emitted(
        ir,
        &objects,
        &native_properties,
        &names,
        &document_properties,
        &entries,
        &mut native_dispositions,
    )?;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    // FreeCAD opens the first member as its document stream before resolving
    // named side entries. Keep Document.xml first even when a BRep name sorts
    // before it lexically.
    let mut entries = entries.into_iter().collect::<Vec<_>>();
    entries.sort_by(|(left, _), (right, _)| {
        (left != "Document.xml", left).cmp(&(right != "Document.xml", right))
    });
    for (name, bytes) in entries {
        archive
            .start_file(name, options)
            .map_err(std::io::Error::other)?;
        archive.write_all(&bytes)?;
    }
    let bytes = archive
        .finish()
        .map_err(std::io::Error::other)?
        .into_inner();
    for curve in &ir.model.curves {
        if curve.geometry.procedural_construction().is_some()
            && super::geometry::located_curve(ir, curve)
                .is_some_and(|(row, _)| !row.starts_with("8 ") && !row.starts_with("9 "))
        {
            losses.push(FreecadLossCode::ExportProceduralReduced.note(format!(
                "curve {} is written through its current solved carrier",
                curve.id
            )));
        }
    }
    for surface in &ir.model.surfaces {
        if surface.geometry.procedural_construction().is_some()
            && super::geometry::located_surface(ir, surface).is_some_and(|(row, _)| {
                !["6 ", "7 ", "10 ", "11 "]
                    .iter()
                    .any(|prefix| row.starts_with(prefix))
            })
        {
            losses.push(FreecadLossCode::ExportProceduralReduced.note(format!(
                "surface {} is written through its current solved carrier",
                surface.id
            )));
        }
    }
    let mut dispositions = super::coverage::complete(ir, &emitted_meshes, &mut losses);
    super::coverage::xml_reductions(&mut dispositions, document_reduced, gui_reduced);
    let mut body = ExportBody::synthesized(bytes, ir, ArenaCoverage::Declared(dispositions));
    body.native_dispositions = native_dispositions;
    body.losses = losses;
    Ok(body)
}

fn has_unassociated_reference_parts(
    property: &crate::native::PropertyRecord,
) -> Result<bool, CodecError> {
    match &property.body {
        Some(crate::native::PropertyBody::Persisted { values, links, .. }) => {
            super::xml::expression_metadata_omitted(&property.type_name, values, links)
        }
        _ => Ok(false),
    }
}

/// A partial native value tree is writable only after omitting its missing parts.
fn note_omitted_value_parts(
    property: &crate::native::PropertyRecord,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    if let Some(crate::native::PropertyBody::Persisted { values, .. }) = &property.body {
        if !values.has_missing()
            && !super::xml::expression_metadata_omitted(
                &property.type_name,
                values,
                property.links(),
            )?
        {
            return Ok(());
        }
        let missing: Vec<_> = (0..values.slot_count())
            .filter(|order| {
                values
                    .binary_search_by_key(order, |value| value.order)
                    .is_err()
            })
            .collect();
        losses.push(
            cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                "property {} ({}) omits undecoded value or unassociated reference metadata at source positions {:?}",
                property.id, property.name, missing,
            )),
        );
    }
    Ok(())
}
