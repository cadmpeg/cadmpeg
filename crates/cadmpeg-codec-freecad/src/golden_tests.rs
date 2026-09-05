// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect` and `decode` over the committed
//! fixtures.
//!
//! `corpus/freecad_fcstd/fixtures/*.FCStd` are the frozen inputs. This harness
//! never writes them. `UPDATE_GOLDEN=1` rewrites `tests/golden/decode/` and
//! `tests/golden/inspect/`, and nothing else.
//!
//! `tests/golden/inspect/` pins the container summary and
//! `tests/golden/decode/` pins the decoded document: the IR, the decode
//! report's losses, and source fidelity.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::Path;

use cadmpeg_codec_step::StepCodec;
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::compare::texts_agree;
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};

use super::FcstdCodec;

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "FCStd";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-freecad golden";

/// The `FreeCAD` goldens have no `tests/golden/fixtures/` tree. Their inputs are
/// the charter fixtures under `corpus/freecad_fcstd/fixtures/`, one `.FCStd` per
/// golden basename.
fn harness() -> Harness {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate manifest sits two levels below the repository root")
        .join("corpus/freecad_fcstd/fixtures");
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
        .with_fixture_dir(fixtures)
}

/// The branches this codec pins, in golden-directory order.
fn branches() -> [Branch; 3] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
        Branch::named("encode", encode_snapshot),
    ]
}

/// Serializes one container summary. An inspect error is frozen too: refusing a
/// container is contract-relevant behavior, so this never panics on codec
/// output.
fn inspect_snapshot(bytes: &[u8]) -> String {
    let value =
        match FcstdCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
            Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// Serializes one decoded document: the IR, the decode report, and source
/// fidelity. A decode error is frozen too. Native arena values are omitted;
/// namespace versions, arena populations, and record identities are pinned.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match FcstdCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
    {
        Ok(result) => {
            let native_shape = native_shape(&result.ir().native);
            let mut ir = serde_json::to_value(result.ir()).expect("serialize ir");
            if let Some(native) = ir.get_mut("native") {
                *native = serde_json::json!({
                    "__elided": "native arena values are omitted; structure is pinned by identity",
                    "__arena_counts": native_shape["counts"].clone(),
                    "__shape_sha256": native_shape["sha256"].clone(),
                });
            }
            serde_json::json!({
                "ir": ir,
                "report": serde_json::to_value(result.report()).expect("serialize report"),
                "source_fidelity": serde_json::to_value(result.source_fidelity())
                    .expect("serialize source_fidelity"),
            })
        }
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
    };
    snapshot_text(&value)
}

/// Summarizes native arena structure without hashing platform-dependent values.
fn native_shape(native: &cadmpeg_ir::Native) -> serde_json::Value {
    let mut counts = serde_json::Map::new();
    let mut shape = serde_json::Map::new();
    for (format, namespace) in &native.0 {
        let mut namespace_counts = serde_json::Map::new();
        let mut namespace_shape = serde_json::Map::new();
        for (arena, records) in &namespace.arenas {
            let mut ids = records
                .iter()
                .map(|record| record.id().to_owned())
                .collect::<Vec<_>>();
            ids.sort_unstable();
            namespace_counts.insert(arena.clone(), serde_json::json!(records.len()));
            namespace_shape.insert(
                arena.clone(),
                serde_json::json!({
                    "count": records.len(),
                    "ids": ids,
                }),
            );
        }
        counts.insert(format.clone(), serde_json::Value::Object(namespace_counts));
        shape.insert(
            format.clone(),
            serde_json::json!({
                "version": namespace.version(),
                "arenas": namespace_shape,
            }),
        );
    }
    let shape = serde_json::Value::Object(shape);
    serde_json::json!({
        "counts": counts,
        "sha256": cadmpeg_ir::hash::canonical_json_sha256(&shape),
    })
}

/// Serializes one re-encoded document by archive membership: entry names in
/// write order, plus length and digest of each decompressed payload.
///
/// Digests cover decompressed payloads; ZIP bytes depend on the deflate library.
///
/// # Panics
///
/// Panics when a produced archive is not a readable ZIP, or when two runs in one
/// process produce different bytes.
fn encode_snapshot(bytes: &[u8]) -> String {
    let value = match encode_once(bytes) {
        Ok((report, archive)) => {
            let (_, second) = encode_once(bytes).expect("a second encode of the same input");
            assert!(
                archive == second,
                "the FCStd writer produced different bytes for the same input in one process"
            );
            serde_json::json!({
                "entries": archive_entries(&archive),
                "report": serde_json::to_value(&report).expect("serialize export report"),
            })
        }
        Err(error) => serde_json::json!({ "encode_error": error }),
    };
    snapshot_text(&value)
}

/// Decodes `bytes` and writes the document back, returning the export report and
/// the produced archive, or the first refusal as text.
fn encode_once(bytes: &[u8]) -> Result<(cadmpeg_ir::ExportReport, Vec<u8>), String> {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .map_err(|error| error.to_string())?;
    let mut produced = Vec::new();
    let report = Encoder::plan(
        &FcstdCodec,
        EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut produced))
    .map_err(|error| error.to_string())?;
    Ok((report, produced))
}

/// Lists an archive's entries in stored order with each decompressed payload's
/// length and digest.
///
/// # Panics
///
/// Panics when the bytes are not a readable ZIP or an entry cannot be inflated.
fn archive_entries(archive: &[u8]) -> serde_json::Value {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive.to_vec()))
        .expect("the FCStd writer produced a readable ZIP");
    let entries = (0..zip.len())
        .map(|index| {
            let mut entry = zip.by_index(index).expect("archive entry");
            let name = entry.name().to_owned();
            let mut payload = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut payload)
                .unwrap_or_else(|error| panic!("inflate {name}: {error}"));
            serde_json::json!({
                "name": name,
                "payload_len": payload.len(),
                "payload_sha256": cadmpeg_ir::hash::sha256_hex(&payload),
            })
        })
        .collect::<Vec<_>>();
    serde_json::Value::Array(entries)
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

/// Serializes what one fixture exports as, or the refusal the export reports.
///
/// Pins STEP output by content for this crate's fixtures. Export errors are
/// frozen too.
fn step_snapshot(bytes: &[u8]) -> String {
    let decoded =
        match FcstdCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
            Ok(decoded) => decoded,
            Err(error) => return format!("decode_error: {error}\n"),
        };
    let mut exported = Vec::new();
    match Encoder::plan(
        &StepCodec::default(),
        EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut exported))
    {
        Ok(_) => String::from_utf8(exported).expect("the STEP writer produces UTF-8"),
        Err(error) => format!("export_error: {error}\n"),
    }
}

/// Compares two STEP texts, holding structure exact and numbers to a tolerance.
///
/// # Errors
///
/// Returns a description locating the first line that disagrees.
fn step_texts_agree(expected: &str, actual: &str) -> Result<(), String> {
    let expected = expected.replace("\r\n", "\n");
    let actual = actual.replace("\r\n", "\n");
    if expected == actual {
        return Ok(());
    }
    let mut expected_lines = expected.lines();
    let mut actual_lines = actual.lines();
    let mut line = 0usize;
    loop {
        line += 1;
        match (expected_lines.next(), actual_lines.next()) {
            (None, None) => return Ok(()),
            (Some(left), Some(right)) if lines_agree(left, right) => {}
            (left, right) => {
                return Err(format!(
                    "at line {line}\n    golden: {}\n    actual: {}",
                    left.unwrap_or("<end of file>"),
                    right.unwrap_or("<end of file>")
                ))
            }
        }
    }
}

/// Whether two STEP lines agree, tolerating only platform-scale numeric drift.
fn lines_agree(left: &str, right: &str) -> bool {
    texts_agree(left, right)
}

/// Compares every fixture's STEP export against `tests/golden/step/`.
fn check_step_branch(update: bool) -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/step");
    let mut failures = Vec::new();
    let mut produced = BTreeSet::new();
    for (name, bytes) in harness().fixture_inputs() {
        let actual = step_snapshot(&bytes);
        let path = dir.join(format!("{name}.step"));
        produced.insert(format!("{name}.step"));
        if update {
            std::fs::write(&path, actual.as_bytes())
                .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(expected) => {
                if let Err(mismatch) = step_texts_agree(&expected, &actual) {
                    failures.push(format!(
                        "fixture `{name}`: STEP export diverged from {}\n    {mismatch}",
                        path.display()
                    ));
                }
            }
            Err(error) => failures.push(format!(
                "fixture `{name}`: cannot read {} ({error}); regenerate with `{REGENERATE}`",
                path.display()
            )),
        }
    }
    for orphan in std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .map(|entry| entry.expect("directory entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !produced.contains(name))
    {
        failures.push(format!(
            "golden `{orphan}` under {} has no fixture behind it; delete the golden or restore the input",
            dir.display()
        ));
    }
    failures
}

/// Every committed STEP golden still matches what the export produces.
#[test]
fn step_goldens_hold() {
    let failures = check_step_branch(std::env::var_os("UPDATE_GOLDEN").is_some());
    assert!(
        failures.is_empty(),
        "{} STEP golden(s) drifted; if the change is intended regenerate with `{REGENERATE}` and review the diff:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Exporting the same document twice produces identical text.
#[test]
fn step_output_is_deterministic() {
    for (name, bytes) in harness().fixture_inputs() {
        assert!(
            step_snapshot(&bytes) == step_snapshot(&bytes),
            "fixture `{name}`: two STEP exports in one process disagree"
        );
    }
}

mod step_comparison {
    use super::step_texts_agree;

    /// The two values one exported vector carries with and without a one
    /// unit-in-the-last-place perturbation of every libm transcendental.
    const LINUX_VECTOR: &str = "#95 = VECTOR('',#37,0.8823529411764706);\n";
    const PERTURBED_VECTOR: &str = "#95 = VECTOR('',#37,0.8823529411764707);\n";

    #[test]
    fn last_place_disagreement_agrees() {
        assert!(step_texts_agree(LINUX_VECTOR, PERTURBED_VECTOR).is_ok());
    }

    #[test]
    fn tiny_signed_residual_agrees() {
        let positive = "#329 = DIRECTION('',(1.,0.,0.0000000000000002));\n";
        let negative = "#329 = DIRECTION('',(1.,0.,-0.0000000000000002));\n";
        assert!(step_texts_agree(positive, negative).is_ok());
    }

    #[test]
    fn a_moved_instance_reference_disagrees() {
        let moved = "#95 = VECTOR('',#38,0.8823529411764706);\n";
        assert!(step_texts_agree(LINUX_VECTOR, moved).is_err());
    }

    #[test]
    fn a_renumbered_record_disagrees() {
        let moved = "#96 = VECTOR('',#37,0.8823529411764706);\n";
        assert!(step_texts_agree(LINUX_VECTOR, moved).is_err());
    }

    #[test]
    fn a_different_entity_disagrees() {
        let moved = "#95 = DIRECTION('',#37,0.8823529411764706);\n";
        assert!(step_texts_agree(LINUX_VECTOR, moved).is_err());
    }

    #[test]
    fn a_sign_flip_disagrees() {
        let moved = "#95 = VECTOR('',#37,-0.8823529411764706);\n";
        assert!(step_texts_agree(LINUX_VECTOR, moved).is_err());
    }

    #[test]
    fn a_change_above_the_tolerance_disagrees() {
        let moved = "#95 = VECTOR('',#37,0.8823539411764706);\n";
        let error = step_texts_agree(LINUX_VECTOR, moved)
            .expect_err("a change above the tolerance must be reported");
        assert!(error.contains("at line 1"), "{error}");
    }

    #[test]
    fn a_dropped_record_disagrees() {
        assert!(step_texts_agree(&format!("{LINUX_VECTOR}#96 = X();\n"), LINUX_VECTOR).is_err());
    }
}
