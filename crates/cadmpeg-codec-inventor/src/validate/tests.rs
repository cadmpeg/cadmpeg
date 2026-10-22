// SPDX-License-Identifier: Apache-2.0
//! Kept native states resolve to pairs without a fabricated availability record.

use std::collections::HashSet;

#[test]
fn an_independent_pair_needs_no_success_or_retired_issue_carrier() {
    let pairs = HashSet::from(["seg"]);
    let mut findings = Vec::new();
    super::validate_segment_states(&mut findings, &pairs, [], [], "metadata");
    assert!(findings.is_empty());
}

#[test]
fn every_kept_metadata_state_still_needs_its_actual_pair() {
    let pairs = HashSet::from(["seg"]);
    let mut findings = Vec::new();
    super::validate_segment_states(&mut findings, &pairs, ["orphan"], [], "metadata");
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("has no paired segment"));
}

#[test]
fn duplicate_kept_metadata_states_are_still_rejected() {
    let pairs = HashSet::from(["seg"]);
    let mut findings = Vec::new();
    super::validate_segment_states(&mut findings, &pairs, ["seg", "seg"], [], "metadata");
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("repeats"));
}
