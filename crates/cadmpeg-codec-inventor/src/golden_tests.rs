// SPDX-License-Identifier: Apache-2.0
//! Golden inspect and decode snapshots over field-built CFB declarations.
//!
//! Inputs are constructed in code by [`crate::test_support::fixture`] and
//! [`crate::test_support::primary_envelope_fixture`]. Shared harness:
//! [`cadmpeg_test_support::golden`]. `UPDATE_GOLDEN=1` rewrites goldens only.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};

use crate::InventorCodec;

const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-inventor golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), "ipt", REGENERATE)
}

fn branches() -> [Branch; 2] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
    ]
}

fn inputs() -> Vec<(String, Vec<u8>)> {
    vec![
        ("structural".to_string(), crate::test_support::fixture(true)),
        (
            "primary".to_string(),
            crate::test_support::primary_envelope_fixture(),
        ),
    ]
}

fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match InventorCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()) {
        Ok(summary) => serde_json::to_value(summary).expect("serialize inspect summary"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match InventorCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()) {
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

#[test]
fn primary_golden_snapshots_hold() {
    harness().check_inputs(&inputs(), &branches());
}

#[test]
fn primary_golden_output_is_deterministic() {
    harness().check_determinism_inputs(&inputs(), &branches());
}
