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

/// Whether the emitted output still names an archive entry: a `file`/`File`
/// reference in an object or document property, an extension declaration, or
/// the rendered GUI document. This mirrors the decode's side-entry parsing
/// (`side_entries` collects the same attributes), so a regenerated consumer
/// under a new file name no longer counts as a reference to the old entry.
fn entry_referenced(
    name: &str,
    objects: &[xml::Object],
    document_properties: &BTreeMap<String, String>,
    gui: &str,
) -> bool {
    let file = format!("file=\"{name}\"");
    let file_upper = format!("File=\"{name}\"");
    objects.iter().any(|object| {
        object.properties.values().any(|property| {
            property.contains(file.as_str()) || property.contains(file_upper.as_str())
        }) || object.extensions.iter().any(|extension| {
            extension.contains(file.as_str()) || extension.contains(file_upper.as_str())
        })
    }) || document_properties
        .values()
        .any(|property| property.contains(file.as_str()) || property.contains(file_upper.as_str()))
        || gui.contains(file.as_str())
        || gui.contains(file_upper.as_str())
}

pub(super) fn write(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    schema: &str,
) -> Result<ExportBody, CodecError> {
    let projected = super::native_geometry::project(ctx, ir)?;
    let (ir, mut losses) = projected.as_ref().map_or_else(
        || (ir, Vec::new()),
        |(projected, losses)| (projected, losses.clone()),
    );
    losses.extend(super::synthesis_graph::admission(ir, ctx)?);
    let current_topology = crate::topology_transfer::current_bindings(ctx, ir)?;
    let root_kinds = brep::RootKinds::new(ctx, ir, &current_topology)?;
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
    // Retained opaque entry bytes replay verbatim when the writer emits
    // their entry. Their names are the side-entry availability for typed
    // property replay below.
    let mut retained: HashMap<String, Vec<u8>> = HashMap::new();
    if let Some(namespace) = namespace {
        if let Ok(records) = namespace.arena_as::<crate::native::EntryRecord>("entries") {
            for record in records {
                if let Some(bytes) = record.payload() {
                    retained.insert(record.name().to_owned(), bytes.to_vec());
                }
            }
        }
    }
    let available: HashSet<String> = retained.keys().cloned().collect();
    for property in &native_properties {
        xml::validate(property, &available)?;
    }
    names = super::names::prepare(ir, &native_objects, &native_properties);
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
        // An opaque construction (native passthrough, stored geometry, or a
        // base feature) has no regenerable content: the native object is the
        // complete record and replays, keeping its links, placement, and
        // declaration order, which the ordinal ranks derive from (#238 §5.5).
        let represented = neutral_feature.is_some_and(|feature| {
            !matches!(
                feature.evaluation.definition().operation(),
                FeatureOperation::Native { .. }
                    | FeatureOperation::StoredGeometry { .. }
                    | FeatureOperation::BaseFeature { .. }
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
            if feature.needs_recompute && schema == "2" {
                // The schema 2 grammar carries no recompute marker, so the
                // writer reports the stored staleness instead of dropping
                // it (#209 §2.5).
                losses.push(
                    cadmpeg_ir::codec::write::ExportLossCode::EvaluationStale.note(format!(
                        "feature {} needs recompute and the schema 2 grammar carries no recompute marker",
                        feature.id
                    )),
                );
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
        match design::features::write(ir, feature, &mut object, &names, schema) {
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
    // Native values are emitted from their typed tree. Retained XML and entry
    // bytes are not consulted. Every opaque payload has an explicit loss.
    let objects_names: HashSet<_> = objects.iter().map(|object| object.name.clone()).collect();
    for property in &native_properties {
        if crate::attributes::projection_held(ir)
            && !crate::attributes::consumed(ir, property, &native_objects)
            && crate::attributes::values(property).is_some()
            && ir
                .model
                .attributes
                .iter()
                .any(|attribute| attribute.native_ref.as_deref() == Some(property.id.as_str()))
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
            match xml::native_named(property, &names, &available)? {
                Some(property_xml) => object.set(&property.name, property_xml),
                None => {
                    if property.body.is_none() {
                        losses.push(FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
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
                    FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                        "document property {} has no decoded body and is omitted",
                        property.id
                    )),
                );
            } else if let Some(property_xml) = xml::native(property, &available)? {
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
                object.set_text("Label", label);
            }
            if let Some(suppressed) = feature.suppressed {
                object.set_boolean("Suppressed", suppressed);
            }
        }
    }
    let mut body_names = HashMap::new();
    let mut emitted_topology = Vec::new();
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
        // The BRep root location couples with the owner object's Placement:
        // the decode restores an owner's placement from its shape root, so
        // the root carries the owner's occurrence transform (#238 §5.5).
        let root_override = ir
            .model
            .occurrences
            .iter()
            .find(|occurrence| names.get(occurrence.id.as_str()) == Some(&name))
            .and_then(|occurrence| occurrence.transform);
        let Some(serialized) =
            brep::serialize_with_uses(ctx, ir, &bodies, &root_kinds, root_override)?
        else {
            for body in bodies {
                losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "body {} is omitted because a required carrier has no OCCT text encoding",
                    body.id
                )));
                body_names.remove(body.id.as_str());
            }
            continue;
        };
        let created = !objects.iter().any(|object| object.name == name);
        if created {
            objects.push(Object::new(
                name.clone(),
                "Part::Feature",
                bodies[0].name.as_deref().unwrap_or(bodies[0].id.as_str()),
            ));
        }
        let file = format!("{name}.brp");
        entries.insert(file.clone(), serialized.bytes);
        brep::remember_uses(ctx, &mut emitted_topology, &name, serialized.uses)?;
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
        object.mark_geometry_written();
        // The Label of a plain `Part::Feature` carrying exactly one body is
        // that body's native home; a body output by a feature, or one of
        // several bodies in one object, has no Label home (#238 §5.5).
        let named: Vec<_> = bodies.iter().filter(|body| body.name.is_some()).collect();
        if (!created || bodies.len() > 1) && !named.is_empty() {
            let ids = named
                .iter()
                .map(|body| body.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            losses.push(
                FreecadLossCode::BodyNameNotTransferred
                    .note(format!("object {name} has no Label home for bodies {ids}",)),
            );
        }
        // Independent body color and visibility live in the view provider
        // (`ShapeColor`/`Visibility` from `body.color`/`body.visible`); the
        // bound appearance goes to `ShapeMaterial` alone (#238 §5.5).
        // FreeCAD persists the application's placement and the BRep root
        // location as coupled lanes. Restoring the property does not compose
        // the same placement twice. Keep canonical construction placements and
        // typed native placements; body-only objects use their BRep root.
    }
    if let Some(serialized) = brep::unowned_with_uses(ctx, ir, &mut losses, &root_kinds)? {
        let mut name = "FreeGeometry".to_owned();
        while objects.iter().any(|object| object.name == name) {
            name.push('_');
        }
        let file = format!("{name}.brp");
        entries.insert(file.clone(), serialized.bytes);
        brep::remember_uses(ctx, &mut emitted_topology, &name, serialized.uses)?;
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
    design::sketches(ir, &mut objects, &mut names, &mut losses)?;
    super::parameters::prepare(ir, &mut objects, &mut names)?;
    design::spreadsheets(ir, &mut objects, &mut names)?;
    super::parameters::write(ir, &mut objects, &names, &mut losses)?;
    super::products::write(ctx, ir, &mut objects, &mut names, &body_names, &mut losses)?;
    super::attributes::write_products(
        ir,
        &names,
        &mut objects,
        &mut document_properties,
        &mut losses,
        &native_properties,
    )?;
    // The object Label is the feature's native home. Where a definition
    // and a feature share an object and their labels differ, the feature
    // name wins and the definition label is a documented loss (#238 §5.5).
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
                    .filter(|definition| definition.label.as_deref() != Some(label.as_str()))
                {
                    losses.push(FreecadLossCode::ProductLabelNotTransferred.note(format!(
                        "definition {} label differs from the feature label sharing object {}",
                        definition.id, object.name,
                    )));
                }
                object.set_text("Label", label);
            }
        }
    }
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
    let topology_join =
        super::native::TopologyJoin::from_current(ctx, ir, current_topology, &emitted_topology)?;
    let dictionaries = super::native::maps(
        ctx,
        ir,
        &topology_join,
        &mut objects,
        &names,
        &native_properties,
        super::native::MapOutput {
            archive_entries: &mut entries,
            losses: &mut losses,
            outcomes: &mut native_dispositions,
        },
    )?;
    super::synthesis_graph::close(&mut objects, &mut losses)?;
    if objects.iter().all(Object::geometry_empty)
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
    // Design objects enter `<Objects>` and `<ObjectData>` in ascending
    // feature ordinal order, ties by arena order; other objects keep their
    // slots. Replayed natives arrive in native arena order, which generally
    // disagrees with declaration order; the declaration order is the
    // ordinal's native home (#238 §5.5).
    {
        let mut rank_of: HashMap<&str, (u64, usize)> = HashMap::new();
        for (index, feature) in ir.model.features.iter().enumerate() {
            if let Some(name) = names.get(feature.id.as_str()) {
                rank_of
                    .entry(name.as_str())
                    .or_insert((feature.ordinal, index));
            }
        }
        let mut design = Vec::new();
        let mut slots = Vec::new();
        for object in objects.drain(..) {
            if rank_of.contains_key(object.name.as_str()) {
                slots.push(None);
                design.push(object);
            } else {
                slots.push(Some(object));
            }
        }
        design.sort_by_key(|object| rank_of[object.name.as_str()]);
        let mut design = design.into_iter();
        objects = slots
            .into_iter()
            .map(|slot| {
                slot.unwrap_or_else(|| design.next().expect("design slot has no design object"))
            })
            .collect();
    }
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
    let gui_bytes = if gui.is_empty() {
        None
    } else {
        Some(gui.clone())
    };
    if let Some(bytes) = gui_bytes {
        entries.insert("GuiDocument.xml".into(), bytes.into_bytes());
    }
    // Retained opaque entries replay at their physical archive position, but
    // only when the emitted output still refers to them by name. An entry
    // whose references were dropped or rewritten (a regenerated string table
    // or element map under a new file name, a dropped payload property)
    // stays dropped with its omission loss, instead of reappearing as
    // unreferenced bytes the re-decode would report as unrepresented.
    // Entries nothing ever referred to (pure opaque sidecars) replay.
    // Entries consumed by a typed record that regenerates under a new name
    // never replay: the regenerated entry carries their content.
    if let Some(namespace) = namespace {
        if let Ok(records) = namespace.arena_as::<crate::native::EntryRecord>("entries") {
            let mut consumed: HashSet<String> = HashSet::new();
            if let Ok(tables) =
                namespace.arena_as::<crate::native::StringTableRecord>("string_tables")
            {
                consumed.extend(tables.iter().filter_map(|table| table.source_entry.clone()));
            }
            if let Ok(maps) =
                namespace.arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
            {
                consumed.extend(maps.iter().filter_map(|map| map.source_entry.clone()));
            }
            for record in records {
                let name = record.name();
                if entries.contains_key(name) || !retained.contains_key(name) {
                    continue;
                };
                let referred = entry_referenced(name, &objects, &document_properties, &gui);
                let orphan = record.referenced_by().is_empty() && !consumed.contains(name);
                if referred || orphan {
                    if let Some(bytes) = retained.get(name) {
                        entries.insert(name.to_owned(), bytes.clone());
                    }
                }
            }
        }
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
    topology_join.record_emitted(ctx, &objects, &entries, &mut native_dispositions)?;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    // Retained opaque entries replay at their physical archive position;
    // generated entries with a native entry record take that position, and
    // generated entries without one (new synthesized content) sort after.
    // FreeCAD opens the first member as its document stream before resolving
    // named side entries. Keep Document.xml first even when a BRep name sorts
    // before it lexically.
    let record_order = |name: &str| {
        namespace
            .and_then(|namespace| {
                namespace
                    .arena_as::<crate::native::EntryRecord>("entries")
                    .ok()
            })
            .and_then(|records| {
                records
                    .iter()
                    .find(|record| record.name() == name)
                    .map(|record| record.order())
            })
    };
    let mut entries = entries.into_iter().collect::<Vec<_>>();
    entries.sort_by(|(left, _), (right, _)| {
        (
            left != "Document.xml",
            record_order(left).is_none(),
            record_order(left),
            left,
        )
            .cmp(&(
                right != "Document.xml",
                record_order(right).is_none(),
                record_order(right),
                right,
            ))
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
