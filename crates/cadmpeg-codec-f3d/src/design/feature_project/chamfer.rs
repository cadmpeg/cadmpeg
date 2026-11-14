// SPDX-License-Identifier: Apache-2.0
use super::{design_positive_length, unique_feature_match, ProjectInputs};
use crate::design::edge_resolve::resolved_edge_treatment_group_with_corners;
use crate::design::face_resolve::design_angle;
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::chamfer_set::DesignChamferSetSpec;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand;
use crate::records::parameters::DesignParameter;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    edge_treatments::{ChamferForm, ChamferGroup, ChamferSpec},
    EdgeSelection, FeatureDefinition, FeatureOperation,
};
use cadmpeg_ir::report::loss::LossNote;

pub(super) fn project(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<FeatureDefinition, CodecError> {
    if parameters.is_empty() {
        if let Some(definition) = project_fixed_chamfer(
            scope,
            inputs.construction_groups,
            inputs.edge_operands,
            inputs.edge_identity_operands,
            inputs.edge_treatment_vertex_operands,
            inputs.histories,
            (ctx, losses),
        )? {
            return Ok(definition);
        }
    }
    if let Some(definition) = project_chamfer(scope, parameters, inputs, ctx, losses)? {
        return Ok(definition);
    }
    if let Some(sets) = scope.chamfer_sets() {
        let mut groups = Vec::new();
        for set in &sets.sets {
            let spec = set_spec(scope, set.spec, inputs).unwrap_or_else(|| {
                losses.push(F3dLossCode::ChamferDefinitionUnresolved.note(format!(
                    "Chamfer set {} of {} retains its form with unresolved dimensions; its parameter ownership or scalar is incomplete.",
                    set.record_index, scope.id
                )));
                ChamferSpec::Unresolved { form: Some(set_form(set.spec)) }
            });
            let group = unique_feature_match(inputs.construction_groups.iter().filter(|group| {
                native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
                    && group.record_index == set.group_record_index
            }));
            let edges = match group {
                Some(group) => selection(scope, group, inputs, ctx, losses)?,
                None => unresolved_selection(scope, losses),
            };
            groups.push(ChamferGroup { edges, spec });
        }
        if let Ok(groups) = groups.try_into() {
            return Ok(FeatureDefinition::Operation(FeatureOperation::Chamfer {
                groups,
                flip_direction: false,
            }));
        }
    }
    losses.push(F3dLossCode::ChamferDefinitionUnresolved.note(format!(
        "Chamfer {} retains its selectors and parameter expressions with an unresolved specification; the dimensional lanes do not establish a complete pairing.",
        scope.id
    )));
    let mut groups = Vec::new();
    let mut native_groups = inputs
        .construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
                && group.extrude_role().is_none()
        })
        .collect::<Vec<_>>();
    native_groups.sort_by_key(|group| group.scope_reference_ordinal);
    for group in native_groups {
        groups.push(ChamferGroup {
            edges: selection(scope, group, inputs, ctx, losses)?,
            spec: ChamferSpec::Unresolved { form: None },
        });
    }
    let groups = groups.try_into().unwrap_or_else(|_| {
        cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
            edges: unresolved_selection(scope, losses),
            spec: ChamferSpec::Unresolved { form: None },
        })
    });
    Ok(FeatureDefinition::Operation(FeatureOperation::Chamfer {
        groups,
        flip_direction: false,
    }))
}

fn set_form(spec: DesignChamferSetSpec) -> ChamferForm {
    match spec {
        DesignChamferSetSpec::EqualDistance { .. } => ChamferForm::Distance,
        DesignChamferSetSpec::TwoDistances { .. } => ChamferForm::TwoDistances,
        DesignChamferSetSpec::DistanceAngle { .. } => ChamferForm::DistanceAngle,
    }
}

fn set_spec(
    scope: &DesignParameterScope,
    spec: DesignChamferSetSpec,
    inputs: &ProjectInputs<'_>,
) -> Option<ChamferSpec> {
    let parameter = |record_index| {
        let stream = native_stream(&scope.id)?;
        let owner = unique_feature_match(inputs.owners.iter().filter(|owner| {
            native_stream(owner.id()) == Some(stream)
                && owner.scope_record_index() == scope.record_index
                && owner.record_index() == record_index
        }))?;
        unique_feature_match(inputs.native.iter().filter(|parameter| {
            native_stream(&parameter.id) == Some(stream)
                && parameter.record_index == owner.parameter_record_index()
                && parameter.owner_record_index() == Some(record_index)
        }))
    };
    match spec {
        DesignChamferSetSpec::EqualDistance {
            distance_owner_record_index,
        } => Some(ChamferSpec::Distance {
            distance: design_positive_length(parameter(distance_owner_record_index)?)?,
        }),
        DesignChamferSetSpec::TwoDistances {
            first_owner_record_index,
            second_owner_record_index,
        } => {
            if first_owner_record_index == second_owner_record_index {
                return None;
            }
            Some(ChamferSpec::TwoDistances {
                first: design_positive_length(parameter(first_owner_record_index)?)?,
                second: design_positive_length(parameter(second_owner_record_index)?)?,
            })
        }
        DesignChamferSetSpec::DistanceAngle {
            distance_owner_record_index,
            angle_owner_record_index,
        } => {
            if distance_owner_record_index == angle_owner_record_index {
                return None;
            }
            Some(ChamferSpec::DistanceAngle {
                distance: design_positive_length(parameter(distance_owner_record_index)?)?,
                angle: cadmpeg_ir::scalar::InteriorAngle::try_from(design_angle(parameter(
                    angle_owner_record_index,
                )?)?)
                .ok()?,
            })
        }
    }
}

fn selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<EdgeSelection, CodecError> {
    resolved_edge_treatment_group_with_corners(
        group,
        crate::design::edge_resolve::EdgeTreatmentInputs {
            groups: inputs.construction_groups,
            operands: inputs.edge_operands,
            identity_operands: inputs.edge_identity_operands,
            vertex_operands: inputs.edge_treatment_vertex_operands,
            histories: inputs.histories,
            previous_state_id: scope.previous_history_state_id(),
            feature_id: &crate::ids::neutral_feature_id(scope),
            treatment_radius: None,
        },
        ctx,
        losses,
    )
}

fn unresolved_selection(scope: &DesignParameterScope, losses: &mut Vec<LossNote>) -> EdgeSelection {
    losses.push(F3dLossCode::ChamferSelectionUnresolved.note(format!(
        "Chamfer {} retains its scope selection because its construction group is missing or ambiguous.",
        scope.id
    )));
    EdgeSelection::Native(scope.id.clone())
}

pub(super) fn retains_parameters(definition: &FeatureDefinition) -> bool {
    match definition {
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
            let first = std::mem::discriminant(&groups.as_slice()[0].spec);
            groups.as_slice().iter().any(|group| {
                group.spec.is_unresolved() || std::mem::discriminant(&group.spec) != first
            })
        }
        _ => false,
    }
}

fn project_chamfer(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    enum Lanes<'a> {
        Distance(&'a [&'a DesignParameter]),
        TwoDistances(&'a [&'a DesignParameter], &'a [&'a DesignParameter]),
        DistanceAngle(&'a [&'a DesignParameter], &'a [&'a DesignParameter]),
    }
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        FeatureDefinition, FeatureOperation,
    };

    let construction_groups = inputs.construction_groups;
    let edge_operands = inputs.edge_operands;
    let edge_identity_operands = inputs.edge_identity_operands;
    let edge_treatment_vertex_operands = inputs.edge_treatment_vertex_operands;
    let histories = inputs.histories;
    let native_scope = native_stream(&scope.id);
    let mut edge_groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == native_scope
            && group.scope_record_index == scope.record_index
            && group.extrude_role().is_none()
    }) {
        edge_groups.push(group);
    }
    edge_groups.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    let ordered_parameters =
        |matches_kind: &dyn Fn(&str) -> bool| -> Result<Vec<&DesignParameter>, CodecError> {
            {
                let mut matches = Vec::new();
                for parameter in parameters
                    .iter()
                    .filter(|(_, parameter)| matches_kind(parameter.source_kind_str()))
                    .copied()
                {
                    matches.push(parameter);
                }
                matches.sort_by_key(|left| left.0);
                let mut out = Vec::new();
                for (_, parameter) in matches {
                    out.push(parameter);
                }
                Ok(out)
            }
        };
    let distances = ordered_parameters(&|kind| kind == "Distance")?;
    let first_distances = ordered_parameters(&|kind| kind == "Distance 1")?;
    let second_distances = ordered_parameters(&|kind| kind == "Distance 2")?;
    let left_distances = ordered_parameters(&|kind| kind == "leftDistance")?;
    let right_distances = ordered_parameters(&|kind| kind == "rightDistance")?;
    let angles =
        ordered_parameters(&|kind| matches!(kind, "Angle" | "Rotate Angle" | "rotateAngle"))?;
    let face_groups = edge_groups
        .iter()
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X10)
        .count();
    let one_specification = [&distances, &first_distances, &left_distances]
        .into_iter()
        .map(Vec::len)
        .sum::<usize>()
        == 1;
    let shared_specification =
        one_specification && face_groups != 0 && edge_groups.len() - face_groups == 1;
    let group_count = if shared_specification {
        1
    } else {
        edge_groups.len().max(1)
    };
    let edge_role_assumed = shared_specification
        && edge_groups.iter().any(|group| {
            !matches!(
                group.role(),
                DesignOperandRole::ROLE_0X10 | DesignOperandRole::BODIES_B
            )
        });
    if !parameters.iter().all(|(_, parameter)| {
        matches!(
            parameter.source_kind_str(),
            "Distance"
                | "Distance 1"
                | "Distance 2"
                | "leftDistance"
                | "rightDistance"
                | "Angle"
                | "Rotate Angle"
                | "rotateAngle"
        )
    }) {
        return Ok(None);
    }

    let lanes = if !left_distances.is_empty() || !right_distances.is_empty() {
        if !distances.is_empty() || !first_distances.is_empty() || !second_distances.is_empty() {
            return Ok(None);
        }
        if right_distances.is_empty() && !angles.is_empty() {
            if left_distances.len() != group_count || angles.len() != group_count {
                return Ok(None);
            }
            Lanes::DistanceAngle(&left_distances, &angles)
        } else if right_distances.is_empty() {
            if left_distances.len() != group_count {
                return Ok(None);
            }
            Lanes::Distance(&left_distances)
        } else {
            if !angles.is_empty()
                || left_distances.len() != group_count
                || right_distances.len() != group_count
            {
                return Ok(None);
            }
            Lanes::TwoDistances(&left_distances, &right_distances)
        }
    } else if !first_distances.is_empty() || !second_distances.is_empty() {
        if !distances.is_empty()
            || !angles.is_empty()
            || first_distances.len() != group_count
            || second_distances.len() != group_count
        {
            return Ok(None);
        }
        Lanes::TwoDistances(&first_distances, &second_distances)
    } else if !angles.is_empty() {
        if distances.len() != group_count || angles.len() != group_count {
            return Ok(None);
        }
        Lanes::DistanceAngle(&distances, &angles)
    } else if !distances.is_empty() {
        if distances.len() != group_count {
            return Ok(None);
        }
        Lanes::Distance(&distances)
    } else {
        return Ok(None);
    };
    let mut candidates = Vec::new();
    for ordinal in 0..group_count {
        let spec = match lanes {
            Lanes::Distance(distance) => design_positive_length(distance[ordinal])
                .map(|distance| ChamferSpec::Distance { distance }),
            Lanes::TwoDistances(first, second) => design_positive_length(first[ordinal])
                .zip(design_positive_length(second[ordinal]))
                .map(|(first, second)| ChamferSpec::TwoDistances { first, second }),
            Lanes::DistanceAngle(distance, angle) => design_positive_length(distance[ordinal])
                .zip(
                    design_angle(angle[ordinal])
                        .and_then(|value| cadmpeg_ir::scalar::InteriorAngle::try_from(value).ok()),
                )
                .map(|(distance, angle)| ChamferSpec::DistanceAngle { distance, angle }),
        };
        let spec = or_none!(spec);
        candidates.push(spec);
    }
    let mut groups = Vec::new();
    if edge_groups.is_empty() {
        groups.push(ChamferGroup {
            edges: unresolved_selection(scope, losses),
            spec: candidates[0],
        });
    }
    for (ordinal, group) in edge_groups.into_iter().enumerate() {
        let spec = *or_none!(candidates.get(if group_count == 1 { 0 } else { ordinal }));
        let group = ChamferGroup {
            edges: resolved_edge_treatment_group_with_corners(
                group,
                crate::design::edge_resolve::EdgeTreatmentInputs {
                    groups: construction_groups,
                    operands: edge_operands,
                    identity_operands: edge_identity_operands,
                    vertex_operands: edge_treatment_vertex_operands,
                    histories,
                    previous_state_id: scope.previous_history_state_id(),
                    feature_id: &crate::ids::neutral_feature_id(scope),
                    treatment_radius: None,
                },
                ctx,
                losses,
            )?,
            spec,
        };
        groups.push(group);
    }
    let groups = or_none!(groups.try_into().ok());
    if edge_role_assumed {
        losses.push(crate::loss::F3dLossCode::ChamferEdgeRoleAssumed.note(format!(
            "Chamfer scope {} shares one specification between face groups and a group whose role is not the edge role; the group is read as the edge group.",
            scope.id
        )));
    }
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups,
            flip_direction: false,
        },
    )))
}

fn project_fixed_chamfer(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    edge_treatment_vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
    (ctx, losses): (
        &DecodeContext<'_>,
        &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    ),
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        FeatureDefinition, FeatureOperation,
    };

    let fixed = or_none!(scope.fixed_chamfer_parameters());
    let stream = or_none!(native_stream(&scope.id));
    let group = unique_feature_match(construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    }));
    let spec = match fixed {
        crate::records::feature::fixed_parameters::DesignFixedChamferParameters::EqualDistance { distance } => {
            ChamferSpec::Distance {
                distance: or_none!(cadmpeg_ir::scalar::PositiveLength::new(distance.value.get() * 10.0)),
            }
        }
        crate::records::feature::fixed_parameters::DesignFixedChamferParameters::TwoDistances { first, second } => {
            ChamferSpec::TwoDistances {
                first: or_none!(cadmpeg_ir::scalar::PositiveLength::new(first.value.get() * 10.0)),
                second: or_none!(cadmpeg_ir::scalar::PositiveLength::new(second.value.get() * 10.0)),
            }
        }
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
                edges: match group {
                    None => unresolved_selection(scope, losses),
                    Some(group) => resolved_edge_treatment_group_with_corners(
                        group,
                        crate::design::edge_resolve::EdgeTreatmentInputs {
                            groups: construction_groups,
                            operands: edge_operands,
                            identity_operands: edge_identity_operands,
                            vertex_operands: edge_treatment_vertex_operands,
                            histories,
                            previous_state_id: scope.previous_history_state_id(),
                            feature_id: &crate::ids::neutral_feature_id(scope),
                            treatment_radius: None,
                        },
                        ctx,
                        losses,
                    )?,
                },
                spec,
            }),
            flip_direction: false,
        },
    )))
}

pub(super) fn retained_parameters<'a>(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'a>,
) -> Vec<&'a DesignParameter> {
    let stream = native_stream(&scope.id);
    inputs
        .native
        .iter()
        .filter(|parameter| {
            native_stream(&parameter.id) == stream
                && (inputs.owners.iter().any(|owner| {
                    native_stream(owner.id()) == stream
                        && owner.scope_record_index() == scope.record_index
                        && owner.parameter_record_index() == parameter.record_index
                }) || parameter.owner_record_index().is_some_and(|owner| {
                    scope
                        .reference_members()
                        .values()
                        .any(|member| member == owner)
                        || scope.chamfer_sets().is_some_and(|sets| {
                            sets.sets.iter().any(|set| match set.spec {
                                DesignChamferSetSpec::EqualDistance {
                                    distance_owner_record_index,
                                } => owner == distance_owner_record_index,
                                DesignChamferSetSpec::TwoDistances {
                                    first_owner_record_index,
                                    second_owner_record_index,
                                } => {
                                    owner == first_owner_record_index
                                        || owner == second_owner_record_index
                                }
                                DesignChamferSetSpec::DistanceAngle {
                                    distance_owner_record_index,
                                    angle_owner_record_index,
                                } => {
                                    owner == distance_owner_record_index
                                        || owner == angle_owner_record_index
                                }
                            })
                        })
                }))
        })
        .collect()
}
