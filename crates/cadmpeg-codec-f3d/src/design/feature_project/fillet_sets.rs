// SPDX-License-Identifier: Apache-2.0
//! Neutral projection of independently dimensioned Fillet sets.

use super::{design_length, design_positive_length, unique_feature_match, ProjectInputs};
use crate::design::edge_resolve::{
    resolved_edge_treatment_group_with_corners, EdgeTreatmentInputs,
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
use cadmpeg_ir::features::{
    edge_treatments::{FilletGroup, RadiusForm, RadiusSpec, VariableRadii, VariableRadius},
    EdgeSelection, FeatureOperation,
};
use cadmpeg_ir::report::loss::LossNote;

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
    let parameter = |owner, kind| {
        unique_feature_match(parameters.iter().filter_map(|(_, parameter)| {
            (parameter.owner_record_index() == Some(owner)
                && native_stream(&parameter.id) == Some(stream)
                && parameter.source_kind_str() == kind)
                .then_some(*parameter)
        }))
    };
    let feature_id = neutral_feature_id(scope);
    let mut projected = Vec::new();
    for set in &sets.sets {
        let (form, law) = radius_law(set, parameter);
        let radius = match law {
            Some((radius, reordered)) => {
                if reordered {
                    losses.push(F3dLossCode::FeatureFilletMidpointOrderAssumed.note(format!(
                        "{}: Fillet set {} applies its paired midpoint radii in increasing edge position.",
                        scope.id, set.record_index
                    )));
                }
                radius
            }
            None => {
                losses.push(F3dLossCode::FeatureFilletSetInputUnresolved.note(format!(
                    "{}: Fillet set {} retains its radius inputs in the native set.",
                    scope.id, set.record_index
                )));
                RadiusSpec::Unresolved { form: Some(form) }
            }
        };

        let mut tangency_weight = None;
        if let Some(owner) = set.tangency_weight_owner_record_index {
            tangency_weight = parameter(owner, "TangencyWeight")
                .filter(|parameter| parameter.unit().is_none())
                .map(DesignParameter::evaluated_value);
            if tangency_weight.is_none() {
                losses.push(F3dLossCode::FeatureFilletSetInputUnresolved.note(format!(
                    "{}: Fillet set {} retains its tangency-weight input in the native set.",
                    scope.id, set.record_index
                )));
            }
        }
        let edges = if let Some(group) = unique_feature_match(
            groups().filter(|group| group.record_index == set.group_record_index),
        ) {
            resolved_edge_treatment_group_with_corners(
                group,
                EdgeTreatmentInputs {
                    groups: inputs.construction_groups,
                    operands: inputs.edge_operands,
                    identity_operands: inputs.edge_identity_operands,
                    vertex_operands: inputs.edge_treatment_vertex_operands,
                    histories: inputs.histories,
                    previous_state_id: scope.previous_history_state_id(),
                    feature_id: &feature_id,
                    treatment_radius: None,
                },
                ctx,
                losses,
            )?
        } else {
            EdgeSelection::Native(format!(
                "{stream}:design-construction-operand-group#{}",
                set.group_record_index
            ))
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
