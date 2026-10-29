// SPDX-License-Identifier: Apache-2.0
//! Geometry held only in typed native `BRep` records still belongs to CADIR.
//!
//! This temporary projection is used only when the neutral Model class is not
//! held. It consumes typed records, never archive entries or retained bytes.

use crate::{brep, native};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Option<(CadIr, Vec<LossNote>)>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
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
    let property_reader = crate::property_reader::PropertyReader::typed(ctx);
    let mut typed_properties = Vec::new();
    for property in properties {
        if super::xml::native(&property)?.is_some() {
            typed_properties.push(property);
        }
    }
    let mut projected = ir.clone();
    let mut scope = projected.scope;
    scope.model = true;
    projected.set_scope(scope).map_err(CodecError::malformed)?;
    let mut curves = brep::transfer_text_curves(ctx, &payloads, &typed_properties)?;
    let surfaces = brep::transfer_text_surfaces(ctx, &payloads, &typed_properties, &mut curves)?;
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
    let topology_occurrences = crate::topology_transfer::transfer_projection(
        ctx,
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
        &property_reader,
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
    crate::drawing::transfer_neutral(
        &property_reader,
        &mut projected.model,
        &drawings,
        &typed_properties,
    )?;
    crate::annotation::transfer_neutral(
        &property_reader,
        &mut projected.model,
        &annotations,
        &typed_properties,
        &drawings,
    )?;
    if let Some(gui) = super::target::projection::gui_xml_lossy(ctx, namespace, &mut losses)? {
        let documents = namespace.arena_as::<native::DocumentFacts>("document")?;
        let version = documents
            .first()
            .and_then(|document| document.program_version.as_deref());
        let entries = std::collections::BTreeMap::new();
        let graph = crate::gui::transfer_rendered(
            ctx,
            &mut projected,
            gui.xml.as_bytes(),
            &crate::gui::GuiSources {
                entries: &entries,
                objects: &objects,
                properties: &typed_properties,
                payloads: &payloads,
                topology_occurrences: &topology_occurrences,
                requires_alpha_conversion: crate::gui::requires_alpha_conversion(version),
            },
            &gui.origins,
        )?;
        losses.extend(graph.losses);
    }
    super::body_styles::apply(&mut projected, &payloads, &typed_properties)?;
    crate::attributes::transfer(ctx, &mut projected, &objects, &typed_properties)?;
    Ok(Some((projected, losses)))
}

#[cfg(test)]
mod tests;
