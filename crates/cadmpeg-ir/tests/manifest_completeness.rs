// SPDX-License-Identifier: Apache-2.0
//! Doc section 8.6 / 10 Phase 2 manifest-completeness ratchet.
//!
//! The per-crate `parser-manifest.toml` makes the half-migrated state
//! auditable only if it lists every decoder module. A module that reads
//! untrusted bytes but carries no `[[module]]` entry escapes the migration
//! gate entirely — the failure that let `appearance.rs` ship as a live leaf
//! decoder with no manifest row. This test enrolls every `cadmpeg-codec-*`
//! crate from the filesystem and fails when a decoder `src` module is absent
//! from that crate's manifest.
//!
//! Scope is the codec crates. In a codec crate every `src` module is a
//! decoder except the non-decoder classes named below, so "listed" is a
//! filesystem-derivable invariant. `cadmpeg-ir` is excluded: it is the
//! platform crate whose manifest lists only the primitive readers that touch
//! untrusted bytes, not its model/validation modules, and no
//! decoder/non-decoder classifier over its tree exists to drive completeness
//! there.
//!
//! Non-decoder exclusions (matched by basename, documented in each manifest
//! header): the crate root `lib.rs`; the encoder-only `writer*.rs` modules,
//! which implement `Encoder` and carry no decode read path; and the test and
//! fuzz glue (`tests.rs`, `*_tests.rs`, `*_test_support.rs`, `fuzzing.rs`).
//! A module may still be listed despite matching an exclusion (some crates
//! enroll `lib.rs`); the exclusion only lifts the *requirement* to list it.
//! Detection is textual on filenames, so a decoder hidden behind an excluded
//! naming pattern would slip; that residual is held by review discipline.

use std::collections::BTreeSet;
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

/// Lists every `cadmpeg-codec-*` crate directory under `crates/` that holds a
/// `parser-manifest.toml`, so enrollment tracks the filesystem.
fn codec_crates(root: &Path) -> Vec<(String, PathBuf)> {
    let mut crates = Vec::new();
    for entry in fs::read_dir(root.join("crates"))
        .expect("crates dir is readable")
        .flatten()
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("cadmpeg-codec-") && path.join("parser-manifest.toml").is_file() {
            crates.push((name, path));
        }
    }
    crates.sort();
    crates
}

/// Parses the set of `src`-relative module paths from a manifest's
/// `path = "crates/<crate>/src/<module>"` lines into their basenames.
fn manifest_modules(manifest: &Path) -> BTreeSet<String> {
    let text = fs::read_to_string(manifest).expect("manifest is readable");
    let mut modules = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("path") else {
            continue;
        };
        let value = rest
            .trim_start()
            .strip_prefix('=')
            .expect("manifest path row is `path = \"...\"`")
            .trim()
            .trim_matches('"');
        let basename = value
            .rsplit_once("/src/")
            .map(|(_, tail)| tail)
            .unwrap_or(value);
        modules.insert(basename.to_string());
    }
    modules
}

/// True for `src` modules that are not decoders and so need no manifest entry.
fn is_non_decoder(rel: &str) -> bool {
    let base = rel.rsplit('/').next().unwrap_or(rel);
    base == "lib.rs"
        || base == "tests.rs"
        || base == "fuzzing.rs"
        || base.ends_with("_tests.rs")
        || base.ends_with("_test_support.rs")
        || base.starts_with("writer")
}

/// Every `.rs` file under `dir`, as paths relative to `dir` using `/`.
fn src_modules(dir: &Path) -> Vec<String> {
    let mut modules = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let rel = path
                    .strip_prefix(dir)
                    .expect("source path is under src dir")
                    .to_string_lossy()
                    .replace('\\', "/");
                modules.push(rel);
            }
        }
    }
    modules.sort();
    modules
}

/// Parses every `fuzz_targets = [...]` array in a manifest into the flat set of
/// target names it names. Arrays are single-line in these manifests, so a line
/// parser suffices without a TOML dependency.
fn manifest_fuzz_targets(manifest: &Path) -> BTreeSet<String> {
    let text = fs::read_to_string(manifest).expect("manifest is readable");
    let mut targets = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("fuzz_targets") else {
            continue;
        };
        let value = rest
            .trim_start()
            .strip_prefix('=')
            .expect("manifest fuzz_targets row is `fuzz_targets = [...]`")
            .trim();
        let inner = value
            .strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .expect("fuzz_targets value is a single-line array");
        for name in inner.split(',') {
            let name = name.trim().trim_matches('"');
            if !name.is_empty() {
                targets.insert(name.to_string());
            }
        }
    }
    targets
}

/// The set of fuzz targets registered as `[[bin]]` entries in the
/// `cadmpeg-fuzz` manifest whose path is under `fuzz_targets/` — the seed
/// generators (`src/bin/*`) are excluded, so this is exactly the runnable
/// fuzz-target set `cargo fuzz run` accepts.
fn registered_fuzz_targets(root: &Path) -> BTreeSet<String> {
    let text = fs::read_to_string(root.join("crates/cadmpeg-fuzz/Cargo.toml"))
        .expect("fuzz manifest is readable");
    let mut registered = BTreeSet::new();
    let mut pending_name: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line == "[[bin]]" || line.starts_with('[') && line != "[[bin]]" {
            pending_name = None;
        }
        if let Some(rest) = line.strip_prefix("name") {
            if let Some(eq) = rest.trim_start().strip_prefix('=') {
                pending_name = Some(eq.trim().trim_matches('"').to_string());
            }
        } else if let Some(rest) = line.strip_prefix("path") {
            if let Some(eq) = rest.trim_start().strip_prefix('=') {
                let path = eq.trim().trim_matches('"');
                if path.starts_with("fuzz_targets/") {
                    if let Some(name) = pending_name.take() {
                        registered.insert(name);
                    }
                }
            }
        }
    }
    registered
}

/// Doc section 7 reachability / Phase 2 exit gate item 4: every manifest
/// `fuzz_targets` entry must resolve to a registered fuzz target. Commit
/// `c203b937` exists because sldprt manifest entries once named unregistered
/// targets; without this guard a renamed or dropped `[[bin]]` turns every
/// citing manifest entry into a dangling reachability claim that CI still
/// passes.
#[test]
fn every_manifest_fuzz_target_is_registered() {
    let root = workspace_root();
    let registered = registered_fuzz_targets(&root);
    assert!(
        !registered.is_empty(),
        "no registered fuzz targets parsed from crates/cadmpeg-fuzz/Cargo.toml"
    );

    let mut violations = Vec::new();
    for (crate_name, crate_dir) in codec_crates(&root) {
        for target in manifest_fuzz_targets(&crate_dir.join("parser-manifest.toml")) {
            if !registered.contains(&target) {
                violations.push(format!(
                    "{crate_name}: parser-manifest.toml names fuzz target `{target}` with no \
                     matching `[[bin]]` in crates/cadmpeg-fuzz/Cargo.toml; register the target \
                     or fix the entry"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "manifest fuzz-target reachability violated:\n{}",
        violations.join("\n")
    );
}

#[test]
fn every_decoder_module_is_manifested() {
    let root = workspace_root();

    let mut violations = Vec::new();
    for (crate_name, crate_dir) in codec_crates(&root) {
        let listed = manifest_modules(&crate_dir.join("parser-manifest.toml"));
        for rel in src_modules(&crate_dir.join("src")) {
            if is_non_decoder(&rel) {
                continue;
            }
            if !listed.contains(&rel) {
                violations.push(format!(
                    "{crate_name}: src/{rel} is a decoder module with no `[[module]]` \
                     entry in parser-manifest.toml; add it as migrated or legacy"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "manifest completeness violated:\n{}",
        violations.join("\n")
    );
}
