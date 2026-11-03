// SPDX-License-Identifier: Apache-2.0
//! Base-commit golden access (#209 "Base and main", #219).
//!
//! CI decision: base resolves exactly as #209 states, `git merge-base HEAD
//! origin/main`, after fetching `origin/main`. The contract and test jobs
//! fetch it explicitly (`.github/workflows/ci.yml`). A missing `origin/main`
//! fails loudly here; a missing base is never a skip.
//!
//! Goldens are read from git objects (`git show <base>:<path>`). No copies of
//! base goldens are vendored into the tree.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::data_checks::DecodeGolden;

/// Workspace root: this crate lives at `crates/<name>`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("cadmpeg-test-support lives at crates/<name>")
        .to_owned()
}

/// Resolve the #209 base commit: `git merge-base HEAD origin/main`.
///
/// `CADIR_BASE` overrides the revision for a local run against another
/// history point; CI leaves it unset and resolves base per #209.
///
/// # Panics
///
/// Panics when the revision cannot be resolved. A missing `origin/main`
/// names the fetch that restores it instead of skipping the check.
#[must_use]
pub fn resolve_base() -> String {
    let root = workspace_root();
    if let Ok(candidate) = std::env::var("CADIR_BASE") {
        let candidate = candidate.trim().to_owned();
        assert!(
            !candidate.is_empty(),
            "CADIR_BASE is blank; unset it to resolve base per #209",
        );
        assert!(
            is_commit(&root, &candidate),
            "CADIR_BASE {candidate} does not resolve to a commit under {}",
            root.display(),
        );
        return candidate;
    }
    let output = std::process::Command::new("git")
        .args(["merge-base", "HEAD", "origin/main"])
        .current_dir(&root)
        .output()
        .expect("run git merge-base HEAD origin/main");
    assert!(
        output.status.success(),
        "cannot resolve base: `git merge-base HEAD origin/main` failed under {}: {}; \
         #209 defines base as that merge base, so fetch the upstream branch \
         (`git fetch origin main`) and rerun; a missing base is a hard failure, never a skip",
        root.display(),
        String::from_utf8_lossy(&output.stderr).trim(),
    );
    let base = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    assert!(
        !base.is_empty(),
        "cannot resolve base: `git merge-base HEAD origin/main` printed nothing; \
         fetch the upstream branch (`git fetch origin main`) and rerun",
    );
    base
}

fn is_commit(root: &Path, revision: &str) -> bool {
    std::process::Command::new("git")
        .args(["cat-file", "-e", &format!("{revision}^{{commit}}")])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The text of `path` (workspace-relative, forward slashes) as committed at
/// `base`, or `None` when the path did not exist at base.
///
/// # Panics
///
/// Panics when git fails for any reason other than the path's absence at
/// base, so a corrupt base revision cannot pass as an absent golden.
#[must_use]
pub fn base_text_at(root: &Path, base: &str, path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["show", &format!("{base}:{path}")])
        .current_dir(root)
        .output()
        .expect("run git show <base>:<path>");
    if output.status.success() {
        return Some(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let diagnostic = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        diagnostic.contains("does not exist") || diagnostic.contains("not in"),
        "cannot read base golden {path} at {base}: {diagnostic}",
    );
    None
}

/// Parse one golden text into a [`DecodeGolden`], mirroring
/// [`super::data_checks::decode_goldens`]. Returns `None` when the golden
/// pins a decode error, so it holds no base document to compare.
///
/// # Panics
///
/// Panics when the text is not JSON, or holds neither a document nor a
/// `decode_error`.
#[must_use]
pub fn decode_golden_from_text(relative: &str, text: &str) -> Option<DecodeGolden> {
    let value: Value = serde_json::from_str(text)
        .unwrap_or_else(|error| panic!("{relative}: base golden is not JSON: {error}"));
    let decode = value.get("decode").unwrap_or(&value);
    let Some(ir) = decode.get("ir") else {
        assert!(
            decode.get("decode_error").is_some(),
            "{relative}: base golden holds neither `ir` nor `decode_error`",
        );
        return None;
    };
    Some(DecodeGolden {
        path: relative.to_owned(),
        ir: ir.clone(),
        report: decode.get("report").cloned().unwrap_or(Value::Null),
        fidelity: decode
            .get("source_fidelity")
            .cloned()
            .unwrap_or(Value::Null),
    })
}

/// The base form of one head golden, or `None` when the golden did not exist
/// at base or pins a base decode error. Either absence means there is no base
/// document to compare, so the caller reports nothing for that golden.
#[must_use]
pub fn base_decode_golden(base: &str, head: &DecodeGolden) -> Option<DecodeGolden> {
    base_text_at(&workspace_root(), base, &head.path)
        .and_then(|text| decode_golden_from_text(&head.path, &text))
}

/// Every head golden that exists at both base and head, in head path order,
/// with the resolved base revision.
///
/// A golden absent at base, or pinning a base decode error, contributes no
/// pair. The caller fails loudly when no pair remains (see #219: a check that
/// fails on a codec whose Phase 3 unit has not landed stays failing and is
/// reported with the codec, not skipped).
#[must_use]
pub fn goldens_with_base(crate_name: &str) -> (String, Vec<(DecodeGolden, DecodeGolden)>) {
    let base = resolve_base();
    let pairs = super::data_checks::decode_goldens(crate_name)
        .into_iter()
        .filter_map(|head| base_decode_golden(&base, &head).map(|at_base| (head, at_base)))
        .collect();
    (base, pairs)
}

#[cfg(test)]
mod tests;
