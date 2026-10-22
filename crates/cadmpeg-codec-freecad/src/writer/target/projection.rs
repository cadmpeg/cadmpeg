// SPDX-License-Identifier: Apache-2.0
//! Patch eligibility compares the current typed graph with its neutral view.
//!
//! This module reads no retained source image, historical digest, or report.

use crate::{brep, native};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{ArenaName, CadIr};
use std::collections::BTreeMap;

pub(super) fn agrees(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<(), CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return decline("there is no typed FreeCAD graph");
    };
    let objects = namespace.arena_as::<native::ObjectRecord>("objects")?;
    let mut properties = namespace.arena_as::<native::PropertyRecord>("properties")?;
    // Parsers that operate on XML receive a temporary serialization of CURRENT
    // typed values. Their native evidence fields never become a content source.
    for property in &mut properties {
        let Some(xml) = super::super::xml::native(property)? else {
            return decline("the typed property has no independent projection");
        };
        property.xml = native::RetainedXml::from_source(&xml, 0)?;
    }
    let payloads = namespace.arena_as::<brep::ShapePayloadRecord>("shape_payloads")?;
    let products = namespace.arena_as::<native::ProductNodeRecord>("product_nodes")?;
    let joints = namespace.arena_as::<native::joint::JointRecord>("joints")?;
    let drawings = namespace.arena_as::<native::DrawingRecord>("drawings")?;
    let annotations = namespace.arena_as::<native::SemanticAnnotationRecord>("annotations")?;
    let documents = namespace.arena_as::<native::DocumentFacts>("document")?;
    let version = documents
        .first()
        .and_then(|document| document.program_version.as_deref());
    let mut projected = CadIr::empty();
    projected.native.clone_from(&ir.native);
    projected.source.clone_from(&ir.source);
    let mut curves = brep::transfer_text_curves(ctx, &payloads, &properties)?;
    let surfaces = brep::transfer_text_surfaces(ctx, &payloads, &properties, &mut curves)?;
    projected.model.curves = curves.curves;
    projected
        .model
        .add_procedural_curves(curves.procedural)?
        .map_err(CodecError::malformed)?;
    projected.model.surfaces = surfaces.surfaces;
    projected
        .model
        .add_procedural_surfaces(surfaces.procedural)?
        .map_err(CodecError::malformed)?;
    let mut losses = Vec::new();
    let mut maps = namespace.arena_as::<native::element_map::ElementMapRecord>("element_maps")?;
    let occurrences = crate::topology_transfer::transfer(
        ctx,
        &mut projected,
        &payloads,
        &properties,
        &mut losses,
    )?;
    crate::topology_transfer::apply_body_names(&mut projected, &payloads, &properties);
    crate::element_map::bind_topology(&mut maps, &occurrences);
    if ir.scope.design {
        crate::design::transfer(
            ctx,
            &mut projected,
            &objects,
            &properties,
            &payloads,
            &[],
            version,
        )?;
    }
    let (definitions, occurrences) = crate::product::transfer_neutral(
        ctx,
        &products,
        &joints,
        &objects,
        &properties,
        &payloads,
        &projected.model.bodies,
    )?;
    projected.model.product_definitions = definitions;
    projected.model.occurrences = occurrences;
    projected.model.assembly_joints =
        crate::joint::transfer_neutral(&joints, &projected.model.occurrences)?;
    crate::drawing::transfer_neutral(ctx, &mut projected.model, &drawings, &properties)?;
    crate::annotation::transfer_neutral(
        ctx,
        &mut projected.model,
        &annotations,
        &properties,
        &drawings,
    )?;
    crate::writer::ordinals::apply(&properties, &mut projected)?;
    if let Some(gui) = gui_xml(namespace)? {
        let entries = BTreeMap::new();
        crate::gui::transfer(
            ctx,
            &mut projected,
            gui.as_bytes(),
            &crate::gui::GuiSources {
                entries: &entries,
                objects: &objects,
                properties: &properties,
                payloads: &payloads,
                element_maps: &maps,
                requires_alpha_conversion: crate::gui::requires_alpha_conversion(version),
            },
        )?;
    }
    crate::writer::body_styles::apply(&mut projected, &payloads, &properties)?;
    crate::attributes::transfer(&mut projected, &objects, &properties)?;
    let current = serde_json::to_value(&ir.model).map_err(CodecError::malformed)?;
    let candidate = serde_json::to_value(&projected.model).map_err(CodecError::malformed)?;
    for arena in ArenaName::ALL {
        if ir.scope.holds(arena.class()) && current[arena.as_str()] != candidate[arena.as_str()] {
            return decline(&format!(
                "current typed FreeCAD projection disagrees with neutral arena {}",
                arena.as_str()
            ));
        }
    }
    if ir.tolerances != projected.tolerances {
        return decline("current typed FreeCAD projection has different document tolerances");
    }
    Ok(())
}

fn decline<T>(reason: &str) -> Result<T, CodecError> {
    Err(CodecError::UnsupportedContent(reason.to_owned()))
}

pub(in crate::writer) fn gui_xml(
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<Option<String>, CodecError> {
    gui_xml_impl(namespace, None)
}

pub(in crate::writer) fn gui_xml_lossy(
    namespace: &cadmpeg_ir::native::NativeNamespace,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<String>, CodecError> {
    gui_xml_impl(namespace, Some(losses))
}

fn gui_xml_impl(
    namespace: &cadmpeg_ir::native::NativeNamespace,
    mut losses: Option<&mut Vec<cadmpeg_ir::report::loss::LossNote>>,
) -> Result<Option<String>, CodecError> {
    use crate::writer::xml;
    let documents = namespace.arena_as::<native::GuiDocumentRecord>("gui_documents")?;
    let providers = namespace.arena_as::<native::GuiViewProviderRecord>("gui_view_providers")?;
    let properties = namespace.arena_as::<native::GuiPropertyRecord>("gui_properties")?;
    let Some(document) = documents.first() else {
        return Ok(None);
    };
    if documents.len() != 1 {
        if let Some(losses) = losses.as_deref_mut() {
            losses.push(crate::loss::FreecadLossCode::ExportArenaNotTransferred.note("multiple native GUI documents share one target document; additional GUI roots are omitted"));
        } else {
            return decline("multiple typed GUI documents require synthesis");
        }
    }
    let mut output = format!(
        "<Document SchemaVersion=\"{}\"><ViewProviderData Count=\"{}\">",
        xml::escaped(document.schema_version.as_deref().unwrap_or("1")),
        providers.len()
    );
    for provider in &providers {
        let mut encoded_properties = Vec::new();
        for property in properties
            .iter()
            .filter(|property| property.owner == provider.id)
        {
            let Some(value) = xml::native_gui(property)? else {
                if let Some(losses) = losses.as_deref_mut() {
                    losses.push(
                        crate::loss::FreecadLossCode::ExportNativePropertyNotTransferred.note(
                            format!(
                                "GUI property {} has no independent typed projection",
                                property.id
                            ),
                        ),
                    );
                    continue;
                }
                return decline("GUI property needs source-only payload");
            };
            encoded_properties.push(value);
        }
        output.push_str("<ViewProvider name=\"");
        output.push_str(&xml::escaped(&provider.name));
        output.push('"');
        if let Some(expanded) = provider.expanded {
            output.push_str(" expanded=\"");
            output.push_str(&u8::from(expanded).to_string());
            output.push('"');
        }
        output.push_str("><Properties Count=\"");
        output.push_str(&encoded_properties.len().to_string());
        output.push_str("\">");
        for value in encoded_properties {
            output.push_str(&value);
        }
        output.push_str("</Properties></ViewProvider>");
    }
    output.push_str("</ViewProviderData>");
    for state in &document.states {
        if !state.values.is_empty() || !state.side_entries.is_empty() {
            if let Some(losses) = losses.as_deref_mut() {
                losses.push(
                    crate::loss::FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                        "GUI state {} has no independent neutral projection",
                        state.id
                    )),
                );
                continue;
            }
            return decline("nested GUI state needs an independent typed projection");
        }
        let attributes = state
            .attributes
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect::<Vec<_>>();
        output.push_str(&xml::element(&state.kind, &attributes));
    }
    output.push_str("</Document>");
    Ok(Some(output))
}
