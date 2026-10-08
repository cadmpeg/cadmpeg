// SPDX-License-Identifier: Apache-2.0
//! Feature-completeness predicates for NX decode.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::{
    features::{
        BodyRetentionMode, BodySelection, BodyTrimSide, BooleanOp, CurveProjectionDirection,
        CurveProjectionDirectionState, DesignParameter, Feature, FeatureDefinition,
        FeatureOperation, LoftSection, ParameterId, SweepSection, SweepShape, TrimRegion,
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

const PROPERTIES: &str = "nx feature completeness source properties";

/// Run a completeness predicate under a service decode budget.
#[cfg(test)]
pub(crate) fn decode_check<T>(
    predicate: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> T {
    crate::test_support::with_decode_context(predicate)
        .expect("the completeness check stays within the service budget")
}

fn has_property(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    key: &str,
) -> Result<bool, CodecError> {
    Ok(ctx
        .get_btree_map(&feature.source_properties, key, PROPERTIES)?
        .is_some())
}

pub(crate) fn output_free_native_snapshot(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    if !(feature.evaluation.outputs().is_empty()
        && feature.name.as_deref() == Some("MASTER SNAPSHOT BODY")
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved
            })
        ))
    {
        return Ok(false);
    }
    match ctx.get_btree_map(&feature.source_properties, "operation_record", PROPERTIES)? {
        Some(record) => Ok(!ctx.trim_text(record, PROPERTIES)?.is_empty()),
        None => Ok(false),
    }
}

/// Return whether a feature's primary body is local to the history namespace.
///
/// Offset-store and unbound object-namespace bodies are retained as native
/// feature-local identities. They do not create neutral current-body outputs;
/// the saved segment image remains the only neutral body census.
pub(crate) fn output_free_local_body_construction(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    Ok(feature.evaluation.outputs().is_empty()
        && has_property(ctx, feature, "primary_body_reference")?
        && !has_property(ctx, feature, "primary_body_segment_use")?)
}

/// Return whether a pattern record is construction-only and has no neutral
/// body-output obligation.
///
/// Pattern construction records without a primary-body field describe the
/// seed and transform graph. A body-affecting pattern has at least one body
/// reference occurrence, even when the occurrence is too ambiguous to become
/// a primary writer. Keep that distinction explicit so an incomplete body
/// binding cannot be mistaken for a construction-only record.
pub(crate) fn output_free_pattern_construction(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    Ok(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
    ) && has_no_body_result_or_reference(ctx, feature)?)
}

/// Return whether a `TRIMMED_SH` record is a construction-only operation.
///
/// NX uses the typed trim-surface family for records that carry no body
/// occurrence or primary-body field. Those records have no body result to
/// bind; a body marker makes the output obligation explicit again.
pub(super) fn output_free_trim_surface_construction(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    Ok(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::TrimSurface { .. })
    ) && has_no_body_result_or_reference(ctx, feature)?)
}

pub(crate) fn active_configuration_state_is_incomplete(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> Result<bool, CodecError> {
    if configuration_suppression_differs(ctx, ir, configuration)? {
        return Ok(true);
    }
    let Some(bodies) = configuration.bodies.as_deref() else {
        return Ok(true);
    };
    let mut closure_storage = ctx.reserve_scoped(0, "nx configuration active features")?;
    let required_features = if ir.model.features.is_empty() {
        BTreeMap::new()
    } else {
        let closure = closure_storage
            .with_storage(|| crate::native::history::active_feature_closure(ctx, ir, bodies))?;
        let Ok(active_features) = closure else {
            return Ok(true);
        };
        active_features
    };
    configuration_state_differs(ctx, ir, configuration, &required_features)
}

/// Whether a feature's suppression differs from the configuration's
/// suppression state, or the feature has no suppression state.
fn configuration_suppression_differs(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> Result<bool, CodecError> {
    ctx.any_by(
        &ir.model.features,
        |feature| {
            let Some(suppressed) = feature.suppressed else {
                return Ok(true);
            };
            let state_suppressed = ctx
                .get_btree_map(
                    &configuration.feature_states,
                    &feature.id,
                    "nx configuration feature states",
                )?
                .is_some_and(|state| state.evaluation.is_suppressed());
            Ok(state_suppressed != suppressed)
        },
        "nx configuration suppression",
    )
}

/// Whether the configuration's feature states and parameter values differ
/// from the model for the active features and every suppressed feature.
fn configuration_state_differs(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
    required_features: &BTreeMap<cadmpeg_ir::features::FeatureId, usize>,
) -> Result<bool, CodecError> {
    const STATES: &str = "nx configuration feature states";
    let is_suppressed_only = |feature: &Feature| -> Result<bool, CodecError> {
        Ok(feature.suppressed == Some(true)
            && !ctx.contains_key_btree_map(required_features, &feature.id, STATES)?)
    };
    let mut suppressed_only_count = 0usize;
    for feature in ctx.admit_iter(&ir.model.features, STATES)? {
        if is_suppressed_only(feature)? {
            suppressed_only_count += 1;
        }
    }
    if Some(configuration.feature_states.len())
        != required_features.len().checked_add(suppressed_only_count)
    {
        return Ok(true);
    }
    let state_is_incomplete = |feature: &Feature| -> Result<bool, CodecError> {
        let Some(state) = ctx.get_btree_map(&configuration.feature_states, &feature.id, STATES)?
        else {
            return Ok(true);
        };
        Ok(Some(state.evaluation.is_suppressed()) != feature.suppressed
            || !ctx.equal(&state.dependencies, &feature.dependencies, STATES)?
            || !ctx.equal(
                state.evaluation.outputs(),
                feature.evaluation.outputs().as_slice(),
                STATES,
            )?
            || !ctx.equal(&state.definition, feature.evaluation.definition(), STATES)?)
    };
    for &index in required_features.values() {
        ctx.charge_work(1, STATES)?;
        let Some(feature) = ir.model.features.get(index) else {
            return Err(CodecError::malformed(
                "NX active feature closure names a missing feature",
            ));
        };
        if state_is_incomplete(feature)? {
            return Ok(true);
        }
    }
    if ctx.any_by(
        &ir.model.features,
        |feature| Ok(is_suppressed_only(feature)? && state_is_incomplete(feature)?),
        STATES,
    )? {
        return Ok(true);
    }

    Ok(
        configuration.parameter_values.len() != ir.model.parameters.len()
            || ctx.any_by(
                &ir.model.parameters,
                |parameter| {
                    let Some(value) = &parameter.value else {
                        return Ok(true);
                    };
                    match ctx.get_btree_map(
                        &configuration.parameter_values,
                        &parameter.id,
                        "nx configuration parameter values",
                    )? {
                        Some(configured) => Ok(!ctx.equal(
                            configured,
                            value,
                            "nx configuration parameter values",
                        )?),
                        None => Ok(true),
                    }
                },
                "nx configuration parameter values",
            )?,
    )
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

/// Name and unit key of an expression parameter within its owner.
type ExpressionKey<'p> = (&'p str, Option<&'p str>);

fn expression_unit<'p>(
    ctx: &DecodeContext<'_>,
    parameter: &'p DesignParameter,
) -> Result<Option<&'p str>, CodecError> {
    Ok(ctx
        .get_btree_map(
            &parameter.properties,
            "unit",
            "nx expression parameter unit",
        )?
        .map(String::as_str))
}

/// The dependencies a parameter's expression names, when every name resolves
/// to exactly one parameter of the same owner and unit.
fn expected_expression_dependencies(
    ctx: &DecodeContext<'_>,
    workspace: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    parameter: &DesignParameter,
    ids_by_name: &BTreeMap<ExpressionKey<'_>, Vec<&ParameterId>>,
) -> Result<Option<Vec<ParameterId>>, CodecError> {
    const NAMES: &str = "nx expression name index";
    let unit = match expression_unit(ctx, parameter)? {
        None => None,
        Some(unit @ ("millimeter" | "inch" | "degree")) => Some(unit),
        Some(_) => return Ok(None),
    };
    let Some(ids) = ctx.get_btree_map(ids_by_name, &(parameter.name.as_str(), unit), NAMES)? else {
        return Ok(None);
    };
    let [_] = ids.as_slice() else {
        return Ok(None);
    };
    let mut dependencies = Vec::new();
    for name in crate::native::om::expression_parameter_names(ctx, &parameter.expression) {
        let name = name?;
        let Some(ids) = ctx.get_btree_map(ids_by_name, &(name, unit), NAMES)? else {
            return Ok(None);
        };
        let [dependency] = ids.as_slice() else {
            return Ok(None);
        };
        if ctx.contains(&dependencies, *dependency, "nx expression dependencies")? {
            continue;
        }
        let dependency = workspace.with_storage(|| {
            dependency.try_clone_for_decode(ctx, "nx expression dependency identity")
        })?;
        ctx.reserve_scoped_vec(
            workspace,
            &mut dependencies,
            1,
            "nx expression dependencies",
        )?;
        dependencies.push(dependency);
    }
    Ok(Some(dependencies))
}

pub(crate) fn incomplete_expression_parameters(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<BTreeSet<ParameterId>, CodecError> {
    const OWNERS: &str = "nx expression parameter owners";
    let mut owners_storage = ctx.reserve_scoped(0, OWNERS)?;
    let mut parameters_by_owner = BTreeMap::<_, Vec<&DesignParameter>>::new();
    for parameter in ctx.admit_iter(&ir.model.parameters, OWNERS)? {
        owners_storage.with_storage(|| {
            ctx.push_btree_group(
                &mut parameters_by_owner,
                &parameter.owner,
                parameter,
                OWNERS,
                "nx owned expression parameters",
            )
        })?;
    }
    let mut incomplete = BTreeSet::new();
    for parameters in ctx
        .admit_iter(&parameters_by_owner, OWNERS)?
        .map(|(_, parameters)| parameters)
    {
        let mut workspace = ctx.reserve_scoped(0, "nx expression completeness workspace")?;
        let mut ids_by_name = BTreeMap::<ExpressionKey<'_>, Vec<&ParameterId>>::new();
        for &parameter in ctx.admit_iter(parameters, "nx expression name index")? {
            let name_key = (parameter.name.as_str(), expression_unit(ctx, parameter)?);
            workspace.with_storage(|| {
                ctx.push_btree_group(
                    &mut ids_by_name,
                    name_key,
                    &parameter.id,
                    "nx expression name index",
                    "nx expression name identities",
                )
            })?;
        }
        let mut expected = Vec::new();
        for &parameter in ctx.admit_iter(parameters, "nx expected expression dependencies")? {
            let dependencies =
                expected_expression_dependencies(ctx, &mut workspace, parameter, &ids_by_name)?;
            ctx.reserve_scoped_vec(
                &mut workspace,
                &mut expected,
                1,
                "nx expected expression dependencies",
            )?;
            expected.push(dependencies);
        }
        let mut indices = BTreeMap::new();
        for (index, parameter) in ctx
            .admit_iter(parameters, "nx expression parameter index")?
            .enumerate()
        {
            workspace.with_storage(|| {
                ctx.insert_btree_map(
                    &mut indices,
                    &parameter.id,
                    index,
                    "nx expression parameter index",
                )
            })?;
        }
        let mut emitted = BTreeSet::new();
        let mut evaluated = BTreeMap::<&ParameterId, f64>::new();
        loop {
            ctx.charge_work(1, "nx expression parameter evaluation pass")?;
            let mut ready = None;
            for (index, dependencies) in expected.iter().enumerate() {
                ctx.charge_work(1, "nx expression parameter readiness")?;
                let Some(dependencies) = dependencies else {
                    continue;
                };
                if ctx.contains_btree_set(&emitted, &index, "nx emitted expression parameters")? {
                    continue;
                }
                let dependencies_ready = ctx.all_by(
                    dependencies,
                    |dependency| {
                        if !ctx.contains_key_btree_map(
                            &evaluated,
                            dependency,
                            "nx evaluated expression parameters",
                        )? {
                            return Ok(false);
                        }
                        match ctx.get_btree_map(
                            &indices,
                            dependency,
                            "nx expression parameter index",
                        )? {
                            Some(index) => ctx.contains_btree_set(
                                &emitted,
                                index,
                                "nx emitted expression parameters",
                            ),
                            None => Ok(false),
                        }
                    },
                    "nx expression parameter readiness",
                )?;
                if dependencies_ready {
                    ready = Some(index);
                    break;
                }
            }
            let Some(index) = ready else {
                break;
            };
            let Some(&parameter) = parameters.get(index) else {
                break;
            };
            let unit = expression_unit(ctx, parameter)?;
            let value = crate::native::om::evaluate_parameterized_expression(
                ctx,
                &parameter.expression,
                |name| {
                    let Some(ids) =
                        ctx.get_btree_map(&ids_by_name, &(name, unit), "nx expression name index")?
                    else {
                        return Ok(None);
                    };
                    let [dependency] = ids.as_slice() else {
                        return Ok(None);
                    };
                    Ok(ctx
                        .get_btree_map(
                            &evaluated,
                            *dependency,
                            "nx evaluated expression parameters",
                        )?
                        .copied())
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
                    cadmpeg_core::convert::f64_from_i64(*value)
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
                        workspace.with_storage(|| {
                            ctx.insert_btree_map(
                                &mut evaluated,
                                &parameter.id,
                                native_value,
                                "nx evaluated expression parameters",
                            )
                        })?;
                    }
                }
            }
            workspace.with_storage(|| {
                ctx.insert_btree_set(&mut emitted, index, "nx emitted expression parameters")
            })?;
        }
        for (index, &parameter) in ctx
            .admit_iter(parameters, "nx incomplete expression parameters")?
            .enumerate()
        {
            let dependencies_match = match expected.get(index) {
                Some(Some(dependencies)) => ctx.equal(
                    dependencies.as_slice(),
                    parameter.dependencies.as_slice(),
                    "nx expression dependency comparison",
                )?,
                _ => false,
            };
            if !dependencies_match
                || !ctx.contains_btree_set(&emitted, &index, "nx emitted expression parameters")?
                || !ctx.contains_key_btree_map(
                    &evaluated,
                    &parameter.id,
                    "nx evaluated expression parameters",
                )?
            {
                ctx.insert_btree_set(
                    &mut incomplete,
                    parameter
                        .id
                        .try_clone_for_decode(ctx, "nx incomplete expression identity")?,
                    "nx incomplete expression parameters",
                )?;
            }
        }
    }
    Ok(incomplete)
}

pub(crate) fn trim_surface_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::TrimSurface {
        faces, tool, keep, ..
    }) = feature.evaluation.definition()
    else {
        return Ok(true);
    };
    Ok(face_selection_is_incomplete(ctx, faces)?
        || path_ref_is_incomplete(ctx, tool)?
        || matches!(keep, TrimRegion::Unresolved))
}

pub(crate) fn extend_surface_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
        faces,
        distance,
        method,
    }) = feature.evaluation.definition()
    else {
        return Ok(true);
    };
    Ok(face_selection_is_incomplete(ctx, faces)?
        || distance.is_none()
        || matches!(method, cadmpeg_ir::features::SurfaceExtension::Unresolved))
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
    body_selection_is_incomplete(operands.target())
        || body_selection_is_incomplete(operands.tools())
}

pub(crate) fn trim_bodies_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::TrimBodies { operands, keep }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    body_selection_is_incomplete(operands.targets())
        || body_selection_is_incomplete(operands.tools())
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

pub(crate) fn hole_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        profile,
        face,
        placements,
        shape,

        extent,
        ..
    }) = feature.evaluation.definition()
    else {
        return Ok(true);
    };
    let construction = shape.construction();
    let exit_kind = shape.exit_kind();
    let diameter = shape.diameter();
    let construction_incomplete = match construction {
        cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. } => {
            hole_feature_is_incomplete(
                ctx,
                profile.as_ref(),
                face.as_ref(),
                placements.as_deref(),
                (kind, exit_kind.as_ref()),
                diameter.map(Into::into),
                extent.as_ref(),
            )?
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
                ctx,
                profile.as_ref(),
                face.as_ref(),
                placements.as_deref(),
                (&kind, exit_kind.as_ref()),
                diameter.map(Into::into),
                extent.as_ref(),
            )? || diameter.is_none_or(|diameter| major_diameter.get() <= diameter.get())
        }
    };
    Ok(construction_incomplete
        || match extent {
            Some(extent) => {
                termination_dependency_is_incomplete(ctx, extent, &feature.dependencies)?
            }
            None => false,
        }
        || match profile {
            Some(profile) => {
                planar_profile_dependency_is_incomplete(ctx, profile, &feature.dependencies)?
            }
            None => false,
        })
}

pub(crate) fn chamfer_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) =
        feature.evaluation.definition()
    else {
        return Ok(true);
    };
    ctx.any_by(
        groups,
        |group| {
            Ok(
                edge_selection_is_incomplete(ctx, &group.edges)?
                    || group.spec.is_unresolved(),
            )
        },
        "nx chamfer groups",
    )
}

pub(super) fn fillet_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        feature.evaluation.definition()
    else {
        return Ok(true);
    };
    ctx.any_by(
        groups,
        |group| {
            Ok(edge_selection_is_incomplete(ctx, &group.edges)?
                || group.radius.is_unresolved())
        },
        "nx fillet groups",
    )
}

pub(super) fn face_blend_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::FaceBlend { operands, radius }) =
        feature.evaluation.definition()
    else {
        return Ok(true);
    };
    Ok(
        face_selection_is_incomplete(ctx, operands.first_faces())?
            || face_selection_is_incomplete(ctx, operands.second_faces())?
            || radius.is_unresolved(),
    )
}

pub(super) fn shell_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    definition: &FeatureDefinition,
) -> Result<bool, CodecError> {
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
        return Ok(true);
    };
    Ok(match bodies {
        Some(bodies) => body_selection_is_incomplete(bodies),
        None => false,
    } || face_selection_is_incomplete(ctx, removed_faces)?
        || thickness.is_none()
        || outward.is_none()
        || mode.is_none()
        || join.is_none()
        || resolve_intersections.is_none()
        || allow_self_intersections.is_none())
}

pub(super) fn offset_surface_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::OffsetSurface { faces, distance }) =
        feature.evaluation.definition()
    else {
        return Ok(true);
    };
    Ok(face_selection_is_incomplete(ctx, faces)? || distance.is_none())
}

pub(crate) fn sphere_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Sphere { op, .. }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    matches!(op, BooleanOp::Unresolved)
}

pub(super) fn thicken_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Thicken {
        faces,
        thickness,
        side,
    }) = feature.evaluation.definition()
    else {
        return Ok(true);
    };
    Ok(face_selection_is_incomplete(ctx, faces)? || thickness.is_none() || side.is_none())
}

pub(super) fn draft_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Draft {
        faces,
        anchor,
        angle,
        outward,
    }) = feature.evaluation.definition()
    else {
        return Ok(true);
    };
    Ok(face_selection_is_incomplete(ctx, faces)?
        || match anchor {
            cadmpeg_ir::features::DraftAnchor::NeutralPlane { plane, .. } => {
                face_selection_is_incomplete(ctx, plane)?
            }
            cadmpeg_ir::features::DraftAnchor::PartingLine { tool, .. } => {
                face_selection_is_incomplete(ctx, tool)?
            }
        }
        || anchor.pull().is_none()
        || anchor
            .pull()
            .and_then(|pull| pull.plane.as_ref())
            .is_some_and(|plane| plane.as_str().is_empty())
        || angle.is_none()
        || outward.is_none())
}

pub(super) fn replace_face_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands }) =
        feature.evaluation.definition()
    else {
        return Ok(true);
    };
    Ok(face_selection_is_incomplete(ctx, operands.targets())?
        || face_selection_is_incomplete(ctx, operands.replacements())?)
}

pub(crate) fn loft_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Loft {
        sections,
        guidance,
        op,
        ..
    }) = feature.evaluation.definition()
    else {
        return Ok(true);
    };
    Ok(sections.len() < 2
        || ctx.any_by(
            sections,
            |section| {
                Ok(loft_section_is_incomplete(ctx, section)?
                    || match section {
                        LoftSection::Profile(profile) => profile_dependency_is_incomplete(
                            ctx,
                            profile,
                            &feature.dependencies,
                        )?,
                        LoftSection::Point(_) => false,
                    })
            },
            "nx loft sections",
        )?
        || match guidance {
            cadmpeg_ir::features::LoftGuidance::Guides(guides) => ctx.any_by(
                guides,
                |guide| path_ref_is_incomplete(ctx, guide),
                "nx loft guides",
            )?,
            cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                path_ref_is_incomplete(ctx, centerline)?
            }
        }
        || matches!(op, BooleanOp::Unresolved))
}

pub(crate) fn extrude_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
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
        return Ok(true);
    };
    Ok(profile_ref_is_incomplete(ctx, profile)?
        || profile_dependency_is_incomplete(ctx, profile, &feature.dependencies)?
        || matches!(
            direction,
            cadmpeg_ir::features::ExtrudeDirection::Unresolved {}
        )
        || extrude_start_is_incomplete(ctx, start)?
        || extrude_extent_is_incomplete(ctx, extent, &feature.dependencies)?
        || matches!(op, BooleanOp::Unresolved)
        || solid.is_none()
        || match direction {
            cadmpeg_ir::features::ExtrudeDirection::Explicit {
                source: Some(cadmpeg_ir::features::ExtrusionDirectionSource::Edge { reference }),
                ..
            } => path_ref_is_incomplete(ctx, reference)?,
            _ => false,
        })
}

pub(crate) fn revolve_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, op }) =
        feature.evaluation.definition()
    else {
        return Ok(true);
    };
    revolve_feature_is_incomplete(ctx, construction, *op, &feature.dependencies)
}

pub(crate) fn rib_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Rib { construction, op }) =
        feature.evaluation.definition()
    else {
        return Ok(true);
    };
    Ok(rib_feature_is_incomplete(ctx, construction, *op)?
        || match &construction.profile {
            Some(profile) => {
                planar_profile_dependency_is_incomplete(ctx, profile, &feature.dependencies)?
            }
            None => false,
        })
}

pub(crate) fn sweep_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape,

        path,

        orientation,
        transition,
        transformation,
        ..
    }) = feature.evaluation.definition()
    else {
        return Ok(true);
    };
    let sections_incomplete = match shape {
        SweepShape::Unresolved { section, sections }
        | SweepShape::Surface { section, sections } => {
            sweep_sections_are_incomplete(ctx, section, sections, &feature.dependencies)?
        }
        SweepShape::Solid {
            section, sections, ..
        } => sweep_sections_are_incomplete(ctx, section, sections, &feature.dependencies)?,
    };
    Ok(sections_incomplete
        || match path {
            Some(path) => path_ref_is_incomplete(ctx, path)?,
            None => true,
        }
        || sweep_mode_is_incomplete(shape.mode())
        || match orientation {
            Some(orientation) => sweep_orientation_is_incomplete(ctx, orientation)?,
            None => true,
        }
        || transition.is_none()
        || transformation.is_none())
}

/// Whether a sweep cross-section is unresolved or references an incomplete
/// profile or a profile feature outside the sweep's dependencies.
fn sweep_sections_are_incomplete<G>(
    ctx: &DecodeContext<'_>,
    section: &SweepSection<G>,
    sections: &[SweepSection<G>],
    dependencies: &[cadmpeg_ir::features::FeatureId],
) -> Result<bool, CodecError> {
    let section_is_incomplete = |section: &SweepSection<G>| {
        if section.is_unresolved() {
            return Ok(true);
        }
        match section.referenced_profile() {
            Some(profile) => Ok(planar_profile_ref_is_incomplete(ctx, profile)?
                || planar_profile_dependency_is_incomplete(ctx, profile, dependencies)?),
            None => Ok(false),
        }
    };
    Ok(section_is_incomplete(section)?
        || ctx.any_by(sections, section_is_incomplete, "nx sweep sections")?)
}

fn positive_feature_length(length: Length) -> bool {
    length.get() > 0.0
}

fn has_no_body_result_or_reference(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    if !feature.evaluation.outputs().is_empty() {
        return Ok(false);
    }
    let mut keys = feature.source_properties.keys();
    while keys.len() > 0 {
        let Some(key) = ctx.next_charged(&mut keys, PROPERTIES)? else {
            break;
        };
        let key = key.as_str();
        if key == "primary_body_reference"
            || key == "primary_body_object_index"
            || key == "primary_body_data_block"
            || key.starts_with("body_reference.")
            || key.starts_with("body_reference_occurrence.")
        {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
