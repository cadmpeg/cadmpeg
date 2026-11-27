// SPDX-License-Identifier: Apache-2.0
//! Seed-body selections of body patterns.

use super::body_selections::{
    bind_body_recipe_body_selection, bind_direct_body_recipe_body_selection,
    FeatureBodySelectionInputs,
};
use super::history_states::effective_scope_previous_history_state_id;
use crate::history::body_candidates::BodyCandidates;
use crate::records::topology::extrude_selection::DesignOperandRole;

pub(super) fn bind_pattern_body_selections<'a>(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'a>,
    body_candidates: &mut BodyCandidates<'a>,
) {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let scopes = inputs.scopes;
    let groups = inputs.groups;
    let body_recipe_operands = inputs.body_recipe_operands;

    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let stream = crate::ids::native_stream(&scope.id);
        let mut matching_groups = groups.iter().filter(|group| {
            group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::BODIES_B
                && !group.members().is_empty()
                && crate::ids::native_stream(&group.id) == stream
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some() {
            continue;
        }
        let mut seed = if seeds.is_empty() {
            Some(PatternSeed::Bodies(BodySelection::Native(group.id.clone())))
        } else {
            None
        };
        let feature_id = &feature.id;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = definition
            else {
                return;
            };
            if let Some(seed) = seed.take() {
                seeds.push(seed);
            }
            let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() else {
                return;
            };
            if let Some(previous_state_id) = scope.previous_history_state_id() {
                bind_body_recipe_body_selection(
                    selection,
                    feature_id,
                    previous_state_id,
                    scope,
                    groups,
                    body_recipe_operands,
                );
                super::tail_bodies::bind_tail_history_bodies(
                    selection,
                    feature_id,
                    previous_state_id,
                    scope,
                    inputs,
                );
            } else {
                let native = selection.clone();
                bind_direct_body_recipe_body_selection(selection, scope, inputs, body_candidates);
                // The recipe identity of the operands outranks a face match.
                super::generated_bodies::keep_recipe_body_slots(selection, native, scope, inputs);
                // A scope that stores no preceding state takes it from its
                // history state, where the operands bound their body slots.
                if let Some(previous_state_id) =
                    effective_scope_previous_history_state_id(scope, inputs.histories)
                {
                    bind_body_recipe_body_selection(
                        selection,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                    super::tail_bodies::bind_tail_history_bodies(
                        selection,
                        feature_id,
                        previous_state_id,
                        scope,
                        inputs,
                    );
                }
            }
        });
    }
}
