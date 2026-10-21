// SPDX-License-Identifier: Apache-2.0
//! Feature-completeness predicates for NX decode.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::{
    features::{
        BodyRetentionMode, BodySelection, BodyTrimSide, BooleanOp, CurveProjectionDirection,
        CurveProjectionDirectionState, Feature, FeatureDefinition, FeatureOperation, LoftSection,
        ParameterId, TrimRegion,
    },
    scalar::Length,
};
use std::collections::BTreeSet;

pub(crate) mod operands;
use operands::{
    body_selection_is_incomplete, edge_selection_is_incomplete, extrude_extent_is_incomplete,
    extrude_start_is_incomplete, face_selection_is_incomplete, hole_feature_is_incomplete,
    loft_section_is_incomplete, path_ref_is_incomplete, planar_profile_dependency_is_incomplete,
    planar_profile_ref_is_incomplete, profile_dependency_is_incomplete, profile_ref_is_incomplete,
    revolve_feature_is_incomplete, rib_feature_is_incomplete, sweep_mode_is_incomplete,
    sweep_orientation_is_incomplete, termination_dependency_is_incomplete,
};

/// Orthonormal-frame handedness acceptance for datum CS completeness.
const EPS_ORTHONORMAL_FRAME: f64 = 1.0e-9;
/// Perpendicularity acceptance scaled by direction magnitudes.
const EPS_PERPENDICULAR: f64 = 1.0e-9;
/// Relative acceptance for the native numeric expression cache.
const EXPRESSION_CACHE_RELATIVE_TOLERANCE: f64 = 64.0 * f64::EPSILON;

pub(crate) fn output_free_native_snapshot(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.evaluation.outputs().is_empty()
        && feature.name.as_deref() == Some("MASTER SNAPSHOT BODY")
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved
            })
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
    has_no_body_result_or_reference(feature)
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
        )
}

/// Return whether a `TRIMMED_SH` record is a construction-only operation.
///
/// NX uses the typed trim-surface family for records that carry no body
/// occurrence or primary-body field. Those records have no body result to
/// bind; a body marker makes the output obligation explicit again.
pub(super) fn output_free_trim_surface_construction(
    feature: &cadmpeg_ir::features::Feature,
) -> bool {
    has_no_body_result_or_reference(feature)
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::TrimSurface { .. })
        )
}

/// Whether the configuration has no body selection, its active-feature
/// closure does not resolve, or its suppression, feature states, or parameter
/// values differ from the model.
pub(crate) fn active_configuration_state_is_incomplete(
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> bool {
    // The base model is the active configuration. It must not restate that
    // model's parameter caches or definitions as configuration overrides.
    if !configuration.parameter_overrides.is_empty()
        || !configuration.parameter_values.is_empty()
        || !configuration.feature_states.is_empty()
    {
        return true;
    }
    let Some(bodies) = configuration.bodies.as_deref() else {
        return true;
    };
    if bodies
        .iter()
        .any(|id| !ir.model.bodies.iter().any(|body| &body.id == id))
    {
        return true;
    }
    !ir.model.features.is_empty()
        && crate::native::history::active_feature_closure(ir, bodies).is_err()
}

/// Whether an admitted frame misses the NX datum contract: perpendicular
/// axes within a bound scaled by their lengths, and a handedness within the
/// frame tolerance of one on both sides. The axes are unit directions, so
/// every length and product below is finite.
pub(super) fn datum_coordinate_system_is_incomplete(
    x_axis: UnitVector3,
    y_axis: UnitVector3,
    z_axis: UnitVector3,
) -> bool {
    let [x_axis, y_axis, z_axis] = [x_axis, y_axis, z_axis].map(Vector3::from);
    if !directions_are_perpendicular(x_axis, y_axis)
        || !directions_are_perpendicular(y_axis, z_axis)
        || !directions_are_perpendicular(z_axis, x_axis)
    {
        return true;
    }
    let handedness = x_axis.cross(y_axis).dot(z_axis);
    (handedness - 1.0).abs() > EPS_ORTHONORMAL_FRAME
}

pub(super) fn projected_curve_direction_is_incomplete(direction: CurveProjectionDirection) -> bool {
    match direction {
        CurveProjectionDirection::Vector(_) => false,
        CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved) => true,
        CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal) => false,
    }
}

fn directions_are_perpendicular(first: Vector3, second: Vector3) -> bool {
    first.dot(second).abs() <= EPS_PERPENDICULAR * (first.norm() * second.norm())
}

pub(crate) fn incomplete_expression_parameters(
    ir: &CadIr,
) -> Result<BTreeSet<ParameterId>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::ParameterValue;
    use std::collections::BTreeMap;
    let ids = ir
        .model
        .parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| (&parameter.id, index))
        .collect::<BTreeMap<_, _>>();
    let mut incomplete = BTreeSet::new();
    let mut evaluated = BTreeMap::<&ParameterId, f64>::new();
    let scalar = |value: &ParameterValue| match value {
        ParameterValue::Length(value) => Some(value.get()),
        ParameterValue::Angle(value) => Some(value.get()),
        ParameterValue::Real(value) => Some(value.get()),
        ParameterValue::Integer(value) => cadmpeg_core::convert::f64_from_i64(*value),
        _ => None,
    };
    loop {
        let mut progressed = false;
        for parameter in &ir.model.parameters {
            if evaluated.contains_key(&parameter.id) || incomplete.contains(&parameter.id) {
                continue;
            }
            let unit = parameter.properties.get("unit").map(String::as_str);
            if parameter.value.is_none()
                || unit.is_some_and(|unit| !matches!(unit, "millimeter" | "inch" | "degree"))
                || parameter
                    .expression
                    .dependencies()
                    .any(|id| !ids.contains_key(id))
            {
                incomplete.insert(parameter.id.clone());
                continue;
            }
            if parameter
                .expression
                .dependencies()
                .any(|id| !evaluated.contains_key(id))
            {
                continue;
            }
            let stored = parameter.value.as_ref().and_then(scalar);
            let literal = parameter
                .value
                .as_ref()
                .and_then(|value| parameter.expression.literal_value_as(value))
                .as_ref()
                .and_then(scalar);
            let native_value = if let Some(literal) = literal {
                Some(match unit {
                    Some("inch") => literal / 25.4,
                    Some("degree") => literal.to_degrees(),
                    _ => literal,
                })
            } else {
                parameter
                    .expression
                    .render_with(|id| ids.get(id).map(|index| format!("p{index}")))
                    .and_then(|text| {
                        crate::native::om::evaluate_parameterized_expression(&text, |name| {
                            let index: usize = name.strip_prefix('p')?.parse().ok()?;
                            evaluated.get(&ir.model.parameters.get(index)?.id).copied()
                        })
                    })
                    .map(|value| value.get())
            };
            let canonical = native_value.and_then(|value| {
                unit.map_or(Some(value), |unit| {
                    crate::native::om::canonical_expression_value(unit, value)
                })
            });
            if let (Some(native), Some(canonical), Some(stored)) = (native_value, canonical, stored)
            {
                let tolerance = EXPRESSION_CACHE_RELATIVE_TOLERANCE
                    * canonical.abs().max(stored.abs()).max(1.0);
                if canonical.is_finite()
                    && stored.is_finite()
                    && (canonical - stored).abs() <= tolerance
                {
                    evaluated.insert(&parameter.id, native);
                    progressed = true;
                    continue;
                }
            }
            incomplete.insert(parameter.id.clone());
        }
        if !progressed {
            break;
        }
    }
    // Cyclic identities remain in their expression tokens; only the ability
    // to verify their cached evaluation is incomplete.
    incomplete.extend(
        ir.model
            .parameters
            .iter()
            .filter(|parameter| !evaluated.contains_key(&parameter.id))
            .map(|parameter| parameter.id.clone()),
    );
    Ok(incomplete)
}

pub(crate) fn trim_surface_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::TrimSurface {
        faces, tool, keep, ..
    }) = feature.evaluation.definition()
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || path_ref_is_incomplete(tool)
        || matches!(keep, TrimRegion::Unresolved)
}

pub(crate) fn extend_surface_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
        faces,
        distance,
        method,
    }) = feature.evaluation.definition()
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || distance.is_none()
        || matches!(method, cadmpeg_ir::features::SurfaceExtension::Unresolved)
}

pub(crate) fn sew_bodies_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::SewBodies { bodies, .. }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    body_selection_is_incomplete(bodies)
}

pub(crate) fn combine_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    let target = operands.target();
    let tools = operands.tools();
    body_selection_is_incomplete(target) || body_selection_is_incomplete(tools)
}

pub(crate) fn trim_bodies_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::TrimBodies { operands, keep }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    let targets = operands.targets();
    let tools = operands.tools();
    body_selection_is_incomplete(targets)
        || body_selection_is_incomplete(tools)
        || matches!(keep, BodyTrimSide::Unresolved)
}

pub(super) fn delete_body_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, mode }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    body_selection_is_incomplete(bodies) || matches!(mode, BodyRetentionMode::Unresolved)
}

pub(crate) fn hole_definition_is_incomplete(ir: &cadmpeg_ir::CadIr, feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        profile,
        face,
        placements,
        shape,

        extent,
        ..
    }) = feature.evaluation.definition()
    else {
        return true;
    };
    let construction = shape.construction();
    let exit_kind = shape.exit_kind();
    let diameter = shape.diameter();
    let construction_incomplete = match construction {
        cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. } => {
            hole_feature_is_incomplete(
                ir,
                profile.as_ref(),
                face.as_ref(),
                placements.as_deref(),
                (kind, exit_kind.as_ref()),
                diameter
                    .and_then(|diameter| ir.dimension_value(diameter))
                    .map(Into::into),
                extent.as_ref(),
            )
        }
        cadmpeg_ir::features::holes::HoleConstruction::NativeThread {
            major_diameter,
            drill_point_angle,
            ..
        } => {
            let kind = cadmpeg_ir::features::holes::HoleKind::SimpleDrilled {
                drill_point_angle: drill_point_angle.clone(),
            };
            hole_feature_is_incomplete(
                ir,
                profile.as_ref(),
                face.as_ref(),
                placements.as_deref(),
                (&kind, exit_kind.as_ref()),
                diameter
                    .and_then(|diameter| ir.dimension_value(diameter))
                    .map(Into::into),
                extent.as_ref(),
            ) || diameter
                .and_then(|diameter| ir.dimension_value(diameter))
                .zip(ir.dimension_value(major_diameter))
                .is_none_or(|(diameter, major)| major.get() <= diameter.get())
        }
    };
    construction_incomplete
        || extent.as_ref().is_some_and(|extent| {
            termination_dependency_is_incomplete(extent, &feature.dependencies)
        })
        || profile.as_ref().is_some_and(|profile| {
            planar_profile_dependency_is_incomplete(profile, &feature.dependencies)
        })
}

pub(crate) fn chamfer_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    groups
        .iter()
        .any(|group| edge_selection_is_incomplete(&group.edges) || group.spec.is_unresolved())
}

pub(super) fn fillet_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    groups
        .iter()
        .any(|group| edge_selection_is_incomplete(&group.edges) || group.radius.is_unresolved())
}

pub(super) fn face_blend_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::FaceBlend { operands, radius }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    let first_faces = operands.first_faces();
    let second_faces = operands.second_faces();
    face_selection_is_incomplete(first_faces)
        || face_selection_is_incomplete(second_faces)
        || radius.is_unresolved()
}

pub(super) fn shell_definition_is_incomplete(definition: &FeatureDefinition) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Shell {
        bodies,
        removed_faces,
        thickness,
        outward,
        mode,
        join,
        resolve_intersections,
        allow_self_intersections,
    }) = definition
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

pub(super) fn offset_surface_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::OffsetSurface { faces, distance }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    face_selection_is_incomplete(faces) || distance.is_none()
}

pub(crate) fn sphere_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Sphere { op, .. }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    matches!(op, BooleanOp::Unresolved)
}

pub(super) fn thicken_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Thicken {
        faces,
        thickness,
        side,
    }) = feature.evaluation.definition()
    else {
        return true;
    };
    face_selection_is_incomplete(faces) || thickness.is_none() || side.is_none()
}

pub(super) fn draft_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Draft {
        faces,
        anchor,
        angle,
        outward,
    }) = feature.evaluation.definition()
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

pub(super) fn replace_face_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    let targets = operands.targets();
    let replacements = operands.replacements();
    face_selection_is_incomplete(targets) || face_selection_is_incomplete(replacements)
}

pub(crate) fn loft_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Loft {
        sections,
        guidance,
        op,
        ..
    }) = feature.evaluation.definition()
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
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile,
        direction,
        start,
        extent,
        op,
        solid,
        ..
    }) = feature.evaluation.definition()
    else {
        return true;
    };
    profile_ref_is_incomplete(profile)
        || profile_dependency_is_incomplete(profile, &feature.dependencies)
        || matches!(
            direction,
            cadmpeg_ir::features::ExtrudeDirection::Unresolved {}
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
    let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, op }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    revolve_feature_is_incomplete(construction, *op, &feature.dependencies)
}

pub(crate) fn rib_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Rib { construction, op }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    rib_feature_is_incomplete(construction, *op)
        || construction.profile.as_ref().is_some_and(|profile| {
            planar_profile_dependency_is_incomplete(profile, &feature.dependencies)
        })
}

pub(crate) fn sweep_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape,

        path,

        orientation,
        transition,
        transformation,
        ..
    }) = feature.evaluation.definition()
    else {
        return true;
    };
    let mode = shape.mode();
    let mut profiles = shape.referenced_profiles();
    shape.any_section_is_unresolved()
        || profiles.clone().any(planar_profile_ref_is_incomplete)
        || profiles
            .any(|profile| planar_profile_dependency_is_incomplete(profile, &feature.dependencies))
        || path.as_ref().is_none_or(path_ref_is_incomplete)
        || sweep_mode_is_incomplete(mode)
        || orientation
            .as_ref()
            .is_none_or(sweep_orientation_is_incomplete)
        || transition.is_none()
        || transformation.is_none()
}

fn positive_feature_length(length: Length) -> bool {
    length.get() > 0.0
}

fn has_no_body_result_or_reference(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.evaluation.outputs().is_empty()
        && !feature.source_properties.keys().any(|key| {
            key == "primary_body_reference"
                || key == "primary_body_object_index"
                || key == "primary_body_data_block"
                || key.as_str().starts_with("body_reference.")
                || key.as_str().starts_with("body_reference_occurrence.")
        })
}

#[cfg(test)]
mod tests;
