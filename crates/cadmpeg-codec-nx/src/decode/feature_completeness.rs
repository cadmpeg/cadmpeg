// SPDX-License-Identifier: Apache-2.0
//! Feature-completeness predicates for NX decode.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{
    features::{
        BodyRetentionMode, BodySelection, BodyTrimSide, BooleanOp, CurveProjectionDirection,
        CurveProjectionDirectionState, Feature, FeatureDefinition, LoftSection, ParameterId,
        TrimRegion,
    },
    scalar::Length,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) mod operands;
use operands::{
    body_selection_is_incomplete, edge_selection_is_incomplete, extrude_extent_is_incomplete,
    extrude_start_is_incomplete, face_selection_is_incomplete, hole_feature_is_incomplete,
    hole_specification_is_incomplete, loft_section_is_incomplete, path_ref_is_incomplete,
    profile_dependency_is_incomplete, profile_ref_is_incomplete, revolve_feature_is_incomplete,
    rib_feature_is_incomplete, sweep_mode_is_incomplete, sweep_orientation_is_incomplete,
    termination_dependency_is_incomplete,
};

/// Orthonormal-frame handedness acceptance for datum CS completeness.
const EPS_ORTHONORMAL_FRAME: f64 = 1.0e-9;
/// Unit-length acceptance for authored feature directions.
const EPS_UNIT_DIRECTION: f64 = 1.0e-9;
/// Perpendicularity acceptance scaled by direction magnitudes.
const EPS_PERPENDICULAR: f64 = 1.0e-9;

pub(crate) fn output_free_native_snapshot(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.evaluation.outputs().is_empty()
        && feature.name.as_deref() == Some("MASTER SNAPSHOT BODY")
        && matches!(
            feature.evaluation.definition(),
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
    feature.evaluation.outputs().is_empty()
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
    feature.evaluation.outputs().is_empty()
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Pattern { .. }
        )
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
    feature.evaluation.outputs().is_empty()
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::TrimSurface { .. }
        )
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
    let mut required_features = if ir.model.features.is_empty() {
        BTreeMap::new()
    } else {
        let Ok(active_features) = crate::native::history::active_feature_closure(ir, bodies) else {
            return true;
        };
        active_features
    };
    required_features.extend(
        ir.model
            .features
            .iter()
            .enumerate()
            .filter(|(_, feature)| feature.suppressed == Some(true))
            .map(|(index, feature)| (feature.id.clone(), index)),
    );
    if configuration.feature_states.len() != required_features.len() {
        return true;
    }
    if required_features.iter().any(|(id, &index)| {
        let feature = &ir.model.features[index];
        let Some(state) = configuration.feature_states.get(id) else {
            return true;
        };
        Some(state.evaluation.is_suppressed()) != feature.suppressed
            || state.dependencies != feature.dependencies
            || state.evaluation.outputs() != feature.evaluation.outputs().as_slice()
            || &state.definition != feature.evaluation.definition()
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
        CurveProjectionDirection::Vector(_) => false,
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
                ) => Some(value.get()),
                (Some("degree"), Some(cadmpeg_ir::features::ParameterValue::Angle(value))) => {
                    Some(value.get())
                }
                (None, Some(cadmpeg_ir::features::ParameterValue::Real(value))) => {
                    Some(value.get())
                }
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
            if expected[index].as_deref() != Some(parameter.dependencies.as_slice())
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
    } = feature.evaluation.definition()
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
    } = feature.evaluation.definition()
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || distance.is_none()
        || matches!(method, cadmpeg_ir::features::SurfaceExtension::Unresolved)
}

pub(crate) fn sew_bodies_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::SewBodies { bodies, .. } = feature.evaluation.definition() else {
        return true;
    };
    body_selection_is_incomplete(bodies)
}

pub(crate) fn combine_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Combine { operands, .. } = feature.evaluation.definition() else {
        return true;
    };
    let target = operands.target();
    let tools = operands.tools();
    body_selection_is_incomplete(target) || body_selection_is_incomplete(tools)
}

pub(crate) fn trim_bodies_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::TrimBodies { operands, keep } = feature.evaluation.definition() else {
        return true;
    };
    let targets = operands.targets();
    let tools = operands.tools();
    body_selection_is_incomplete(targets)
        || body_selection_is_incomplete(tools)
        || matches!(keep, BodyTrimSide::Unresolved)
}

pub(crate) fn delete_body_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::DeleteBody { bodies, mode } = feature.evaluation.definition() else {
        return true;
    };
    body_selection_is_incomplete(bodies) || matches!(mode, BodyRetentionMode::Unresolved)
}

pub(crate) fn hole_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Hole {
        profile,
        face,
        placements,
        shape,

        extent,
        ..
    } = feature.evaluation.definition()
    else {
        return true;
    };
    let construction = shape.construction();
    let exit_kind = shape.exit_kind();
    let diameter = shape.diameter();
    let (construction_incomplete, specification) = match construction {
        cadmpeg_ir::features::HoleConstruction::Form {
            kind,
            specification,
        } => (
            hole_feature_is_incomplete(
                profile.as_ref().map(AsRef::as_ref),
                face.as_ref(),
                placements.as_deref(),
                (kind, exit_kind.as_ref()),
                diameter.map(Into::into),
                extent.as_ref(),
            ),
            specification.as_deref(),
        ),
        cadmpeg_ir::features::HoleConstruction::NativeThread {
            major_diameter,
            drill_point_angle,
            ..
        } => {
            let kind = cadmpeg_ir::features::HoleKind::SimpleDrilled {
                drill_point_angle: *drill_point_angle,
            };
            (
                hole_feature_is_incomplete(
                    profile.as_ref().map(AsRef::as_ref),
                    face.as_ref(),
                    placements.as_deref(),
                    (&kind, exit_kind.as_ref()),
                    diameter.map(Into::into),
                    extent.as_ref(),
                ) || diameter.is_none_or(|diameter| major_diameter.get() <= diameter.get()),
                None,
            )
        }
    };
    construction_incomplete
        || hole_specification_is_incomplete(specification)
        || extent.as_ref().is_some_and(|extent| {
            termination_dependency_is_incomplete(extent, &feature.dependencies)
        })
        || profile
            .as_ref()
            .is_some_and(|profile| profile_dependency_is_incomplete(profile, &feature.dependencies))
}

pub(crate) fn chamfer_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Chamfer { groups, .. } = feature.evaluation.definition() else {
        return true;
    };
    groups
        .iter()
        .any(|group| edge_selection_is_incomplete(&group.edges) || group.spec.is_unresolved())
}

pub(crate) fn fillet_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Fillet { groups } = feature.evaluation.definition() else {
        return true;
    };
    groups
        .iter()
        .any(|group| edge_selection_is_incomplete(&group.edges) || group.radius.is_unresolved())
}

pub(crate) fn face_blend_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::FaceBlend { operands, radius } = feature.evaluation.definition() else {
        return true;
    };
    let first_faces = operands.first_faces();
    let second_faces = operands.second_faces();
    face_selection_is_incomplete(first_faces)
        || face_selection_is_incomplete(second_faces)
        || radius.is_unresolved()
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
        || thickness.is_none()
        || outward.is_none()
        || mode.is_none()
        || join.is_none()
        || resolve_intersections.is_none()
        || allow_self_intersections.is_none()
}

pub(crate) fn offset_surface_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::OffsetSurface { faces, distance } = feature.evaluation.definition()
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || distance.is_none_or(|distance| !distance.get().is_finite())
}

pub(crate) fn sphere_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Sphere { op, .. } = feature.evaluation.definition() else {
        return true;
    };
    matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn thicken_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Thicken {
        faces,
        thickness,
        side,
    } = feature.evaluation.definition()
    else {
        return true;
    };
    face_selection_is_incomplete(faces) || thickness.is_none() || side.is_none()
}

pub(crate) fn draft_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Draft {
        faces,
        anchor,
        angle,
        outward,
    } = feature.evaluation.definition()
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
        || anchor.pull().is_none()
        || anchor
            .pull()
            .and_then(|pull| pull.plane.as_ref())
            .is_some_and(|plane| plane.as_str().is_empty())
        || angle.is_none()
        || outward.is_none()
}

pub(crate) fn replace_face_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::ReplaceFace { operands } = feature.evaluation.definition() else {
        return true;
    };
    let targets = operands.targets();
    let replacements = operands.replacements();
    face_selection_is_incomplete(targets) || face_selection_is_incomplete(replacements)
}

pub(crate) fn loft_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Loft {
        sections,
        guidance,
        op,
        ..
    } = feature.evaluation.definition()
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
        || match guidance {
            cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                guides.iter().any(path_ref_is_incomplete)
            }
            cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                path_ref_is_incomplete(centerline)
            }
        }
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
    } = feature.evaluation.definition()
    else {
        return true;
    };
    profile_ref_is_incomplete(profile)
        || profile_dependency_is_incomplete(profile, &feature.dependencies)
        || matches!(
            direction,
            cadmpeg_ir::features::ExtrudeDirection::Unresolved
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
    let FeatureDefinition::Revolve { construction, op } = feature.evaluation.definition() else {
        return true;
    };
    revolve_feature_is_incomplete(construction, *op, &feature.dependencies)
}

pub(crate) fn rib_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Rib { construction, op } = feature.evaluation.definition() else {
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
        shape,

        path,

        orientation,
        transition,
        transformation,
        ..
    } = feature.evaluation.definition()
    else {
        return true;
    };
    let section = shape.section();
    let sections = shape.sections();
    let mode = shape.mode();
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
        || sweep_mode_is_incomplete(mode)
        || orientation
            .as_ref()
            .is_none_or(sweep_orientation_is_incomplete)
        || transition.is_none()
        || transformation.is_none()
}

pub(crate) fn positive_feature_length(length: Length) -> bool {
    length.get() > 0.0
}

pub(crate) fn valid_feature_direction(direction: Vector3) -> bool {
    direction.norm().is_finite() && direction.norm() > 0.0
}

pub(crate) fn finite_feature_point(point: Point3) -> bool {
    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
}
