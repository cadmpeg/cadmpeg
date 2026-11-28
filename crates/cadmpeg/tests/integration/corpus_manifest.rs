// SPDX-License-Identifier: Apache-2.0
//! Check every corpus specimen: pin its dialect, and require it to pass
//! `cadmpeg check`.
//!
//! `corpus/manifest.toml` records hand-written facts about each fixture from
//! its donor or curator. One field is derived: `dialect`, by running this codec's
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
//! `UPDATE_CORPUS_DIALECTS=1 cargo test -p cadmpeg --test integration corpus_manifest`
//! rewrites the pins in place; review the diff as a classification change.
//!

use std::fmt::Write as _;
use std::io::Cursor;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

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

fn compiled_formats() -> std::collections::BTreeSet<String> {
    cadmpeg_registry::InputCatalog::with_builtins()
        .descriptors()
        .filter(|descriptor| descriptor.codec().is_some())
        .map(|descriptor| descriptor.format_id().as_str().to_owned())
        .collect()
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

/// Classifies one fixture: the primary layer's registry id, as the codec's
/// classification path reads it.
///
/// The manifest's format vocabulary is the input catalog's ids, and
/// `cadmpeg-registry` owns that catalog, so the lookup selects its input
/// descriptor. A key the catalog does not know is a manifest that
/// names a format this build does not ship, which is a failure and not a skip.
///
/// `None` means the codec identified no registry row for the primary layer —
/// a real classification outcome, pinned as `"none"` so the manifest states it
/// as the classified primary row.
fn classify(format: &str, bytes: &[u8]) -> String {
    let catalog = cadmpeg_registry::InputCatalog::with_builtins();
    let codec = catalog
        .descriptors()
        .find(|descriptor| descriptor.format_id().as_str() == format)
        .and_then(cadmpeg_registry::InputDescriptor::codec)
        .unwrap_or_else(|| panic!("manifest format {format:?} has no codec in this build"));
    let dialects = codec
        .classify_dialects(&mut Cursor::new(bytes), &InspectOptions::default())
        .unwrap_or_else(|error| panic!("classifying a {format} fixture: {error}"));
    dialects
        .unwrap_or_else(|| panic!("{format} classification reported no primary layer"))
        .primary()
        .dialect()
        .as_str()
        .to_owned()
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

    let compiled = compiled_formats();
    let mut pins = Vec::new();
    let mut mismatches = Vec::new();
    for entry in &manifest.files {
        assert!(
            cadmpeg_registry::Format::is_known_name(&entry.format).expect("registry loads"),
            "unknown manifest format {:?}",
            entry.format
        );
        if !compiled.contains(&entry.format) {
            pins.push(entry.dialect.clone().expect("missing dialect pin"));
            continue;
        }
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
    let registry =
        std::fs::read_to_string(repo_root().join("crates/cadmpeg-registry/docs/dialects.toml"))
            .unwrap();
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
            "{} pins {dialect:?}, which is not a row in crates/cadmpeg-registry/docs/dialects.toml",
            entry.filename
        );
    }
}

/// Every specimen passes `cadmpeg check`: it decodes, its neutral model and
/// native namespaces validate, and its measurement completes.
#[test]
fn corpus_specimens_pass_check() {
    let manifest: Manifest =
        toml::from_str(&std::fs::read_to_string(manifest_path()).unwrap()).unwrap();
    let compiled = compiled_formats();
    let files: Vec<_> = manifest
        .files
        .iter()
        .filter(|entry| compiled.contains(&entry.format))
        .collect();
    let formats_with_specimens = [
        "catia", "creo", "f3d", "fcstd", "inventor", "nx", "sldprt", "step",
    ];
    assert!(
        !files.is_empty()
            || !formats_with_specimens
                .iter()
                .any(|format| compiled.contains(*format)),
        "compiled formats with corpus specimens must run at least one"
    );
    let corpus = repo_root().join("corpus");
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(files.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(entry) = files.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let output = Command::new(env!("CARGO_BIN_EXE_cadmpeg"))
                        .arg("check")
                        .arg("--input-format")
                        .arg(&entry.format)
                        .arg(corpus.join(&entry.filename))
                        .output()
                        .unwrap();
                    if output.status.success() {
                        continue;
                    }
                    let report = [output.stdout, output.stderr].concat();
                    let report = String::from_utf8_lossy(&report);
                    let errors: Vec<&str> = report
                        .lines()
                        .filter(|line| {
                            line.starts_with("check:")
                                || line.starts_with("error:")
                                || line.contains("[error/")
                        })
                        .collect();
                    failures.lock().unwrap().push(format!(
                        "{} ({}):\n{}",
                        entry.filename,
                        output.status,
                        errors.join("\n")
                    ));
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(
        failures.is_empty(),
        "{} specimen(s) failed `cadmpeg check`:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
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
