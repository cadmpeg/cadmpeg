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
    if let Some(namespace) = namespace {
        let entries = namespace.arena_as::<crate::native::EntryRecord>("entries")?;
        super::validate_entry_names(&entries)?;
    }
    for property in &native_properties {
        xml::validate(property)?;
    }
    names = super::names::prepare(ir, &native_objects, &native_properties);
    let mut entries = BTreeMap::<String, Vec<u8>>::new();
    let unreadable_owners: HashSet<_> = native_properties
        .iter()
        .filter(|property| matches!(property.body, crate::native::PropertyBody::Unreadable(_)))
        .map(|property| property.owner.as_str())
        .collect();
    // Source-only objects remain typed content. A neutral counterpart owns its
    // complete object and is regenerated below, so stale native properties can
    // never override a neutral edit.
    for original in &native_objects {
        if !super::native::retained(ir, original, &native_properties)? {
            continue;
        }
        if unreadable_owners.contains(original.id().as_str()) {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "object {} is omitted because its source properties are unreadable",
                original.name()
            )));
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
        let label = neutral_feature
            .and_then(|feature| feature.name.as_deref())
            .unwrap_or(original.name());
        let mut object = Object::new(name, &original.type_name, label);
        object
            .declaration_attributes
            .clone_from(&original.attributes);
        object.dependencies = original
            .dependencies
            .iter()
            .filter_map(|id| names.get(id).cloned())
            .collect();
        if let Some(feature) = neutral_feature {
            if let FeatureOperation::Native { kind, .. } =
                feature.evaluation.definition().operation()
            {
                kind.as_str().clone_into(&mut object.kind);
            }
            object.dependencies = feature
                .dependencies
                .iter()
                .filter_map(|dependency| names.get(dependency.as_str()).cloned())
                .collect();
        }
        objects.push(object);
    }
    for feature in &ir.model.features {
        if feature
            .native_ref
            .as_deref()
            .is_some_and(|native| unreadable_owners.contains(native))
        {
            continue;
        }
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
            object.set(
                "Suppressed",
                xml::scalar(
                    "Suppressed",
                    "App::PropertyBool",
                    "Bool",
                    u8::from(suppressed),
                ),
            );
        }
        objects.push(object);
    }
    // Native values are emitted from their typed tree. Retained XML and entry
    // bytes are not consulted. Every opaque payload has an explicit loss.
    let objects_names: HashSet<_> = objects.iter().map(|object| object.name.clone()).collect();
    for property in &native_properties {
        if crate::attributes::projection_held(ir)
            && !crate::attributes::consumed(ir, property, &native_objects)
            && crate::attributes::values(property).is_some()
        {
            // Canonical attributes own these values and their deletion.
            continue;
        }
        if !super::native::property_held(ir, property, &native_objects) {
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
                    || crate::property_policy::has_neutral_form(&property.name))
            {
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
            match xml::native_named(property, &names)? {
                Some(property_xml) => object.set(&property.name, property_xml),
                None => losses.push(FreecadLossCode::ExportNativePropertyNotTransferred.note(
                    format!(
                        "native property {} has no independently writable typed payload",
                        property.id
                    ),
                )),
            }
        } else if property.owner == crate::native::native_id("document", "0") {
            if let Some(property_xml) = xml::native(property)? {
                document_properties.insert(property.name.clone(), property_xml);
            } else {
                losses.push(
                    FreecadLossCode::ExportNativePropertyNotTransferred
                        .note(format!("document property {} is omitted", property.id)),
                );
            }
        }
    }
    super::native::operands(ir, &mut objects, &names, &native_properties, &mut losses)?;
    super::attributes::write(
        ir,
        &names,
        &mut objects,
        &mut document_properties,
        &mut losses,
        &native_properties,
    )?;
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
                object.set("Label", xml::string("Label", label));
            }
            if let Some(suppressed) = feature.suppressed {
                object.set(
                    "Suppressed",
                    xml::scalar(
                        "Suppressed",
                        "App::PropertyBool",
                        "Bool",
                        u8::from(suppressed),
                    ),
                );
            }
        }
    }
    let mut body_names = HashMap::new();
    let mut shape_groups = BTreeMap::<String, Vec<&cadmpeg_ir::topology::Body>>::new();
    for (index, body) in ir.model.bodies.iter().enumerate() {
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
        shape_groups.entry(name).or_default().push(body);
    }
    for (name, bodies) in shape_groups {
        let Some(bytes) = brep::serialize(ir, &bodies)? else {
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
        let file = format!("{name}.brp");
        entries.insert(file.clone(), bytes);
        let object = objects
            .iter_mut()
            .find(|object| object.name == name)
            .ok_or_else(|| CodecError::malformed("body owner is absent"))?;
        object.set(
            "Shape",
            xml::property(
                "Shape",
                "Part::PropertyPartShape",
                &xml::element("Part", &[("file", file)]),
            ),
        );
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
    if let Some(bytes) = brep::unowned(ir, &mut losses)? {
        let mut name = "FreeGeometry".to_owned();
        while objects.iter().any(|object| object.name == name) {
            name.push('_');
        }
        let file = format!("{name}.brp");
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
        objects.push(object);
    }
    design::sketches(ir, &mut objects, &mut names, &mut losses)?;
    super::parameters::prepare(ir, &mut objects, &mut names)?;
    design::spreadsheets(ir, &mut objects, &mut names)?;
    super::parameters::write(ir, &mut objects, &names, &mut losses)?;
    super::products::write(ir, &mut objects, &mut names, &body_names, &mut losses)?;
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
                object.set("Label", xml::string("Label", label));
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
    let gui =
        super::presentation::write(ir, &names, &body_names, &objects, &mut entries, &mut losses)?;
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
    let mut dispositions = super::coverage::complete(ir, &mut losses);
    super::coverage::xml_reductions(&mut dispositions, document_reduced, gui_reduced);
    let mut body = ExportBody::synthesized(bytes, ir, ArenaCoverage::Declared(dispositions));
    body.native_dispositions = native_dispositions;
    body.losses = losses;
    Ok(body)
}
