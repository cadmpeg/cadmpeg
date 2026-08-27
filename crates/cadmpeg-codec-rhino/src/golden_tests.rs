// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect` and `decode` over the committed
//! fixtures.
//!
//! `tests/golden/fixtures/*.3dm` are the frozen inputs.
//! Fixtures stay frozen; `UPDATE_GOLDEN=1` rewrites goldens only.
//! `inspect` pins the container summary; `decode` pins the IR, losses, and
//! source fidelity. Shared harness: [`cadmpeg_test_support::golden`].

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::Path;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions, EncodeInput, Encoder, TargetRequest};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};

use super::{RhinoArchiveVersion, RhinoCodec, RhinoEncoder};

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "3dm";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-rhino golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
}

/// The branches this codec pins, in golden-directory order.
fn branches() -> [Branch; 2] {
    [
        Branch::new("inspect", inspect_snapshot),
        Branch::new("decode", decode_snapshot),
    ]
}

/// Serializes one container summary. An inspect error is frozen too: refusing a
/// container is contract-relevant behavior, so this never panics on codec
/// output.
fn inspect_snapshot(bytes: &[u8]) -> String {
    let value =
        match RhinoCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
            Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// Serializes one decoded document: the IR, the decode report, and source
/// fidelity. A decode error is frozen too: refusing a document is
/// contract-relevant behavior, so this never panics on codec output.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match RhinoCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
    {
        Ok(result) => serde_json::json!({
            "ir": serde_json::to_value(result.ir()).expect("serialize ir"),
            "report": serde_json::to_value(result.report()).expect("serialize report"),
            "source_fidelity": serde_json::to_value(result.source_fidelity())
                .expect("serialize source_fidelity"),
        }),
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
    };
    snapshot_text(&value)
}

/// Every committed golden still matches what the codec produces.
#[test]
fn golden_snapshots_hold() {
    harness().check(&branches());
}

/// Putting the same bytes through a branch twice produces identical text.
#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism(&branches());
}

/// Archive targets `tests/golden/encode/` covers, as `(golden infix, target)`.
const ENCODE_TARGETS: [(&str, RhinoArchiveVersion); 2] = [
    ("v5", RhinoArchiveVersion::V5),
    ("v8", RhinoArchiveVersion::V8),
];

/// The archive [`RhinoEncoder`] produces for one target, or the refusal it
/// reports.
///
/// The request is [`TargetRequest::Explicit`], which is what the golden name
/// `<fixture>.v5.bin` states: this fixture written at archive 50, whatever the
/// fixture's own archive word is. The `Inherit` path resolves against the source
/// instead and so cannot be pinned per target; it is covered by the unit tests
/// in `writer/tests/targets.rs`.
fn encode_outcome(bytes: &[u8], version: RhinoArchiveVersion) -> Option<Result<Vec<u8>, String>> {
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .ok()?;
    let mut encoded = Vec::new();
    let written = Encoder::plan(
        &RhinoEncoder::new(version),
        EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
        TargetRequest::Explicit(version.target()),
    )
    .and_then(|plan| plan.write_to(&mut encoded));
    Some(match written {
        Ok(_) => Ok(encoded),
        Err(error) => Err(error.to_string()),
    })
}

/// Compares every fixture's encode outcome against `tests/golden/encode/`.
fn check_encode_branch(fixtures: &[(String, Vec<u8>)], update: bool) -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/encode");
    let mut failures = Vec::new();
    let mut produced = BTreeSet::new();
    for (name, bytes) in fixtures {
        for (target, version) in ENCODE_TARGETS {
            let archive = dir.join(format!("{name}.{target}.bin"));
            let refusal = dir.join(format!("{name}.{target}.err.txt"));
            match encode_outcome(bytes, version) {
                None => {}
                Some(Ok(written)) => {
                    produced.insert(format!("{name}.{target}.bin"));
                    if update {
                        write_golden(&archive, &written);
                        remove_golden(&refusal);
                        continue;
                    }
                    match std::fs::read(&archive) {
                        Ok(expected) if expected == written => {}
                        Ok(expected) => failures.push(format!(
                            "fixture `{name}` target {target}: written archive differs from {}: {}",
                            archive.display(),
                            first_byte_difference(&expected, &written)
                        )),
                        Err(error) => failures.push(format!(
                            "fixture `{name}` target {target}: cannot read {} ({error}); regenerate with `{REGENERATE}`",
                            archive.display()
                        )),
                    }
                }
                Some(Err(message)) => {
                    produced.insert(format!("{name}.{target}.err.txt"));
                    let written = format!("{message}\n");
                    if update {
                        write_golden(&refusal, written.as_bytes());
                        remove_golden(&archive);
                        continue;
                    }
                    match std::fs::read_to_string(&refusal) {
                        Ok(expected) if expected.replace("\r\n", "\n") == written => {}
                        Ok(expected) => failures.push(format!(
                            "fixture `{name}` target {target}: refusal differs from {}\n    golden: {}\n    actual: {}",
                            refusal.display(),
                            expected.trim_end(),
                            written.trim_end()
                        )),
                        Err(error) => failures.push(format!(
                            "fixture `{name}` target {target}: cannot read {} ({error}); regenerate with `{REGENERATE}`",
                            refusal.display()
                        )),
                    }
                }
            }
        }
    }
    for orphan in std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .map(|entry| entry.expect("directory entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !produced.contains(name))
    {
        failures.push(format!(
            "golden `{orphan}` under {} is produced by no fixture and target; delete it or restore its input",
            dir.display()
        ));
    }
    failures
}

fn write_golden(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

/// Removes the golden of the outcome a fixture no longer produces.
fn remove_golden(path: &Path) {
    if path.exists() {
        std::fs::remove_file(path)
            .unwrap_or_else(|error| panic!("remove {}: {error}", path.display()));
    }
}

/// Locates the first differing byte, or the length disagreement when one side is
/// a prefix of the other.
fn first_byte_difference(expected: &[u8], actual: &[u8]) -> String {
    match expected
        .iter()
        .zip(actual)
        .position(|(left, right)| left != right)
    {
        Some(offset) => format!(
            "first difference at offset {offset}: golden 0x{:02x}, written 0x{:02x} (lengths {} and {})",
            expected[offset],
            actual[offset],
            expected.len(),
            actual.len()
        ),
        None => format!(
            "one side is a prefix of the other (lengths {} and {})",
            expected.len(),
            actual.len()
        ),
    }
}

/// Every committed encode golden still matches what the encoder produces.
#[test]
fn encode_goldens_hold() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let failures = check_encode_branch(&harness().fixture_inputs(), update);
    assert!(
        failures.is_empty(),
        "{} encode golden(s) drifted; if the change is intended regenerate with `{REGENERATE}` and review the diff:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Encoding the same input twice produces identical bytes.
#[test]
fn encode_output_is_deterministic() {
    for (name, bytes) in harness().fixture_inputs() {
        for (target, version) in ENCODE_TARGETS {
            assert!(
                encode_outcome(&bytes, version) == encode_outcome(&bytes, version),
                "fixture `{name}` target {target}: two encodes in one process disagree"
            );
        }
    }
}
