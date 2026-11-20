// SPDX-License-Identifier: Apache-2.0
//! Complete unresolved treatment members from historical recipe boundaries.

use super::{
    native_stream, resolved_edge_operand, DesignConstructionOperandGroup,
    DesignEdgeIdentityOperand, DesignEdgeOperand,
};
use crate::records::topology::historical_context::AsmFaceBoundary;
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

pub(crate) fn apply_features(
    features: &mut [cadmpeg_ir::features::Feature],
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identities: &[DesignEdgeIdentityOperand],
    boundaries: &[AsmFaceBoundary],
    losses: &mut Vec<LossNote>,
) {
    for feature in features {
        let id = &feature.id;
        feature.evaluation.edit(|definition, _| {
            apply(
                definition, id, groups, operands, identities, boundaries, losses,
            );
        });
    }
}

pub(crate) fn apply(
    definition: &mut FeatureDefinition,
    feature: &FeatureId,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identities: &[DesignEdgeIdentityOperand],
    boundaries: &[AsmFaceBoundary],
    losses: &mut Vec<LossNote>,
) {
    let operation = match definition {
        FeatureDefinition::Operation(operation)
        | FeatureDefinition::PostProcess { operation, .. } => operation,
    };
    let selections: Vec<&mut EdgeSelection> = match operation {
        FeatureOperation::Fillet { groups } => groups.iter_mut().map(|g| &mut g.edges).collect(),
        FeatureOperation::Chamfer { groups, .. } => {
            groups.iter_mut().map(|g| &mut g.edges).collect()
        }
        _ => return,
    };
    if !selections.iter().any(|selection| {
        matches!(
            selection,
            EdgeSelection::Native(_) | EdgeSelection::HistoricalPartial { .. }
        )
    }) {
        return;
    }
    let boundaries = boundaries.iter().map(|b| (b.id.as_str(), b)).collect();
    for selection in selections {
        let native = match selection {
            EdgeSelection::Native(native) => native.as_str(),
            EdgeSelection::HistoricalPartial { native, .. } => native.as_str(),
            _ => continue,
        };
        let mut matches = groups.iter().filter(|g| g.id == native);
        let Some(group) = matches.next().filter(|_| matches.next().is_none()) else {
            continue;
        };
        apply_group(
            selection,
            group,
            operands,
            identities,
            &boundaries,
            feature,
            losses,
        );
    }
}

pub(super) fn apply_group(
    selection: &mut EdgeSelection,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignEdgeOperand],
    identities: &[DesignEdgeIdentityOperand],
    boundaries: &HashMap<&str, &AsmFaceBoundary>,
    feature: &FeatureId,
    losses: &mut Vec<LossNote>,
) {
    if !matches!(
        selection,
        EdgeSelection::Native(_) | EdgeSelection::HistoricalPartial { .. }
    ) || !group.lost_edge_references.is_empty()
    {
        return;
    }
    let Some((state_id, members)) = group_members(group, operands) else {
        return;
    };
    let state = crate::ids::feature_input_topology_id(feature, state_id);
    let (mut edges, mut unresolved) = match selection {
        EdgeSelection::Native(_) => (
            Vec::new(),
            members.iter().map(|o| o.id.clone()).collect::<Vec<_>>(),
        ),
        EdgeSelection::HistoricalPartial {
            state: original,
            edges,
            unresolved,
            ..
        } if *original == state => (
            edges.iter().cloned().collect(),
            unresolved.iter().map(|id| id.as_str().to_owned()).collect(),
        ),
        _ => return,
    };
    let mut notes = Vec::new();
    let mut selected = false;
    for operand in members {
        if !unresolved.contains(&operand.id) || resolved_edge_operand(operand).is_some() {
            continue;
        }
        let Some(edge) = crate::history::fragment_edge::context_edge(operand, boundaries) else {
            continue;
        };
        if (edge.inferred || !operand.transition_deleted_edge_slots.contains(&edge.slot))
            && identities.iter().any(|identity| {
                native_stream(&identity.id) == native_stream(&group.id)
                    && identity.scope_record_index == group.scope_record_index
                    && identity.group_record_index == group.record_index
                    && identity.record_index() == operand.record_index()
                    && (identity.resolved_edge_slot.is_some()
                        || !identity.resolved_edge_slots.is_empty())
            })
        {
            continue;
        }
        let id = crate::ids::history_input_edge_id(
            &crate::ids::feature_input_prefix(feature, state_id),
            edge.slot,
        );
        if !edges.contains(&id) {
            edges.push(id);
        }
        unresolved.retain(|id| id != &operand.id);
        if edge.inferred {
            notes.push(crate::history::inferred_edge_selection_note(
                &operand.id,
                state_id,
                edge.slot,
            ));
        }
        selected = true;
    }
    if !selected {
        return;
    }
    let result = if unresolved.is_empty() {
        EdgeSelection::historical(state, edges, group.id.clone())
    } else {
        EdgeSelection::historical_partial(state, edges, unresolved, group.id.clone())
    };
    if let Ok(result) = result {
        *selection = result;
        losses.extend(notes);
    }
}

fn group_members<'a>(
    group: &DesignConstructionOperandGroup,
    operands: &'a [DesignEdgeOperand],
) -> Option<(i64, Vec<&'a DesignEdgeOperand>)> {
    let mut members = Vec::new();
    for member in group.members() {
        let mut matches = operands.iter().filter(|o| {
            native_stream(&o.id) == native_stream(&group.id)
                && o.scope_record_index == group.scope_record_index
                && o.record_index() == member.value
        });
        let operand = matches.next().filter(|_| matches.next().is_none())?;
        if operand.recipe_structure.is_none()
            || members
                .iter()
                .any(|o: &&DesignEdgeOperand| o.id == operand.id)
        {
            return None;
        }
        members.push(operand);
    }
    let state_id = members.first().and_then(|o| o.recipe_state_id)?;
    if members.iter().any(|o| o.recipe_state_id != Some(state_id)) {
        return None;
    }
    Some((state_id, members))
}
