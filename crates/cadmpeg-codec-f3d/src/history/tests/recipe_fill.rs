// SPDX-License-Identifier: Apache-2.0
use crate::history::recipe_face_replaces_selection;

#[test]
fn a_recipe_without_a_closed_edge_node_replaces_any_selection() {
    assert!(recipe_face_replaces_selection(false, false, false));
    assert!(recipe_face_replaces_selection(false, false, true));
}

#[test]
fn a_closed_edge_recipe_face_fills_an_empty_selection_only() {
    assert!(recipe_face_replaces_selection(true, true, false));
    assert!(!recipe_face_replaces_selection(true, false, false));
}

#[test]
fn a_closed_edge_recipe_face_the_transition_left_unchanged_does_not_start_or_profile_an_extrude() {
    assert!(!recipe_face_replaces_selection(true, true, true));
}
