use super::{
    design_positive_length, full_round_fillet_source, native_scope_definition,
    project_face_selection, project_full_round_fillet, project_variable_fillet,
    resolved_fillet_assignments, ProjectInputs,
};
use crate::design::edge_resolve::{
    project_fixed_fillet_with_corners, resolved_edge_treatment_group_with_corners,
};
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand;
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::{DesignEdgeIdentityOperand, DesignEdgeOperand};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::edge_treatments::{
    FilletGroup, FullRoundFilletGroup, FullRoundSideSelection, RadiusSpec,
};
use cadmpeg_ir::features::{EdgeSelection, FaceSelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashSet;

pub(super) enum FilletProjection<'a> {
    Existing(FeatureDefinition),
    Promoted {
        definition: FeatureDefinition,
        source: Box<FilletSource<'a>>,
    },
}

#[derive(serde::Serialize)]
pub(super) struct FilletSource<'a> {
    scope: &'a DesignParameterScope,
    parameters: Vec<&'a DesignParameter>,
    owners: Vec<&'a DesignParameterOwner>,
    groups: Vec<&'a DesignConstructionOperandGroup>,
    edge_operands: Vec<&'a DesignEdgeOperand>,
    edge_identities: Vec<&'a DesignEdgeIdentityOperand>,
    face_operands: Vec<&'a DesignFaceOperand>,
    vertex_operands: Vec<&'a DesignEdgeTreatmentVertexOperand>,
    entity_selections:
        Vec<&'a crate::records::topology::entity_selection::DesignEntitySelectionOperand>,
}

pub(super) fn project<'a>(
    ctx: &DecodeContext<'_>,
    inputs: &ProjectInputs<'a>,
    scope: &'a DesignParameterScope,
    parameters: &[(u32, &'a DesignParameter)],
    native_scope: &str,
    losses: &mut Vec<LossNote>,
) -> Result<FilletProjection<'a>, CodecError> {
    if scope.fillet_sets().is_some_and(|sets| {
        sets.projects_short_sets(
            inputs
                .construction_groups
                .iter()
                .filter(|group| {
                    native_stream(&group.id) == Some(native_scope)
                        && group.scope_record_index == scope.record_index
                        && !group.members().is_empty()
                })
                .map(|group| group.record_index),
        )
    }) {
        return native_scope_definition(scope, parameters).map(FilletProjection::Existing);
    }
    if parameters.is_empty()
        && crate::design::decode::operands::fixed_fillet_assignment_is_unresolved(
            scope,
            inputs.construction_groups,
        )
    {
        return project_partial(ctx, inputs, scope, parameters, native_scope, losses);
    }

    if parameters.is_empty() {
        if let Some(definition) = project_full_round_fillet(
            scope,
            inputs.construction_groups,
            inputs.face_operands,
            inputs.owners,
            inputs.fillet_radius_groups,
            inputs.histories,
        )? {
            return Ok(FilletProjection::Existing(definition));
        }
    }

    let mut assignments = Vec::new();
    for assignment in inputs.fillet_radius_groups {
        if native_stream(&assignment.id) == Some(native_scope)
            && assignment.scope_record_index == scope.record_index
        {
            assignments.push(assignment);
        }
    }
    assignments.sort_by_key(|left| left.group_ordinal);
    if !assignments.is_empty()
        && inputs.construction_groups.iter().any(|group| {
            native_stream(&group.id) == Some(native_scope)
                && group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::ROLE_0X10
                && !group.members().is_empty()
                && assignments
                    .iter()
                    .all(|assignment| assignment.group_record_index != group.record_index)
        })
    {
        return project_partial(ctx, inputs, scope, parameters, native_scope, losses);
    }
    if !assignments.is_empty() {
        let Some(assignments) = resolved_fillet_assignments(&assignments, parameters) else {
            return project_partial(ctx, inputs, scope, parameters, native_scope, losses);
        };
        let mut groups = Vec::new();
        for resolved in assignments {
            let edge_radius = match &resolved.radius {
                RadiusSpec::Constant { radius } => Some(radius.get()),
                _ => None,
            };
            let edges = if let Some(group) = inputs.construction_groups.iter().find(|group| {
                native_stream(&group.id) == Some(native_scope)
                    && group.record_index == resolved.assignment.group_record_index
            }) {
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
                        treatment_radius: edge_radius,
                    },
                    ctx,
                    losses,
                )?
            } else {
                EdgeSelection::Native(resolved.assignment.id.clone())
            };
            groups.push(FilletGroup {
                edges,
                radius: resolved.radius,
                tangency_weight: resolved.tangency_weight,
            });
        }
        let groups = groups.try_into().ok();
        return groups.map_or_else(
            || project_partial(ctx, inputs, scope, parameters, native_scope, losses),
            |groups| {
                Ok(FilletProjection::Existing(FeatureDefinition::Operation(
                    FeatureOperation::Fillet { groups },
                )))
            },
        );
    }
    if let Some(definition) = project_variable_fillet(scope, parameters, inputs, ctx, losses)? {
        return Ok(FilletProjection::Existing(definition));
    }
    if parameters.is_empty() {
        if let Some(definition) = project_fixed_fillet_with_corners(
            scope,
            inputs.construction_groups,
            inputs.edge_operands,
            inputs.edge_identity_operands,
            inputs.edge_treatment_vertex_operands,
            inputs.histories,
            (ctx, losses),
        )? {
            return Ok(FilletProjection::Existing(definition));
        }
    }
    let [(_, parameter)] = parameters else {
        return project_partial(ctx, inputs, scope, parameters, native_scope, losses);
    };
    let Some(radius) = parameter
        .is_fillet_radius()
        .then(|| design_positive_length(parameter))
        .flatten()
    else {
        return project_partial(ctx, inputs, scope, parameters, native_scope, losses);
    };
    Ok(FilletProjection::Existing(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(FilletGroup {
                edges: EdgeSelection::Native(scope.id.clone()),
                radius: RadiusSpec::Constant { radius },
                tangency_weight: None,
            }),
        },
    )))
}

fn project_partial<'a>(
    ctx: &DecodeContext<'_>,
    inputs: &ProjectInputs<'a>,
    scope: &'a DesignParameterScope,
    parameters: &[(u32, &'a DesignParameter)],
    stream: &str,
    losses: &mut Vec<LossNote>,
) -> Result<FilletProjection<'a>, CodecError> {
    if !parameters.is_empty()
        || scope.fixed_fillet_parameters().is_some()
        || scope.fillet_sets().is_some()
        || inputs.fillet_radius_groups.iter().any(|assignment| {
            native_stream(&assignment.id) == Some(stream)
                && assignment.scope_record_index == scope.record_index
        })
    {
        return native_scope_definition(scope, parameters).map(FilletProjection::Existing);
    }
    let source = Box::new(FilletSource::new(inputs, scope));
    if source.parameters.is_empty() && source.owners.is_empty() {
        if let Some((group, _)) = full_round_fillet_source(
            scope,
            inputs.construction_groups,
            inputs.face_operands,
            inputs.owners,
            inputs.fillet_radius_groups,
        )
        .filter(|(group, _)| {
            scope.reference_members().values().all(|index| {
                *index == group.record_index
                    || group.members().iter().any(|member| member.value == *index)
                    || group
                        .frame
                        .trailing_records()
                        .iter()
                        .any(|record| record.value == *index)
            })
        }) {
            let center_faces =
                project_face_selection(scope, group, inputs.face_operands, inputs.histories)?;
            if matches!(
                center_faces,
                FaceSelection::Native(_) | FaceSelection::HistoricalPartial { .. }
            ) {
                losses.push(crate::loss::F3dLossCode::FeatureFilletFullRoundCenterUnresolved.note(
                    format!("Fillet scope {} retains its bounded-face full-round structure with an unresolved center selection {} (DR-321).", scope.id, group.id),
                ));
            }
            return Ok(FilletProjection::Promoted {
                definition: FeatureDefinition::Operation(FeatureOperation::FullRoundFillet {
                    groups: cadmpeg_ir::features::NonEmptyMembers::one(
                        FullRoundFilletGroup::new(
                            center_faces,
                            FullRoundSideSelection::Automatic,
                            FullRoundSideSelection::Automatic,
                        )
                        .map_err(CodecError::malformed)?,
                    ),
                }),
                source,
            });
        }
    }
    let mut selected = source
        .groups
        .iter()
        .copied()
        .filter(|group| group.scope_record_index == scope.record_index)
        .collect::<Vec<_>>();
    selected.sort_by_key(|group| (group.scope_reference_ordinal, group.record_index));
    let mut seen = HashSet::new();
    let mut groups = Vec::new();
    for group in selected {
        if !seen.insert(group.record_index) {
            continue;
        }
        let unique = source
            .groups
            .iter()
            .filter(|candidate| candidate.record_index == group.record_index)
            .count()
            == 1;
        let edges = if unique && group.role() == DesignOperandRole::BODIES_B {
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
            )?
        } else {
            EdgeSelection::Native(group.id.clone())
        };
        groups.push(FilletGroup {
            edges,
            radius: RadiusSpec::Unresolved { form: None },
            tangency_weight: None,
        });
    }
    if groups.is_empty() {
        groups.push(FilletGroup {
            edges: EdgeSelection::Native(scope.id.clone()),
            radius: RadiusSpec::Unresolved { form: None },
            tangency_weight: None,
        });
    }
    losses.push(crate::loss::F3dLossCode::FeatureFilletRadiusUnresolved.note(format!(
        "Fillet scope {} retains {} selection group(s) with an unresolved radius law and dimension; no decoded radius carrier establishes a value (DR-321).",
        scope.id, groups.len(),
    )));
    if !scope.reference_members().is_empty() || !source.owners.is_empty() {
        let links = source
            .owners
            .iter()
            .map(|owner| (owner.record_index(), owner.parameter_record_index()))
            .collect::<Vec<_>>();
        losses.push(crate::loss::F3dLossCode::FeatureFilletReferencesUnresolved.note(format!(
            "Fillet scope {} retains ordered references {:?} and owner-to-parameter links {links:?}; missing definitions and unclassified reference contributions do not establish a radius or weight (DR-321).",
            scope.id, scope.reference_members(),
        )));
    }
    Ok(FilletProjection::Promoted {
        definition: FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: groups.try_into().map_err(CodecError::from)?,
        }),
        source,
    })
}

impl<'a> FilletSource<'a> {
    fn new(inputs: &ProjectInputs<'a>, scope: &'a DesignParameterScope) -> Self {
        let stream = native_stream(&scope.id);
        let in_stream = |id: &str| stream.is_some() && native_stream(id) == stream;
        let named = |index| {
            scope
                .reference_members()
                .values()
                .any(|value| *value == index)
        };
        let owners = inputs
            .owners
            .iter()
            .filter(|owner| {
                in_stream(owner.id())
                    && (owner.scope_record_index() == scope.record_index
                        || named(owner.record_index()))
            })
            .collect::<Vec<_>>();
        let parameters = inputs
            .source_parameters
            .iter()
            .filter(|parameter| {
                in_stream(&parameter.id)
                    && (named(parameter.record_index)
                        || owners
                            .iter()
                            .any(|owner| owner.parameter_record_index() == parameter.record_index)
                        || parameter.owner_record_index().is_some_and(|index| {
                            named(index) || owners.iter().any(|owner| owner.record_index() == index)
                        }))
            })
            .collect();
        Self {
            scope,
            parameters,
            owners,
            groups: inputs
                .construction_groups
                .iter()
                .filter(|group| {
                    in_stream(&group.id)
                        && (group.scope_record_index == scope.record_index
                            || named(group.record_index))
                })
                .collect(),
            edge_operands: inputs
                .edge_operands
                .iter()
                .filter(|operand| {
                    in_stream(&operand.id) && operand.scope_record_index == scope.record_index
                })
                .collect(),
            edge_identities: inputs
                .edge_identity_operands
                .iter()
                .filter(|operand| {
                    in_stream(&operand.id) && operand.scope_record_index == scope.record_index
                })
                .collect(),
            face_operands: inputs
                .face_operands
                .iter()
                .filter(|operand| {
                    in_stream(&operand.id) && operand.scope_record_index == scope.record_index
                })
                .collect(),
            vertex_operands: inputs
                .edge_treatment_vertex_operands
                .iter()
                .filter(|operand| {
                    in_stream(&operand.id) && operand.scope_record_index == scope.record_index
                })
                .collect(),
            entity_selections: inputs
                .entity_selection_operands
                .iter()
                .filter(|operand| {
                    in_stream(&operand.id) && operand.scope_record_index == scope.record_index
                })
                .collect(),
        }
    }
}
