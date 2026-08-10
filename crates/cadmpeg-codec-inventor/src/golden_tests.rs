// SPDX-License-Identifier: Apache-2.0
//! Golden inspect and decode snapshots over a field-built CFB declaration.
//!
//! The input is constructed from explicit CFB fields by [`crate::tests::fixture`].
//! Snapshot regeneration never writes input bytes.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::golden::snapshot_text;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};

use crate::InventorCodec;

const INSPECT: &str = include_str!("../tests/golden/inspect/structural.json");
const DECODE: &str = include_str!("../tests/golden/decode/structural.json");

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
            "ir": result.ir,
            "report": result.report,
            "source_fidelity": result.source_fidelity,
        }),
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
    };
    snapshot_text(&value)
}

#[test]
fn golden_snapshots_hold() {
    let bytes = crate::tests::fixture(true);
    let inspect = inspect_snapshot(&bytes);
    let decode = decode_snapshot(&bytes);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
        std::fs::write(root.join("inspect/structural.json"), &inspect)
            .expect("write inspect golden");
        std::fs::write(root.join("decode/structural.json"), &decode).expect("write decode golden");
        return;
    }
    assert_eq!(INSPECT, inspect);
    assert_eq!(DECODE, decode);
}

#[test]
fn golden_output_is_deterministic() {
    let bytes = crate::tests::fixture(true);
    assert_eq!(inspect_snapshot(&bytes), inspect_snapshot(&bytes));
    assert_eq!(decode_snapshot(&bytes), decode_snapshot(&bytes));
}
