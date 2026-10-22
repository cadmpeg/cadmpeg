// SPDX-License-Identifier: Apache-2.0
//! Damaged inputs keep their relative names across discovery and every branch.

use super::super::{stems, Branch, Harness};
use super::{EnvGuard, TempTree};

fn snapshot(bytes: &[u8]) -> String {
    super::super::snapshot_text(&serde_json::json!({"bytes": bytes}))
}

#[test]
fn discovers_regular_and_damaged_inputs_without_entering_other_fixture_directories() {
    let tree = TempTree::named("damaged-discovery");
    let fixtures = tree.0.join("tests/golden/fixtures");
    std::fs::create_dir_all(fixtures.join("damaged/group")).expect("damaged fixture tree");
    std::fs::create_dir_all(fixtures.join("unrelated")).expect("unrelated tree");
    for (name, bytes) in [
        ("same.bin", b"regular".as_slice()),
        ("damaged/same.bin", b"damaged".as_slice()),
        ("damaged/group/cut.bin", b"cut".as_slice()),
        ("unrelated/unused.bin", b"unused".as_slice()),
    ] {
        std::fs::write(fixtures.join(name), bytes).expect("temporary fixture");
    }
    let inputs = Harness::new(tree.manifest_dir(), "bin", "regenerate").fixture_inputs();
    assert_eq!(
        inputs,
        vec![
            ("damaged/group/cut".into(), b"cut".to_vec()),
            ("damaged/same".into(), b"damaged".to_vec()),
            ("same".into(), b"regular".to_vec()),
        ]
    );
}

#[test]
fn regeneration_keeps_frozen_damaged_input_bytes_and_separates_root_and_named_branches() {
    let tree = TempTree::named("damaged-file-regeneration");
    let fixtures = tree.0.join("tests/golden/fixtures");
    std::fs::create_dir_all(fixtures.join("damaged")).expect("damaged tree");
    std::fs::write(fixtures.join("same.bin"), [1, 2]).expect("regular fixture");
    std::fs::write(fixtures.join("damaged/same.bin"), [3, 4]).expect("damaged fixture");
    let _update = EnvGuard::set("UPDATE_GOLDEN", Some("1"));
    let harness = Harness::new(tree.manifest_dir(), "bin", "regenerate");
    harness.check(&[Branch::named("decode", snapshot), Branch::root(snapshot)]);
    for branch in ["decode/", ""] {
        assert_eq!(
            std::fs::read_to_string(tree.0.join(format!("tests/golden/{branch}same.json")))
                .expect("regular golden"),
            snapshot(&[1, 2])
        );
        assert_eq!(
            std::fs::read_to_string(
                tree.0
                    .join(format!("tests/golden/{branch}damaged/same.json"))
            )
            .expect("damaged golden"),
            snapshot(&[3, 4])
        );
    }
    assert_eq!(
        std::fs::read(fixtures.join("same.bin")).expect("regular bytes"),
        [1, 2]
    );
    assert_eq!(
        std::fs::read(fixtures.join("damaged/same.bin")).expect("damaged bytes"),
        [3, 4]
    );
}

#[test]
fn code_built_damaged_paths_are_checked_for_orphans_in_named_and_root_branches() {
    let tree = TempTree::named("damaged-built-orphans");
    let inputs = vec![("damaged/group/cut".into(), vec![7])];
    let names = vec!["damaged/group/cut".into()];
    let harness = Harness::new(tree.manifest_dir(), "bin", "regenerate");
    let branches = [Branch::named("decode", snapshot), Branch::root(snapshot)];
    {
        let _update = EnvGuard::set("UPDATE_GOLDEN", Some("1"));
        harness.check_inputs(&inputs, &branches);
    }
    for (branch, prefix) in branches.iter().zip(["decode/", ""]) {
        let orphan = tree
            .0
            .join(format!("tests/golden/{prefix}damaged/orphan.json"));
        std::fs::write(&orphan, snapshot(&[8])).expect("orphan golden");
        let failures = harness.compare_branch(branch, &inputs, &names, false, false);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(
            failures[0].contains("damaged/orphan.json"),
            "{}",
            failures[0]
        );
        assert!(
            failures[0].contains("has no input `damaged/orphan`"),
            "{}",
            failures[0]
        );
        let file_failures = harness.compare_branch(branch, &inputs, &names, false, true);
        assert_eq!(file_failures.len(), 1);
        assert!(
            file_failures[0].contains("has no `damaged/orphan.bin` fixture"),
            "{}",
            file_failures[0]
        );
    }
}

#[test]
fn damaged_only_fixture_directory_is_not_vacuous_and_optional_absence_keeps_regular_inputs() {
    let tree = TempTree::named("damaged-only-discovery");
    let fixtures = tree.0.join("tests/golden/fixtures");
    std::fs::create_dir_all(&fixtures).expect("fixture root");
    std::fs::write(fixtures.join("regular.bin"), [1]).expect("regular fixture");
    assert_eq!(stems(&fixtures, "bin"), ["regular"]);
    std::fs::remove_file(fixtures.join("regular.bin")).expect("remove temporary regular fixture");
    std::fs::create_dir_all(fixtures.join("damaged")).expect("damaged tree");
    std::fs::write(fixtures.join("damaged/cut.bin"), [2]).expect("damaged fixture");
    assert_eq!(
        Harness::new(tree.manifest_dir(), "bin", "regenerate").fixture_inputs(),
        [("damaged/cut".into(), vec![2])]
    );
}

#[test]
#[should_panic(expected = "golden input name must be a relative stem")]
fn code_built_names_cannot_escape_the_golden_directory() {
    let tree = TempTree::named("damaged-invalid-name");
    Harness::new(tree.manifest_dir(), "bin", "regenerate").check_inputs(
        &[("damaged/../../fixtures/source".into(), vec![1])],
        &[Branch::named("decode", snapshot)],
    );
}
