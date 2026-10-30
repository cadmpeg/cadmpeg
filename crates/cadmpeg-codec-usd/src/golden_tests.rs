// SPDX-License-Identifier: Apache-2.0
//! Golden inspect, decode, and encode snapshots.
//!
//! Inputs are the hand-written and OpenUSD-authored layers and packages under
//! `tests/golden/fixtures`, USDC bootstrap headers built in code, and IR
//! example documents for the writer. Shared harness:
//! [`cadmpeg_test_support::golden`]. `UPDATE_GOLDEN=1` rewrites goldens only.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};
use cadmpeg_test_support::EditableDecodeResult;

use cadmpeg_ir::codec::write::target::TargetRequest;

use crate::test_support::{encode, try_encode, usdc_header};
use crate::UsdCodec;

const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test-fast golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), "usda", REGENERATE)
}

fn layer_branches() -> [Branch; 3] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
        Branch::named("encode", encode_snapshot),
    ]
}

/// The text, binary, and package fixtures and the USDC headers.
fn layer_inputs() -> Vec<(String, Vec<u8>)> {
    let mut inputs = harness().fixture_inputs();
    for extension in ["usdc", "usdz"] {
        inputs.extend(
            Harness::new(env!("CARGO_MANIFEST_DIR"), extension, REGENERATE).fixture_inputs(),
        );
    }
    inputs.push(("usdc_0_8_0".to_owned(), usdc_header([0, 8, 0])));
    inputs.push(("usdc_0_3_0".to_owned(), usdc_header([0, 3, 0])));
    inputs
}

fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match UsdCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()) {
        Ok(summary) => serde_json::to_value(summary).expect("serialize inspect summary"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match UsdCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()) {
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

/// The layer the writer produces from the decoded layer, with the inherited
/// target, and its report.
fn encode_snapshot(bytes: &[u8]) -> String {
    let value = match UsdCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()) {
        Ok(result) => match try_encode(result.ir(), TargetRequest::Inherit) {
            Ok((text, report)) => serde_json::json!({
                "report": report,
                "usda": text.lines().collect::<Vec<_>>(),
            }),
            Err(error) => serde_json::json!({ "encode_error": error.to_string() }),
        },
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
    };
    snapshot_text(&value)
}

/// The layer the writer produces from an IR example document.
fn ir_encode_snapshot(bytes: &[u8]) -> String {
    let name = std::str::from_utf8(bytes).expect("example name");
    let ir = match name {
        "unit_cube" => cadmpeg_ir::examples::unit_cube(),
        "directed_subd_sum" => cadmpeg_ir::examples::directed_subd_sum(),
        other => panic!("no example {other}"),
    }
    .expect("build the example");
    let (text, report) = encode(&ir);
    snapshot_text(&serde_json::json!({
        "report": report,
        "usda": text.lines().collect::<Vec<_>>(),
    }))
}

fn example_inputs() -> Vec<(String, Vec<u8>)> {
    ["unit_cube", "directed_subd_sum"]
        .into_iter()
        .map(|name| (format!("example_{name}"), name.as_bytes().to_vec()))
        .collect()
}

#[test]
fn golden_snapshots_hold() {
    harness().check_inputs(&layer_inputs(), &layer_branches());
}

#[test]
fn golden_example_encodes_hold() {
    harness().check_inputs(
        &example_inputs(),
        &[Branch::named("encode_ir", ir_encode_snapshot)],
    );
}

#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism_inputs(&layer_inputs(), &layer_branches());
    harness().check_determinism_inputs(
        &example_inputs(),
        &[Branch::named("encode_ir", ir_encode_snapshot)],
    );
}
