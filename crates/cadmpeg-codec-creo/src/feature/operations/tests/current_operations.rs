// SPDX-License-Identifier: Apache-2.0

const DISPLAY: &[u8] = b"\xe3Extrude id 7\0";
const CONFLICTING_BINDINGS: &[u8] =
    b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
    \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0";

#[test]
fn display_decodes_one_current_operation() {
    assert_eq!(super::super::operations(DISPLAY).len(), 1);
}

#[test]
fn competing_recipe_bindings_keep_both_states() {
    assert_eq!(
        super::super::operation_states(CONFLICTING_BINDINGS).len(),
        2
    );
}

#[test]
fn agreeing_candidates_project_without_a_current_candidate() {
    let states = super::super::operations(b"\xe3Extrude id 7\0\xe3Extrude id 7\0");
    assert_eq!(states.len(), 1);
    assert!(states[0].display_state_conflict);
    assert!(!states[0].display_name_stored());
}

#[test]
fn disagreeing_candidates_withhold_operation() {
    assert!(super::super::operations(b"\xe3Extrude id 7\0\xe3Hole id 7\0").is_empty());
    assert!(super::super::operations(CONFLICTING_BINDINGS).is_empty());
}
