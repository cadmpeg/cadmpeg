// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.

mod body_bounds;
mod body_centers;
mod body_incidence;
mod body_recipe;
mod body_recipe_selections;
mod body_selection;
pub(super) mod budgets;
mod combine_producer_bodies;
mod component_spaces;
mod delete_face;
mod draft;
mod edge_loops;
mod edge_operands;
mod face_boundaries;
mod fallback_faces;
mod feature_inputs;
mod feature_outputs;
mod header_index;
mod mirror;
mod move_face;
mod path_selections;
mod planes;
mod recipe_projection;
mod recipes;
mod reverse_normal_bodies;
mod selections;
mod shell;
mod state_pairs;
mod state_visits;
mod stored_chain;
mod surface_stitch;
mod terminal_operands;

mod recipe_transitions;

mod projection_reuse;

mod topology_projection;

pub(super) fn bind_feature_body_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &super::body_selections::FeatureBodySelectionInputs<'_>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let (mut losses, refusals) =
        super::body_selections::bind_feature_body_selections_with_refusals(features, inputs);
    losses.extend(super::body_selections::native_combine_operand_refusal_notes(features, refusals));
    losses
}
