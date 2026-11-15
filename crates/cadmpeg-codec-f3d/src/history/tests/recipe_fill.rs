// SPDX-License-Identifier: Apache-2.0
use std::collections::HashSet;

use crate::history::{recipe_face_replaces_selection, selection_has_unchanged_face};

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

#[test]
fn a_selection_with_a_face_the_transition_left_unchanged_is_unchanged() {
    let changed = HashSet::from([260, 261]);
    assert!(!selection_has_unchanged_face(&[260], &changed));
    assert!(selection_has_unchanged_face(&[102], &changed));
    assert!(selection_has_unchanged_face(&[260, 102], &changed));
    assert!(!selection_has_unchanged_face(&[], &changed));
}
