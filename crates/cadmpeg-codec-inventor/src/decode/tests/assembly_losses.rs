// SPDX-License-Identifier: Apache-2.0
use crate::decode::nested_visibility_loss;
use crate::loss::InventorLossCode;

#[test]
fn hidden_occurrences_below_the_root_report_the_nested_visibility_loss() {
    let note = nested_visibility_loss(2).expect("two nested hides are reported");
    assert_eq!(
        note.code,
        InventorLossCode::AssemblyVisibilityNestedNative.kind()
    );
    assert_eq!(note.code.local_code(), "assembly.visibility-nested-native");
    assert!(note.message.contains('2'));
}

#[test]
fn no_nested_hide_reports_no_loss() {
    assert!(nested_visibility_loss(0).is_none());
}
