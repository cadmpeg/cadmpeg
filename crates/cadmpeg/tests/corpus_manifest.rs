// SPDX-License-Identifier: Apache-2.0
//! Pin each corpus fixture's dialect and fail when classification drifts.
//!
//! `corpus/manifest.toml` records donor-supplied facts about each fixture. One
//! field is not donor-supplied: `dialect` is derived, by running this codec's
//! own `inspect()` over the fixture's bytes and reading the primary layer's
//! registry id out of the resulting `ContainerSummary`. Pinning it turns
//! classification into a golden — a codec change that reclassifies a corpus
//! file fails here instead of passing silently.
//!
//! `authoring_app_version` sits beside it and means something different: what
//! the producing application called itself. Producer and dialect are separate
//! axes and neither is derivable from the other, so the manifest carries both.
//!
//! The pin is only about the bytes it was derived from, so this test verifies
//! each fixture's `sha256` before classifying it. A fixture whose bytes moved
//! fails on the digest, not on a dialect derived from something else.
//!
//! `UPDATE_CORPUS_DIALECTS=1 cargo test -p cadmpeg --test corpus_manifest`
//! rewrites the pins in place; review the diff as a classification change.

#![allow(clippy::unwrap_used)] // Test code: a failed unwrap is a test failure.

use std::fmt::Write as _;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_core::decode::InspectOptions;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Environment variable that rewrites the pins instead of comparing them.
const UPDATE_VAR: &str = "UPDATE_CORPUS_DIALECTS";

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default, rename = "file")]
    files: Vec<Entry>,
}

/// The manifest fields this test reads. Donor prose fields are ignored.
#[derive(Debug, Deserialize)]
struct Entry {
    filename: String,
    format: String,
    sha256: String,
    /// The derived pin. Absent until the first `UPDATE_CORPUS_DIALECTS=1` run.
    #[serde(default)]
    dialect: Option<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn manifest_path() -> PathBuf {
    repo_root().join("corpus/manifest.toml")
}

/// Classifies one fixture: the primary layer's registry id, as `inspect()` reads it.
///
/// The manifest's format vocabulary is the input catalog's ids, and
/// `cadmpeg-registry` owns that catalog, so the lookup goes through
/// `InputCatalog::by_id`. A key the catalog does not know is a manifest that
/// names a format this build does not ship, which is a failure and not a skip.
///
/// `None` means the codec identified no registry row for the primary layer —
/// a real classification outcome, pinned as `"none"` so the manifest states it
/// rather than omitting the field.
fn classify(format: &str, bytes: &[u8]) -> String {
    let catalog = cadmpeg_registry::InputCatalog::with_builtins();
    let codec = catalog
        .by_id(format)
        .unwrap_or_else(|| panic!("manifest format {format:?} has no codec in this build"));
    let summary = codec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .unwrap_or_else(|error| panic!("inspecting a {format} fixture: {error}"));
    summary
        .dialects
        .as_ref()
        .unwrap_or_else(|| panic!("{format} inspect reported no primary layer"))
        .primary()
        .dialect
        .as_ref()
        .map_or_else(|| "none".to_owned(), |id| id.as_str().to_owned())
}

/// Rewrites the `dialect` pin inside each `[[file]]` block, preserving comments.
///
/// The manifest is a hand-edited, comment-carrying document, so this edits the
/// lines a pin occupies rather than reserializing the parsed value. The pin sits
/// immediately after `format`, which every entry declares.
fn rewrite(text: &str, pins: &[String]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut entry = 0;
    for line in text.lines() {
        if line.starts_with("dialect = ") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
        if line.starts_with("format = ") {
            writeln!(out, "dialect = {:?}", pins[entry]).unwrap();
            entry += 1;
        }
    }
    assert_eq!(entry, pins.len(), "one pin written per [[file]] entry");
    out
}

#[test]
fn corpus_manifest_dialects_are_pinned_from_classification() {
    let path = manifest_path();
    let text = std::fs::read_to_string(&path).unwrap();
    let manifest: Manifest = toml::from_str(&text).unwrap();
    assert!(
        !manifest.files.is_empty(),
        "corpus/manifest.toml declares no fixtures"
    );

    let mut pins = Vec::new();
    let mut mismatches = Vec::new();
    for entry in &manifest.files {
        let bytes = std::fs::read(repo_root().join("corpus").join(&entry.filename))
            .unwrap_or_else(|error| panic!("reading {}: {error}", entry.filename));
        let mut digest = String::new();
        for byte in Sha256::digest(&bytes) {
            write!(digest, "{byte:02x}").unwrap();
        }
        assert_eq!(
            digest, entry.sha256,
            "{} does not match its manifest sha256; the pin below describes different bytes",
            entry.filename
        );
        let observed = classify(&entry.format, &bytes);
        if entry.dialect.as_deref() != Some(observed.as_str()) {
            mismatches.push(format!(
                "  {}: pinned {:?}, classified {observed:?}",
                entry.filename,
                entry.dialect.as_deref().unwrap_or("<absent>")
            ));
        }
        pins.push(observed);
    }

    if std::env::var_os(UPDATE_VAR).is_some() {
        std::fs::write(&path, rewrite(&text, &pins)).unwrap();
        return;
    }
    assert!(
        mismatches.is_empty(),
        "corpus dialect pins drifted from classification:\n{}\nrerun with {UPDATE_VAR}=1 and \
         review the diff",
        mismatches.join("\n")
    );
}

/// Every pinned id is a row in the identity registry.
///
/// A pin that names no registry row is a dead string: nothing can join it back
/// to a dialect's discriminants, witness, or support claims.
#[test]
fn corpus_manifest_dialects_name_registry_rows() {
    let manifest: Manifest =
        toml::from_str(&std::fs::read_to_string(manifest_path()).unwrap()).unwrap();
    let registry = std::fs::read_to_string(repo_root().join("docs/dialects.toml")).unwrap();
    let declared = registry_ids(&registry);
    for entry in &manifest.files {
        let Some(dialect) = entry.dialect.as_deref() else {
            panic!(
                "{} has no dialect pin; rerun with {UPDATE_VAR}=1",
                entry.filename
            );
        };
        if dialect == "none" {
            continue;
        }
        assert!(
            declared.contains(dialect),
            "{} pins {dialect:?}, which is not a row in docs/dialects.toml",
            entry.filename
        );
    }
}

/// Every `id` declared by a `[[dialect]]` row in the identity registry.
fn registry_ids(registry: &str) -> std::collections::BTreeSet<String> {
    #[derive(Deserialize)]
    struct Registry {
        #[serde(default, rename = "dialect")]
        dialects: Vec<Row>,
    }
    #[derive(Deserialize)]
    struct Row {
        id: String,
    }
    let parsed: Registry = toml::from_str(registry).unwrap();
    parsed.dialects.into_iter().map(|row| row.id).collect()
}
