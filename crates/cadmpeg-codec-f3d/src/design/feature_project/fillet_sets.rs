// SPDX-License-Identifier: Apache-2.0
//! Neutral projection of independently dimensioned Fillet sets.

use super::{design_length, design_positive_length, unique_feature_match, ProjectInputs};
use crate::design::edge_resolve::{
    resolved_fillet_scope_groups, ScopeTreatmentGroup, ScopeTreatmentInputs,
};
use crate::ids::{native_stream, neutral_feature_id};
use crate::loss::F3dLossCode;
use crate::records::feature::fillet_set::{
    DesignFilletMidpointOwners, DesignFilletSet, DesignFilletSetLaw,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::edge_treatments::{
    FilletGroup, RadiusForm, RadiusSpec, VariableRadii, VariableRadius,
};
use cadmpeg_ir::features::selections::EdgeSelection;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::HashMap;

pub(super) fn project(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<FeatureOperation>, CodecError> {
    let Some(sets) = scope.fillet_sets() else {
        return Ok(None);
    };
    if !sets.supports_set_law_projection() {
        return Ok(None);
    }
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(None);
    };
    let groups = || {
        inputs.construction_groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
    };
    if groups().any(|group| {
        !group.members().is_empty()
            && sets
                .sets
                .iter()
                .all(|set| set.group_record_index != group.record_index)
    }) {
        return Ok(None);
    }
    let scope_weights = pair_scope_weights(scope, stream, &sets.sets, parameters, losses);
    let parameter = |owner, kind| {
        unique_feature_match(parameters.iter().filter_map(|(_, parameter)| {
            (parameter.owner_record_index() == Some(owner)
                && native_stream(&parameter.id) == Some(stream)
                && parameter.source_kind_str() == kind)
                .then_some(*parameter)
        }))
    };
    let mut laws = Vec::new();
    let mut scope_groups = Vec::new();
    for set in &sets.sets {
        let mut set_losses = Vec::new();
        let (form, law) = radius_law(set, parameter);
        let radius = match law {
            Some((radius, reordered)) => {
                if reordered {
                    set_losses.push(F3dLossCode::FeatureFilletMidpointOrderAssumed.note(format!(
                        "{}: Fillet set {} applies its paired midpoint radii in increasing edge position.",
                        scope.id, set.record_index
                    )));
                }
                radius
            }
            None => {
                set_losses.push(F3dLossCode::FeatureFilletSetInputUnresolved.note(format!(
                    "{}: Fillet set {} retains its radius inputs in the native set.",
                    scope.id, set.record_index
                )));
                RadiusSpec::Unresolved { form: Some(form) }
            }
        };

        let mut tangency_weight = scope_weights.get(&set.group_record_index).copied();
        if let Some(owner) = set.tangency_weight_owner_record_index {
            tangency_weight = parameter(owner, "TangencyWeight")
                .filter(|parameter| parameter.unit().is_none())
                .map(DesignParameter::evaluated_value);
            if tangency_weight.is_none() {
                set_losses.push(F3dLossCode::FeatureFilletSetInputUnresolved.note(format!(
                    "{}: Fillet set {} retains its tangency-weight input in the native set.",
                    scope.id, set.record_index
                )));
            }
        }
        scope_groups.push(ScopeTreatmentGroup {
            group: unique_feature_match(
                groups().filter(|group| group.record_index == set.group_record_index),
            ),
            radius: match &radius {
                RadiusSpec::Constant { radius } => Some(radius.get()),
                _ => None,
            },
        });
        laws.push((radius, tangency_weight, set_losses));
    }
    let feature_id = neutral_feature_id(scope);
    let selections = resolved_fillet_scope_groups(
        &scope_groups,
        ScopeTreatmentInputs {
            groups: inputs.construction_groups,
            operands: inputs.edge_operands,
            identity_operands: inputs.edge_identity_operands,
            vertex_operands: inputs.edge_treatment_vertex_operands,
            histories: inputs.histories,
            previous_state_id: scope.previous_history_state_id(),
            feature_id: &feature_id,
        },
        ctx,
    )?;
    let mut projected = Vec::new();
    for ((set, (radius, tangency_weight, set_losses)), selection) in
        sets.sets.iter().zip(laws).zip(selections)
    {
        losses.extend(set_losses);
        let edges = if let Some(selection) = selection {
            losses.extend(selection.losses);
            selection.edges
        } else {
            losses.push(F3dLossCode::FeatureFilletSetGroupUnresolved.note(format!(
                "{}: Fillet set {} has an unresolved edge operand group at record {}.",
                scope.id, set.record_index, set.group_record_index
            )));
            EdgeSelection::Unresolved
        };
        projected.push(FilletGroup {
            edges,
            radius,
            tangency_weight,
        });
    }
    Ok(projected
        .try_into()
        .ok()
        .map(|groups| FeatureOperation::Fillet { groups }))
}

fn pair_scope_weights(
    scope: &DesignParameterScope,
    stream: &str,
    sets: &[DesignFilletSet],
    parameters: &[(u32, &DesignParameter)],
    losses: &mut Vec<LossNote>,
) -> HashMap<u32, FiniteReal> {
    let mut weights = parameters
        .iter()
        .copied()
        .filter(|(_, parameter)| {
            native_stream(&parameter.id) == Some(stream)
                && parameter.source_kind_str() == "TangencyWeight"
                && !sets.iter().any(|set| {
                    set.tangency_weight_owner_record_index
                        .is_some_and(|owner| parameter.owner_record_index() == Some(owner))
                })
        })
        .collect::<Vec<_>>();
    if weights.is_empty() {
        return HashMap::new();
    }
    weights.sort_unstable_by_key(|(ordinal, _)| *ordinal);
    let group_order = scope
        .reference_members()
        .values()
        .copied()
        .filter(|record| sets.iter().any(|set| set.group_record_index == *record))
        .collect::<Vec<_>>();
    if weights.len() == sets.len()
        && weights
            .iter()
            .all(|(_, parameter)| parameter.unit().is_none())
        && weights.windows(2).all(|pair| pair[0].0 != pair[1].0)
        && group_order.len() == sets.len()
        && sets.iter().all(|set| {
            set.is_short_constant()
                && group_order
                    .iter()
                    .filter(|record| **record == set.group_record_index)
                    .count()
                    == 1
        })
    {
        losses.push(F3dLossCode::FeatureFilletSetInputUnresolved.note(format!(
            "{}: Short Fillet sets assign tangency weights by owner-local and group reference order.",
            scope.id
        )));
        return group_order
            .into_iter()
            .zip(
                weights
                    .into_iter()
                    .map(|(_, weight)| weight.evaluated_value()),
            )
            .collect();
    }
    losses.push(F3dLossCode::FeatureFilletSetInputUnresolved.note(format!(
        "{}: Fillet sets leave scope-owned tangency weights unassigned.",
        scope.id
    )));
    HashMap::new()
}

fn radius_law<'a>(
    set: &DesignFilletSet,
    parameter: impl Fn(u32, &'static str) -> Option<&'a DesignParameter>,
) -> (RadiusForm, Option<(RadiusSpec, bool)>) {
    let positive = |owner, kind| design_positive_length(parameter(owner, kind)?);
    match set.law {
        DesignFilletSetLaw::Constant {
            radius_owner_record_index,
        } => (
            RadiusForm::Constant,
            positive(radius_owner_record_index, "Radius")
                .map(|radius| (RadiusSpec::Constant { radius }, false)),
        ),
        DesignFilletSetLaw::Chordal {
            chord_length_owner_record_index,
        } => (
            RadiusForm::Chordal,
            positive(chord_length_owner_record_index, "ChordLen")
                .map(|chord_length| (RadiusSpec::Chordal { chord_length }, false)),
        ),
        DesignFilletSetLaw::Asymmetric {
            offset_one_owner_record_index,
            offset_two_owner_record_index,
        } => (
            RadiusForm::Asymmetric,
            positive(offset_one_owner_record_index, "EdgeOffset1")
                .zip(positive(offset_two_owner_record_index, "EdgeOffset2"))
                .map(|(offset_one, offset_two)| {
                    (
                        RadiusSpec::Asymmetric {
                            offset_one,
                            offset_two,
                        },
                        false,
                    )
                }),
        ),
        DesignFilletSetLaw::Variable {
            start_radius_owner_record_index,
            end_radius_owner_record_index,
            middle,
        } => {
            let resolve = || {
                let sample = |owner: DesignFilletMidpointOwners| {
                    let position = parameter(owner.position_owner_record_index, "MidParams")?;
                    (position.unit().is_none()).then_some(VariableRadius {
                        parameter: position.evaluated_value().get(),
                        radius: design_length(parameter(
                            owner.radius_owner_record_index,
                            "MidRadius",
                        )?)?,
                    })
                };
                let mut middle = [sample(middle[0])?, sample(middle[1])?];
                let reordered = middle[0].parameter > middle[1].parameter;
                if reordered {
                    middle.swap(0, 1);
                }
                let [first, second] = middle;
                let points = VariableRadii::new(vec![
                    VariableRadius {
                        parameter: 0.0,
                        radius: design_length(parameter(
                            start_radius_owner_record_index,
                            "StartRadius",
                        )?)?,
                    },
                    first,
                    second,
                    VariableRadius {
                        parameter: 1.0,
                        radius: design_length(parameter(
                            end_radius_owner_record_index,
                            "EndRadius",
                        )?)?,
                    },
                ])
                .ok()?;
                Some((RadiusSpec::Variable { points }, reordered))
            };
            (RadiusForm::Variable, resolve())
        }
    }
}

#[cfg(test)]
mod tests;
