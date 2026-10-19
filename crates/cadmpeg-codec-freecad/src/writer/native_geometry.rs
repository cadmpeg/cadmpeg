// SPDX-License-Identifier: Apache-2.0
//! Geometry held only in typed native BRep records still belongs to CADIR.
//!
//! This temporary projection is used only when the neutral Model class is not
//! held. It consumes typed records, never archive entries or retained bytes.

use crate::{brep, native};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

pub(super) fn project(ir: &CadIr) -> Result<Option<(CadIr, Vec<LossNote>)>, CodecError> {
    if ir.scope.model {
        return Ok(None);
    }
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(None);
    };
    let payloads = namespace.arena_as::<brep::ShapePayloadRecord>("shape_payloads")?;
    if payloads.is_empty() {
        return Ok(None);
    }
    let properties = namespace.arena_as::<native::PropertyRecord>("properties")?;
    let mut typed_properties = Vec::new();
    for mut property in properties {
        let Some(xml) = super::xml::native(&property)? else {
            continue;
        };
        property.xml = native::RetainedXml::from_source(&xml, 0)?;
        typed_properties.push(property);
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let mut projected = ir.clone();
    let mut scope = projected.scope;
    scope.model = true;
    projected.set_scope(scope).map_err(CodecError::malformed)?;
    let mut curves = brep::transfer_text_curves(&ctx, &payloads, &typed_properties)?;
    let surfaces = brep::transfer_text_surfaces(&ctx, &payloads, &typed_properties, &mut curves)?;
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
    let topology_occurrences = crate::topology_transfer::transfer(
        &ctx,
        &mut projected,
        &payloads,
        &typed_properties,
        &mut losses,
    )?;
    crate::topology_transfer::apply_body_names(&mut projected, &payloads, &typed_properties);
    let objects = namespace.arena_as::<native::ObjectRecord>("objects")?;
    let products = namespace.arena_as::<native::ProductNodeRecord>("product_nodes")?;
    let joints = namespace.arena_as::<native::joint::JointRecord>("joints")?;
    let (definitions, occurrences) = crate::product::transfer_neutral(
        &ctx,
        &products,
        &joints,
        &objects,
        &typed_properties,
        &payloads,
        &projected.model.bodies,
    )?;
    projected.model.product_definitions = definitions;
    projected.model.occurrences = occurrences;
    projected.model.assembly_joints =
        crate::joint::transfer_neutral(&joints, &projected.model.occurrences)?;
    let drawings = namespace.arena_as::<native::DrawingRecord>("drawings")?;
    let annotations = namespace.arena_as::<native::SemanticAnnotationRecord>("annotations")?;
    crate::drawing::transfer_neutral(&ctx, &mut projected.model, &drawings, &typed_properties)?;
    crate::annotation::transfer_neutral(
        &ctx,
        &mut projected.model,
        &annotations,
        &typed_properties,
        &drawings,
    )?;
    if let Some(gui) = super::target::projection::gui_xml_lossy(namespace, &mut losses)? {
        let mut element_maps =
            namespace.arena_as::<native::element_map::ElementMapRecord>("element_maps")?;
        crate::element_map::bind_topology(&mut element_maps, &topology_occurrences);
        let documents = namespace.arena_as::<native::DocumentFacts>("document")?;
        let version = documents
            .first()
            .and_then(|document| document.program_version.as_deref());
        let entries = std::collections::BTreeMap::new();
        let graph = crate::gui::transfer(
            &ctx,
            &mut projected,
            gui.as_bytes(),
            &crate::gui::GuiSources {
                entries: &entries,
                objects: &objects,
                properties: &typed_properties,
                payloads: &payloads,
                element_maps: &element_maps,
                requires_alpha_conversion: crate::gui::requires_alpha_conversion(version),
            },
        )?;
        losses.extend(graph.losses);
    }
    super::body_styles::apply(&mut projected, &payloads, &typed_properties)?;
    crate::attributes::transfer(&mut projected, &objects, &typed_properties)?;
    ctx.finish_session()?;
    Ok(Some((projected, losses)))
}
