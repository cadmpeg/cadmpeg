// SPDX-License-Identifier: Apache-2.0
//! Shared §4.5 contract helpers: codec lookup, fixture enumeration, decoding.
//!
//! Fixture roots mirror the conformance module: STEP reads
//! `tests/fixtures`, FreeCAD reads the corpus fixtures plus the canonical
//! `tests/golden/fixtures/damaged` tree, every other fixture-backed codec
//! reads `tests/golden/fixtures`. Code-built-only codecs (inventor, sat, nx,
//! pvz) have no frozen inputs, so the §4.5 file tests do not cover them.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_ir::codec::{DecodeFailure, DecodeOptions, DecodeResult};

/// The §4.5 loss code in local wire form.
pub(crate) const UNREADABLE: &str = "record.unreadable";

/// One frozen fixture input and whether it lives under a `damaged/` directory.
pub(crate) struct Fixture {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub damaged: bool,
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_root(format: &str) -> (PathBuf, Option<&'static str>) {
    let root = workspace();
    match format {
        "step" => (
            root.join("crates/cadmpeg-codec-step/tests/fixtures"),
            Some("p21"),
        ),
        "fcstd" => (root.join("corpus/freecad_fcstd/fixtures"), Some("FCStd")),
        _ => (
            root.join(format!(
                "crates/cadmpeg-codec-{format}/tests/golden/fixtures"
            )),
            None,
        ),
    }
}

/// Every frozen fixture of one codec in path order, including `damaged/`.
pub(crate) fn fixtures(format: &str) -> Vec<Fixture> {
    let root = workspace();
    let (directory, extension) = fixture_root(format);
    let mut found = Vec::new();
    let mut pending = vec![directory];
    if format == "fcstd" {
        pending.push(root.join("crates/cadmpeg-codec-freecad/tests/golden/fixtures/damaged"));
    }
    while let Some(directory) = pending.pop() {
        for path in std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
        {
            if path.is_dir() {
                pending.push(path);
            } else if extension
                .is_none_or(|wanted| path.extension().is_some_and(|ext| ext == wanted))
            {
                let bytes = std::fs::read(&path).expect("fixture bytes");
                let damaged = path
                    .components()
                    .any(|component| component.as_os_str() == "damaged");
                found.push(Fixture {
                    path,
                    bytes,
                    damaged,
                });
            }
        }
    }
    found.sort_by(|left, right| left.path.cmp(&right.path));
    assert!(!found.is_empty(), "{format} has no fixtures");
    found
}

/// The §9.4 fixed sample of one codec's fixtures: every value in release,
///
/// the first in path order in a debug build. The sample never shrinks.
pub(crate) fn sampled_fixtures(format: &str) -> Vec<Fixture> {
    let mut fixtures = fixtures(format);
    fixtures.sort_by(|left, right| left.path.cmp(&right.path));
    if cfg!(debug_assertions) {
        fixtures.truncate(1);
    }
    fixtures
}

/// Every damaged fixture of one codec, in path order, in every build.
///
/// Damaged inputs are few and carry the §4.5 assertions, so no debug sample
/// applies to them: a test that examined no damaged input would pass
/// vacuously.
pub(crate) fn damaged_fixtures(format: &str) -> Vec<Fixture> {
    let mut damaged: Vec<Fixture> = fixtures(format)
        .into_iter()
        .filter(|fixture| fixture.damaged)
        .collect();
    damaged.sort_by(|left, right| left.path.cmp(&right.path));
    damaged
}

/// The fixture's decode golden pins a decode error, which the golden test checks.
pub(crate) fn pins_decode_error(fixture: &Path) -> bool {
    let (Some(directory), Some(stem)) = (fixture.parent(), fixture.file_stem()) else {
        return false;
    };
    let golden = directory
        .with_file_name("decode")
        .join(stem)
        .with_extension("json");
    std::fs::read(golden)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|golden| golden.get("decode_error").is_some())
}

/// Decodes one input with default options.
pub(crate) fn decode(format: &str, bytes: &[u8]) -> Result<DecodeResult, DecodeFailure> {
    let catalog = cadmpeg_registry::InputCatalog::with_builtins();
    let Some(codec) = catalog
        .descriptors()
        .find(|descriptor| descriptor.format_id().as_str() == format)
        .and_then(|descriptor| descriptor.codec())
    else {
        panic!("{format} has no codec in this build");
    };
    codec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
}

/// The decoded document as CADIR JSON.
#[must_use]
pub(crate) fn document(decoded: &DecodeResult) -> serde_json::Value {
    serde_json::to_value(decoded.ir()).expect("CADIR JSON")
}

/// The decode report as JSON.
#[must_use]
pub(crate) fn report(decoded: &DecodeResult) -> serde_json::Value {
    serde_json::to_value(decoded.report()).expect("report JSON")
}

/// Whether a whole-file refusal is the `Truncated` the cut-short rule allows.
#[must_use]
pub(crate) fn is_truncated(failure: &DecodeFailure) -> bool {
    matches!(
        failure,
        DecodeFailure::Codec(cadmpeg_core::CodecError::Truncated { .. })
    )
}

/// Runs the comparison and returns its panic text instead of panicking.
#[must_use]
pub(crate) fn check_agree(
    before: &serde_json::Value,
    after: &serde_json::Value,
    allowances: &cadmpeg_test_support::comparison::Allowances,
) -> Option<String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cadmpeg_test_support::comparison::assert_decodes_agree(
            before,
            after,
            allowances,
            cadmpeg_test_support::comparison::Mode::Same,
        );
    }))
    .err()
    .map(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_owned())
            })
            .unwrap_or_else(|| "the comparison panicked without text".to_owned())
    })
}

/// Asserts that collected findings are empty, naming every failure at once.
pub(crate) fn assert_no_findings(findings: &[String]) {
    assert!(
        findings.is_empty(),
        "{} finding(s):\n{}",
        findings.len(),
        findings.join("\n")
    );
}
