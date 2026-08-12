// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect` and `decode` over the committed
//! fixtures.
//!
//! `tests/fixtures/*.p21` are the frozen inputs.
//! Fixtures stay frozen; `UPDATE_GOLDEN=1` rewrites goldens only.
//! `inspect` pins the container summary; `decode` pins the IR, losses, and
//! source fidelity. Shared harness: [`cadmpeg_test_support::golden`].

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{elide_local_digests, snapshot_text, Branch, Harness};

use super::StepCodec;

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "p21";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-step golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
        .with_fixture_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
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
    let value = match StepCodec::default()
        .inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default())
    {
        Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

/// Serializes one decoded document: the IR, the decode report, and source
/// fidelity. A decode error is frozen too: refusing a document is
/// contract-relevant behavior, so this never panics on codec output.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match StepCodec::default()
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
    {
        Ok(mut result) => {
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
