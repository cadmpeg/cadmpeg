// SPDX-License-Identifier: Apache-2.0
//! Feature-completeness predicates for NX decode.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, BodyTrimSide, BooleanOp, CurveProjectionDirection,
    CurveProjectionDirectionState, Feature, FeatureDefinition, Length, LoftSection, ParameterId,
    TrimRegion,
};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, BTreeSet};

mod operands;
pub(crate) use operands::{
    body_selection_is_incomplete, body_selections_overlap, chamfer_spec_is_incomplete,
    edge_selection_is_incomplete, extrude_extent_is_incomplete, extrude_start_is_incomplete,
    face_selection_is_incomplete, face_selections_overlap, hole_auxiliary_semantics_are_incomplete,
    hole_feature_is_incomplete, loft_section_is_incomplete, path_ref_is_incomplete,
    pattern_feature_is_incomplete, pattern_is_incomplete, pattern_occurrence_count,
    profile_dependency_is_incomplete, profile_ref_is_incomplete, radius_spec_is_incomplete,
    resolved_body_selection_len, revolve_feature_is_incomplete, rib_feature_is_incomplete,
    sweep_mode_is_incomplete, sweep_orientation_is_incomplete,
    termination_dependency_is_incomplete, termination_is_incomplete,
};

/// Orthonormal-frame handedness acceptance for datum CS completeness.
const EPS_ORTHONORMAL_FRAME: f64 = 1.0e-9;
/// Unit-length acceptance for authored feature directions.
const EPS_UNIT_DIRECTION: f64 = 1.0e-9;
/// Perpendicularity acceptance scaled by direction magnitudes.
const EPS_PERPENDICULAR: f64 = 1.0e-9;

pub(crate) fn output_free_native_snapshot(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.outputs.is_empty()
        && feature.name.as_deref() == Some("MASTER SNAPSHOT BODY")
        && matches!(
            &feature.definition,
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Unresolved
            }
        )
        && feature
            .source_properties
            .get("operation_record")
            .is_some_and(|record| !record.trim().is_empty())
}

/// Return whether a feature's primary body is local to the history namespace.
///
/// Offset-store and unbound object-namespace bodies are retained as native
/// feature-local identities. They do not create neutral current-body outputs;
/// the saved segment image remains the only neutral body census.
pub(crate) fn output_free_local_body_construction(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.outputs.is_empty()
        && feature
            .source_properties
            .contains_key("primary_body_reference")
        && !feature
            .source_properties
            .contains_key("primary_body_segment_use")
}

/// Return whether a pattern record is construction-only and has no neutral
/// body-output obligation.
///
/// Pattern construction records without a primary-body field describe the
/// seed and transform graph. A body-affecting pattern has at least one body
/// reference occurrence, even when the occurrence is too ambiguous to become
/// a primary writer. Keep that distinction explicit so an incomplete body
/// binding cannot be mistaken for a construction-only record.
pub(crate) fn output_free_pattern_construction(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.outputs.is_empty()
        && matches!(&feature.definition, FeatureDefinition::Pattern { .. })
        && !feature.source_properties.keys().any(|key| {
            key == "primary_body_reference"
                || key == "primary_body_object_index"
                || key == "primary_body_data_block"
                || key.starts_with("body_reference.")
                || key.starts_with("body_reference_occurrence.")
        })
}

/// Return whether a `TRIMMED_SH` record is a construction-only operation.
///
/// NX uses the typed trim-surface family for records that carry no body
/// occurrence or primary-body field. Those records have no body result to
/// bind; a body marker makes the output obligation explicit again.
pub(crate) fn output_free_trim_surface_construction(
    feature: &cadmpeg_ir::features::Feature,
) -> bool {
    feature.outputs.is_empty()
        && matches!(&feature.definition, FeatureDefinition::TrimSurface { .. })
        && !feature.source_properties.keys().any(|key| {
            key == "primary_body_reference"
                || key == "primary_body_object_index"
                || key == "primary_body_data_block"
                || key.starts_with("body_reference.")
                || key.starts_with("body_reference_occurrence.")
        })
}

pub(crate) fn active_configuration_state_is_incomplete(
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> bool {
    let suppressed_features = configuration.suppressed_features().collect::<BTreeSet<_>>();
    if ir.model.features.iter().any(|feature| {
        feature
            .suppressed
            .is_none_or(|suppressed| suppressed_features.contains(&feature.id) != suppressed)
    }) {
        return true;
    }
    let Some(bodies) = configuration.bodies.resolved() else {
        return true;
    };
    let active_features = if ir.model.features.is_empty() {
        BTreeSet::new()
    } else {
        let Ok(active_features) = crate::native::history::active_feature_closure(ir, bodies) else {
            return true;
        };
        active_features
    };
    if configuration.feature_states.len() != active_features.len() {
        return true;
    }
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<BTreeMap<_, _>>();
    if active_features.iter().any(|id| {
        let (Some(feature), Some(state)) = (features.get(id), configuration.feature_states.get(id))
        else {
            return true;
        };
        state.suppressed
            || state.dependencies != feature.dependencies
            || state.outputs != feature.outputs
            || state.definition != feature.definition
    }) {
        return true;
    }

    configuration.parameter_values.len() != ir.model.parameters.len()
        || ir.model.parameters.iter().any(|parameter| {
            parameter.value.as_ref().is_none_or(|value| {
                configuration.parameter_values.get(&parameter.id) != Some(value)
            })
        })
}

pub(crate) fn datum_plane_is_incomplete(origin: Point3, normal: Vector3, u_axis: Vector3) -> bool {
    !finite_feature_point(origin)
        || !valid_feature_direction(normal)
        || !valid_feature_direction(u_axis)
        || !directions_are_perpendicular(normal, u_axis)
}

pub(crate) fn datum_coordinate_system_is_incomplete(
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    z_axis: Vector3,
) -> bool {
    if !finite_feature_point(origin)
        || !unit_feature_direction(x_axis)
        || !unit_feature_direction(y_axis)
        || !unit_feature_direction(z_axis)
        || !directions_are_perpendicular(x_axis, y_axis)
        || !directions_are_perpendicular(y_axis, z_axis)
        || !directions_are_perpendicular(z_axis, x_axis)
    {
        return true;
    }
    let handedness = x_axis.cross(y_axis).dot(z_axis);
    !handedness.is_finite() || (handedness - 1.0).abs() > EPS_ORTHONORMAL_FRAME
}

pub(crate) fn projected_curve_direction_is_incomplete(direction: CurveProjectionDirection) -> bool {
    match direction {
        CurveProjectionDirection::Vector(direction) => !valid_feature_direction(direction),
        CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved) => true,
        CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal) => false,
    }
}

pub(crate) fn unit_feature_direction(direction: Vector3) -> bool {
    valid_feature_direction(direction) && (direction.norm() - 1.0).abs() <= EPS_UNIT_DIRECTION
}

pub(crate) fn directions_are_perpendicular(first: Vector3, second: Vector3) -> bool {
    let scale = first.norm() * second.norm();
    scale.is_finite() && first.dot(second).abs() <= EPS_PERPENDICULAR * scale
}

pub(crate) fn incomplete_expression_parameters(ir: &CadIr) -> BTreeSet<ParameterId> {
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.owner.clone())
        .collect::<BTreeSet<_>>();
    let mut incomplete = BTreeSet::new();
    for owner in parameter_owners {
        let parameters = ir
            .model
            .parameters
            .iter()
            .filter(|parameter| parameter.owner == owner)
            .collect::<Vec<_>>();
        let mut ids_by_name = BTreeMap::<(&str, Option<&str>), Vec<&ParameterId>>::new();
        for parameter in &parameters {
            ids_by_name
                .entry((
                    parameter.name.as_str(),
                    parameter.properties.get("unit").map(String::as_str),
                ))
                .or_default()
                .push(&parameter.id);
        }
        let expected = parameters
            .iter()
            .map(|parameter| {
                let unit = match parameter.properties.get("unit").map(String::as_str) {
                    None => None,
                    Some(unit @ ("millimeter" | "inch" | "degree")) => Some(unit),
                    Some(_) => return None,
                };
                let [_] = ids_by_name
                    .get(&(parameter.name.as_str(), unit))?
                    .as_slice()
                else {
                    return None;
                };
                let mut seen = BTreeSet::new();
                let dependencies = crate::native::expression_parameter_names(&parameter.expression)
                    .into_iter()
                    .map(|name| {
                        let [dependency] = ids_by_name.get(&(name, unit))?.as_slice() else {
                            return None;
                        };
                        Some((*dependency).clone())
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(
                    dependencies
                        .into_iter()
                        .filter(|dependency| seen.insert(dependency.clone()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let indices = parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| (&parameter.id, index))
            .collect::<BTreeMap<_, _>>();
        let mut emitted = BTreeSet::new();
        let mut evaluated = BTreeMap::<ParameterId, f64>::new();
        while let Some(index) = (0..parameters.len()).find(|index| {
            !emitted.contains(index)
                && expected[*index].as_ref().is_some_and(|dependencies| {
                    dependencies.iter().all(|dependency| {
                        evaluated.contains_key(dependency)
                            && indices
                                .get(dependency)
                                .is_some_and(|index| emitted.contains(index))
                    })
                })
        }) {
            let parameter = parameters[index];
            let unit = parameter.properties.get("unit").map(String::as_str);
            let value =
                crate::native::evaluate_parameterized_expression(&parameter.expression, |name| {
                    let [dependency] = ids_by_name.get(&(name, unit))?.as_slice() else {
                        return None;
                    };
                    evaluated.get(*dependency).copied()
                });
            let stored = match (unit, parameter.value.as_ref()) {
                (
                    Some("millimeter" | "inch"),
                    Some(cadmpeg_ir::features::ParameterValue::Length(value)),
                ) => Some(value.0),
                (Some("degree"), Some(cadmpeg_ir::features::ParameterValue::Angle(value))) => {
                    Some(value.0)
                }
                (None, Some(cadmpeg_ir::features::ParameterValue::Real(value))) => Some(*value),
                (None, Some(cadmpeg_ir::features::ParameterValue::Integer(value))) => {
                    Some(*value as f64)
                }
                _ => None,
            };
            if let Some(native_value) = value {
                let canonical_value = unit.map_or(Some(native_value), |unit| {
                    crate::native::canonical_expression_value(unit, native_value)
                });
                if let (Some(canonical_value), Some(stored)) = (canonical_value, stored) {
                    let tolerance =
                        64.0 * f64::EPSILON * canonical_value.abs().max(stored.abs()).max(1.0);
                    if canonical_value.is_finite()
                        && stored.is_finite()
                        && (canonical_value - stored).abs() <= tolerance
                    {
                        evaluated.insert(parameter.id.clone(), native_value);
                    }
                }
            }
            emitted.insert(index);
        }
        for (index, parameter) in parameters.into_iter().enumerate() {
            if expected[index].as_ref() != Some(&parameter.dependencies)
                || !emitted.contains(&index)
                || !evaluated.contains_key(&parameter.id)
            {
                incomplete.insert(parameter.id.clone());
            }
        }
    }
    incomplete
}

pub(crate) fn trim_surface_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::TrimSurface {
        faces, tool, keep, ..
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || path_ref_is_incomplete(tool)
        || matches!(keep, TrimRegion::Unresolved)
}

pub(crate) fn extend_surface_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::ExtendSurface {
        faces,
        distance,
        method,
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || distance.is_none_or(|distance| !positive_feature_length(distance))
        || matches!(method, cadmpeg_ir::features::SurfaceExtension::Unresolved)
}

pub(crate) fn sew_bodies_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::SewBodies {
        bodies,
        gap_tolerance,
    } = &feature.definition
    else {
        return true;
    };
    body_selection_is_incomplete(bodies)
        || resolved_body_selection_len(bodies).is_some_and(|count| count < 2)
        || gap_tolerance.is_some_and(|tolerance| !positive_feature_length(tolerance))
}

pub(crate) fn combine_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Combine { target, tools, .. } = &feature.definition else {
        return true;
    };
    body_selection_is_incomplete(target)
        || body_selection_is_incomplete(tools)
        || resolved_body_selection_len(target) != Some(1)
        || body_selections_overlap(target, tools)
}

pub(crate) fn trim_bodies_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::TrimBodies {
        targets,
        tools,
        keep,
    } = &feature.definition
    else {
        return true;
    };
    body_selection_is_incomplete(targets)
        || body_selection_is_incomplete(tools)
        || body_selections_overlap(targets, tools)
        || matches!(keep, BodyTrimSide::Unresolved)
}

pub(crate) fn delete_body_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::DeleteBody { bodies, mode } = &feature.definition else {
        return true;
    };
    body_selection_is_incomplete(bodies) || matches!(mode, BodyRetentionMode::Unresolved)
}

pub(crate) fn hole_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Hole {
        profile,
        profile_filter,
        face,
        placements,
        construction,
        exit_kind,
        diameter,
        extent,
        bottom,
        taper_angle,
        ..
    } = &feature.definition
    else {
        return true;
    };
    let (construction_incomplete, specification) = match construction {
        cadmpeg_ir::features::HoleConstruction::Form {
            kind,
            specification,
        } => (
            hole_feature_is_incomplete(
                profile.as_ref(),
                face.as_ref(),
                placements.as_deref(),
                (kind, exit_kind.as_ref()),
                *diameter,
                extent.as_ref(),
            ),
            specification.as_deref(),
        ),
        cadmpeg_ir::features::HoleConstruction::NativeThread {
            major_diameter,
            thread_depth,
            pitch,
            drill_point_angle,
        } => {
            let kind = cadmpeg_ir::features::HoleKind::SimpleDrilled {
                drill_point_angle: *drill_point_angle,
            };
            (
                hole_feature_is_incomplete(
                    profile.as_ref(),
                    face.as_ref(),
                    placements.as_deref(),
                    (&kind, exit_kind.as_ref()),
                    *diameter,
                    extent.as_ref(),
                ) || !positive_feature_length(*major_diameter)
                    || !positive_feature_length(*thread_depth)
                    || pitch.is_some_and(|pitch| !positive_feature_length(pitch))
                    || diameter.is_none_or(|diameter| major_diameter.0 <= diameter.0),
                None,
            )
        }
    };
    construction_incomplete
        || hole_auxiliary_semantics_are_incomplete(
            profile_filter.as_ref(),
            bottom.as_ref(),
            *taper_angle,
            specification,
        )
        || extent.as_ref().is_some_and(|extent| {
            termination_dependency_is_incomplete(extent, &feature.dependencies)
        })
        || profile
            .as_ref()
            .is_some_and(|profile| profile_dependency_is_incomplete(profile, &feature.dependencies))
}

pub(crate) fn chamfer_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Chamfer { groups, .. } = &feature.definition else {
        return true;
    };
    groups.is_empty()
        || groups.iter().any(|group| {
            edge_selection_is_incomplete(&group.edges) || chamfer_spec_is_incomplete(&group.spec)
        })
}

pub(crate) fn fillet_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Fillet { groups } = &feature.definition else {
        return true;
    };
    groups.is_empty()
        || groups.iter().any(|group| {
            edge_selection_is_incomplete(&group.edges)
                || radius_spec_is_incomplete(&group.radius)
                || group
                    .tangency_weight
                    .is_some_and(|weight| !weight.is_finite())
        })
}

pub(crate) fn face_blend_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::FaceBlend {
        first_faces,
        second_faces,
        radius,
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(first_faces)
        || face_selection_is_incomplete(second_faces)
        || face_selections_overlap(first_faces, second_faces)
        || radius_spec_is_incomplete(radius)
}

pub(crate) fn shell_definition_is_incomplete(definition: &FeatureDefinition) -> bool {
    let FeatureDefinition::Shell {
        bodies,
        removed_faces,
        thickness,
        outward,
        mode,
        join,
        resolve_intersections,
        allow_self_intersections,
    } = definition
    else {
        return true;
    };
    bodies.as_ref().is_some_and(body_selection_is_incomplete)
        || face_selection_is_incomplete(removed_faces)
        || thickness.is_none_or(|thickness| !positive_feature_length(thickness))
        || outward.is_none()
        || mode.is_none()
        || join.is_none()
        || resolve_intersections.is_none()
        || allow_self_intersections.is_none()
}

pub(crate) fn offset_surface_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::OffsetSurface { faces, distance } = &feature.definition else {
        return true;
    };
    face_selection_is_incomplete(faces) || distance.is_none_or(|distance| !distance.0.is_finite())
}

pub(crate) fn sphere_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Sphere { center, radius, op } = &feature.definition else {
        return true;
    };
    !finite_feature_point(*center)
        || !positive_feature_length(*radius)
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn thicken_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Thicken {
        faces,
        thickness,
        side,
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || thickness.is_none_or(|thickness| !positive_feature_length(thickness))
        || side.is_none()
}

pub(crate) fn draft_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Draft {
        faces,
        anchor,
        angle,
        outward,
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || match anchor {
            cadmpeg_ir::features::DraftAnchor::NeutralPlane { plane, .. } => {
                face_selection_is_incomplete(plane)
            }
            cadmpeg_ir::features::DraftAnchor::PartingLine { tool, .. } => {
                face_selection_is_incomplete(tool)
            }
        }
        || anchor
            .pull()
            .is_none_or(|pull| !valid_feature_direction(pull.direction))
        || anchor
            .pull()
            .and_then(|pull| pull.plane.as_ref())
            .is_some_and(|plane| plane.as_str().is_empty())
        || angle.is_none_or(|angle| !valid_draft_angle(angle))
        || outward.is_none()
}

pub(crate) fn replace_face_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::ReplaceFace {
        targets,
        replacements,
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(targets)
        || face_selection_is_incomplete(replacements)
        || face_selections_overlap(targets, replacements)
}

pub(crate) fn loft_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Loft {
        sections,
        centerline,
        guides,
        op,
        max_degree,
        ..
    } = &feature.definition
    else {
        return true;
    };
    sections.len() < 2
        || sections.iter().any(loft_section_is_incomplete)
        || sections.iter().any(|section| {
            matches!(
                section,
                LoftSection::Profile(profile)
                    if profile_dependency_is_incomplete(profile, &feature.dependencies)
            )
        })
        || centerline.as_ref().is_some_and(path_ref_is_incomplete)
        || guides.iter().any(path_ref_is_incomplete)
        || (centerline.is_some() && !guides.is_empty())
        || max_degree.is_some_and(|degree| degree == 0)
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn extrude_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Extrude {
        profile,
        direction,
        start,
        extent,
        op,
        solid,
        ..
    } = &feature.definition
    else {
        return true;
    };
    profile_ref_is_incomplete(profile)
        || profile_dependency_is_incomplete(profile, &feature.dependencies)
        || matches!(
            direction,
            cadmpeg_ir::features::ExtrudeDirection::Unresolved
        )
        || matches!(
            direction,
            cadmpeg_ir::features::ExtrudeDirection::Explicit { vector, .. }
                if !valid_feature_direction(*vector)
        )
        || extrude_start_is_incomplete(start)
        || extrude_extent_is_incomplete(extent, &feature.dependencies)
        || matches!(op, BooleanOp::Unresolved)
        || solid.is_none()
        || matches!(
            direction,
            cadmpeg_ir::features::ExtrudeDirection::Explicit {
                source: Some(cadmpeg_ir::features::ExtrusionDirectionSource::Edge { reference }),
                ..
            } if path_ref_is_incomplete(reference)
        )
}

pub(crate) fn revolve_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Revolve { construction, op } = &feature.definition else {
        return true;
    };
    revolve_feature_is_incomplete(construction, *op, &feature.dependencies)
}

pub(crate) fn rib_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Rib { construction, op } = &feature.definition else {
        return true;
    };
    rib_feature_is_incomplete(construction, *op)
        || construction
            .profile
            .as_ref()
            .is_some_and(|profile| profile_dependency_is_incomplete(profile, &feature.dependencies))
}

pub(crate) fn sweep_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Sweep {
        section,
        sections,
        path,
        mode,
        orientation,
        transition,
        transformation,
        twist,
        scale,
        ..
    } = &feature.definition
    else {
        return true;
    };
    matches!(section, cadmpeg_ir::features::SweepSection::Unresolved(_))
        || section
            .referenced_profile()
            .is_some_and(profile_ref_is_incomplete)
        || section
            .referenced_profile()
            .is_some_and(|profile| profile_dependency_is_incomplete(profile, &feature.dependencies))
        || sections.iter().any(|section| {
            matches!(section, cadmpeg_ir::features::SweepSection::Unresolved(_))
                || section
                    .referenced_profile()
                    .is_some_and(profile_ref_is_incomplete)
        })
        || sections.iter().any(|section| {
            section.referenced_profile().is_some_and(|profile| {
                profile_dependency_is_incomplete(profile, &feature.dependencies)
            })
        })
        || path.as_ref().is_none_or(path_ref_is_incomplete)
        || sweep_mode_is_incomplete(*mode)
        || orientation
            .as_ref()
            .is_none_or(sweep_orientation_is_incomplete)
        || transition.is_none()
        || transformation.is_none()
        || twist.is_some_and(|twist| !twist.0.is_finite())
        || scale.is_some_and(|scale| !scale.is_finite() || scale <= 0.0)
}

pub(crate) fn positive_feature_length(length: Length) -> bool {
    length.0.is_finite() && length.0 > 0.0
}

pub(crate) fn valid_draft_angle(angle: cadmpeg_ir::features::Angle) -> bool {
    angle.0.is_finite() && angle.0.abs() < std::f64::consts::FRAC_PI_2
}

pub(crate) fn valid_feature_direction(direction: Vector3) -> bool {
    direction.norm().is_finite() && direction.norm() > 0.0
}

pub(crate) fn finite_feature_point(point: Point3) -> bool {
    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
}
