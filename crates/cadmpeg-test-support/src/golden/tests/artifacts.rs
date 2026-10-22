// SPDX-License-Identifier: Apache-2.0
//! Binary/text branch orphan checks retain exact relative artifact identities.

use super::super::artifact_names;
use super::TempTree;

#[test]
fn artifact_names_descend_damaged_and_keep_unexpected_root_entries_visible() {
    let tree = TempTree::named("damaged-artifact-names");
    let dir = tree.0.join("tests/golden/decode");
    std::fs::create_dir_all(dir.join("damaged/group")).expect("damaged group");
    std::fs::create_dir_all(dir.join("unexpected")).expect("unexpected branch entry");
    for name in [
        "same.bin",
        "same.report.json",
        "damaged/same.bin",
        "damaged/group/cut.err.txt",
    ] {
        std::fs::write(dir.join(name), b"artifact").expect("temporary artifact");
    }
    assert_eq!(
        artifact_names(&dir),
        [
            "damaged/group/cut.err.txt",
            "damaged/same.bin",
            "same.bin",
            "same.report.json",
            "unexpected"
        ]
    );
}

#[test]
fn custom_regular_fixture_root_also_reads_the_canonical_damaged_tree() {
    let tree = TempTree::named("damaged-custom-root");
    let regular = tree.0.join("external-fixtures");
    let damaged = tree.0.join("tests/golden/fixtures/damaged");
    std::fs::create_dir_all(&regular).expect("custom root");
    std::fs::create_dir_all(&damaged).expect("canonical damaged root");
    std::fs::write(regular.join("regular.bin"), b"regular bytes").expect("regular input");
    std::fs::write(damaged.join("cut.bin"), b"damaged bytes").expect("damaged input");
    let inputs = super::super::Harness::new(tree.manifest_dir(), "bin", "regenerate")
        .with_fixture_dir(regular)
        .fixture_inputs();
    assert_eq!(
        inputs,
        [
            ("damaged/cut".into(), b"damaged bytes".to_vec()),
            ("regular".into(), b"regular bytes".to_vec())
        ]
    );
}

#[test]
#[should_panic(expected = "duplicate damaged fixture stem across fixture roots")]
fn duplicate_damaged_stems_across_custom_and_canonical_roots_are_not_discarded() {
    let tree = TempTree::named("damaged-duplicate-custom-root");
    let regular = tree.0.join("external-fixtures");
    let damaged = tree.0.join("tests/golden/fixtures/damaged");
    std::fs::create_dir_all(regular.join("damaged")).expect("custom damaged root");
    std::fs::create_dir_all(&damaged).expect("canonical damaged root");
    std::fs::write(regular.join("damaged/same.bin"), b"one input").expect("custom input");
    std::fs::write(damaged.join("same.bin"), b"another input").expect("canonical input");
    let _inputs = super::super::Harness::new(tree.manifest_dir(), "bin", "regenerate")
        .with_fixture_dir(regular)
        .fixture_inputs();
}
