// SPDX-License-Identifier: Apache-2.0
//! Decode-coverage counters for transferred features and numeric carriers.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BooleanOp, ChamferSpec, EdgeSelection, ExtrudeExtent, ExtrudeStart, FaceSelection,
    FeatureDefinition as IrFeatureDefinition, HoleKind, ProfileRef, RadiusForm, RadiusSpec,
    RevolveExtent,
};

use crate::container::ContainerScan;

use super::super::feature_history::replayed_torus_minor_radius;
use super::ir::{
    body_selection_has_unresolved_operands, face_selection_has_unresolved_operands,
    pattern_kind_has_unresolved_operands, surface_boundary_has_unresolved_operands,
    termination_has_unresolved_operands,
};

/// Count transferred features by kind and record the counts in `coverage`.
///
/// Walks the transferred feature list once, classifying each feature by its
/// definition and by whether its operands resolved, then writes one
/// `transferred_*` entry per counted kind. Reads the built model; emits no
/// entities and no annotations.
pub(in super::super) fn collect_feature_coverage(
    scan: &ContainerScan,
    ir: &CadIr,
    geometry_generator_feature_count: usize,
    feature_result_topology_count: usize,
    feature_result_edge_count: usize,
    coverage: &mut cadmpeg_ir::Coverage,
) {
    let native_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| matches!(feature.definition, IrFeatureDefinition::Native { .. }))
        .count();
    let mut unresolved_datum_plane_feature_count = 0;
    let mut unresolved_datum_coordinate_system_feature_count = 0;
    let mut unresolved_boundary_surface_feature_count = 0;
    let mut extrude_feature_count = 0;
    let mut incomplete_extrude_feature_count = 0;
    let mut unresolved_extrude_profile_feature_count = 0;
    let mut native_extrude_profile_feature_count = 0;
    let mut incomplete_extrude_start_feature_count = 0;
    let mut incomplete_extrude_termination_feature_count = 0;
    let mut unresolved_extrude_boolean_operation_feature_count = 0;
    let mut revolve_feature_count = 0;
    let mut incomplete_revolve_feature_count = 0;
    let mut unresolved_revolve_profile_feature_count = 0;
    let mut native_revolve_profile_feature_count = 0;
    let mut unresolved_revolve_axis_feature_count = 0;
    let mut incomplete_revolve_extent_feature_count = 0;
    let mut unresolved_revolve_boolean_operation_feature_count = 0;
    let mut hole_feature_count = 0;
    let mut incomplete_hole_feature_count = 0;
    let mut unresolved_hole_location_feature_count = 0;
    let mut unresolved_hole_profile_feature_count = 0;
    let mut native_hole_profile_feature_count = 0;
    let mut unresolved_hole_face_selection_feature_count = 0;
    let mut native_hole_face_selection_feature_count = 0;
    let mut unresolved_hole_direction_feature_count = 0;
    let mut unresolved_hole_kind_feature_count = 0;
    let mut unresolved_hole_diameter_feature_count = 0;
    let mut incomplete_hole_termination_feature_count = 0;
    let mut fillet_feature_count = 0;
    let mut incomplete_fillet_feature_count = 0;
    let mut unresolved_fillet_edge_selection_feature_count = 0;
    let mut native_fillet_edge_selection_feature_count = 0;
    let mut unresolved_fillet_radius_feature_count = 0;
    let mut unresolved_fillet_radius_without_generated_surface_feature_count = 0;
    let mut unresolved_fillet_radius_with_generated_surface_feature_count = 0;
    let mut variable_radius_fillet_feature_count = 0;
    let mut chamfer_feature_count = 0;
    let mut incomplete_chamfer_feature_count = 0;
    let mut unresolved_chamfer_edge_selection_feature_count = 0;
    let mut native_chamfer_edge_selection_feature_count = 0;
    let mut unresolved_chamfer_spec_feature_count = 0;
    let mut draft_feature_count = 0;
    let mut incomplete_draft_feature_count = 0;
    let mut explicitly_unresolved_draft_feature_count = 0;
    let mut unresolved_draft_face_selection_feature_count = 0;
    let mut native_draft_face_selection_feature_count = 0;
    let mut unresolved_draft_neutral_plane_feature_count = 0;
    let mut native_draft_neutral_plane_feature_count = 0;
    let mut unresolved_draft_direction_feature_count = 0;
    let mut unresolved_draft_angle_feature_count = 0;
    let mut unresolved_draft_outward_feature_count = 0;
    let mut filled_surface_feature_count = 0;
    let mut incomplete_filled_surface_feature_count = 0;
    let mut unresolved_filled_surface_boundary_feature_count = 0;
    let mut unresolved_filled_surface_support_feature_count = 0;
    let mut unresolved_filled_surface_continuity_feature_count = 0;
    let mut unresolved_filled_surface_merge_feature_count = 0;
    let mut knit_surface_feature_count = 0;
    let mut incomplete_knit_surface_feature_count = 0;
    let mut unresolved_knit_surface_faces_feature_count = 0;
    let mut native_knit_surface_faces_feature_count = 0;
    let mut unresolved_knit_surface_merge_feature_count = 0;
    let mut unresolved_knit_surface_solid_feature_count = 0;
    let mut thicken_feature_count = 0;
    let mut incomplete_thicken_feature_count = 0;
    let mut unresolved_thicken_faces_feature_count = 0;
    let mut unresolved_thicken_thickness_feature_count = 0;
    let mut unresolved_thicken_side_feature_count = 0;
    let mut section_shape_feature_count = 0;
    let mut incomplete_section_shape_feature_count = 0;
    let mut pattern_feature_count = 0;
    let mut incomplete_pattern_feature_count = 0;
    let mut unresolved_pattern_seed_feature_count = 0;
    let mut unresolved_pattern_transform_feature_count = 0;
    let mut native_axis_helix_feature_count = 0;
    for feature in &ir.model.features {
        match &feature.definition {
            IrFeatureDefinition::DatumPlaneUnresolved => {
                unresolved_datum_plane_feature_count += 1;
            }
            IrFeatureDefinition::DatumCoordinateSystemUnresolved => {
                unresolved_datum_coordinate_system_feature_count += 1;
            }
            IrFeatureDefinition::BoundarySurfaceUnresolved => {
                unresolved_boundary_surface_feature_count += 1;
            }
            IrFeatureDefinition::Extrude {
                profile,
                start,
                extent,
                op,
                ..
            } => {
                extrude_feature_count += 1;
                let unresolved_profile = matches!(profile, ProfileRef::Unresolved(_));
                let native_profile = matches!(profile, ProfileRef::Native(_));
                let incomplete_start = matches!(
                    start,
                    ExtrudeStart::FromFace { face, .. }
                        if face_selection_has_unresolved_operands(face)
                );
                let incomplete_termination = match extent {
                    ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                        termination_has_unresolved_operands(&side.termination)
                    }
                    ExtrudeExtent::TwoSided { first, second } => {
                        termination_has_unresolved_operands(&first.termination)
                            || termination_has_unresolved_operands(&second.termination)
                    }
                };
                let unresolved_op = *op == BooleanOp::Unresolved;
                unresolved_extrude_profile_feature_count += usize::from(unresolved_profile);
                native_extrude_profile_feature_count += usize::from(native_profile);
                incomplete_extrude_start_feature_count += usize::from(incomplete_start);
                incomplete_extrude_termination_feature_count += usize::from(incomplete_termination);
                unresolved_extrude_boolean_operation_feature_count += usize::from(unresolved_op);
                incomplete_extrude_feature_count += usize::from(
                    unresolved_profile
                        || native_profile
                        || incomplete_start
                        || incomplete_termination
                        || unresolved_op,
                );
            }
            IrFeatureDefinition::Revolve { construction, op } => {
                revolve_feature_count += 1;
                let unresolved_profile = construction
                    .profile
                    .as_ref()
                    .is_none_or(|profile| matches!(profile, ProfileRef::Unresolved(_)));
                let native_profile = matches!(construction.profile, Some(ProfileRef::Native(_)));
                let unresolved_axis = construction.axis.is_none();
                let incomplete_extent =
                    construction
                        .extent
                        .as_ref()
                        .is_none_or(|extent| match extent {
                            RevolveExtent::OneSided { termination }
                            | RevolveExtent::Symmetric { termination } => {
                                termination_has_unresolved_operands(termination)
                            }
                            RevolveExtent::TwoSided { first, second } => {
                                termination_has_unresolved_operands(first)
                                    || termination_has_unresolved_operands(second)
                            }
                        });
                let unresolved_op = *op == BooleanOp::Unresolved;
                unresolved_revolve_profile_feature_count += usize::from(unresolved_profile);
                native_revolve_profile_feature_count += usize::from(native_profile);
                unresolved_revolve_axis_feature_count += usize::from(unresolved_axis);
                incomplete_revolve_extent_feature_count += usize::from(incomplete_extent);
                unresolved_revolve_boolean_operation_feature_count += usize::from(unresolved_op);
                incomplete_revolve_feature_count += usize::from(
                    unresolved_profile
                        || native_profile
                        || unresolved_axis
                        || incomplete_extent
                        || unresolved_op,
                );
            }
            IrFeatureDefinition::Hole {
                profile,
                face,
                position,
                direction,
                placements,
                kind,
                exit_kind,
                diameter,
                extent,
                ..
            } => {
                hole_feature_count += 1;
                let unresolved_location =
                    profile.is_none() && position.is_none() && placements.is_empty();
                let unresolved_profile = matches!(profile, Some(ProfileRef::Unresolved(_)));
                let native_profile = matches!(profile, Some(ProfileRef::Native(_)));
                let unresolved_face = matches!(
                    face,
                    Some(FaceSelection::Unresolved | FaceSelection::HistoricalPartial { .. })
                );
                let native_face = matches!(face, Some(FaceSelection::Native(_)));
                let unresolved_direction = direction.is_none()
                    && !placements.iter().any(|placement| {
                        matches!(
                            placement,
                            cadmpeg_ir::features::HolePlacement::Directed { .. }
                        )
                    });
                let unresolved_kind = matches!(kind, HoleKind::Unresolved { .. })
                    || matches!(exit_kind, Some(HoleKind::Unresolved { .. }));
                let unresolved_diameter = diameter.is_none();
                let incomplete_termination = extent
                    .as_ref()
                    .is_none_or(termination_has_unresolved_operands);
                unresolved_hole_location_feature_count += usize::from(unresolved_location);
                unresolved_hole_profile_feature_count += usize::from(unresolved_profile);
                native_hole_profile_feature_count += usize::from(native_profile);
                unresolved_hole_face_selection_feature_count += usize::from(unresolved_face);
                native_hole_face_selection_feature_count += usize::from(native_face);
                unresolved_hole_direction_feature_count += usize::from(unresolved_direction);
                unresolved_hole_kind_feature_count += usize::from(unresolved_kind);
                unresolved_hole_diameter_feature_count += usize::from(unresolved_diameter);
                incomplete_hole_termination_feature_count += usize::from(incomplete_termination);
                incomplete_hole_feature_count += usize::from(
                    unresolved_location
                        || unresolved_profile
                        || native_profile
                        || unresolved_face
                        || native_face
                        || unresolved_direction
                        || unresolved_kind
                        || unresolved_diameter
                        || incomplete_termination,
                );
            }
            IrFeatureDefinition::Fillet { groups } => {
                fillet_feature_count += 1;
                let unresolved_edges = groups.is_empty()
                    || groups.iter().any(|group| {
                        matches!(
                            &group.edges,
                            EdgeSelection::Unresolved | EdgeSelection::HistoricalPartial { .. }
                        )
                    });
                let native_edges = groups
                    .iter()
                    .any(|group| matches!(&group.edges, EdgeSelection::Native(_)));
                let unresolved_radius = groups.is_empty()
                    || groups
                        .iter()
                        .any(|group| matches!(&group.radius, RadiusSpec::Unresolved { .. }));
                let variable_radius = groups.iter().any(|group| {
                    matches!(
                        &group.radius,
                        RadiusSpec::Unresolved {
                            form: Some(RadiusForm::Variable)
                        }
                    )
                });
                let has_generated_surface = feature
                    .id
                    .as_str()
                    .strip_prefix("creo:model:feature#")
                    .and_then(|value| value.parse::<u32>().ok())
                    .is_some_and(|feature_id| {
                        scan.surfaces
                            .rows
                            .iter()
                            .any(|row| row.feature_id == feature_id)
                    });
                unresolved_fillet_edge_selection_feature_count += usize::from(unresolved_edges);
                native_fillet_edge_selection_feature_count += usize::from(native_edges);
                unresolved_fillet_radius_feature_count += usize::from(unresolved_radius);
                unresolved_fillet_radius_without_generated_surface_feature_count +=
                    usize::from(unresolved_radius && !has_generated_surface);
                unresolved_fillet_radius_with_generated_surface_feature_count +=
                    usize::from(unresolved_radius && has_generated_surface);
                variable_radius_fillet_feature_count += usize::from(variable_radius);
                incomplete_fillet_feature_count +=
                    usize::from(unresolved_edges || native_edges || unresolved_radius);
            }
            IrFeatureDefinition::Chamfer { groups, .. } => {
                chamfer_feature_count += 1;
                let unresolved_edges = groups.is_empty()
                    || groups.iter().any(|group| {
                        matches!(
                            &group.edges,
                            EdgeSelection::Unresolved | EdgeSelection::HistoricalPartial { .. }
                        )
                    });
                let native_edges = groups
                    .iter()
                    .any(|group| matches!(&group.edges, EdgeSelection::Native(_)));
                let unresolved_spec = groups.is_empty()
                    || groups
                        .iter()
                        .any(|group| matches!(&group.spec, ChamferSpec::Unresolved { .. }));
                unresolved_chamfer_edge_selection_feature_count += usize::from(unresolved_edges);
                native_chamfer_edge_selection_feature_count += usize::from(native_edges);
                unresolved_chamfer_spec_feature_count += usize::from(unresolved_spec);
                incomplete_chamfer_feature_count +=
                    usize::from(unresolved_edges || native_edges || unresolved_spec);
            }
            IrFeatureDefinition::Draft {
                faces,
                neutral_plane,
                pull_direction,
                angle,
                outward,
                ..
            } => {
                draft_feature_count += 1;
                let unresolved_faces = matches!(
                    faces,
                    FaceSelection::Unresolved | FaceSelection::HistoricalPartial { .. }
                );
                let native_faces = matches!(faces, FaceSelection::Native(_));
                let unresolved_neutral_plane = matches!(
                    neutral_plane,
                    FaceSelection::Unresolved | FaceSelection::HistoricalPartial { .. }
                );
                let native_neutral_plane = matches!(neutral_plane, FaceSelection::Native(_));
                let unresolved_direction = pull_direction.is_none();
                let unresolved_angle = angle.is_none();
                let unresolved_outward = outward.is_none();
                unresolved_draft_face_selection_feature_count += usize::from(unresolved_faces);
                native_draft_face_selection_feature_count += usize::from(native_faces);
                unresolved_draft_neutral_plane_feature_count +=
                    usize::from(unresolved_neutral_plane);
                native_draft_neutral_plane_feature_count += usize::from(native_neutral_plane);
                unresolved_draft_direction_feature_count += usize::from(unresolved_direction);
                unresolved_draft_angle_feature_count += usize::from(unresolved_angle);
                unresolved_draft_outward_feature_count += usize::from(unresolved_outward);
                incomplete_draft_feature_count += usize::from(
                    unresolved_faces
                        || native_faces
                        || unresolved_neutral_plane
                        || native_neutral_plane
                        || unresolved_direction
                        || unresolved_angle
                        || unresolved_outward,
                );
            }
            IrFeatureDefinition::DraftUnresolved => {
                draft_feature_count += 1;
                incomplete_draft_feature_count += 1;
                explicitly_unresolved_draft_feature_count += 1;
            }
            IrFeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                continuity,
                merge_result,
                ..
            } => {
                filled_surface_feature_count += 1;
                let unresolved_boundary = surface_boundary_has_unresolved_operands(boundary);
                let unresolved_support = face_selection_has_unresolved_operands(support_faces);
                let unresolved_continuity = continuity.is_none();
                let unresolved_merge = merge_result.is_none();
                unresolved_filled_surface_boundary_feature_count +=
                    usize::from(unresolved_boundary);
                unresolved_filled_surface_support_feature_count += usize::from(unresolved_support);
                unresolved_filled_surface_continuity_feature_count +=
                    usize::from(unresolved_continuity);
                unresolved_filled_surface_merge_feature_count += usize::from(unresolved_merge);
                incomplete_filled_surface_feature_count += usize::from(
                    unresolved_boundary
                        || unresolved_support
                        || unresolved_continuity
                        || unresolved_merge,
                );
            }
            IrFeatureDefinition::KnitSurface {
                faces,
                merge_entities,
                create_solid,
                ..
            } => {
                knit_surface_feature_count += 1;
                let unresolved_faces = matches!(
                    faces,
                    FaceSelection::Unresolved | FaceSelection::HistoricalPartial { .. }
                );
                let native_faces = matches!(faces, FaceSelection::Native(_));
                let unresolved_merge = merge_entities.is_none();
                let unresolved_solid = create_solid.is_none();
                unresolved_knit_surface_faces_feature_count += usize::from(unresolved_faces);
                native_knit_surface_faces_feature_count += usize::from(native_faces);
                unresolved_knit_surface_merge_feature_count += usize::from(unresolved_merge);
                unresolved_knit_surface_solid_feature_count += usize::from(unresolved_solid);
                incomplete_knit_surface_feature_count += usize::from(
                    unresolved_faces || native_faces || unresolved_merge || unresolved_solid,
                );
            }
            IrFeatureDefinition::Thicken {
                faces,
                thickness,
                side,
            } => {
                thicken_feature_count += 1;
                let unresolved_faces = face_selection_has_unresolved_operands(faces);
                let unresolved_thickness = thickness.is_none();
                let unresolved_side = side.is_none();
                unresolved_thicken_faces_feature_count += usize::from(unresolved_faces);
                unresolved_thicken_thickness_feature_count += usize::from(unresolved_thickness);
                unresolved_thicken_side_feature_count += usize::from(unresolved_side);
                incomplete_thicken_feature_count +=
                    usize::from(unresolved_faces || unresolved_thickness || unresolved_side);
            }
            IrFeatureDefinition::SectionShape { first, second, .. } => {
                section_shape_feature_count += 1;
                incomplete_section_shape_feature_count += usize::from(
                    body_selection_has_unresolved_operands(first)
                        || body_selection_has_unresolved_operands(second),
                );
            }
            IrFeatureDefinition::Pattern { seeds, pattern } => {
                pattern_feature_count += 1;
                let unresolved_seeds = seeds.is_empty()
                    || seeds.iter().any(|seed| match seed {
                        cadmpeg_ir::features::PatternSeed::Feature(_) => false,
                        cadmpeg_ir::features::PatternSeed::Faces(faces) => {
                            face_selection_has_unresolved_operands(faces)
                        }
                        cadmpeg_ir::features::PatternSeed::Bodies(bodies) => {
                            matches!(
                                bodies,
                                cadmpeg_ir::features::BodySelection::Unresolved
                                    | cadmpeg_ir::features::BodySelection::Native(_)
                            )
                        }
                        cadmpeg_ir::features::PatternSeed::Occurrences(occurrences) => {
                            occurrences.is_empty()
                        }
                    });
                let unresolved_transform = pattern_kind_has_unresolved_operands(pattern);
                unresolved_pattern_seed_feature_count += usize::from(unresolved_seeds);
                unresolved_pattern_transform_feature_count += usize::from(unresolved_transform);
                incomplete_pattern_feature_count +=
                    usize::from(unresolved_seeds || unresolved_transform);
            }
            IrFeatureDefinition::HelixNativeAxis { .. } => {
                native_axis_helix_feature_count += 1;
            }
            _ => {}
        }
    }
    let explicitly_unresolved_feature_count = unresolved_datum_plane_feature_count
        + unresolved_datum_coordinate_system_feature_count
        + unresolved_boundary_surface_feature_count
        + explicitly_unresolved_draft_feature_count;
    let incomplete_recognized_feature_count = incomplete_hole_feature_count
        + incomplete_fillet_feature_count
        + incomplete_chamfer_feature_count
        + incomplete_draft_feature_count;
    let incomplete_sweep_feature_count =
        incomplete_extrude_feature_count + incomplete_revolve_feature_count;
    let incomplete_surface_operation_feature_count = incomplete_filled_surface_feature_count
        + incomplete_knit_surface_feature_count
        + incomplete_thicken_feature_count;
    let incomplete_other_construction_feature_count = incomplete_section_shape_feature_count
        + incomplete_pattern_feature_count
        + native_axis_helix_feature_count;
    coverage.extend([
        (
            crate::coverage::TRANSFERRED_FEATURE_COUNT,
            ir.model.features.len(),
        ),
        (
            crate::coverage::TRANSFERRED_FEATURE_RESULT_EDGE_COUNT,
            feature_result_edge_count,
        ),
        (
            crate::coverage::TRANSFERRED_FEATURE_RESULT_TOPOLOGY_COUNT,
            feature_result_topology_count,
        ),
        (
            crate::coverage::TRANSFERRED_TYPED_FEATURE_COUNT,
            ir.model.features.len() - native_feature_count,
        ),
        (
            crate::coverage::TRANSFERRED_NATIVE_FEATURE_COUNT,
            native_feature_count,
        ),
        (
            crate::coverage::TRANSFERRED_GEOMETRY_GENERATOR_FEATURE_COUNT,
            geometry_generator_feature_count,
        ),
        (
            crate::coverage::TRANSFERRED_EXPLICITLY_UNRESOLVED_FEATURE_COUNT,
            explicitly_unresolved_feature_count,
        ),
        (
            crate::coverage::TRANSFERRED_INCOMPLETE_SWEEP_FEATURE_COUNT,
            incomplete_sweep_feature_count,
        ),
        (
            crate::coverage::TRANSFERRED_INCOMPLETE_RECOGNIZED_FEATURE_COUNT,
            incomplete_recognized_feature_count,
        ),
        (
            crate::coverage::TRANSFERRED_INCOMPLETE_SURFACE_OPERATION_FEATURE_COUNT,
            incomplete_surface_operation_feature_count,
        ),
        (
            crate::coverage::TRANSFERRED_INCOMPLETE_OTHER_CONSTRUCTION_FEATURE_COUNT,
            incomplete_other_construction_feature_count,
        ),
    ]);
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_DATUM_PLANE_FEATURE_COUNT,
        unresolved_datum_plane_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_DATUM_COORDINATE_SYSTEM_FEATURE_COUNT,
        unresolved_datum_coordinate_system_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_BOUNDARY_SURFACE_FEATURE_COUNT,
        unresolved_boundary_surface_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_EXTRUDE_FEATURE_COUNT,
        extrude_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_EXTRUDE_FEATURE_COUNT,
        incomplete_extrude_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_EXTRUDE_PROFILE_FEATURE_COUNT,
        unresolved_extrude_profile_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_NATIVE_EXTRUDE_PROFILE_FEATURE_COUNT,
        native_extrude_profile_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_EXTRUDE_START_FEATURE_COUNT,
        incomplete_extrude_start_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_EXTRUDE_TERMINATION_FEATURE_COUNT,
        incomplete_extrude_termination_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_EXTRUDE_BOOLEAN_OPERATION_FEATURE_COUNT,
        unresolved_extrude_boolean_operation_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_REVOLVE_FEATURE_COUNT,
        revolve_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_REVOLVE_FEATURE_COUNT,
        incomplete_revolve_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_REVOLVE_PROFILE_FEATURE_COUNT,
        unresolved_revolve_profile_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_NATIVE_REVOLVE_PROFILE_FEATURE_COUNT,
        native_revolve_profile_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_REVOLVE_AXIS_FEATURE_COUNT,
        unresolved_revolve_axis_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_REVOLVE_EXTENT_FEATURE_COUNT,
        incomplete_revolve_extent_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_REVOLVE_BOOLEAN_OPERATION_FEATURE_COUNT,
        unresolved_revolve_boolean_operation_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_HOLE_FEATURE_COUNT,
        hole_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_HOLE_FEATURE_COUNT,
        incomplete_hole_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_HOLE_LOCATION_FEATURE_COUNT,
        unresolved_hole_location_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_HOLE_PROFILE_FEATURE_COUNT,
        unresolved_hole_profile_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_NATIVE_HOLE_PROFILE_FEATURE_COUNT,
        native_hole_profile_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_HOLE_FACE_SELECTION_FEATURE_COUNT,
        unresolved_hole_face_selection_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_NATIVE_HOLE_FACE_SELECTION_FEATURE_COUNT,
        native_hole_face_selection_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_HOLE_DIRECTION_FEATURE_COUNT,
        unresolved_hole_direction_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_HOLE_KIND_FEATURE_COUNT,
        unresolved_hole_kind_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_HOLE_DIAMETER_FEATURE_COUNT,
        unresolved_hole_diameter_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_HOLE_TERMINATION_FEATURE_COUNT,
        incomplete_hole_termination_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_FILLET_FEATURE_COUNT,
        fillet_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_FILLET_FEATURE_COUNT,
        incomplete_fillet_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_FILLET_EDGE_SELECTION_FEATURE_COUNT,
        unresolved_fillet_edge_selection_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_NATIVE_FILLET_EDGE_SELECTION_FEATURE_COUNT,
        native_fillet_edge_selection_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_FILLET_RADIUS_FEATURE_COUNT,
        unresolved_fillet_radius_feature_count,
    );
    coverage.record(crate::coverage::TRANSFERRED_UNRESOLVED_FILLET_RADIUS_WITHOUT_GENERATED_SURFACE_FEATURE_COUNT, unresolved_fillet_radius_without_generated_surface_feature_count);
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_FILLET_RADIUS_WITH_GENERATED_SURFACE_FEATURE_COUNT,
        unresolved_fillet_radius_with_generated_surface_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_VARIABLE_RADIUS_FILLET_FEATURE_COUNT,
        variable_radius_fillet_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_CHAMFER_FEATURE_COUNT,
        chamfer_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_CHAMFER_FEATURE_COUNT,
        incomplete_chamfer_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_CHAMFER_EDGE_SELECTION_FEATURE_COUNT,
        unresolved_chamfer_edge_selection_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_NATIVE_CHAMFER_EDGE_SELECTION_FEATURE_COUNT,
        native_chamfer_edge_selection_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_CHAMFER_SPEC_FEATURE_COUNT,
        unresolved_chamfer_spec_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_DRAFT_FEATURE_COUNT,
        draft_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_DRAFT_FEATURE_COUNT,
        incomplete_draft_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_EXPLICITLY_UNRESOLVED_DRAFT_FEATURE_COUNT,
        explicitly_unresolved_draft_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_DRAFT_FACE_SELECTION_FEATURE_COUNT,
        unresolved_draft_face_selection_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_NATIVE_DRAFT_FACE_SELECTION_FEATURE_COUNT,
        native_draft_face_selection_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_DRAFT_NEUTRAL_PLANE_FEATURE_COUNT,
        unresolved_draft_neutral_plane_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_NATIVE_DRAFT_NEUTRAL_PLANE_FEATURE_COUNT,
        native_draft_neutral_plane_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_DRAFT_DIRECTION_FEATURE_COUNT,
        unresolved_draft_direction_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_DRAFT_ANGLE_FEATURE_COUNT,
        unresolved_draft_angle_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_DRAFT_OUTWARD_FEATURE_COUNT,
        unresolved_draft_outward_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_FILLED_SURFACE_FEATURE_COUNT,
        filled_surface_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_FILLED_SURFACE_FEATURE_COUNT,
        incomplete_filled_surface_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_FILLED_SURFACE_BOUNDARY_FEATURE_COUNT,
        unresolved_filled_surface_boundary_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_FILLED_SURFACE_SUPPORT_FEATURE_COUNT,
        unresolved_filled_surface_support_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_FILLED_SURFACE_CONTINUITY_FEATURE_COUNT,
        unresolved_filled_surface_continuity_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_FILLED_SURFACE_MERGE_FEATURE_COUNT,
        unresolved_filled_surface_merge_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_KNIT_SURFACE_FEATURE_COUNT,
        knit_surface_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_KNIT_SURFACE_FEATURE_COUNT,
        incomplete_knit_surface_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_KNIT_SURFACE_FACES_FEATURE_COUNT,
        unresolved_knit_surface_faces_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_NATIVE_KNIT_SURFACE_FACES_FEATURE_COUNT,
        native_knit_surface_faces_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_KNIT_SURFACE_MERGE_FEATURE_COUNT,
        unresolved_knit_surface_merge_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_KNIT_SURFACE_SOLID_FEATURE_COUNT,
        unresolved_knit_surface_solid_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_THICKEN_FEATURE_COUNT,
        thicken_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_THICKEN_FEATURE_COUNT,
        incomplete_thicken_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_THICKEN_FACES_FEATURE_COUNT,
        unresolved_thicken_faces_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_THICKEN_THICKNESS_FEATURE_COUNT,
        unresolved_thicken_thickness_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_THICKEN_SIDE_FEATURE_COUNT,
        unresolved_thicken_side_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_SECTION_SHAPE_FEATURE_COUNT,
        section_shape_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_SECTION_SHAPE_FEATURE_COUNT,
        incomplete_section_shape_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_PATTERN_FEATURE_COUNT,
        pattern_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_INCOMPLETE_PATTERN_FEATURE_COUNT,
        incomplete_pattern_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_PATTERN_SEED_FEATURE_COUNT,
        unresolved_pattern_seed_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_UNRESOLVED_PATTERN_TRANSFORM_FEATURE_COUNT,
        unresolved_pattern_transform_feature_count,
    );
    coverage.record(
        crate::coverage::TRANSFERRED_NATIVE_AXIS_HELIX_FEATURE_COUNT,
        native_axis_helix_feature_count,
    );
}

#[derive(Default)]
pub(in super::super) struct TorusParameterCoverage {
    pub(super) radius_overrides: usize,
    pub(super) replayed_minor_radii: usize,
    pub(super) outline_extents: usize,
    pub(super) five_coordinate_envelopes: usize,
    pub(super) split_coordinate_envelopes: usize,
}

pub(in super::super) fn torus_parameter_coverage(scan: &ContainerScan) -> TorusParameterCoverage {
    let rows = scan.surfaces.parameters.iter().filter_map(|record| {
        crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .map(|row| (record, row))
    });
    TorusParameterCoverage {
        radius_overrides: rows
            .clone()
            .filter(|(record, row)| record.torus_radius_overrides(row.type_byte).is_some())
            .count(),
        replayed_minor_radii: rows
            .clone()
            .filter(|(record, row)| replayed_torus_minor_radius(scan, row, record).is_some())
            .count(),
        outline_extents: rows
            .clone()
            .filter(|(record, row)| record.torus_outline_frame(row.type_byte).is_some())
            .count(),
        five_coordinate_envelopes: rows
            .clone()
            .filter(|(record, row)| {
                record
                    .type26_five_coordinate_envelope(row.type_byte)
                    .is_some()
            })
            .count(),
        split_coordinate_envelopes: rows
            .filter(|(record, row)| {
                record
                    .type26_split_coordinate_envelope(row.type_byte)
                    .is_some()
            })
            .count(),
    }
}

pub(in super::super) fn legacy_numeric_coverage<T>(
    records: &[crate::legacy::NumericRecord<T>],
) -> (usize, usize, usize) {
    records.iter().fold(
        (0usize, 0usize, 0usize),
        |(scalars, arrays, elements), record| {
            (
                scalars
                    + usize::from(matches!(
                        record.payload,
                        crate::legacy::NumericPayload::Scalar { .. }
                    )),
                arrays
                    + usize::from(matches!(
                        record.payload,
                        crate::legacy::NumericPayload::Array { .. }
                    )),
                elements.saturating_add(
                    usize::try_from(record.payload.element_count()).unwrap_or(usize::MAX),
                ),
            )
        },
    )
}
