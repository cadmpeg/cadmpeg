// SPDX-License-Identifier: Apache-2.0
//! Append file-backed damaged cases without changing the fixed legacy builders.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cadmpeg_test_support::golden::artifact_names;

fn inputs_at(root: &Path) -> Vec<(String, Vec<u8>)> {
    let fixtures = root.join("fixtures");
    if !fixtures.join("damaged").is_dir() {
        return Vec::new();
    }
    artifact_names(&fixtures)
        .into_iter()
        .filter_map(|name| {
            let stem = name.strip_suffix(".f3d")?.to_owned();
            name.starts_with("damaged/").then(|| {
                let bytes = std::fs::read(fixtures.join(&name))
                    .unwrap_or_else(|error| panic!("read fixture {name}: {error}"));
                (stem, bytes)
            })
        })
        .collect()
}

pub(super) fn check_snapshots(update: bool, root: &Path, failures: &mut Vec<String>) {
    let mut produced: BTreeMap<&str, BTreeSet<String>> =
        ["inspect", "decode", "replay", "generate", "patch"]
            .into_iter()
            .map(|branch| (branch, BTreeSet::new()))
            .collect();
    for (name, input) in inputs_at(root) {
        for (branch, snapshot) in [
            ("inspect", super::inspect_snapshot(&input)),
            ("decode", super::decode_snapshot(&input)),
        ] {
            produced
                .get_mut(branch)
                .expect("snapshot branch")
                .insert(format!("{name}.json"));
            super::compare_text(
                update,
                &root.join(branch).join(format!("{name}.json")),
                &snapshot,
                failures,
            );
        }
        for (branch, comparison, outcome) in [
            (
                "replay",
                super::ByteComparison::Exact,
                super::replay_outcome(&input),
            ),
            (
                "generate",
                super::ByteComparison::Decoded,
                super::generate_outcome(&input),
            ),
            (
                "patch",
                super::ByteComparison::Exact,
                super::patch_outcome(&input),
            ),
        ] {
            if let Some(outcome) = &outcome {
                let extension = if outcome.is_ok() { "bin" } else { "err.txt" };
                produced
                    .get_mut(branch)
                    .expect("encode branch")
                    .insert(format!("{name}.{extension}"));
            }
            super::compare_encode(update, root, branch, &name, comparison, outcome, failures);
        }
    }
    for (branch, names) in produced {
        let dir = root.join(branch);
        if !dir.join("damaged").is_dir() {
            continue;
        }
        for orphan in artifact_names(&dir)
            .into_iter()
            .filter(|name| name.starts_with("damaged/") && !names.contains(name))
        {
            failures.push(format!("golden `{orphan}` under {} is produced by no damaged fixture; delete it or restore its input", dir.display()));
        }
    }
}

#[test]
fn damaged_outputs_are_deterministic() {
    for (name, bytes) in inputs_at(&super::golden_root()) {
        assert_eq!(
            super::decode_snapshot(&bytes),
            super::decode_snapshot(&bytes),
            "fixture `{name}`: nondeterministic decode"
        );
        assert_eq!(
            super::inspect_snapshot(&bytes),
            super::inspect_snapshot(&bytes),
            "fixture `{name}`: nondeterministic inspect"
        );
        assert_eq!(
            super::replay_outcome(&bytes),
            super::replay_outcome(&bytes),
            "fixture `{name}`: nondeterministic replay"
        );
        assert_eq!(
            super::generate_outcome(&bytes),
            super::generate_outcome(&bytes),
            "fixture `{name}`: nondeterministic generate"
        );
        assert_eq!(
            super::patch_outcome(&bytes),
            super::patch_outcome(&bytes),
            "fixture `{name}`: nondeterministic patch"
        );
    }
}

#[test]
fn damaged_fixture_discovery_keeps_nested_names_without_touching_the_legacy_list() {
    let root = tempfile::tempdir().expect("temporary damaged fixture tree");
    let fixtures = root.path().join("fixtures");
    std::fs::create_dir_all(fixtures.join("damaged/group")).expect("damaged group");
    std::fs::write(fixtures.join("legacy.f3d"), b"legacy").expect("legacy input");
    std::fs::write(fixtures.join("damaged/group/cut.f3d"), b"frozen cut bytes")
        .expect("damaged input");
    assert_eq!(
        inputs_at(root.path()),
        [("damaged/group/cut".into(), b"frozen cut bytes".to_vec())]
    );
    assert_eq!(
        std::fs::read(fixtures.join("legacy.f3d")).expect("legacy bytes"),
        b"legacy"
    );
}
