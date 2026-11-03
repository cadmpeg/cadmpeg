// SPDX-License-Identifier: Apache-2.0
//! Conversion from a PSB container to [`CadIr`].
//!
//! Decode transfers reference geometry, display meshes, features, sketches,
//! and supported native, extrusion, revolution, and circular B-rep routes.
//! Source metadata records the namespace census, units, and layout identity.
//! Preserved native records and geometry sections retain unresolved source
//! geometry. The report states geometry and topology losses for withheld data.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;

use crate::container;

mod analytic;
mod build;
mod coverage;
mod curve_expressions;
mod document_projection;
mod expanded;
pub(crate) mod feature_history;
mod holes;
mod materials;
mod native;
mod native_records;
mod quadratic;
mod records;
mod sketch;
mod sketch_ids;
mod sketch_transfer;
mod source_carriers;
mod surfaces;
mod sweep;
pub(crate) mod uniqueness;

use crate::decode::build::ir::{build_container_ir, build_ir, BuiltIr};
use crate::decode::build::report::build_report;

/// Scope by #209 §1.1: a class is held when the scan located a part of it,
/// even when the transfer failed. The model name locates the root product
/// and occurrence, so the `native_model_name` and `relation_model_name`
/// goldens hold `model` while their geometry fails. Geometry, thumbnail,
/// appearance, and reference sections and rows locate model parts; feature,
/// sketch, equation, and family sources locate design parts.
fn located_scope(scan: &container::ContainerScan<'_>) -> cadmpeg_ir::ContentScope {
    use crate::container::SectionRole;
    let sections = &scan.framing.sections;
    let has_role = |role: SectionRole| sections.iter().any(|section| section.role() == role);
    let has_section = |name: &str| sections.iter().any(|section| section.name() == name);
    let surfaces = &scan.surfaces;
    let curves = &scan.curves;
    let model = scan.framing.model_name.is_some()
        || has_role(SectionRole::PsbGeometry)
        || has_role(SectionRole::Thumbnail)
        || has_section("NeuPrtSld")
        || has_section("NeuAsmSld")
        || has_section("MdlRefInfo")
        || has_section("DEPDB_DATA")
        || !surfaces.rows.is_empty()
        || !surfaces.nonvisible_rows.is_empty()
        || !surfaces.cross_section_rows.is_empty()
        || !surfaces.parameters.is_empty()
        || !surfaces.nonvisible_parameters.is_empty()
        || !surfaces.cross_section_parameters.is_empty()
        || !surfaces.contours.is_empty()
        || !surfaces.nonvisible_contours.is_empty()
        || !surfaces.cross_section_contours.is_empty()
        || surfaces.prototype_count != 0
        || !surfaces.prototype_records.is_empty()
        || !surfaces.nonvisible_prototype_records.is_empty()
        || !surfaces.prototype_field_refusals.is_empty()
        || !surfaces.nonvisible_prototype_field_refusals.is_empty()
        || !surfaces.legacy_carriers.is_empty()
        || !curves.tabulated_cylinder_replays.is_empty()
        || !curves.prototypes.is_empty()
        || !curves.nonvisible_prototypes.is_empty()
        || !curves.cross_section_prototypes.is_empty()
        || !curves.parameters.is_empty()
        || !curves.nonvisible_parameters.is_empty()
        || !curves.expressions.is_empty()
        || !curves.pcurves.is_empty()
        || !curves.two_chart_pcurves.is_empty()
        || !curves.fc_coordinates.is_empty()
        || !curves.fc05_circles.is_empty()
        || !curves.fc05_cylinder_cap_pairs.is_empty()
        || !curves.prototype_pcurves.is_empty()
        || !curves.prototype_topology.is_empty()
        || !curves.bound_prototype_pcurves.is_empty()
        || !curves.topology_rows.is_empty()
        || !curves.nonvisible_topology_rows.is_empty()
        || !curves.cross_section_rows.is_empty()
        || !scan.topology.half_edges.is_empty()
        || !scan.topology.loops.is_empty()
        || !scan.topology.face_components.is_empty()
        || !scan.topology.vertices.is_empty()
        || !scan.topology.half_edge_vertex_incidence.is_empty()
        || !scan.topology.unstatable_vertex_orbits.is_empty()
        || !scan.planes.datum_boundary_planes.is_empty()
        || !scan.planes.local_systems.is_empty()
        || !scan.planes.cross_section_local_systems.is_empty()
        || !scan.planes.envelopes.is_empty()
        || !scan.planes.cross_section_envelopes.is_empty()
        || !scan.planes.outlines.is_empty()
        || !scan.planes.positional_frames.is_empty()
        || !scan.planes.cross_section_outlines.is_empty()
        || !scan.planes.datums.is_empty()
        || !scan.planes.datum_cylinders.is_empty()
        || !scan.references.lines.is_empty()
        || !scan.references.circles.is_empty()
        || !scan.references.conics.is_empty()
        || !scan.references.ellipses.is_empty()
        || !scan.primitives.double_xar_tables.is_empty()
        || !scan.primitives.scalar_arrays.is_empty()
        || !scan.primitives.triangle_strips.is_empty()
        || !scan.loop_arrays.frames.is_empty()
        || !scan.loop_arrays.records.is_empty()
        // A section kept only because its compressed payload would not expand
        // was still located; its failed part is given up (§4.5) and the class
        // stays in scope (§4.1). The retained bytes name both classes (§1.3).
        || !scan.framing.expansion_losses.is_empty();
    let features = &scan.features;
    let design = has_role(SectionRole::ModelData)
        || has_section("MdlStatus")
        || has_section("DEPDB_DATA")
        || has_section("FamilyInf")
        || scan.framing.legacy_family_table.is_some()
        || scan.framing.family_table.is_some()
        || scan.framing.layout.legacy_ascii().is_some()
        || !features.ids.is_empty()
        || !features.rows.is_empty()
        || !features.round_replay_scalars.is_empty()
        || !features.depdb_recipe_rows.is_empty()
        || !features.choices.is_empty()
        || !features.choice_fields.is_empty()
        || !features.geometry_tables.is_empty()
        || !features.loop_history_entries.is_empty()
        || !features.affected_ids.is_empty()
        || !features.replay_affected_ids.is_empty()
        || !features.surface_merge_replay_affected_ids.is_empty()
        || !features.loop_restore_directions.is_empty()
        || !features.revolution_extents.is_empty()
        || !features.definitions.is_empty()
        || !features.section_transforms.is_empty()
        || !features.operation_states.is_empty()
        || !features.operations.is_empty()
        || !features.reference_names.is_empty()
        || !features.entities.is_empty()
        || !features.entity_references.is_empty()
        || !features.entity_tables.is_empty()
        || !features.legacy_rounds.is_empty()
        || !curves.expressions.is_empty()
        || !scan.planes.datums.is_empty()
        || !scan.framing.expansion_losses.is_empty();
    cadmpeg_ir::ContentScope { model, design }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn with_service_context<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test root is admitted");
    run(&ctx)
}

/// Decode a `.prt` stream into an IR document and decode body; the sealed
/// wrapper stamps the report identity from `ir.source`.
///
/// The stream is read from its beginning. When the context requests only the
/// container, the returned IR contains source metadata and preserved geometry
/// sections but no transferred entities.
pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let scan = container::scan_bytes(ctx, root.window())?;
    let classification = crate::dialect::classify(&scan)?;
    // Admit section identities before model construction.
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(scan.framing.sections.len()),
        "admit Creo sections",
    )?;
    // Scope follows the located parts (§1.1): a class whose parts were found
    // but failed stays in scope, so scope is derived before the failed parts
    // are excluded from transfer (§4.5).
    let scope = located_scope(&scan);
    let mut scan = scan;
    container::exclude_incomplete_sections(&mut scan);
    let BuiltIr {
        mut ir,
        annotations,
        unknowns,
        unknown_classes,
        coverage,
        brep_diagnostics,
        transfer_losses,
    } = if ctx.container_only() {
        build_container_ir(ctx, &scan, &classification)?
    } else {
        build_ir(ctx, &scan, &classification)?
    };
    ir.scope = if ctx.container_only() {
        cadmpeg_ir::ContentScope::container()
    } else {
        scope
    };
    let mut body = build_report(
        &scan,
        &classification,
        &ir,
        coverage,
        &brep_diagnostics,
        ctx.container_only(),
    )?;
    body.losses.extend(transfer_losses);
    if !ctx.container_only() {
        document_projection::project(ctx, &scan, &mut ir, &mut body.losses)?;
    }
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::for_decode(ctx, annotations);
    let untyped_marks: Vec<(String, cadmpeg_ir::ArenaClasses)> = unknowns
        .iter()
        .zip(unknown_classes)
        .map(|(record, classes)| (record.id().as_str().to_owned(), classes))
        .collect();
    source_fidelity.attach_native_unknown_records(&mut ir, "creo", unknowns, ctx)?;
    for (id, classes) in untyped_marks {
        source_fidelity.mark_untyped(&id, classes)?;
    }
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}
