// SPDX-License-Identifier: Apache-2.0
//! Inspect and decode goldens for the NIST MBE PMI producer-evidence subset.
//!
//! Inputs are the `LicenseRef-NIST-PD` files in `corpus/manifest.toml`. The
//! snapshots live under `tests/nist_pmi_golden/{inspect,decode}/` so they stay
//! out of each codec crate's default golden tree and out of `cargo test-fast`.
//!
//! The snapshot comparison is `#[ignore]`. PR CI only checks that the golden
//! names match the manifest. The scheduled workflow
//! `.github/workflows/nist-pmi-goldens.yml` runs the ignored tests.
//!
//! Regenerate with:
//! `UPDATE_GOLDEN=1 cargo test -p cadmpeg --test nist_pmi_goldens -- --ignored`

#![allow(clippy::unwrap_used)] // Test code: a failed unwrap is a test failure.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_registry::InputCatalog;
use cadmpeg_test_support::golden::{elide_local_digests, snapshot_text, snapshots_agree};
use serde::Deserialize;

/// Environment variable that rewrites drifted or missing goldens.
const UPDATE_VAR: &str = "UPDATE_GOLDEN";

/// Regeneration hint quoted in every snapshot failure.
const REGENERATE: &str =
    "UPDATE_GOLDEN=1 cargo test -p cadmpeg --test nist_pmi_goldens -- --ignored";

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default, rename = "file")]
    files: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    filename: String,
    format: String,
    license: String,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/nist_pmi_golden")
}

fn nist_entries() -> Vec<Entry> {
    let text = std::fs::read_to_string(repo_root().join("corpus/manifest.toml")).unwrap();
    let manifest: Manifest = toml::from_str(&text).unwrap();
    let entries: Vec<Entry> = manifest
        .files
        .into_iter()
        .filter(|entry| entry.license == "LicenseRef-NIST-PD")
        .collect();
    assert!(
        !entries.is_empty(),
        "corpus/manifest.toml declares no LicenseRef-NIST-PD files"
    );
    let mut stems = BTreeSet::new();
    for entry in &entries {
        assert!(
            stems.insert(golden_stem(&entry.filename)),
            "NIST golden stem collision for {}",
            entry.filename
        );
    }
    entries
}

fn golden_stem(filename: &str) -> String {
    Path::new(filename)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn golden_path(kind: &str, stem: &str) -> PathBuf {
    golden_root().join(kind).join(format!("{stem}.json"))
}

fn json_stems(kind: &str) -> BTreeSet<String> {
    let dir = golden_root().join(kind);
    if !dir.is_dir() {
        return BTreeSet::new();
    }
    std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("read entry: {error}"))
                .path()
        })
        .filter_map(|path| {
            if path.extension()? != "json" {
                return None;
            }
            Some(path.file_stem()?.to_string_lossy().into_owned())
        })
        .collect()
}

fn codec_for<'a>(catalog: &'a InputCatalog, format: &str) -> &'a dyn Codec {
    catalog
        .descriptors()
        .find(|descriptor| descriptor.format_id().as_str() == format)
        .and_then(cadmpeg_registry::InputDescriptor::codec)
        .unwrap_or_else(|| panic!("manifest format {format:?} has no codec in this build"))
}

fn inspect_snapshot(codec: &dyn Codec, bytes: &[u8]) -> String {
    let value = match codec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()) {
        Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

fn decode_snapshot(codec: &dyn Codec, bytes: &[u8]) -> String {
    let value = match codec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()) {
        Ok(result) => {
            let mut result = cadmpeg_test_support::EditableDecodeResult::from(result);
            if let Some(source) = result.ir_mut().source.as_mut() {
                elide_local_digests(&mut source.attributes);
            }
            serde_json::json!({
                "ir": serde_json::to_value(result.ir()).expect("serialize ir"),
                "report": serde_json::to_value(result.report()).expect("serialize report"),
                "source_fidelity": serde_json::to_value(result.source_fidelity())
                    .expect("serialize source_fidelity"),
            })
        }
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
    };
    snapshot_text(&value)
}

fn compare_golden(path: &Path, actual: &str, update: bool) -> Option<String> {
    let mismatch = match std::fs::read_to_string(path) {
        Ok(expected) => snapshots_agree(&expected.replace("\r\n", "\n"), actual).err(),
        Err(error) => Some(format!("cannot read {} ({error})", path.display())),
    };
    let mismatch = mismatch?;
    if update {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        }
        std::fs::write(path, actual.as_bytes())
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        return None;
    }
    Some(format!(
        "{} diverged\n    {mismatch}\n    regenerate with `{REGENERATE}`",
        path.display()
    ))
}

/// The ignored snapshot suite has one inspect and one decode golden per NIST CAD file.
#[test]
fn nist_pmi_golden_names_match_manifest() {
    let entries = nist_entries();
    let expected: BTreeSet<String> = entries
        .iter()
        .map(|entry| golden_stem(&entry.filename))
        .collect();
    for kind in ["inspect", "decode"] {
        let found = json_stems(kind);
        let missing: Vec<_> = expected.difference(&found).cloned().collect();
        let extra: Vec<_> = found.difference(&expected).cloned().collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "tests/nist_pmi_golden/{kind} does not match LicenseRef-NIST-PD files:\n  missing: {missing:?}\n  extra: {extra:?}\n  {REGENERATE}"
        );
    }
}

/// Every NIST producer-evidence CAD file's inspect and decode snapshots hold.
#[test]
#[ignore = "producer-evidence decode goldens; run via .github/workflows/nist-pmi-goldens.yml"]
fn nist_pmi_goldens_hold() {
    let entries = nist_entries();
    let catalog = InputCatalog::with_builtins();
    let update = std::env::var_os(UPDATE_VAR).is_some();
    let mut failures = Vec::new();
    for entry in &entries {
        let bytes = std::fs::read(repo_root().join("corpus").join(&entry.filename))
            .unwrap_or_else(|error| panic!("reading {}: {error}", entry.filename));
        let stem = golden_stem(&entry.filename);
        let codec = codec_for(&catalog, &entry.format);
        let inspect = inspect_snapshot(codec, &bytes);
        let decode = decode_snapshot(codec, &bytes);
        failures.extend(compare_golden(
            &golden_path("inspect", &stem),
            &inspect,
            update,
        ));
        failures.extend(compare_golden(
            &golden_path("decode", &stem),
            &decode,
            update,
        ));
    }
    assert!(
        failures.is_empty(),
        "{} NIST PMI golden(s) drifted:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Two runs over the same NIST bytes produce identical inspect and decode text.
#[test]
#[ignore = "producer-evidence decode goldens; run via .github/workflows/nist-pmi-goldens.yml"]
fn nist_pmi_goldens_are_deterministic() {
    let catalog = InputCatalog::with_builtins();
    for entry in nist_entries() {
        let bytes = std::fs::read(repo_root().join("corpus").join(&entry.filename))
            .unwrap_or_else(|error| panic!("reading {}: {error}", entry.filename));
        let codec = codec_for(&catalog, &entry.format);
        let inspect_a = inspect_snapshot(codec, &bytes);
        let inspect_b = inspect_snapshot(codec, &bytes);
        assert_eq!(
            inspect_a, inspect_b,
            "{} inspect is nondeterministic",
            entry.filename
        );
        let decode_a = decode_snapshot(codec, &bytes);
        let decode_b = decode_snapshot(codec, &bytes);
        assert_eq!(
            decode_a, decode_b,
            "{} decode is nondeterministic",
            entry.filename
        );
    }
}
