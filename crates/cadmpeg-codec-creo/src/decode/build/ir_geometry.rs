// SPDX-License-Identifier: Apache-2.0
//! Scanned analytic, sketch, and B-rep transfer plus coverage counters.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::AnnotationBuilder;

use crate::container::ContainerScan;

use super::super::analytic::{
    reconcile_support_apex_cone_parameter_branches, retain_unresolved_surface_carriers,
    transfer_analytic_pcurve_carriers, transfer_topology_bound_planes,
};
use super::super::coverage::{
    curve_transfer_coverage, design_constraint_transfer_coverage, surface_transfer_coverage,
};
use super::super::feature_history::{
    feature_relation_table_expected_rows, feature_relation_table_missing_rows,
    feature_solver_table_missing_rows, transfer_resolved_extrusion_vertex_orbit_curves,
    transfer_resolved_revolution_surfaces, transfer_resolved_revolution_vertex_orbit_curves,
};
use super::super::sketch_transfer::transfer_sketches;
use super::super::surfaces::{
    transfer_active_datum_cylinders, transfer_cap_pair_cylinders,
    transfer_carrier_intersection_curves, transfer_circular_sweep_cylinders,
    transfer_constrained_slot_fillet_cylinders, transfer_cross_section_planes,
    transfer_fc05_cap_circles, transfer_first_instance_prototype_surfaces, transfer_hole_cylinders,
    transfer_legacy_ascii_surface_carriers, transfer_native_brep, transfer_nurbs_boundary_curves,
    transfer_paired_envelope_spheres, transfer_part_product, transfer_positional_cones,
    transfer_positional_cylinders, transfer_positional_line_extrusion_planes,
    transfer_positional_spline_replays, transfer_positional_tori, transfer_rowless_round_cylinders,
    transfer_split_outline_cylinders, transfer_tabulated_cylinder_spline_extrusions,
    BrepTransferDiagnostics, NativeBrepTransferSummary,
};
use super::super::sweep::{
    transfer_feature_extrusion_surfaces, transfer_resolved_circular_extrusion_breps,
    transfer_resolved_extrusion_breps, transfer_resolved_revolution_breps,
    transfer_saved_spline_curves,
};

pub(super) fn transfer_and_record_scanned_geometry(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    coverage: &mut cadmpeg_ir::Coverage,
    brep_diagnostics: &mut BrepTransferDiagnostics,
) -> Result<(), CodecError> {
    let cross_section_plane_count = transfer_cross_section_planes(scan, ir, annotations);
    let first_instance_prototype_surface_count =
        transfer_first_instance_prototype_surfaces(scan, ir, annotations);
    let positional_spline_replay_count = transfer_positional_spline_replays(scan, ir, annotations);
    let legacy_ascii_surface_carrier_count =
        transfer_legacy_ascii_surface_carriers(scan, ir, annotations);
    let legacy_torus_sphere_carrier_count = scan
        .surfaces
        .legacy_carriers
        .iter()
        .filter(|carrier| {
            matches!(
                carrier.geometry,
                crate::legacy_geometry::LegacySurfaceGeometry::Torus { .. }
                    | crate::legacy_geometry::LegacySurfaceGeometry::Sphere { .. }
            )
        })
        .count();
    let paired_envelope_sphere_count = transfer_paired_envelope_spheres(scan, ir, annotations);
    let positional_torus_count = transfer_positional_tori(scan, ir, annotations);
    let positional_line_extrusion_plane_count =
        transfer_positional_line_extrusion_planes(scan, ir, annotations);
    let tabulated_cylinder_spline_extrusion_count =
        transfer_tabulated_cylinder_spline_extrusions(scan, ir, annotations);
    transfer_fc05_cap_circles(scan, ir, annotations);
    transfer_cap_pair_cylinders(scan, ir, annotations);
    let saved_spline_curve_count = transfer_saved_spline_curves(scan, ir, annotations);
    let sketch_segment_coverage = transfer_sketches(scan, ir, annotations);
    let feature_revolution_surface_count =
        transfer_resolved_revolution_surfaces(scan, ir, annotations);
    let feature_revolution_vertex_orbit_curve_count =
        transfer_resolved_revolution_vertex_orbit_curves(scan, ir, annotations);
    let feature_extrusion_surface_count =
        transfer_feature_extrusion_surfaces(scan, ir, annotations);
    let feature_extrusion_vertex_orbit_curve_count =
        transfer_resolved_extrusion_vertex_orbit_curves(scan, ir, annotations);
    let active_datum_cylinder_count = transfer_active_datum_cylinders(scan, ir, annotations);
    let circular_sweep_cylinder_count = transfer_circular_sweep_cylinders(scan, ir, annotations);
    let positional_cylinders = transfer_positional_cylinders(scan, ir, annotations);
    let positional_cone_count = transfer_positional_cones(scan, ir, annotations);
    let split_outline_cylinder_count = transfer_split_outline_cylinders(scan, ir, annotations);
    let hole_cylinder_count = transfer_hole_cylinders(scan, ir, annotations);
    let constrained_slot_fillet_cylinder_count =
        transfer_constrained_slot_fillet_cylinders(scan, ir, annotations);
    let rowless_round_cylinder_count = transfer_rowless_round_cylinders(scan, ir, annotations);
    let support_apex_cone_branch_count =
        reconcile_support_apex_cone_parameter_branches(scan, ir, annotations);
    let analytic_pcurve_carriers = transfer_analytic_pcurve_carriers(scan, ir, annotations);
    let analytic_pcurve_carrier_count = analytic_pcurve_carriers.len();
    let nurbs_boundary_curves = transfer_nurbs_boundary_curves(ctx, scan, ir, annotations)?;
    let extrusion_plane_boundary_curve_count = nurbs_boundary_curves.extrusion_plane_count;
    let extrusion_plane_section_generator_curve_count =
        nurbs_boundary_curves.extrusion_plane_section_generator_count;
    let shared_extrusion_generator_curve_count =
        nurbs_boundary_curves.shared_extrusion_generator_count;
    let mut derived_intersection_curves = transfer_carrier_intersection_curves(
        scan,
        ir,
        annotations,
        &nurbs_boundary_curves.endpoint_witnesses,
    );
    derived_intersection_curves.extend(nurbs_boundary_curves.ids.iter().cloned());
    let topology_bound_plane_count = transfer_topology_bound_planes(
        scan,
        ir,
        annotations,
        &nurbs_boundary_curves.endpoint_witnesses,
    );
    derived_intersection_curves.extend(transfer_carrier_intersection_curves(
        scan,
        ir,
        annotations,
        &nurbs_boundary_curves.endpoint_witnesses,
    ));
    let NativeBrepTransferSummary {
        topological_point_count,
        native_topological_edge_count,
        diagnostics,
    } = transfer_native_brep(
        scan,
        ir,
        annotations,
        &derived_intersection_curves,
        &analytic_pcurve_carriers,
        &nurbs_boundary_curves.endpoint_witnesses,
    );
    diagnostics.record_coverage(coverage);
    *brep_diagnostics = diagnostics;
    let feature_revolution_brep_count = transfer_resolved_revolution_breps(scan, ir, annotations);
    let feature_circular_extrusion_brep_count =
        transfer_resolved_circular_extrusion_breps(scan, ir, annotations);
    let feature_extrusion_brep_count = transfer_resolved_extrusion_breps(scan, ir, annotations);
    retain_unresolved_surface_carriers(scan, ir, annotations);
    let transferred_part_product = transfer_part_product(scan, ir, annotations);
    let decoded_feature_skamp_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| relations.skamps.len())
        .sum::<usize>();
    let missing_feature_skamp_row_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| {
            feature_solver_table_missing_rows(
                relations.skamp_header.as_ref(),
                relations.skamps.len(),
            )
        })
        .sum::<usize>();
    let skamp_constraint_coverage =
        design_constraint_transfer_coverage(&ir.model.sketch_constraints, ":skamp:", "creo:skamp:");
    let decoded_feature_relation_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| relations.rows.len())
        .sum::<usize>();
    let missing_feature_relation_row_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(feature_relation_table_missing_rows)
        .sum::<usize>();
    let malformed_feature_relation_table_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .filter(|relations| feature_relation_table_expected_rows(relations).is_none())
        .count();
    let decoded_feature_relation_triple_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| relations.triples.len())
        .sum::<usize>();
    let missing_feature_relation_triple_row_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| {
            feature_solver_table_missing_rows(
                relations.triples_header.as_ref(),
                relations.triples.len(),
            )
        })
        .sum::<usize>();
    let relation_constraint_coverage = design_constraint_transfer_coverage(
        &ir.model.sketch_constraints,
        ":relation:",
        "creo:relation:",
    );
    let equation_constraint_coverage = design_constraint_transfer_coverage(
        &ir.model.sketch_constraints,
        ":equation:",
        "creo:equation:",
    );
    let surface_coverage = surface_transfer_coverage(
        &scan.surfaces.rows,
        &ir.model.surfaces,
        &ir.model.procedural_surfaces,
    );
    let decoded_type24_round_edge_envelope_count = scan
        .surfaces
        .parameters
        .iter()
        .filter_map(|record| {
            let row = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
                .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)?;
            (crate::surface::unique_surface_parameter(
                &scan.surfaces.parameters,
                record.surface_id,
            ) == Some(record))
            .then_some(())?;
            record.type24_round_edge_envelope(row.type_byte)
        })
        .count();
    let curve_coverage = curve_transfer_coverage(&scan.curves.topology_rows, &ir.model.curves);
    {
        coverage.record(
            crate::coverage::UNIQUE_VISIBLE_SURFACE_ROW_COUNT,
            surface_coverage.unique_rows,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_VISIBLE_SURFACE_ROW_COUNT,
            surface_coverage.transferred_rows,
        );
        coverage.record(
            crate::coverage::RETAINED_UNKNOWN_VISIBLE_SURFACE_ROW_COUNT,
            surface_coverage.retained_unknown_rows,
        );
        coverage.record(
            crate::coverage::UNTRANSFERRED_VISIBLE_SURFACE_ROW_COUNT,
            surface_coverage
                .unique_rows
                .saturating_sub(surface_coverage.transferred_rows),
        );
        coverage.record(
            crate::coverage::AMBIGUOUS_VISIBLE_SURFACE_ROW_COUNT,
            surface_coverage.ambiguous_rows,
        );
        for kind in crate::decode::coverage::SURFACE_KINDS {
            let (rows, transferred) = surface_coverage.family(kind);
            let keys = crate::coverage::surface_family_keys(kind);
            coverage.record(keys.visible, rows);
            coverage.record(keys.transferred, transferred);
            coverage.record(keys.untransferred, rows.saturating_sub(transferred));
            coverage.record(keys.retained_unknown, surface_coverage.unknown_family(kind));
        }
        coverage.record(
            crate::coverage::UNIQUE_VISIBLE_CURVE_ROW_COUNT,
            curve_coverage.unique_rows,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_VISIBLE_CURVE_ROW_COUNT,
            curve_coverage.transferred_rows,
        );
        coverage.record(
            crate::coverage::RETAINED_UNKNOWN_VISIBLE_CURVE_ROW_COUNT,
            curve_coverage.retained_unknown_rows,
        );
        coverage.record(
            crate::coverage::UNTRANSFERRED_VISIBLE_CURVE_ROW_COUNT,
            curve_coverage
                .unique_rows
                .saturating_sub(curve_coverage.transferred_rows),
        );
        coverage.record(
            crate::coverage::AMBIGUOUS_VISIBLE_CURVE_ROW_COUNT,
            curve_coverage.ambiguous_rows,
        );
        for (type_byte, (rows, transferred)) in &curve_coverage.by_type {
            coverage.record_hex_byte(
                crate::coverage::VISIBLE_CURVE_TYPE_ROW_COUNT,
                *type_byte,
                *rows,
            );
            coverage.record_hex_byte(
                crate::coverage::TRANSFERRED_VISIBLE_CURVE_TYPE_ROW_COUNT,
                *type_byte,
                *transferred,
            );
            coverage.record_hex_byte(
                crate::coverage::RETAINED_UNKNOWN_VISIBLE_CURVE_TYPE_ROW_COUNT,
                *type_byte,
                curve_coverage
                    .unknown_by_type
                    .get(type_byte)
                    .copied()
                    .unwrap_or_default(),
            );
        }
        coverage.record(
            crate::coverage::TRANSFERRED_CROSS_SECTION_PLANE_COUNT,
            cross_section_plane_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FIRST_INSTANCE_PROTOTYPE_SURFACE_COUNT,
            first_instance_prototype_surface_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_POSITIONAL_SPLINE_REPLAY_COUNT,
            positional_spline_replay_count,
        );
        if legacy_ascii_surface_carrier_count != 0 {
            coverage.record(
                crate::coverage::TRANSFERRED_LEGACY_ASCII_SURFACE_CARRIER_COUNT,
                legacy_ascii_surface_carrier_count,
            );
        }
        if legacy_torus_sphere_carrier_count != 0 {
            coverage.record(
                crate::coverage::DECODED_LEGACY_TORUS_OR_SPHERE_CARRIER_COUNT,
                legacy_torus_sphere_carrier_count,
            );
        }
        coverage.record(
            crate::coverage::TRANSFERRED_PAIRED_ENVELOPE_SPHERE_COUNT,
            paired_envelope_sphere_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_POSITIONAL_TORUS_COUNT,
            positional_torus_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_POSITIONAL_LINE_EXTRUSION_PLANE_COUNT,
            positional_line_extrusion_plane_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_TABULATED_CYLINDER_SPLINE_EXTRUSION_COUNT,
            tabulated_cylinder_spline_extrusion_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_SAVED_SPLINE_CURVE_COUNT,
            saved_spline_curve_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_TOPOLOGICAL_POINT_COUNT,
            topological_point_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_NATIVE_TOPOLOGICAL_EDGE_COUNT,
            native_topological_edge_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_ANALYTIC_PCURVE_CARRIER_COUNT,
            analytic_pcurve_carrier_count,
        );
        if support_apex_cone_branch_count != 0 {
            coverage.record(
                crate::coverage::RECONCILED_SUPPORT_APEX_CONE_PARAMETER_BRANCH_COUNT,
                support_apex_cone_branch_count,
            );
        }
        coverage.record(
            crate::coverage::TRANSFERRED_EXTRUSION_PLANE_BOUNDARY_CURVE_COUNT,
            extrusion_plane_boundary_curve_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_EXTRUSION_PLANE_SECTION_GENERATOR_CURVE_COUNT,
            extrusion_plane_section_generator_curve_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_SHARED_EXTRUSION_GENERATOR_CURVE_COUNT,
            shared_extrusion_generator_curve_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_TOPOLOGY_BOUND_PLANE_SURFACE_COUNT,
            topology_bound_plane_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FEATURE_REVOLUTION_SURFACE_COUNT,
            feature_revolution_surface_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FEATURE_REVOLUTION_VERTEX_ORBIT_CURVE_COUNT,
            feature_revolution_vertex_orbit_curve_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FEATURE_EXTRUSION_SURFACE_COUNT,
            feature_extrusion_surface_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FEATURE_EXTRUSION_VERTEX_ORBIT_CURVE_COUNT,
            feature_extrusion_vertex_orbit_curve_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_CIRCULAR_SWEEP_CYLINDER_COUNT,
            circular_sweep_cylinder_count,
        );
        if active_datum_cylinder_count != 0 {
            coverage.record(
                crate::coverage::TRANSFERRED_ACTIVE_DATUM_CYLINDER_COUNT,
                active_datum_cylinder_count,
            );
        }
        coverage.record(
            crate::coverage::TRANSFERRED_HOLE_CYLINDER_COUNT,
            hole_cylinder_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_POSITIONAL_CYLINDER_COUNT,
            positional_cylinders.transferred,
        );
        coverage.record(
            crate::coverage::ROUND_EDGE_COMPLETE_ENVELOPE_COUNT,
            positional_cylinders.round_edge_complete_envelopes,
        );
        coverage.record(
            crate::coverage::ROUND_EDGE_MISSING_SUPPORT_PLANE_COUNT,
            positional_cylinders.round_edge_missing_support_planes,
        );
        coverage.record(
            crate::coverage::ROUND_EDGE_UNSOLVED_CARRIER_COUNT,
            positional_cylinders.round_edge_unsolved_carriers,
        );
        coverage.record(
            crate::coverage::ROUND_EDGE_SOLVED_CARRIER_COUNT,
            positional_cylinders.round_edge_solved_carriers,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_ROUND_EDGE_CARRIER_COUNT,
            positional_cylinders.round_edge_transferred_carriers,
        );
        coverage.record(
            crate::coverage::ROUND_EDGE_NO_PERPENDICULAR_SUPPORT_PAIR_COUNT,
            positional_cylinders.round_edge_no_perpendicular_support_pair,
        );
        coverage.record(
            crate::coverage::ROUND_EDGE_ENDPOINT_INCIDENCE_MISMATCH_COUNT,
            positional_cylinders.round_edge_endpoint_incidence_mismatch,
        );
        coverage.record(
            crate::coverage::ROUND_EDGE_RADIUS_PROJECTION_MISMATCH_COUNT,
            positional_cylinders.round_edge_radius_projection_mismatch,
        );
        coverage.record(
            crate::coverage::ROUND_EDGE_NONUNIQUE_RADIUS_COUNT,
            positional_cylinders.round_edge_nonunique_radius,
        );
        coverage.record(
            crate::coverage::ROUND_EDGE_CARRIER_VALIDATION_FAILURE_COUNT,
            positional_cylinders.round_edge_carrier_validation_failure,
        );
        coverage.record(
            crate::coverage::ROUND_EDGE_REPLAY_CONFLICT_COUNT,
            positional_cylinders.round_edge_replay_conflict,
        );
        coverage.record(
            crate::coverage::AXIAL_INTERVAL_CORNER_ENVELOPE_COUNT,
            positional_cylinders.axial_interval_corner_envelopes,
        );
        coverage.record(
            crate::coverage::AXIAL_INTERVAL_CORNER_SOLVED_CARRIER_COUNT,
            positional_cylinders.axial_interval_corner_solved_carriers,
        );
        coverage.record(
            crate::coverage::DECODED_TYPE24_ROUND_EDGE_ENVELOPE_COUNT,
            decoded_type24_round_edge_envelope_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_POSITIONAL_CONE_COUNT,
            positional_cone_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_SPLIT_OUTLINE_CYLINDER_COUNT,
            split_outline_cylinder_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_CONSTRAINED_SLOT_FILLET_CYLINDER_COUNT,
            constrained_slot_fillet_cylinder_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_ROWLESS_ROUND_CYLINDER_COUNT,
            rowless_round_cylinder_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FEATURE_REVOLUTION_BREP_COUNT,
            feature_revolution_brep_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FEATURE_CIRCULAR_EXTRUSION_BREP_COUNT,
            feature_circular_extrusion_brep_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FEATURE_EXTRUSION_BREP_COUNT,
            feature_extrusion_brep_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_PART_PRODUCT_COUNT,
            usize::from(transferred_part_product),
        );
        coverage.record(
            crate::coverage::DECODED_FEATURE_SEGMENT_ROW_COUNT,
            sketch_segment_coverage.decoded_rows,
        );
        coverage.record(
            crate::coverage::RESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT,
            sketch_segment_coverage.resolved_geometry,
        );
        coverage.record(
            crate::coverage::UNRESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT,
            sketch_segment_coverage
                .decoded_rows
                .saturating_sub(sketch_segment_coverage.resolved_geometry),
        );
        for (family, (decoded, resolved)) in sketch_segment_coverage.families() {
            let keys = crate::coverage::sketch_segment_keys(family);
            coverage.record(keys.decoded, decoded);
            coverage.record(keys.resolved, resolved);
            coverage.record(keys.unresolved, decoded.saturating_sub(resolved));
        }
        coverage.record(
            crate::coverage::MISSING_FEATURE_SEGMENT_ROW_COUNT,
            sketch_segment_coverage.missing_rows,
        );
        coverage.record(
            crate::coverage::DECODED_FEATURE_SKAMP_COUNT,
            decoded_feature_skamp_count,
        );
        coverage.record(
            crate::coverage::MISSING_FEATURE_SKAMP_ROW_COUNT,
            missing_feature_skamp_row_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FEATURE_SKAMP_CONSTRAINT_COUNT,
            skamp_constraint_coverage.transferred,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_NATIVE_FEATURE_SKAMP_CONSTRAINT_COUNT,
            skamp_constraint_coverage.native,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_TYPED_FEATURE_SKAMP_CONSTRAINT_COUNT,
            skamp_constraint_coverage.typed(),
        );
        coverage.record(
            crate::coverage::ACTIVE_FEATURE_SKAMP_CONSTRAINT_COUNT,
            skamp_constraint_coverage.active,
        );
        coverage.record(
            crate::coverage::ACTIVE_NATIVE_FEATURE_SKAMP_CONSTRAINT_COUNT,
            skamp_constraint_coverage.active_native,
        );
        coverage.record(
            crate::coverage::ACTIVE_TYPED_FEATURE_SKAMP_CONSTRAINT_COUNT,
            skamp_constraint_coverage.active_typed(),
        );
        for (kind, count) in &skamp_constraint_coverage.native_by_kind {
            coverage.record_indexed(
                crate::coverage::TRANSFERRED_NATIVE_FEATURE_SKAMP_TYPE_CONSTRAINT_COUNT,
                *kind,
                *count,
            );
        }
        for (kind, count) in &skamp_constraint_coverage.active_native_by_kind {
            coverage.record_indexed(
                crate::coverage::ACTIVE_NATIVE_FEATURE_SKAMP_TYPE_CONSTRAINT_COUNT,
                *kind,
                *count,
            );
        }
        coverage.record(
            crate::coverage::DECODED_FEATURE_RELATION_COUNT,
            decoded_feature_relation_count,
        );
        coverage.record(
            crate::coverage::MISSING_FEATURE_RELATION_ROW_COUNT,
            missing_feature_relation_row_count,
        );
        coverage.record(
            crate::coverage::MALFORMED_FEATURE_RELATION_TABLE_COUNT,
            malformed_feature_relation_table_count,
        );
        coverage.record(
            crate::coverage::DECODED_FEATURE_RELATION_TRIPLE_COUNT,
            decoded_feature_relation_triple_count,
        );
        coverage.record(
            crate::coverage::MISSING_FEATURE_RELATION_TRIPLE_ROW_COUNT,
            missing_feature_relation_triple_row_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FEATURE_RELATION_CONSTRAINT_COUNT,
            relation_constraint_coverage.transferred,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_NATIVE_FEATURE_RELATION_CONSTRAINT_COUNT,
            relation_constraint_coverage.native,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_TYPED_FEATURE_RELATION_CONSTRAINT_COUNT,
            relation_constraint_coverage.typed(),
        );
        coverage.record(
            crate::coverage::ACTIVE_FEATURE_RELATION_CONSTRAINT_COUNT,
            relation_constraint_coverage.active,
        );
        coverage.record(
            crate::coverage::ACTIVE_NATIVE_FEATURE_RELATION_CONSTRAINT_COUNT,
            relation_constraint_coverage.active_native,
        );
        coverage.record(
            crate::coverage::ACTIVE_TYPED_FEATURE_RELATION_CONSTRAINT_COUNT,
            relation_constraint_coverage.active_typed(),
        );
        for (kind, count) in &relation_constraint_coverage.native_by_kind {
            coverage.record_indexed(
                crate::coverage::TRANSFERRED_NATIVE_FEATURE_RELATION_TYPE_CONSTRAINT_COUNT,
                *kind,
                *count,
            );
        }
        for (kind, count) in &relation_constraint_coverage.active_native_by_kind {
            coverage.record_indexed(
                crate::coverage::ACTIVE_NATIVE_FEATURE_RELATION_TYPE_CONSTRAINT_COUNT,
                *kind,
                *count,
            );
        }
        if equation_constraint_coverage.transferred != 0 {
            coverage.record(
                crate::coverage::TRANSFERRED_FEATURE_EQUATION_CONSTRAINT_COUNT,
                equation_constraint_coverage.transferred,
            );
            coverage.record(
                crate::coverage::TRANSFERRED_NATIVE_FEATURE_EQUATION_CONSTRAINT_COUNT,
                equation_constraint_coverage.native,
            );
            coverage.record(
                crate::coverage::TRANSFERRED_TYPED_FEATURE_EQUATION_CONSTRAINT_COUNT,
                equation_constraint_coverage.typed(),
            );
            coverage.record(
                crate::coverage::ACTIVE_FEATURE_EQUATION_CONSTRAINT_COUNT,
                equation_constraint_coverage.active,
            );
            coverage.record(
                crate::coverage::ACTIVE_NATIVE_FEATURE_EQUATION_CONSTRAINT_COUNT,
                equation_constraint_coverage.active_native,
            );
            coverage.record(
                crate::coverage::ACTIVE_TYPED_FEATURE_EQUATION_CONSTRAINT_COUNT,
                equation_constraint_coverage.active_typed(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::AnnotationBuilder;

    use crate::decode::surfaces::BrepTransferDiagnostics;

    use super::transfer_and_record_scanned_geometry;

    #[test]
    fn intersections_revisit_carriers_proven_by_topology_bound_planes() {
        let mut scan = crate::container::scan_bytes(Vec::new());
        scan.surfaces.rows = vec![
            crate::surface::SurfaceRow {
                id: 5,
                type_byte: 0x22,
                kind: crate::surface::SurfaceKind::Plane,
                feature_id: 1,
                reversed: false,
                boundary_type: 1,
                next_surface: 0,
                offset: 10,
            },
            crate::surface::SurfaceRow {
                id: 6,
                type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
                kind: crate::surface::SurfaceKind::Cylinder,
                feature_id: 1,
                reversed: false,
                boundary_type: 0,
                next_surface: 0,
                offset: 11,
            },
        ];
        scan.curves.topology_rows = vec![
            crate::curve::CurveTopologyRow {
                id: 10,
                type_byte: 0,
                feature_id: 1,
                directions: [0; 2],
                faces: [5, 0],
                next_edges: [10, 0],
                offset: 20,
            },
            crate::curve::CurveTopologyRow {
                id: 12,
                type_byte: 0,
                feature_id: 1,
                directions: [0; 2],
                faces: [5, 6],
                next_edges: [12, 12],
                offset: 21,
            },
        ];
        scan.topology.loops.push(crate::topology::Loop {
            face_id: 5,
            half_edges: vec![crate::topology::HalfEdgeId {
                curve_id: 10,
                side: 0,
            }],
        });

        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            id: CurveId::mint("creo:visibgeom:curve#10".to_string()).expect("identity grammar"),
            geometry: CurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 4.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 5.0,
            },
            source_object: None,
        });
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#6".to_string()).expect("identity grammar"),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 5.0,
            },
            source_object: None,
        });

        let arena = DecodeArena::new();
        let bytes = [0_u8];
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("test decode context");
        let mut annotations = AnnotationBuilder::new();
        let mut coverage = cadmpeg_ir::Coverage::default();
        let mut brep_diagnostics = BrepTransferDiagnostics::default();
        transfer_and_record_scanned_geometry(
            &ctx,
            &scan,
            &mut ir,
            &mut annotations,
            &mut coverage,
            &mut brep_diagnostics,
        )
        .expect("synthetic geometry transfer");

        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| {
                curve.id
                    == CurveId::mint("creo:visibgeom:curve#12".to_string())
                        .expect("identity grammar")
            })
            .expect("plane-cylinder intersection curve");
        let CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        } = &curve.geometry
        else {
            panic!("expected exact plane-cylinder circle");
        };
        assert_eq!(*center, Point3::new(0.0, 0.0, 4.0));
        assert_eq!(*axis, Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(*radius, 5.0);
        assert_eq!(
            coverage.get("transferred_topology_bound_plane_surface_count"),
            Some(&1)
        );
    }
}
