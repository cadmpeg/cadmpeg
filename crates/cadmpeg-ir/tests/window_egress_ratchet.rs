// SPDX-License-Identifier: Apache-2.0
//! Window-egress ratchet.
//!
//! `View::window()` is an audited raw-byte egress. This test counts
//! `.window()` call sites in the `src` and `fuzz_targets` trees of every
//! workspace crate and fails if a crate exceeds the frozen ceiling in
//! `window-egress-ratchet.toml`. The count is a monotonically decreasing
//! ratchet: draining a site means lowering its crate ceiling in the same
//! commit; adding one is a violation unless the module's
//! `parser-manifest.toml` `window_egress` list names the new boundary.
//!
//! Enrollment is driven by the filesystem, not the reference file: the test
//! discovers every crate directory under `crates/` and fails any crate that
//! has no ceiling entry, so a newly added crate cannot escape the ratchet by
//! omission. Detection remains textual (`.window()` substring), so a call
//! spelled to dodge the match (`. window()`, an alias, a wrapper method) still
//! bypasses the ceiling.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Absolute path to the workspace root (`crates/cadmpeg-ir` -> `../..`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root is two levels above the crate manifest dir")
        .to_path_buf()
}

/// Parses the `[crates]` table of the reference file into name -> ceiling.
///
/// The file is a flat `name = integer` list under one header, so a line parser
/// avoids pulling a TOML dependency into the test build.
fn read_reference(path: &Path) -> BTreeMap<String, usize> {
    let text = fs::read_to_string(path).expect("reference file is readable");
    let mut ceilings = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let (name, value) = line
            .split_once('=')
            .expect("reference row is `name = count`");
        let count = value
            .trim()
            .parse::<usize>()
            .expect("reference count is an integer");
        ceilings.insert(name.trim().to_string(), count);
    }
    ceilings
}

/// Counts non-overlapping `.window()` occurrences across every `.rs` file in
/// the given trees. The `View::window` definition reads `fn window(`, not
/// `.window()`, so it is not counted; only call sites are.
fn count_window_sites(trees: &[PathBuf]) -> usize {
    let mut total = 0;
    let mut stack: Vec<PathBuf> = trees.iter().filter(|p| p.is_dir()).cloned().collect();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = fs::read_to_string(&path).expect("source file is readable");
                total += text.matches(".window()").count();
            }
        }
    }
    total
}

/// Lists every crate directory under `crates/` (a directory holding a
/// `Cargo.toml`), so enrollment tracks the filesystem, not the reference file.
fn workspace_crates(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root.join("crates"))
        .expect("crates dir is readable")
        .flatten()
    {
        let path = entry.path();
        if path.join("Cargo.toml").is_file() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    names
}

#[test]
fn window_egress_within_frozen_ceilings() {
    let root = workspace_root();
    let reference = read_reference(&root.join("crates/cadmpeg-ir/window-egress-ratchet.toml"));

    let mut violations = Vec::new();
    for crate_name in workspace_crates(&root) {
        let Some(ceiling) = reference.get(&crate_name) else {
            violations.push(format!(
                "{crate_name}: crate has no ceiling in window-egress-ratchet.toml; \
                 enroll it with a frozen `.window()` count"
            ));
            continue;
        };
        let crate_dir = root.join("crates").join(&crate_name);
        let actual = count_window_sites(&[crate_dir.join("src"), crate_dir.join("fuzz_targets")]);
        if actual > *ceiling {
            violations.push(format!(
                "{crate_name}: {actual} `.window()` sites exceed frozen ceiling {ceiling}; \
                 audit the new boundary in parser-manifest.toml window_egress"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "window-egress ratchet violated:\n{}",
        violations.join("\n")
    );
}
