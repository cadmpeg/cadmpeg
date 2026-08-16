// SPDX-License-Identifier: Apache-2.0
//! Sketch constraint emission and sketch arena transfer.

// Barrel re-exports are consumed by sibling modules and decode tests.
#![allow(unused_imports)]

mod constraints;
mod entities;
mod identity;
mod loci;
mod profiles;
mod recipe;
mod skamp_constraints;
mod transfer;

pub(super) use recipe::{
    current_additive_feature_recipe, current_feature_operation, current_feature_recipe,
    current_feature_recipe_parent, feature_is_first_material_operation, feature_recipe,
    feature_recipe_effect, feature_revolution_extent, feature_row_schema_classes,
    feature_schema_class, feature_section_sweep_semantics_conflict,
    first_material_feature_by_definition_order, resolved_feature_schema_class_from_classes,
    row_feature_schema_classes, unique_feature_revolution_extent_kind,
};

pub(super) use constraints::{
    circular_dimension_constraint, close_sketch_constraint_parameter_references,
    joined_relation_incidence, joined_relation_incidence_entities, joined_relation_incidence_link,
    native_section_segment_radius_definition, native_section_segment_verhor_definition,
    reconcile_constraint_entity_references, reconcile_constraint_parameter_reference,
    relation_incidence, relation_incidence_entities, relation_incidence_loci,
    section_angular_entities, section_dimension_constraints,
    section_equation_equal_distance_constraints, section_equation_point_on_line_constraints,
    section_equation_same_coordinate_constraints, section_equation_unsigned_distance_constraints,
    section_linear_distance_vectors, section_segment_radius_constraints,
    section_segment_verhor_definition, section_solver_equation_is_disabled,
    section_solver_relation_is_disabled,
};

pub(super) use loci::{
    active_complete_section_skamps, complete_section_coordinate, complete_section_skamps,
    oriented_arc_midpoint, saved_arc_midpoint, section_degenerate_axis_line,
    section_entity_family_locus, section_incidence_curve_locus, section_point_locus,
    section_saved_entity, section_skamp_active, section_skamp_arc_midpoint,
    section_skamp_arc_midpoint_source, section_skamp_center_entity, section_skamp_circular_entity,
    section_skamp_curve_entity, section_skamp_endpoint, section_skamp_incidence_locus,
    section_skamp_is_arc, section_skamp_is_circular, section_skamp_is_line, section_skamp_is_point,
    section_skamp_line_midpoint_sources, section_skamp_line_pair, section_skamp_locus,
    section_skamp_midpoint, section_skamp_oriented_line, section_skamp_point_locus,
    section_skamp_same_coordinate, section_skamp_same_coordinate_axis,
    section_skamp_same_coordinate_sources, section_skamp_shared_endpoint,
    section_skamp_tangent_loci, unique_bounded_curve_segment, unique_centered_line_segment,
    unique_circle_segment, unique_point_segment, unique_reference_line_segment,
};

#[cfg(test)]
pub(super) use skamp_constraints::sketch_constraint_loci_compatible;
pub(super) use skamp_constraints::{
    section_skamp_constraints_for_geometry, sketch_constraint_loci_compatible_with_policy,
};

pub(super) use identity::{
    ambiguous_section_segment_external_ids, materialized_saved_section_external_ids,
    opaque_section_segment_identity_suffix, saved_section_entity_identity,
    saved_section_entity_is_elided_prototype, saved_section_external_id,
    section_entity_external_ids, section_segment_external_id_counts,
    section_segment_identity_suffix, semantic_saved_section_entities,
    unique_saved_section_internal_ids, unique_section_segment_external_ids,
    unresolved_saved_section_entity, SavedSectionEntityKind,
};

pub(super) use profiles::{
    normalize_section_incidence_curve_family_evidence, resolved_profile_chains,
    resolved_segment_profile_chains, section_incidence_curve_family_evidence,
    section_skamp_has_proven_point_locus, solver_only_section_entities,
    solver_only_section_entity_family, unique_section_incidence_curve_family,
    SectionEntityIncidenceFamily,
};

pub(super) use transfer::transfer_sketches;
