// SPDX-License-Identifier: Apache-2.0
//! Decode-owner resolved-sketch and related unit tests.

use crate::decode::sketch::{
    saved_section_missing_line_geometry, section_axis_line_carrier_with_points,
    section_segment_geometry, section_segment_intersection_carrier_with_missing_line,
};
use crate::decode::sketch_transfer::skamp_constraints::section_skamp_constraints_for_geometry;
use crate::decode::sweep::{extruded_geometry_surface, placed_section_geometry_curve};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::sketches::{SketchConstraint, SketchGeometry, SketchId};
use std::collections::BTreeMap;

mod admission;
mod blind_circular;
mod blind_extent;
mod carrier_solver;
mod chamfer;
mod circular_profile;
mod equation_constraints;
mod equation_scalar_propagation;
mod generated_nurbs;
mod generated_nurbs_extent;
mod generated_rectilinear_extent;
mod generated_source;
mod interpolation_spline;
mod numbered_intersect;
mod nurbs_intersection;
mod resolved;
mod saved_line;
mod schema;
mod section_solver_constraints_require_complete_unique_semantics;
mod sketch_curve;
mod transfer_coverage;
mod zero_orientation;

pub(super) fn with_decode_ctx<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default())
        .expect("test decode context");
    run(&ctx)
}

pub(super) fn synchronize_skamp_count(definition: &mut crate::feature::FeatureDefinition) {
    let relations = definition.relations.as_mut().expect("relations");
    let count = u32::try_from(relations.skamps().len()).expect("skamp count");
    relations
        .skamps
        .as_mut()
        .expect("skamp table")
        .header_mut()
        .expect("skamp header")
        .declared_count = count;
}

pub(super) fn declared_solver_rows<T>(
    table: &mut Option<crate::feature::definitions::SolverSubtable<T>>,
) -> &mut Vec<T> {
    match table {
        Some(crate::feature::definitions::SolverSubtable::Declared { rows, .. }) => rows,
        _ => panic!("fixture requires a declared solver table"),
    }
}

pub(super) fn synchronize_segment_count(definition: &mut crate::feature::FeatureDefinition) {
    let segments = definition.segments.as_mut().expect("segments");
    segments.declared_count =
        u32::try_from(segments.rows.ordinary().count()).expect("segment count");
}

pub(super) fn parameter_slot(value: f64) -> crate::surface::SurfaceParameterScalar {
    crate::surface::SurfaceParameterScalar {
        value: Some(value),
        raw: vec![0],
        offset: 0,
    }
}

pub(super) fn class_911_surface_row(
    feature_id: u32,
    id: u32,
    kind: crate::surface::SurfaceKind,
) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        kind,
        feature_id,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }
}

pub(super) fn simple_drilled_recipe_table(feature_id: u32) -> crate::feature::FeatureEntityTable {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(class_id, source_entity_id, None, None),

        entity_id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let entries = vec![
        entry(21, 204, None),
        entry(22, 203, None),
        entry(19, 200, None),
        entry(11, 200, Some(1)),
        entry(13, 200, Some(2)),
        entry(15, 200, Some(3)),
        entry(17, 200, Some(4)),
        entry(23, 204, None),
        entry(24, 203, None),
        entry(12, 200, Some(1)),
        entry(14, 200, Some(2)),
        entry(16, 200, Some(3)),
        entry(18, 200, Some(4)),
        entry(31, 204, None),
        entry(32, 203, None),
        entry(33, 200, Some(1)),
        entry(34, 200, Some(2)),
        entry(35, 200, Some(3)),
        entry(36, 200, Some(4)),
    ];
    crate::feature::FeatureEntityTable {
        surface_ids: std::collections::BTreeSet::new(),
        feature_id,
        table_class_id: 29,
        entries,
        offset: 0,
    }
    .with_surface_ids([11, 12, 13, 14])
}

pub(super) fn simple_drilled_recipe_surface_rows(
    feature_id: u32,
) -> Vec<crate::surface::SurfaceRow> {
    vec![
        class_911_surface_row(feature_id, 11, crate::surface::SurfaceKind::Cone),
        class_911_surface_row(feature_id, 12, crate::surface::SurfaceKind::Cone),
        class_911_surface_row(feature_id, 13, crate::surface::SurfaceKind::Cylinder),
        class_911_surface_row(feature_id, 14, crate::surface::SurfaceKind::Cylinder),
    ]
}

#[cfg(test)]
pub(super) fn section_axis_line_carrier(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let variable_points = definition.variables.as_ref()?.reconciled_points().0;
    section_axis_line_carrier_with_points(&variable_points, segment)
}

#[cfg(test)]
pub(super) fn section_segment_intersection_carrier(
    definition: &crate::feature::FeatureDefinition,
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let missing_line = saved_section_missing_line_geometry(definition);
    let variable_points = definition
        .variables
        .as_ref()
        .map(|variables| variables.reconciled_points().0)
        .unwrap_or_default();
    section_segment_intersection_carrier_with_missing_line(
        definition,
        radii,
        points,
        segment,
        missing_line.as_ref(),
        &variable_points,
    )
}

#[cfg(test)]
pub(super) fn extruded_segment_surface(
    transform: &crate::placement::FeatureSectionTransform,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SurfaceGeometry> {
    extruded_geometry_surface(transform, &section_segment_geometry(points, segment)?)
}

#[cfg(test)]
pub(super) fn placed_section_curve_geometry(
    transform: &crate::placement::FeatureSectionTransform,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<CurveGeometry> {
    placed_section_geometry_curve(transform, &section_segment_geometry(points, segment)?)
}

#[cfg(test)]
pub(super) fn section_skamp_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    section_skamp_constraints_for_geometry(definition, sketch, None)
}
