// SPDX-License-Identifier: Apache-2.0
//! Tests: section solver constraints require complete unique semantics.

use std::collections::BTreeMap;

use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntityId, SketchGeometry,
    SketchId, SketchLocus, SketchNativeOperand,
};
use cadmpeg_ir::{
    features::ParameterId,
    scalar::{Angle, Length},
};

use super::{
    declared_solver_rows, section_skamp_constraints, synchronize_segment_count,
    synchronize_skamp_count,
};
use crate::decode::records::sketch_section_point_records;
use crate::decode::sketch::coordinates::{resolved_section_coordinates, resolved_section_points};
use crate::decode::sketch::geometry::{
    resolved_section_reference_line_geometry as resolved_section_reference_line_geometry_admitted,
    section_centered_line_geometry, section_point_row_geometry, section_reference_line_geometry,
};
use crate::decode::sketch::radii::resolved_section_radii;
use crate::decode::sketch::skamp::{
    section_line_fixed_coordinate as section_line_fixed_coordinate_admitted,
    section_skamp_point_on_line as section_skamp_point_on_line_admitted,
    section_skamp_saved_point_on_line as section_skamp_saved_point_on_line_admitted,
    section_skamp_selected_point_id, unique_section_skamp_segment,
};

fn resolved_section_reference_line_geometry(
    definition: &crate::feature::definitions::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureReferenceLineSegment,
) -> Option<SketchGeometry> {
    crate::decode::with_test_decode_ctx(|ctx| {
        resolved_section_reference_line_geometry_admitted(
            ctx,
            definition,
            variable_points,
            points,
            segment,
        )
    })
    .expect("test reference line geometry")
}

fn section_line_fixed_coordinate(
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<crate::decode::sketch::axis::SectionAxis> {
    crate::decode::with_test_decode_ctx(|ctx| {
        section_line_fixed_coordinate_admitted(ctx, definition, segment)
    })
    .expect("test fixed-coordinate graph")
}

fn section_skamp_point_on_line(
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Option<(u32, u32, crate::decode::sketch::axis::SectionAxis)> {
    crate::decode::with_test_decode_ctx(|ctx| {
        section_skamp_point_on_line_admitted(ctx, definition, skamp)
    })
    .expect("test point-on-line")
}

fn section_skamp_saved_point_on_line(
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Option<(u32, crate::decode::sketch::axis::SectionAxis, f64)> {
    crate::decode::with_test_decode_ctx(|ctx| {
        section_skamp_saved_point_on_line_admitted(ctx, definition, skamp)
    })
    .expect("test saved point-on-line")
}
use crate::decode::sketch_transfer::constraints::{
    joined_relation_incidence, relation_incidence, section_dimension_constraints,
};
use crate::decode::sketch_transfer::identity::{
    ambiguous_section_segment_external_ids, section_entity_external_ids,
    section_segment_identity_suffix, unique_section_segment_external_ids,
};
use crate::decode::sketch_transfer::loci::{
    section_skamp_endpoint, section_skamp_is_circular, section_skamp_is_line,
    section_skamp_is_point, section_skamp_locus, section_skamp_midpoint,
};
use crate::decode::sketch_transfer::profiles::{
    solver_only_section_entities, solver_only_section_entity_family, SectionEntityIncidenceFamily,
};
use crate::decode::sketch_transfer::skamp_constraints::section_skamp_constraints_for_geometry;

#[test]
fn section_solver_constraints_require_complete_unique_semantics() {
    include!("section_solver_constraints_require_complete_unique_semantics/body.inc");
}
