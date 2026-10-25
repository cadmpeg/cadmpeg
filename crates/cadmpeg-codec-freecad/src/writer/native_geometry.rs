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
    if ir.scope.model {
        return Ok(None);
    }
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(None);
    };
    let payloads = crate::brep::records::payloads_untracked(namespace)?;
    if payloads.is_empty() {
        return Ok(None);
    }
    let properties = namespace.arena_as::<native::PropertyRecord>("properties")?;
    let property_reader = crate::property_reader::PropertyReader::typed(ctx)
        .with_entries(namespace.arena_as::<crate::native::EntryRecord>("entries")?);
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
    let shape_owners = brep::owners::gui(ctx, Some(namespace))?;
    let mut curves = brep::transfer_text_curves(
        ctx,
        &payloads,
        brep::owners::Properties::new(&typed_properties, &shape_owners),
        None,
    )?;
    let surfaces = brep::transfer_text_surfaces(
        ctx,
        &payloads,
        brep::owners::Properties::new(&typed_properties, &shape_owners),
        &mut curves,
        None,
    )?;
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
        ctx,
        &mut projected,
        &payloads,
        brep::owners::Properties::new(&typed_properties, &shape_owners),
        &mut losses,
    )?;
    crate::topology_transfer::apply_body_names(&mut projected, &payloads, &typed_properties);
    let objects = namespace.arena_as::<native::ObjectRecord>("objects")?;
    let products = namespace.arena_as::<native::ProductNodeRecord>("product_nodes")?;
    let joints = namespace.arena_as::<native::joint::JointRecord>("joints")?;
    let (products, unavailable_products) =
        crate::product::for_projection(products, &typed_properties)?;
    let (definitions, occurrences) = crate::product::transfer_neutral(
        &property_reader,
        &products,
        &joints,
        &objects,
        &typed_properties,
        &payloads,
        &projected.model.bodies,
        &unavailable_products,
    )?;
    projected.model.product_definitions = definitions;
    projected.model.occurrences = occurrences;
    projected.model.assembly_joints = crate::joint::transfer_neutral(
        &joints,
        &projected.model.occurrences,
        &typed_properties,
        &std::collections::BTreeSet::new(),
    )?;
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
    if super::target::projection::gui_xml_lossy(namespace, &mut losses)?.is_some() {
        let documents = namespace.arena_as::<native::DocumentFacts>("document")?;
        let version = documents
            .first()
            .and_then(|document| document.program_version.as_deref());
        let entries = std::collections::BTreeMap::new();
        let graph = crate::gui::project_native(
            &mut projected,
            namespace,
            &crate::gui::GuiSources {
                entries: &entries,
                declared_entries: None,
                declared_objects: None,
                admitted_objects: None,
                entry_parents: None,
                entry_owners: None,
                entry_interpretations: None,
                gui_entry_interpretations: None,
                objects: &objects,
                object_sources: None,
                properties: &typed_properties,
                payloads: &payloads,
                topology_occurrences: &topology_occurrences,
                requires_alpha_conversion: crate::gui::requires_alpha_conversion(version),
            },
        )?;
        losses.extend(graph.losses);
    }
    super::body_styles::apply(&mut projected, &payloads, &typed_properties)?;
    crate::attributes::transfer(&mut projected, &objects, &typed_properties)?;
    Ok(Some((projected, losses)))
}
