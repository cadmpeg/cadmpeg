// SPDX-License-Identifier: Apache-2.0
//! Bind the target and tool bodies of a Design `Combine` scope.

use std::collections::BTreeSet;

use cadmpeg_ir::features::selections::{BodyMember, BodySelection};
use cadmpeg_ir::features::{CombineOperands, FeatureId};
use cadmpeg_ir::report::loss::LossNote;

use super::{
    bind_direct_body_recipe_body_selection, bind_operand_list_combine_bodies, body_member,
    combine_external_local_tools, combine_recipe_family_tool_slots,
    effective_scope_previous_history_state_id, historical_brep_source, pattern_combine_tool_slots,
    singleton_revised_input_body_across_state_chain, unique_feature_history_states,
    unique_history_state_pair, BodyCandidates, CombineOperandRefusal, CombineOperandSide,
    FeatureBodySelectionInputs,
};
use crate::records::feature::scope::DesignParameterScope;

/// Bind `target` and `tools` in the history state before the scope.
///
/// A scope with no current or preceding history state binds both operands
/// through its direct body recipes. When the preceding state comes from the
/// history header, an operand that the historical binding leaves native also
/// binds through its direct body recipe. An operand-list operand that its
/// browser node leaves native binds through the body recipes of the scope.
pub(super) fn bind_combine_operands<'a>(
    (target, tools): (&mut BodySelection, &mut BodySelection),
    scope: &DesignParameterScope,
    feature_id: &FeatureId,
    pattern_bodies: Option<&BTreeSet<i64>>,
    inputs: &FeatureBodySelectionInputs<'a>,
    body_candidates: &mut BodyCandidates<'a>,
    refusals: &mut Vec<CombineOperandRefusal>,
    losses: &mut Vec<LossNote>,
) {
    let (Some(state_id), Some(previous_state_id)) = (
        scope.history_state_id(),
        effective_scope_previous_history_state_id(scope, inputs.histories),
    ) else {
        bind_direct_body_recipe_body_selection(target, scope, inputs, body_candidates);
        bind_direct_body_recipe_body_selection(tools, scope, inputs, body_candidates);
        if matches!(
            tools,
            BodySelection::Native(_) | BodySelection::NativeSet(_)
        ) {
            if let Some(local) = combine_external_local_tools(scope) {
                *tools = local;
            }
        }
        return;
    };
    bind_historical_combine_operands(
        (&mut *target, &mut *tools),
        scope,
        (feature_id, state_id, previous_state_id),
        pattern_bodies,
        inputs,
        body_candidates,
        refusals,
        losses,
    );
    if scope.previous_history_state_id().is_none() {
        for operand in [target, tools] {
            if matches!(
                operand,
                BodySelection::Native(_) | BodySelection::NativeSet(_)
            ) {
                bind_direct_body_recipe_body_selection(operand, scope, inputs, body_candidates);
            }
        }
    }
}

fn bind_historical_combine_operands<'a>(
    (target, tools): (&mut BodySelection, &mut BodySelection),
    scope: &DesignParameterScope,
    (feature_id, state_id, previous_state_id): (&FeatureId, i64, i64),
    pattern_bodies: Option<&BTreeSet<i64>>,
    inputs: &FeatureBodySelectionInputs<'a>,
    body_candidates: &mut BodyCandidates<'a>,
    operand_refusals: &mut Vec<CombineOperandRefusal>,
    losses: &mut Vec<LossNote>,
) {
    if let Some(local) = combine_external_local_tools(scope) {
        *tools = local;
    }
    let Some(operation) = scope.combine_operation().filter(|operation| {
        operation.form == crate::records::feature::combine::DesignCombineForm::OperandLists
    }) else {
        bind_recipe_combine_operands(
            (target, tools),
            scope,
            (feature_id, state_id, previous_state_id),
            pattern_bodies,
            (inputs, body_candidates),
            false,
        );
        return;
    };
    let (mut recipe_target, mut recipe_tools) = (target.clone(), tools.clone());
    let mut refusals = Vec::new();
    let shared_body = bind_operand_list_combine_bodies(
        (&mut *target, &mut *tools),
        scope,
        operation,
        (feature_id, previous_state_id),
        (inputs.histories, inputs.scopes),
        &mut refusals,
    ) == Some(true);
    if shared_body {
        operand_refusals.extend(refusals);
        return;
    }
    bind_recipe_combine_operands(
        (&mut recipe_target, &mut recipe_tools),
        scope,
        (feature_id, state_id, previous_state_id),
        pattern_bodies,
        (inputs, body_candidates),
        true,
    );
    let (merged_target, merged_tools) = merge_operand_list_bindings(
        (target.clone(), tools.clone()),
        (recipe_target, recipe_tools),
    );
    operand_refusals.extend(unbound_refusals((&merged_target, &merged_tools), refusals));
    losses.extend(tool_replacement_loss(feature_id, tools, &merged_tools));
    *target = merged_target;
    *tools = merged_tools;
}

fn native_operand(selection: &BodySelection) -> bool {
    matches!(
        selection,
        BodySelection::Native(_) | BodySelection::NativeSet(_)
    )
}

fn names_model_bodies(selection: &BodySelection) -> bool {
    matches!(
        selection,
        BodySelection::Bodies(_)
            | BodySelection::Resolved { .. }
            | BodySelection::ResolvedSet { .. }
    )
}

/// Merge the browser-node slot binding of an operand-list `Combine` with the
/// binding that its body recipes give.
///
/// The target keeps its slot binding and takes the recipe binding when its
/// slot leaves it native. The tools take the recipe binding when it names
/// model bodies or when their slots leave them native. A pair whose target
/// and tools overlap is refused: the tools then keep their slot binding, and
/// when that pair also overlaps, both operands keep their slot bindings.
fn merge_operand_list_bindings(
    (slot_target, slot_tools): (BodySelection, BodySelection),
    (recipe_target, recipe_tools): (BodySelection, BodySelection),
) -> (BodySelection, BodySelection) {
    let target = if native_operand(&slot_target) {
        recipe_target
    } else {
        slot_target.clone()
    };
    let tools = if names_model_bodies(&recipe_tools) || native_operand(&slot_tools) {
        recipe_tools
    } else {
        slot_tools.clone()
    };
    [(target.clone(), tools), (target, slot_tools.clone())]
        .into_iter()
        .find(|(target, tools)| CombineOperands::new(target.clone(), tools.clone()).is_ok())
        .unwrap_or((slot_target, slot_tools))
}

/// The loss for merged tools that replace slot tools that bind.
fn tool_replacement_loss(
    feature_id: &FeatureId,
    slot_tools: &BodySelection,
    merged_tools: &BodySelection,
) -> Option<LossNote> {
    (!native_operand(slot_tools) && merged_tools != slot_tools).then(|| {
        crate::loss::F3dLossCode::FeatureCombineToolRecipeReplacementAssumed.note(format!(
            "Combine {feature_id}: the body recipe tools {:?} replace the slot tools {:?}",
            selection_bodies(merged_tools),
            selection_bodies(slot_tools),
        ))
    })
}

/// The body identities of `selection`, or its native expression.
fn selection_bodies(selection: &BodySelection) -> Vec<&str> {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => bodies
            .as_slice()
            .iter()
            .map(cadmpeg_ir::ids::BodyId::as_str)
            .collect(),
        BodySelection::ResolvedSet { members } => members
            .bodies()
            .map(cadmpeg_ir::ids::BodyId::as_str)
            .collect(),
        BodySelection::Historical { bodies, .. } => bodies
            .as_slice()
            .iter()
            .map(cadmpeg_ir::ids::HistoricalBodyId::as_str)
            .collect(),
        BodySelection::HistoricalSet { members, .. } => members
            .bodies()
            .map(cadmpeg_ir::ids::HistoricalBodyId::as_str)
            .collect(),
        BodySelection::Native(native) => vec![native.as_str()],
        _ => Vec::new(),
    }
}

/// The slot refusals of the operands that stay native.
fn unbound_refusals(
    (target, tools): (&BodySelection, &BodySelection),
    refusals: Vec<CombineOperandRefusal>,
) -> Vec<CombineOperandRefusal> {
    refusals
        .into_iter()
        .filter(|refusal| {
            native_operand(match refusal.side {
                CombineOperandSide::Target => target,
                CombineOperandSide::Tools => tools,
            })
        })
        .collect()
}

/// Bind the operands that the history state of the scope and its body
/// recipes determine. With `operand_lists`, only native operands change.
fn bind_recipe_combine_operands<'a>(
    (target, tools): (&mut BodySelection, &mut BodySelection),
    scope: &DesignParameterScope,
    (feature_id, state_id, previous_state_id): (&FeatureId, i64, i64),
    pattern_bodies: Option<&BTreeSet<i64>>,
    (inputs, body_candidates): (&FeatureBodySelectionInputs<'a>, &mut BodyCandidates<'a>),
    operand_lists: bool,
) {
    let histories = inputs.histories;
    let body_recipe_operands = inputs.body_recipe_operands;
    let target_native = match &*target {
        BodySelection::Native(native) => Some(native.clone()),
        _ => None,
    };
    if target_native.is_none() && !(operand_lists && native_operand(tools)) {
        return;
    }
    let Some((history, state, _)) =
        unique_history_state_pair(histories, state_id, previous_state_id)
    else {
        return;
    };
    let history_states = unique_feature_history_states(history);
    let Some(body) = singleton_revised_input_body_across_state_chain(
        history,
        state,
        previous_state_id,
        &history_states,
    ) else {
        return;
    };
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let input_state = crate::ids::history_input_state_id(&prefix);
    let body_id = crate::ids::history_input_body_id(&prefix, body);
    if let Some(native_id) = target_native {
        if let Ok(historical) = cadmpeg_ir::features::selections::BodySelection::historical(
            crate::ids::history_input_state_id(&prefix),
            vec![body_id],
            native_id,
        ) {
            *target = historical;
        }
    }
    if operand_lists && !native_operand(tools) {
        return;
    }
    let Some(stream) = crate::ids::native_stream(&scope.id) else {
        return;
    };
    let Some(operation) = scope.combine_operation() else {
        return;
    };
    let mut native_tools = Vec::new();
    for tool in operation.tools.iter() {
        let native = format!("{stream}:design-record#{}", tool.record_index);
        native_tools.push(native);
    }
    let current_history_source = historical_brep_source(&state.id);
    let mut historical_tool_rows = Vec::new();
    let mut direct_tool_rows = Vec::new();
    for (record_index, native) in operation
        .tools
        .iter()
        .map(|tool| tool.record_index)
        .zip(&native_tools)
    {
        let mut matching = body_recipe_operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && matches!(
                    operand.owner,
                    crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                )
                && operand.record_index() == record_index
        });
        let Some(operand) = matching.next() else {
            historical_tool_rows.clear();
            direct_tool_rows.clear();
            break;
        };
        if matching.next().is_some() {
            historical_tool_rows.clear();
            direct_tool_rows.clear();
            break;
        }
        if let Some(body) = operand.resolved_body_slot {
            let body = crate::ids::history_input_body_id(&prefix, body);
            let repeated = historical_tool_rows
                .iter()
                .any(|row: &BodyMember<_>| row.body() == &body);
            let row = body_member(body, native.to_owned());
            let (false, Some(row)) = (repeated, row) else {
                historical_tool_rows.clear();
                direct_tool_rows.clear();
                break;
            };
            historical_tool_rows.push(row);
            continue;
        }
        let Some(body) = body_candidates.external(operand, current_history_source) else {
            historical_tool_rows.clear();
            direct_tool_rows.clear();
            break;
        };
        let repeated = direct_tool_rows
            .iter()
            .any(|row: &BodyMember<_>| row.body() == &body);
        let row = body_member(body, native.to_owned());
        let (false, Some(row)) = (repeated, row) else {
            historical_tool_rows.clear();
            direct_tool_rows.clear();
            break;
        };
        direct_tool_rows.push(row);
    }
    if historical_tool_rows.len() == native_tools.len() {
        let Ok(members) =
            cadmpeg_ir::features::selections::BodyMembers::try_from_rows(historical_tool_rows)
        else {
            return;
        };
        *tools = BodySelection::HistoricalSet {
            state: input_state,
            members,
        };
    } else if direct_tool_rows.len() == native_tools.len() {
        *tools = if direct_tool_rows.len() == 1 {
            let Some(row) = direct_tool_rows.pop() else {
                return;
            };
            let (body, native) = row.into_parts();
            let selected = vec![body];
            let Ok(bodies) = cadmpeg_ir::features::selections::DistinctMembers::try_from(selected)
            else {
                return;
            };
            BodySelection::Resolved { bodies, native }
        } else {
            let Ok(members) =
                cadmpeg_ir::features::selections::BodyMembers::try_from_rows(direct_tool_rows)
            else {
                return;
            };
            BodySelection::ResolvedSet { members }
        };
    } else {
        let tool_record_indices = operation
            .tools
            .iter()
            .map(|tool| tool.record_index)
            .collect::<Vec<_>>();
        if let Some(tool_slots) = combine_recipe_family_tool_slots(
            (stream, scope.record_index),
            &tool_record_indices,
            previous_state_id,
            body,
            body_recipe_operands,
            inputs.construction_recipes,
        ) {
            if tool_slots.len() != native_tools.len() {
                return;
            }
            let Some(rows) = combine_historical_rows(&prefix, tool_slots, native_tools) else {
                return;
            };
            let Ok(members) = cadmpeg_ir::features::selections::BodyMembers::try_from_rows(rows)
            else {
                return;
            };
            *tools = BodySelection::HistoricalSet {
                state: input_state,
                members,
            };
            return;
        }
        if let Some(tool_slots) = pattern_bodies
            .and_then(|bodies| pattern_combine_tool_slots(bodies, body, native_tools.len()))
        {
            if tool_slots.len() != native_tools.len() {
                return;
            }
            let Some(rows) = combine_historical_rows(&prefix, tool_slots, native_tools) else {
                return;
            };
            let Ok(members) = cadmpeg_ir::features::selections::BodyMembers::try_from_rows(rows)
            else {
                return;
            };
            *tools = BodySelection::HistoricalSet {
                state: input_state,
                members,
            };
        }
    }
}

fn combine_historical_rows(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slots: Vec<i64>,
    native_tools: Vec<String>,
) -> Option<Vec<cadmpeg_ir::features::selections::BodyMember<cadmpeg_ir::ids::HistoricalBodyId>>> {
    let mut rows = Vec::new();
    for (slot, native) in slots.into_iter().zip(native_tools) {
        let body = crate::ids::history_input_body_id(prefix, slot);
        let row = body_member(body, native)?;

        rows.push(row);
    }
    Some(rows)
}

#[cfg(test)]
mod tests;
