// SPDX-License-Identifier: Apache-2.0
//! Tests: section solver constraints require complete unique semantics.

use std::collections::BTreeMap;

use cadmpeg_ir::features::{Angle, Length, ParameterId};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchCoordinateAxis, SketchEntityId, SketchGeometry, SketchId,
    SketchLocus, SketchNativeOperand,
};

use super::{section_skamp_constraints, synchronize_segment_count, synchronize_skamp_count};
use crate::decode::records::sketch_section_point_records;
use crate::decode::sketch::{
    resolved_section_coordinates, resolved_section_points, resolved_section_radii,
    resolved_section_reference_line_geometry, section_centered_line_geometry,
    section_line_fixed_coordinate, section_point_row_geometry, section_reference_line_geometry,
    section_skamp_point_on_line, section_skamp_saved_point_on_line,
    section_skamp_selected_point_id, unique_section_skamp_segment,
};
use crate::decode::sketch_transfer::{
    ambiguous_section_segment_external_ids, joined_relation_incidence, relation_incidence,
    section_dimension_constraints, section_entity_external_ids, section_segment_identity_suffix,
    section_skamp_constraints_for_geometry, section_skamp_endpoint, section_skamp_is_circular,
    section_skamp_is_line, section_skamp_is_point, section_skamp_locus, section_skamp_midpoint,
    solver_only_section_entities, solver_only_section_entity_family,
    unique_section_segment_external_ids, SectionEntityIncidenceFamily,
};

#[test]
fn section_solver_constraints_require_complete_unique_semantics() {
    include!("section_solver_constraints_require_complete_unique_semantics/body.inc");
}
