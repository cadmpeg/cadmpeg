// SPDX-License-Identifier: Apache-2.0
//! `Cursor`-reference ratchet.
//!
//! CI requires each crate's qualified domain-`cursor` reference count to
//! decrease monotonically.
//!
//! Detection counts the qualified module path `cursor::` (as in
//! `cadmpeg_ir::cursor::Cursor` or `crate::cursor::bounded_len`) in the `src`
//! and `fuzz_targets` trees of every workspace crate. This is deliberately
//! narrower than the bare `Cursor` substring and excludes `std::io::Cursor`.
//!
//! The ceiling is enforced by **exact equality**, not `<=`: a crate whose count
//! drops must lower its ceiling in the same commit, so the toml can never carry
//! stale slack that hides a later regression, and a count that rises fails
//! outright. Enrollment is filesystem-driven: the test discovers every crate
//! directory under `crates/` and fails any crate with no ceiling entry, so a
//! new crate cannot escape the ratchet by omission. A reference spelled to
//! dodge the `cursor::` path (a bare `Cursor` after a domain import) would
//! bypass the count.

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

/// Counts qualified domain-`cursor::` path occurrences across every `.rs` file
/// in the given trees. The lowercase module segment excludes `std::io::Cursor`
/// (whose segment is `io`) and prose mentions of `Cursor`.
fn count_cursor_refs(trees: &[PathBuf]) -> usize {
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
                total += text.matches("cursor::").count();
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
fn cursor_references_within_frozen_ceilings() {
    let root = workspace_root();
    let reference = read_reference(&root.join("crates/cadmpeg-ir/cursor-reference-ratchet.toml"));

    let mut violations = Vec::new();
    for crate_name in workspace_crates(&root) {
        let Some(ceiling) = reference.get(&crate_name) else {
            violations.push(format!(
                "{crate_name}: crate has no ceiling in cursor-reference-ratchet.toml; \
                 enroll it with a frozen `Cursor` count"
            ));
            continue;
        };
        let crate_dir = root.join("crates").join(&crate_name);
        let actual = count_cursor_refs(&[crate_dir.join("src"), crate_dir.join("fuzz_targets")]);
        if actual > *ceiling {
            violations.push(format!(
                "{crate_name}: {actual} domain `cursor::` references exceed frozen ceiling \
                 {ceiling}; a reader was reintroduced — remove it, this ratchet only decreases"
            ));
        } else if actual < *ceiling {
            violations.push(format!(
                "{crate_name}: {actual} domain `cursor::` references are below ceiling {ceiling}; \
                 a reader was migrated — lower the ceiling to {actual} in the same commit so the \
                 ratchet cannot carry stale slack"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "cursor-reference ratchet violated:\n{}",
        violations.join("\n")
    );
}
