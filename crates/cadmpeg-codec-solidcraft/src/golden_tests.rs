// SPDX-License-Identifier: Apache-2.0
//! Golden inspect and decode snapshots over the designs in
//! `tests/golden/fixtures`.
//!
//! Shared harness:
//! [`cadmpeg_test_support::golden`]. `UPDATE_GOLDEN=1` rewrites goldens only.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};
use cadmpeg_test_support::EditableDecodeResult;

use crate::SolidcraftCodec;

const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test-fast golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), "sc", REGENERATE)
}

fn branches() -> [Branch; 2] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
    ]
}

fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match SolidcraftCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()) {
        Ok(summary) => serde_json::to_value(summary).expect("serialize inspect summary"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match SolidcraftCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()) {
        Ok(result) => {
            let result = EditableDecodeResult::from(result);
            serde_json::json!({
                "ir": result.ir(),
                "report": result.report(),
                "source_fidelity": result.source_fidelity(),
            })
        }
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
    };
    snapshot_text(&value)
}

#[test]
fn golden_snapshots_hold() {
    harness().check(&branches());
}

#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism(&branches());
}
