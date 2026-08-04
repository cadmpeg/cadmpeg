// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect` and `decode` over the committed
//! fixtures.
//!
//! `corpus/freecad_fcstd/fixtures/*.FCStd` are the frozen inputs.
//! This harness never writes them: a snapshot test can only tell a decoder
//! change apart from an input change while the inputs hold still, so
//! regenerating an input destroys the evidence the snapshot exists to carry.
//! `UPDATE_GOLDEN=1` rewrites `tests/golden/decode/` and
//! `tests/golden/inspect/`, and nothing else.
//!
//! `tests/golden/inspect/` pins the container summary and
//! `tests/golden/decode/` pins the decoded document: the IR, the decode
//! report's losses, and source fidelity. A feature-typing or loss-accounting
//! change moves the decode branch and `inspect` cannot see it, because an
//! inspect summary describes the container, not what was transferred out of it.
//!
//! [`cadmpeg_codec_core::golden`] holds the enumeration, comparison, and
//! reporting shared with every other codec; this module supplies only this
//! codec's branches.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_codec_core::decode::InspectOptions;
use cadmpeg_codec_core::golden::{snapshot_text, Branch, Harness};
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};

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
        match FcstdCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
            Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// Serializes one decoded document: the IR, the decode report, and source
/// fidelity. A decode error is frozen too: refusing a document is
/// contract-relevant behavior, so this never panics on codec output.
///
/// The retained native arenas are pinned by digest rather than by value. Written
/// out they run to 65MB across these eleven goldens, which no reviewer can read
/// and which swamps every other change in a diff; a length and a hash still fail
/// the moment their content moves.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match FcstdCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
    {
        Ok(result) => {
            let mut ir = serde_json::to_value(&result.ir).expect("serialize ir");
            if let Some(native) = ir.get_mut("native") {
                *native = serde_json::json!({
                    "__elided": "native arenas are pinned by digest, not by value",
                    "__serialized_len": serde_json::to_string(native)
                        .expect("serialize native arenas")
                        .len(),
                    "__sha256": cadmpeg_ir::hash::canonical_json_sha256(native),
                });
            }
            serde_json::json!({
                "ir": ir,
                "report": serde_json::to_value(&result.report).expect("serialize report"),
                "source_fidelity": serde_json::to_value(&result.source_fidelity)
                    .expect("serialize source_fidelity"),
            })
        }
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
