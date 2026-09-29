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
use std::collections::{BTreeMap, BTreeSet};

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

pub(crate) fn active_configuration_state_is_incomplete(
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> bool {
    if configuration_suppression_differs(ir, configuration) {
        return true;
    }
    let Some(bodies) = configuration.bodies.as_deref() else {
        return true;
    };
    let required_features = if ir.model.features.is_empty() {
        BTreeMap::new()
    } else {
        let Ok(active_features) = crate::native::history::active_feature_closure(ir, bodies) else {
            return true;
        };
        active_features
    };
    configuration_state_differs(ir, configuration, &required_features)
}

/// Decode-time form of [`active_configuration_state_is_incomplete`]: the
/// active-feature closure is charged to the decode budget.
pub(crate) fn active_configuration_state_is_incomplete_for_decode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> Result<bool, cadmpeg_core::CodecError> {
    if configuration_suppression_differs(ir, configuration) {
        return Ok(true);
    }
    let Some(bodies) = configuration.bodies.as_deref() else {
        return Ok(true);
    };
    let required_features = if ir.model.features.is_empty() {
        BTreeMap::new()
    } else {
        let Ok(active_features) =
            crate::native::history::active_feature_closure_for_decode(ctx, ir, bodies)?
        else {
            return Ok(true);
        };
        active_features
    };
    Ok(configuration_state_differs(
        ir,
        configuration,
        &required_features,
    ))
}

/// Whether a feature's suppression differs from the configuration's
/// suppression state, or the feature has no suppression state.
fn configuration_suppression_differs(
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> bool {
    ir.model.features.iter().any(|feature| {
        feature.suppressed.is_none_or(|suppressed| {
            configuration
                .feature_states
                .get(&feature.id)
                .is_some_and(|state| state.evaluation.is_suppressed())
                != suppressed
        })
    })
}

/// Whether the configuration's feature states and parameter values differ
/// from the model for the active features and every suppressed feature.
fn configuration_state_differs(
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
    required_features: &BTreeMap<cadmpeg_ir::features::FeatureId, usize>,
) -> bool {
    let mut suppressed_only = ir.model.features.iter().enumerate().filter(|(_, feature)| {
        feature.suppressed == Some(true) && !required_features.contains_key(&feature.id)
    });
    if configuration.feature_states.len()
        != required_features.len() + suppressed_only.clone().count()
    {
        return true;
    }
    let state_is_incomplete = |id: &cadmpeg_ir::features::FeatureId, index: usize| {
        let feature = &ir.model.features[index];
        let Some(state) = configuration.feature_states.get(id) else {
            return true;
        };
        Some(state.evaluation.is_suppressed()) != feature.suppressed
            || state.dependencies != feature.dependencies
            || state.evaluation.outputs() != feature.evaluation.outputs().as_slice()
            || &state.definition != feature.evaluation.definition()
    };
    if required_features
        .iter()
        .any(|(id, &index)| state_is_incomplete(id, index))
        || suppressed_only.any(|(index, feature)| state_is_incomplete(&feature.id, index))
    {
        return true;
    }

    configuration.parameter_values.len() != ir.model.parameters.len()
        || ir.model.parameters.iter().any(|parameter| {
            parameter.value.as_ref().is_none_or(|value| {
                configuration.parameter_values.get(&parameter.id) != Some(value)
            })
        })
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
) -> Result<BTreeSet<ParameterId>, cadmpeg_core::CodecError> {
    let mut parameter_owners = BTreeSet::new();
    for parameter in &ir.model.parameters {
        if !parameter_owners.contains(&parameter.owner) {
            ctx.charge_collection_items(1, "nx expression parameter owners")?;
            parameter_owners.insert(&parameter.owner);
        }
    }
    let mut incomplete = BTreeSet::new();
    for owner in parameter_owners {
        let mut parameters = Vec::new();
        for parameter in ir
            .model
            .parameters
            .iter()
            .filter(|parameter| &parameter.owner == owner)
        {
            ctx.reserve_vec(&mut parameters, 1, "nx owned expression parameters")?;
            parameters.push(parameter);
        }
        let mut ids_by_name = BTreeMap::<(&str, Option<&str>), Vec<&ParameterId>>::new();
        for parameter in &parameters {
            let ids = match ids_by_name.entry((
                parameter.name.as_str(),
                parameter.properties.get("unit").map(String::as_str),
            )) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "nx expression name index")?;
                    entry.insert(Vec::new())
                }
            };
            ctx.reserve_vec(ids, 1, "nx expression name identities")?;
            ids.push(&parameter.id);
        }
        let mut expected = ctx.alloc_filled(
            parameters.len(),
            None::<Vec<ParameterId>>,
            "nx expected expression dependencies",
        )?;
        for (index, parameter) in parameters.iter().enumerate() {
            expected[index] =
                (|| -> Result<Option<Vec<ParameterId>>, cadmpeg_core::CodecError> {
                    let unit = match parameter.properties.get("unit").map(String::as_str) {
                        None => None,
                        Some(unit @ ("millimeter" | "inch" | "degree")) => Some(unit),
                        Some(_) => return Ok(None),
                    };
                    let Some(ids) = ids_by_name.get(&(parameter.name.as_str(), unit)) else {
                        return Ok(None);
                    };
                    let [_] = ids.as_slice() else {
                        return Ok(None);
                    };
                    let mut dependencies = Vec::new();
                    for name in crate::native::om::expression_parameter_names(&parameter.expression)
                    {
                        let Some(ids) = ids_by_name.get(&(name, unit)) else {
                            return Ok(None);
                        };
                        let [dependency] = ids.as_slice() else {
                            return Ok(None);
                        };
                        if dependencies.iter().any(|id| id == *dependency) {
                            continue;
                        }
                        ctx.reserve_vec(&mut dependencies, 1, "nx expression dependencies")?;
                        dependencies.push(dependency.try_clone_for_decode(ctx, "nx expression dependency identity")?);
                    }
                    Ok(Some(dependencies))
                })()?;
        }
        let mut indices = BTreeMap::new();
        for (index, parameter) in parameters.iter().enumerate() {
            ctx.charge_collection_items(1, "nx expression parameter index")?;
            indices.insert(&parameter.id, index);
        }
        let mut emitted = BTreeSet::new();
        let mut evaluated = BTreeMap::<&ParameterId, f64>::new();
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
            let value = crate::native::om::evaluate_parameterized_expression(
                ctx,
                &parameter.expression,
                |name| {
                    let [dependency] = ids_by_name.get(&(name, unit))?.as_slice() else {
                        return None;
                    };
                    evaluated.get(*dependency).copied()
                },
            )?;
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
                let native_value = native_value.get();
                let canonical_value = unit.map_or(Some(native_value), |unit| {
                    crate::native::om::canonical_expression_value(unit, native_value)
                });
                if let (Some(canonical_value), Some(stored)) = (canonical_value, stored) {
                    let tolerance =
                        64.0 * f64::EPSILON * canonical_value.abs().max(stored.abs()).max(1.0);
                    if canonical_value.is_finite()
                        && stored.is_finite()
                        && (canonical_value - stored).abs() <= tolerance
                    {
                        ctx.charge_collection_items(1, "nx evaluated expression parameters")?;
                        evaluated.insert(&parameter.id, native_value);
                    }
                }
            }
            ctx.charge_collection_items(1, "nx emitted expression parameters")?;
            emitted.insert(index);
        }
        for (index, parameter) in parameters.into_iter().enumerate() {
            if expected[index].as_deref() != Some(parameter.dependencies.as_slice())
                || !emitted.contains(&index)
                || !evaluated.contains_key(&parameter.id)
            {
                ctx.charge_collection_items(1, "nx incomplete expression parameters")?;
                incomplete.insert(parameter.id.try_clone_for_decode(ctx, "nx incomplete expression identity")?);
            }
        }
    }
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

pub(crate) fn hole_definition_is_incomplete(feature: &Feature) -> bool {
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
                profile.as_ref(),
                face.as_ref(),
                placements.as_deref(),
                (kind, exit_kind.as_ref()),
                diameter.map(Into::into),
                extent.as_ref(),
            )
        }
        cadmpeg_ir::features::holes::HoleConstruction::NativeThread {
            major_diameter,
            drill_point_angle,
            ..
        } => {
            let kind = cadmpeg_ir::features::holes::HoleKind::SimpleDrilled {
                drill_point_angle: *drill_point_angle,
            };
            hole_feature_is_incomplete(
                profile.as_ref(),
                face.as_ref(),
                placements.as_deref(),
                (&kind, exit_kind.as_ref()),
                diameter.map(Into::into),
                extent.as_ref(),
            ) || diameter.is_none_or(|diameter| major_diameter.get() <= diameter.get())
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
    let profiles = shape.referenced_profiles();
    shape.any_section_is_unresolved()
        || profiles
            .iter()
            .copied()
            .any(planar_profile_ref_is_incomplete)
        || profiles
            .iter()
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
