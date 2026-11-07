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

#[cfg(test)]
mod tests;

/// Whether the emitted output still names an archive entry: a `file`/`File`
/// reference in an object or document property, an extension declaration, or
/// the rendered GUI document. This mirrors the decode's side-entry parsing
/// (`side_entries` collects the same attributes), so a regenerated consumer
/// under a new file name no longer counts as a reference to the old entry.
fn entry_referenced(
    name: &str,
    objects: &[xml::Object],
    document_properties: &indexmap::IndexMap<String, String>,
    gui: &str,
) -> bool {
    let escaped = xml::escaped(name);
    let file = format!("file=\"{escaped}\"");
    let file_upper = format!("File=\"{escaped}\"");
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
    let mut replayed: Vec<String> = Vec::new();
    let mut document_properties = indexmap::IndexMap::new();
    let namespace = ir.native.namespace("fcstd");
    let native_objects = namespace
        .map(|namespace| namespace.arena_as::<ObjectRecord>("objects"))
        .transpose()?
        .unwrap_or_default();
    let mut native_properties = namespace
        .map(|namespace| namespace.arena_as::<PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
    let mut native_entries = namespace
        .map(|namespace| namespace.arena_as::<crate::native::EntryRecord>("entries"))
        .transpose()?
        .unwrap_or_default();
    native_entries.sort_by_key(crate::native::EntryRecord::order);
    let mut entry_names = HashSet::new();
    let mut duplicate_entries = Vec::new();
    native_entries.retain(|entry| {
        if entry_names.insert(entry.name().to_owned()) {
            true
        } else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                    "archive entry {} duplicates output name {} and is omitted",
                    entry.id(),
                    entry.name()
                )),
            );
            duplicate_entries.push(entry.id().to_owned());
            false
        }
    });
    // Retained opaque entry bytes replay verbatim when the writer emits
    // their entry. Their names are the side-entry availability for typed
    // property replay below.
    let mut retained: HashMap<String, Vec<u8>> = HashMap::new();
    for record in &native_entries {
        if let Some(bytes) = record.payload() {
            retained.insert(record.name().to_owned(), bytes.to_vec());
        }
    }
    let empty_shapes = namespace
        .map(|namespace| namespace.arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads"))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .filter(|payload| matches!(payload.payload, crate::brep::ShapePayload::Empty))
        .filter_map(|payload| {
            let property = native_properties
                .iter()
                .find(|property| property.id == payload.property)?;
            let entry = native_entries
                .iter()
                .find(|entry| entry.id() == payload.entry)?;
            if property
                .side_entries()
                .iter()
                .any(|name| name == entry.name())
            {
                retained.insert(entry.name().to_owned(), Vec::new());
                Some(property.id.clone())
            } else {
                None
            }
        })
        .collect::<HashSet<_>>();
    let available: HashSet<String> = retained.keys().cloned().collect();
    native_properties.retain(|property| match xml::validate(property, &available) {
        Ok(()) => true,
        Err(error) => {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                    "property {} cannot be written: {error}",
                    property.id
                )),
            );
            false
        }
    });
    let source_uses = namespace
        .map(|namespace| {
            namespace.arena_as::<crate::native::source_use::SourceUseSelector>("topology_uses")
        })
        .transpose()?
        .unwrap_or_default();
    let shape_payloads = namespace
        .map(|namespace| namespace.arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads"))
        .transpose()?
        .unwrap_or_default();
    let body_properties: HashMap<_, _> = ir
        .model
        .bodies
        .iter()
        .filter_map(|body| {
            let source = source_uses
                .iter()
                .find(|source| Some(source.id.as_str()) == body.native_ref.as_deref())?;
            let payload = shape_payloads
                .iter()
                .find(|payload| payload.id == source.owner)?;
            let property = native_properties
                .iter()
                .find(|property| property.id == payload.property)?;
            Some((
                body.id.to_string(),
                (property.owner.clone(), property.name.clone()),
            ))
        })
        .collect();
    names = super::names::prepare(ir, &native_objects, &native_properties, &body_properties);
    let mut entries = BTreeMap::<String, Vec<u8>>::new();
    // Replays restore the source declaration order: the re-decode derives
    // declaration indices from the emitted sequence, so emitting in arena
    // order would renumber every object and property (#238 §5.5).
    let mut replay_order: Vec<&ObjectRecord> = native_objects.iter().collect();
    replay_order.sort_by_key(|object| object.order);
    // Source-only objects remain typed content. A neutral counterpart owns its
    // complete object and is regenerated below, so stale native properties can
    // never override a neutral edit.
    for original in replay_order {
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
        let replayed_id = original.id().to_owned();
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
        object.declaration_id =
            xml::DeclarationId::Source(original.persistent_id.map(|id| id.to_string()));
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
        replayed.push(replayed_id);
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
            Err(design::features::FeatureError::Unsupported(reason)) => {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "feature {} construction is omitted: {reason}",
                    feature.id
                )));
                continue;
            }
            Err(design::features::FeatureError::Codec(error)) => return Err(error),
        }
        if let Some(suppressed) = feature.suppressed {
            object.set_boolean("Suppressed", suppressed);
        }
        objects.push(object);
    }
    // Native values are emitted from their typed tree. Retained XML and entry
    // bytes are not consulted. Every opaque payload has an explicit loss.
    let objects_names: HashSet<_> = objects.iter().map(|object| object.name.clone()).collect();
    // Properties replay in declaration sequence: the re-decode derives
    // source-order indices from the emitted sequence, and transient
    // declarations precede persisted values as the decode groups them.
    let owner_position: HashMap<&str, usize> = native_objects
        .iter()
        .map(|object| (object.id().as_str(), object.order))
        .collect();
    let mut property_order: Vec<&PropertyRecord> = native_properties.iter().collect();
    property_order.sort_by_key(|property| {
        (
            property.owner != crate::native::native_id("document", "0"),
            owner_position.get(property.owner.as_str()).copied(),
            !matches!(property.body, Some(crate::native::PropertyBody::Transient)),
            property.order,
            property.id.as_str(),
        )
    });
    for property in property_order {
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
        if property.type_name == "Part::PropertyPartShape" && !empty_shapes.contains(&property.id) {
            continue;
        }
        let destination = names
            .get(&property.owner)
            .and_then(|name| objects.iter_mut().find(|object| object.name == *name));
        if let Some(object) = destination {
            let neutral_owned = !native_names.contains(&object.name);
            if neutral_owned
                && (object.properties.contains_key(&property.name)
                    || (crate::property_policy::has_neutral_form(&property.name)
                        && !empty_shapes.contains(&property.id)))
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
    let mut annotations: Vec<_> = ir.model.semantic_annotations.iter().enumerate().collect();
    annotations.sort_by_key(|(index, annotation)| (annotation.order, *index));
    for (index, annotation) in &annotations {
        use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind;
        let default_kind = match annotation.kind {
            SemanticAnnotationKind::Text => "TechDraw::DrawViewAnnotation",
            SemanticAnnotationKind::Dimension => "TechDraw::DrawViewDimension",
            SemanticAnnotationKind::Balloon => "TechDraw::DrawViewBalloon",
            SemanticAnnotationKind::Symbol => "TechDraw::DrawViewSymbol",
            _ => continue,
        };
        let existing = names
            .get(&annotation.object)
            .and_then(|name| objects.iter().position(|object| object.name == *name));
        if existing.is_none() {
            let mut name = format!("Annotation{index}");
            while objects.iter().any(|object| object.name == name) {
                name.push('_');
            }
            names.insert(annotation.object.clone(), name.clone());
            objects.push(Object::new(name, default_kind, "Annotation"));
        }
    }
    super::drawings::write(ir, &mut objects, &mut names, &mut losses);
    for drawing in &ir.model.drawings {
        if let Some(object) = names
            .get(&drawing.object)
            .and_then(|name| objects.iter_mut().find(|object| &object.name == name))
        {
            super::drawings::parameters(
                object,
                &drawing.object,
                &drawing.parameters,
                &native_properties,
                (&names, &available),
                &["X", "Y", "Scale", "Rotation", "Direction"],
                &mut losses,
            )?;
        }
    }
    for (_, annotation) in annotations {
        use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind;
        if !matches!(
            annotation.kind,
            SemanticAnnotationKind::Text
                | SemanticAnnotationKind::Dimension
                | SemanticAnnotationKind::Balloon
                | SemanticAnnotationKind::Symbol
        ) {
            continue;
        }
        let Some(index) = names
            .get(&annotation.object)
            .and_then(|name| objects.iter().position(|object| object.name == *name))
        else {
            continue;
        };
        let object = &mut objects[index];
        super::drawings::parameters(
            object,
            &annotation.object,
            &annotation.parameters,
            &native_properties,
            (&names, &available),
            &[],
            &mut losses,
        )?;
        if annotation.runtime_type.starts_with("TechDraw::") {
            if let Some(height) = annotation.text_style.height {
                super::drawings::scalar(object, "TextSize", "App::PropertyLength", height.get());
            }
            if annotation.text_style.height.is_none()
                && native_properties.iter().any(|property| {
                    property.owner == annotation.object
                        && property.name == "TextSize"
                        && property.type_name == "App::PropertyLength"
                })
                && super::drawings::scalar_value(object, "TextSize")
                    .is_some_and(|value| value > 0.0 && value.is_finite())
            {
                object.properties.shift_remove("TextSize");
                if annotation.parameters.contains_key("TextSize") {
                    losses.push(FreecadLossCode::DrawingStyleNotTransferred.note(format!(
                        "annotation {} parameters.TextSize is omitted with its cleared text height",
                        annotation.id
                    )));
                }
            }
            if let Some(font) = &annotation.text_style.font_family {
                if annotation.text_style.height.is_none() {
                    losses.push(FreecadLossCode::DrawingStyleNotTransferred.note(format!(
                        "annotation {} has a font without a text height",
                        annotation.id
                    )));
                }
                let current = object
                    .properties
                    .get("Font")
                    .and_then(|xml| xml::property_string_value(xml));
                if current.as_deref() != Some(font.as_str()) {
                    object.set(
                        "Font",
                        xml::property(
                            "Font",
                            "App::PropertyFont",
                            &xml::element("String", &[("value", font.clone())]),
                        ),
                    );
                }
            } else if annotation.text_style.height.is_some()
                && native_properties.iter().any(|property| {
                    property.owner == annotation.object
                        && property.name == "Font"
                        && property.type_name == "App::PropertyFont"
                })
                && object
                    .properties
                    .get("Font")
                    .and_then(|xml| xml::property_string_value(xml))
                    .is_some_and(|font| !font.trim().is_empty())
            {
                object.properties.shift_remove("Font");
                if annotation.parameters.contains_key("Font") {
                    losses.push(FreecadLossCode::DrawingStyleNotTransferred.note(format!(
                        "annotation {} parameters.Font is omitted with its cleared font",
                        annotation.id
                    )));
                }
            }
        }
        if annotation.kind == SemanticAnnotationKind::Dimension {
            super::drawings::dimension_kind(annotation, object);
            super::drawings::dimension_references(annotation, object, &names, &mut losses);
            if annotation.text_override.is_some()
                && object
                    .properties
                    .get("Arbitrary")
                    .and_then(|xml| xml::property_bool_value(xml))
                    != Some(true)
            {
                object.set_boolean("Arbitrary", true);
            } else if annotation.text_override.is_none()
                && object
                    .properties
                    .get("Arbitrary")
                    .and_then(|xml| xml::property_bool_value(xml))
                    == Some(true)
                && object
                    .properties
                    .get("FormatSpec")
                    .and_then(|xml| xml::property_string_value(xml))
                    .is_some()
            {
                object.set_boolean("Arbitrary", false);
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "dimension {} cannot preserve the arbitrary flag without override text",
                    annotation.id
                )));
            }
            for (name, value) in [
                ("OverTolerance", annotation.upper_tolerance),
                ("UnderTolerance", annotation.lower_tolerance),
            ] {
                if let Some(value) = value {
                    super::drawings::scalar(
                        object,
                        name,
                        "App::PropertyQuantityConstraint",
                        value.get(),
                    );
                } else if super::drawings::scalar_value(object, name)
                    .is_some_and(|value| value != 0.0)
                {
                    object.properties.shift_remove(name);
                    if annotation.parameters.contains_key(name) {
                        losses.push(FreecadLossCode::DrawingDimensionToleranceNotTransferred.note(format!(
                            "annotation {} parameters.{name} is omitted with its cleared tolerance", annotation.id
                        )));
                    }
                }
            }
            if (annotation.upper_tolerance.is_some() || annotation.lower_tolerance.is_some())
                && object
                    .properties
                    .get("ArbitraryTolerances")
                    .and_then(|xml| xml::property_bool_value(xml))
                    == Some(true)
            {
                object.set_boolean("ArbitraryTolerances", false);
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "dimension {} cannot use arbitrary tolerance text with numeric tolerances",
                    annotation.id
                )));
            }
        }
        let (property, list) = match (&annotation.kind, object.kind.as_str()) {
            (
                SemanticAnnotationKind::Text,
                "TechDraw::DrawViewAnnotation" | "TechDraw::DrawViewAnnotationPython",
            ) => ("Text", true),
            (SemanticAnnotationKind::Text, "App::Annotation" | "App::AnnotationLabel") => {
                ("LabelText", true)
            }
            (
                SemanticAnnotationKind::Text,
                "TechDraw::DrawRichAnno" | "TechDraw::DrawRichAnnoPython",
            ) => ("AnnoText", false),
            (SemanticAnnotationKind::Balloon, "TechDraw::DrawViewBalloon") => ("Text", false),
            (
                SemanticAnnotationKind::Dimension,
                "TechDraw::DrawViewDimension"
                | "TechDraw::DrawViewDimExtent"
                | "TechDraw::LandmarkDimension",
            ) => ("FormatSpec", false),
            (
                SemanticAnnotationKind::Symbol,
                "TechDraw::DrawViewSymbol"
                | "TechDraw::DrawViewSymbolPython"
                | "TechDraw::DrawWeldSymbol"
                | "TechDraw::DrawWeldSymbolPython",
            ) => ("TailText", false),
            _ => {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "annotation {} cannot be written as object type {}",
                    annotation.id, object.kind
                )));
                continue;
            }
        };
        let vector_property = match object.kind.as_str() {
            "App::Annotation" => Some("Position"),
            "App::AnnotationLabel" => Some("TextPosition"),
            _ => None,
        };
        if let Some(name) = vector_property {
            if let Some(position) = annotation.position {
                let [x, y, z] = position.get();
                let current = object.properties.get(name).and_then(|xml| {
                    let document = roxmltree::Document::parse(xml).ok()?;
                    let value = document
                        .root_element()
                        .children()
                        .find(roxmltree::Node::is_element)?;
                    Some([
                        value.attribute("valueX")?.parse::<f64>().ok()?,
                        value.attribute("valueY")?.parse::<f64>().ok()?,
                        value.attribute("valueZ")?.parse::<f64>().ok()?,
                    ])
                });
                if current != Some([x, y, z]) {
                    object.set(
                        name,
                        xml::property(
                            name,
                            "App::PropertyVector",
                            &xml::element(
                                "PropertyVector",
                                &[
                                    ("valueX", x.to_string()),
                                    ("valueY", y.to_string()),
                                    ("valueZ", z.to_string()),
                                ],
                            ),
                        ),
                    );
                }
            } else {
                object.properties.shift_remove(name);
            }
        } else if let Some(position) = annotation.position {
            let [x, y, z] = position.get();
            super::drawings::scalar(object, "X", "App::PropertyDistance", x);
            super::drawings::scalar(object, "Y", "App::PropertyDistance", y);
            if z != 0.0 {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "annotation {} has a page position with a nonzero third coordinate",
                    annotation.id
                )));
            }
        } else {
            object.properties.shift_remove("X");
            object.properties.shift_remove("Y");
        }
        if annotation.text.iter().any(|text| text.trim().is_empty()) {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "annotation {} has blank text fragments that re-decode as absent",
                annotation.id
            )));
        }
        if !list {
            if annotation.text.len() > 1 {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "annotation {} has text fragments beyond the first that are omitted",
                    annotation.id
                )));
            }
            let current = object
                .properties
                .get(property)
                .and_then(|xml| xml::property_string_value(xml));
            let mut text = annotation.text.first().map_or("", String::as_str);
            if annotation.kind == SemanticAnnotationKind::Dimension {
                if let Some(format) = annotation.format.as_deref() {
                    if current.as_deref() == Some(text) || text.is_empty() {
                        text = format;
                    } else if current.as_deref() != Some(format) && format != text {
                        losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                            "dimension {} has conflicting text and format values",
                            annotation.id
                        )));
                        text = format;
                    }
                }
                if annotation.format.as_deref() != Some(text)
                    || annotation.text.as_slice() != [text]
                {
                    losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                        "dimension {} shares one native property for text and format",
                        annotation.id
                    )));
                }
                if let Some(override_text) = annotation.text_override.as_deref() {
                    if text != override_text {
                        losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                            "dimension {} shares one native property for format and override text",
                            annotation.id
                        )));
                    }
                    text = override_text;
                }
            }
            if current.as_deref() != Some(text) {
                object.set_text(property, text);
            }
            continue;
        }
        let current = object.properties.get(property).and_then(|xml| {
            let document = roxmltree::Document::parse(xml).ok()?;
            let list = document
                .descendants()
                .find(|node| node.has_tag_name("StringList"))?;
            list.children()
                .filter(roxmltree::Node::is_element)
                .map(|node| node.attribute("value").map(str::to_owned))
                .collect::<Option<Vec<_>>>()
        });
        if current.as_ref() == Some(&annotation.text) {
            continue;
        }
        let entries = annotation
            .text
            .iter()
            .map(|text| xml::element("String", &[("value", text.clone())]))
            .collect::<String>();
        object.set(
            property,
            xml::property(
                property,
                "App::PropertyStringList",
                &format!(
                    "<StringList count=\"{}\">{entries}</StringList>",
                    annotation.text.len()
                ),
            ),
        );
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
    // A neutral value that already matches the emitted text keeps the native
    // XML: replacing it would drop the source status and canonical spelling
    // for no content change.
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
                let current = object
                    .properties
                    .get("Label")
                    .and_then(|xml| xml::property_string_value(xml));
                if current.as_deref() != Some(label.as_str()) {
                    object.set_text("Label", label);
                }
            }
            if let Some(suppressed) = feature.suppressed {
                let current = object
                    .properties
                    .get("Suppressed")
                    .and_then(|xml| xml::property_bool_value(xml));
                if current != Some(suppressed) {
                    object.set_boolean("Suppressed", suppressed);
                }
            }
        }
    }
    let mut body_names = HashMap::new();
    let mut emitted_topology = Vec::new();
    let mut shape_groups = BTreeMap::<(String, String), Vec<&cadmpeg_ir::topology::Body>>::new();
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
        let property = body_properties
            .get(body.id.as_str())
            .filter(|(owner, _)| names.get(owner) == Some(&name))
            .map_or("Shape", |(_, property)| property.as_str());
        shape_groups
            .entry((name, property.to_owned()))
            .or_default()
            .push(body);
    }
    for ((name, property_name), bodies) in shape_groups {
        // The BRep root location couples with the owner object's Placement:
        // the decode restores an owner's placement from its shape root, so
        // the root carries the owner's occurrence transform (#238 §5.5).
        let root_override = ir
            .model
            .occurrences
            .iter()
            .filter(|_| property_name == "Shape")
            .find(|occurrence| names.get(occurrence.id.as_str()) == Some(&name))
            .and_then(|occurrence| occurrence.transform);
        let Some(serialized) =
            brep::serialize_with_uses(ctx, ir, &bodies, &root_kinds, root_override, &mut losses)?
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
        let original_file = native_properties.iter().find_map(|property| {
            let owner_matches = names.get(&property.owner) == Some(&name);
            let [file] = property.side_entries() else {
                return None;
            };
            (owner_matches
                && property.name == property_name
                && property.type_name == "Part::PropertyPartShape"
                && file.to_ascii_lowercase().ends_with(".brp")
                && entry_names.contains(file)
                && native_properties
                    .iter()
                    .filter(|other| other.side_entries().contains(file))
                    .count()
                    == 1
                && !entries.contains_key(file))
            .then(|| file.clone())
        });
        let file = original_file.unwrap_or_else(|| {
            let mut file = format!("{name}.brp");
            let mut suffix = 1;
            while entries.contains_key(&file) || entry_names.contains(&file) {
                file = format!("{name}_{suffix}.brp");
                suffix += 1;
            }
            file
        });
        entries.insert(file.clone(), serialized.bytes);
        for body in &bodies {
            if !serialized
                .uses
                .iter()
                .any(|use_record| use_record.neutral_id == body.id.as_str())
            {
                body_names.remove(body.id.as_str());
            }
        }
        if property_name == "Shape" {
            brep::remember_uses(ctx, &mut emitted_topology, &name, &file, serialized.uses)?;
        }
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
        if property_name == "Shape" {
            object.mark_geometry_written();
        }
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
                    .note(format!("object {name} has no Label home for bodies {ids}")),
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
    super::mesh_kernel::write(
        ctx,
        ir,
        &native_properties,
        &names,
        (&entry_names, &retained),
        (&mut objects, &mut entries),
        &mut losses,
    )?;
    let covered_points = super::point_kernel::write(
        ctx,
        ir,
        &native_properties,
        &names,
        (&entry_names, &mut objects, &mut entries),
        &mut losses,
    )?;
    // Independent records whose source object replays with every side
    // entry property need no `FreeGeometry` copy: the replayed payload
    // carries them, and the re-decode reproduces them from it.
    let mut covered = HashSet::new();
    for owner in &replayed {
        let Some(name) = names.get(owner.as_str()) else {
            continue;
        };
        let Some(object) = objects.iter().find(|object| object.name == *name) else {
            continue;
        };
        if native_properties
            .iter()
            .filter(|property| property.owner == *owner && !property.side_entries().is_empty())
            .all(|property| object.properties.contains_key(&property.name))
        {
            covered.insert(owner.clone());
        }
    }
    if let Some(serialized) =
        brep::unowned_with_uses(ctx, ir, &mut losses, &root_kinds, &covered, &covered_points)?
    {
        let mut name = "FreeGeometry".to_owned();
        while objects.iter().any(|object| object.name == name) {
            name.push('_');
        }
        let file = format!("{name}.brp");
        entries.insert(file.clone(), serialized.bytes);
        brep::remember_uses(ctx, &mut emitted_topology, &name, &file, serialized.uses)?;
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
    super::parameters::prepare(ir, &mut objects, &mut names, &mut losses)?;
    design::spreadsheets(ir, &mut objects, &mut names, &mut losses)?;
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
    // A label that already matches keeps its native XML with its status.
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
                let current = object
                    .properties
                    .get("Label")
                    .and_then(|xml| xml::property_string_value(xml));
                if current.as_deref() != Some(label.as_str()) {
                    object.set_text("Label", label);
                }
            }
        }
    }
    for original in &native_objects {
        if native_properties
            .iter()
            .any(|property| property.owner == *original.id() && property.name == "Label")
        {
            continue;
        }
        let Some(name) = names.get(original.id().as_str()) else {
            continue;
        };
        let Some(object) = objects.iter_mut().find(|object| &object.name == name) else {
            continue;
        };
        let neutral_name = ir
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(original.id().as_str()))
            .and_then(|feature| feature.name.as_deref());
        if neutral_name.is_none_or(|label| label == original.name())
            && object
                .properties
                .get("Label")
                .and_then(|xml| xml::property_string_value(xml))
                .as_deref()
                == Some(original.name())
        {
            object.properties.shift_remove("Label");
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
    // Neutral ownership skips leave holes: a renamed attribute emits under
    // its new name, shifting every later declaration index on re-decode. A
    // placeholder keeps the arity with no content, so survivors pair and
    // the omission loss names the hole. Placeholders stay document-side and
    // only cover renames: a fully omitted lane (holes, unrepresentable
    // values) stays absent. They are marked dropped after the emission
    // accounting below.
    let mut placeholders: Vec<String> = Vec::new();
    for attribute in &ir.model.attributes {
        let Some(native_ref) = attribute.native_ref.as_deref() else {
            continue;
        };
        let Some(property) = native_properties
            .iter()
            .find(|property| property.id == native_ref)
        else {
            continue;
        };
        if attribute.name == property.name
            || property.owner != crate::native::native_id("document", "0")
            || !document_properties.contains_key(&attribute.name)
            || document_properties.contains_key(&property.name)
        {
            continue;
        }
        let status = property
            .status
            .map(|status| format!(" status=\"{status}\""))
            .unwrap_or_default();
        // The placeholder matches the body's framing so the re-decode groups
        // it with its declaration peers: a transient placeholder would shift
        // the persisted indices it is holding. An empty persisted property
        // projects no neutral attribute.
        let placeholder = if matches!(property.body, Some(crate::native::PropertyBody::Transient)) {
            format!(
                "<_Property name=\"{}\" type=\"{}\"{status}/>",
                xml::escaped(&property.name),
                xml::escaped(&property.type_name)
            )
        } else {
            format!(
                "<Property name=\"{}\" type=\"{}\"{status}></Property>",
                xml::escaped(&property.name),
                xml::escaped(&property.type_name)
            )
        };
        document_properties.insert(property.name.clone(), placeholder);
        placeholders.push(property.id.clone());
    }
    // Replayed properties settle into declaration sequence: neutral passes
    // (attributes, operands, parameters, products) append or replace by
    // name, which disturbs the property order the re-decode derives. Native
    // properties take their declaration slots; neutral additions without a
    // native record keep their appended positions.
    {
        let mut source_of: HashMap<&str, &str> = HashMap::new();
        for source in &native_objects {
            if let Some(name) = names.get(source.id().as_str()) {
                source_of.insert(name.as_str(), source.id().as_str());
            }
        }
        let mut parameter_order: HashMap<&str, Vec<_>> = HashMap::new();
        for (index, parameter) in ir.model.parameters.iter().enumerate() {
            if names.contains_key(&format!("parameter-cell:{}", parameter.id)) {
                continue;
            }
            let owner = parameter
                .owner
                .as_ref()
                .and_then(|id| names.get(id.as_str()))
                .or_else(|| names.get(&format!("parameter-owner:{}", parameter.id)));
            let selector = names
                .get(&format!("parameter-path:{}", parameter.id))
                .map_or(parameter.name.as_str(), String::as_str);
            if let Some(owner) = owner {
                parameter_order.entry(owner.as_str()).or_default().push((
                    parameter.ordinal,
                    index,
                    selector,
                ));
            }
        }
        for parameters in parameter_order.values_mut() {
            parameters.sort_by_key(|(ordinal, index, _)| (*ordinal, *index));
        }
        let mut sequence: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut ordered: Vec<&PropertyRecord> = native_properties.iter().collect();
        ordered.sort_by_key(|property| {
            (
                property.owner.as_str(),
                !matches!(property.body, Some(crate::native::PropertyBody::Transient)),
                property.order,
            )
        });
        for property in ordered {
            sequence
                .entry(property.owner.as_str())
                .or_default()
                .push(property.name.as_str());
        }
        for object in &mut objects {
            let Some(id) = source_of.get(object.name.as_str()) else {
                continue;
            };
            let Some(names) = sequence.get(*id) else {
                continue;
            };
            let parameters = parameter_order.get(object.name.as_str());
            let parameter_names: HashSet<&str> = parameters
                .into_iter()
                .flatten()
                .map(|(_, _, name)| *name)
                .filter(|name| names.contains(name) && object.properties.contains_key(*name))
                .collect();
            let mut ordered_parameters = parameters
                .into_iter()
                .flatten()
                .map(|(_, _, name)| *name)
                .filter(|name| parameter_names.contains(name));
            let mut rebuilt = indexmap::IndexMap::new();
            for name in names {
                let name = if parameter_names.contains(name) {
                    ordered_parameters.next().unwrap_or(*name)
                } else {
                    *name
                };
                if let Some(value) = object.properties.shift_remove(name) {
                    rebuilt.insert(name.to_owned(), value);
                }
            }
            for (name, value) in object.properties.drain(..) {
                rebuilt.insert(name, value);
            }
            object.properties = rebuilt;
        }
        let document_id = crate::native::native_id("document", "0");
        if let Some(names) = sequence.get(document_id.as_str()) {
            let mut rebuilt = indexmap::IndexMap::new();
            for name in names {
                if let Some(value) = document_properties.shift_remove(*name) {
                    rebuilt.insert((*name).to_owned(), value);
                }
            }
            for (name, value) in document_properties.drain(..) {
                rebuilt.insert(name, value);
            }
            document_properties = rebuilt;
        }
    }
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
    // Restore source declaration slots before ordering semantic siblings.
    {
        fn order_slots(objects: &mut Vec<Object>, ranks: &HashMap<&str, (u64, usize)>) {
            let mut ordered = Vec::new();
            let mut slots = Vec::new();
            for object in objects.drain(..) {
                if ranks.contains_key(object.name.as_str()) {
                    slots.push(None);
                    ordered.push(object);
                } else {
                    slots.push(Some(object));
                }
            }
            ordered.sort_by_key(|object| ranks[object.name.as_str()]);
            let mut ordered = ordered.into_iter();
            *objects = slots
                .into_iter()
                .map(|slot| {
                    slot.unwrap_or_else(|| ordered.next().expect("ordered slot has no object"))
                })
                .collect();
        }
        let source_order: HashMap<&str, usize> = native_objects
            .iter()
            .filter_map(|object| {
                names
                    .get(object.id().as_str())
                    .map(|name| (name.as_str(), object.order))
            })
            .collect();
        objects.sort_by_key(|object| {
            let rank = source_order.get(object.name.as_str()).copied();
            (rank.is_none(), rank)
        });

        let mut rank_of: HashMap<&str, (u64, usize)> = HashMap::new();
        for (index, feature) in ir.model.features.iter().enumerate() {
            if let Some(name) = names.get(feature.id.as_str()) {
                rank_of
                    .entry(name.as_str())
                    .or_insert((feature.ordinal, index));
            }
        }
        order_slots(&mut objects, &rank_of);
        let mut roots = HashMap::new();
        for (index, occurrence) in ir.model.occurrences.iter().enumerate() {
            if matches!(
                occurrence.parent,
                cadmpeg_ir::products::OccurrenceParent::Root {}
            ) {
                if let Some(name) = names.get(occurrence.id.as_str()) {
                    roots
                        .entry(name.as_str())
                        .or_insert((u64::from(occurrence.ordinal), index));
                }
            }
        }
        order_slots(&mut objects, &roots);
    }
    for object in &mut objects {
        let mut originals = native_objects.iter().filter(|original| {
            names.get(original.id().as_str()) == Some(&object.name)
                && original.type_name == object.kind
        });
        if let (Some(original), None) = (originals.next(), originals.next()) {
            object.declaration_id =
                xml::DeclarationId::Source(original.persistent_id.map(|id| id.to_string()));
        }
    }
    // The document header preserves the source's producing application and
    // persistence versions, so the re-decode reproduces the document facts.
    let facts = namespace.and_then(|namespace| {
        namespace
            .arena_as::<crate::native::DocumentFacts>("document")
            .ok()
    });
    let program_version = facts
        .as_ref()
        .and_then(|documents| documents.first())
        .and_then(|document| document.program_version.clone());
    let file_version = facts
        .as_ref()
        .and_then(|documents| documents.first())
        .map_or_else(
            || "1".to_owned(),
            |document| document.file_version.as_str().to_owned(),
        );
    let document = xml::document(
        &objects,
        &document_properties,
        schema,
        program_version.as_deref(),
        &file_version,
    );
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
        let mut consumed: HashSet<String> = HashSet::new();
        if let Ok(tables) = namespace.arena_as::<crate::native::StringTableRecord>("string_tables")
        {
            consumed.extend(tables.iter().filter_map(|table| table.source_entry.clone()));
        }
        if let Ok(maps) =
            namespace.arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
        {
            consumed.extend(maps.iter().filter_map(|map| map.source_entry.clone()));
        }
        for record in &native_entries {
            let name = record.name();
            if entries.contains_key(name) || !retained.contains_key(name) {
                continue;
            }
            let referred = entry_referenced(name, &objects, &document_properties, &gui);
            let orphan = record.referenced_by().is_empty() && !consumed.contains(name);
            if referred || orphan {
                if let Some(bytes) = retained.get(name) {
                    entries.insert(name.to_owned(), bytes.clone());
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
    for id in duplicate_entries {
        super::dispositions::record(
            &mut native_dispositions,
            "entries",
            &id,
            cadmpeg_ir::codec::write::NativeDisposition::Dropped,
        );
    }
    // Placeholders keep declaration arity, not content: they stay dropped so
    // the omission loss names them as removed.
    for id in &placeholders {
        super::dispositions::record(
            &mut native_dispositions,
            "properties",
            id,
            cadmpeg_ir::codec::write::NativeDisposition::Dropped,
        );
    }
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
        native_entries
            .iter()
            .find(|record| record.name() == name)
            .map(crate::native::EntryRecord::order)
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
        if let Some(construction) = curve.geometry.procedural_construction().filter(|_| {
            super::geometry::located_curve(ir, curve)
                .is_some_and(|(row, _)| !row.starts_with("8 ") && !row.starts_with("9 "))
        }) {
            losses.push(FreecadLossCode::ExportProceduralReduced.note(format!(
                "curve {} is written through its current solved carrier; construction {} is omitted",
                curve.id, construction
            )));
        }
    }
    for surface in &ir.model.surfaces {
        if let Some(construction) = surface.geometry.procedural_construction().filter(|_| {
            super::geometry::located_surface(ir, surface).is_some_and(|(row, _)| {
                !["6 ", "7 ", "10 ", "11 "]
                    .iter()
                    .any(|prefix| row.starts_with(prefix))
            })
        }) {
            losses.push(FreecadLossCode::ExportProceduralReduced.note(format!(
                "surface {} is written through its current solved carrier; construction {} is omitted",
                surface.id, construction
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
