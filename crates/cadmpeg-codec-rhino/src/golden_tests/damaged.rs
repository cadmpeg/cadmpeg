// SPDX-License-Identifier: Apache-2.0
//! Bespoke binary archive branches retain damaged relative paths and orphans.

#[test]
fn nested_binary_golden_parent_is_created_and_orphan_names_stay_complete() {
    let root = tempfile::tempdir().expect("temporary binary golden tree");
    let path = root.path().join("damaged/group/cut.v5.bin");
    super::write_golden(&path, b"unchanged binary bytes");
    assert_eq!(
        std::fs::read(&path).expect("binary artifact"),
        b"unchanged binary bytes"
    );
    let failures = super::check_encode_branch_at(root.path(), &[], false);
    assert_eq!(failures.len(), 1);
    assert!(
        failures[0].contains("damaged/group/cut.v5.bin"),
        "{}",
        failures[0]
    );
    assert!(
        failures[0].contains("produced by no fixture and target"),
        "{}",
        failures[0]
    );
}
