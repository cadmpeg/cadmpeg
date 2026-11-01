// SPDX-License-Identifier: Apache-2.0
//! Nightly inspect and decode of the NIST MBE PMI producer-evidence subset.
//!
//! Inputs are the `LicenseRef-NIST-PD` files in `corpus/manifest.toml`. This
//! is a live pass over those CAD bytes: it does not commit inspect or decode
//! snapshots. `#[ignore]` keeps it out of `cargo test-fast`. The scheduled
//! workflow `.github/workflows/nist-pmi-hallmarks.yml` runs it.

#![allow(clippy::unwrap_used)] // Test code: a failed unwrap is a test failure.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_registry::InputCatalog;
use serde::Deserialize;

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
    entries
}

fn codec_for<'a>(catalog: &'a InputCatalog, format: &str) -> &'a dyn Codec {
    catalog
        .descriptors()
        .find(|descriptor| descriptor.format_id().as_str() == format)
        .and_then(cadmpeg_registry::InputDescriptor::codec)
        .unwrap_or_else(|| panic!("manifest format {format:?} has no codec in this build"))
}

/// Every NIST producer-evidence CAD file inspects and decodes.
#[test]
#[ignore = "producer-evidence inspect/decode; run via .github/workflows/nist-pmi-hallmarks.yml"]
fn nist_pmi_files_inspect_and_decode() {
    let catalog = InputCatalog::with_builtins();
    let mut failures = Vec::new();
    for entry in nist_entries() {
        let bytes = std::fs::read(repo_root().join("corpus").join(&entry.filename))
            .unwrap_or_else(|error| panic!("reading {}: {error}", entry.filename));
        let codec = codec_for(&catalog, &entry.format);
        if let Err(error) = codec.inspect(
            &mut Cursor::new(bytes.as_slice()),
            &InspectOptions::default(),
        ) {
            failures.push(format!("{} inspect: {error}", entry.filename));
        }
        if let Err(error) = codec.decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        ) {
            failures.push(format!("{} decode: {error}", entry.filename));
        }
    }
    assert!(
        failures.is_empty(),
        "{} NIST PMI file(s) failed inspect or decode:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
