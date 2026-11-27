// SPDX-License-Identifier: Apache-2.0
//! Golden inspect and decode snapshots over the committed Alibre specimens.
//!
//! Shared harness: [`cadmpeg_test_support::golden`]. `UPDATE_GOLDEN=1` rewrites
//! goldens only.

use std::io::Cursor;

use cadmpeg_core::serde_erase::json;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};
use cadmpeg_test_support::EditableDecodeResult;

use crate::AlibreCodec;

const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test-fast golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), "alibre", REGENERATE)
}

fn branches() -> [Branch; 2] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
    ]
}

fn inputs() -> Vec<(String, Vec<u8>)> {
    crate::integration_tests::specimens()
        .into_iter()
        .map(|(name, bytes)| (name.to_owned(), bytes.to_vec()))
        .collect()
}

fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match AlibreCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()) {
        Ok(summary) => json::to_value(summary).expect("serialize inspect summary"),
        Err(error) => cadmpeg_core::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match AlibreCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()) {
        Ok(result) => {
            let result = EditableDecodeResult::from(result);
            cadmpeg_core::json!({
                "ir": result.ir(),
                "report": result.report(),
                "source_fidelity": result.source_fidelity(),
            })
        }
        Err(error) => cadmpeg_core::json!({ "decode_error": error.to_string() }),
    };
    snapshot_text(&value)
}

#[test]
fn golden_snapshots_hold() {
    harness().check_inputs(&inputs(), &branches());
}

#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism_inputs(&inputs(), &branches());
}

/// A geometry-depth decode agrees with the full decode outside the design
/// arenas. This codec has no geometry-depth cut, so every input delivers full.
#[test]
fn geometry_depth_agrees_with_full_depth() {
    let summary =
        cadmpeg_test_support::depth::assert_geometry_agrees_with_full(&AlibreCodec, &inputs());
    assert_eq!(summary.cut, 0, "{summary:?}");
}
