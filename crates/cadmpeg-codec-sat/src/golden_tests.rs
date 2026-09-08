// SPDX-License-Identifier: Apache-2.0
//! Golden inspect and decode snapshots over code-built SAT streams.
//!
//! Inputs come from [`crate::test_support`] builders. Shared harness:
//! [`cadmpeg_test_support::golden`]. `UPDATE_GOLDEN=1` rewrites goldens only.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};

use crate::test_support::BinaryFixtureKind;
use crate::SatCodec;

const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-sat golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), "sat", REGENERATE)
}

fn branches() -> [Branch; 2] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
    ]
}

fn inputs() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "text_sphere".to_string(),
            crate::test_support::text_sphere_stream(1.0),
        ),
        (
            "binary_asm".to_string(),
            crate::test_support::binary_sphere_stream(BinaryFixtureKind::Asm),
        ),
        (
            "binary_acis".to_string(),
            crate::test_support::binary_sphere_stream(BinaryFixtureKind::Acis),
        ),
    ]
}

fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match SatCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()) {
        Ok(summary) => serde_json::to_value(summary).expect("serialize inspect summary"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match SatCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()) {
        Ok(result) => serde_json::json!({
            "ir": result.ir(),
            "report": result.report(),
            "source_fidelity": result.source_fidelity(),
        }),
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
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
