// SPDX-License-Identifier: Apache-2.0
//! Golden inspect and decode snapshots over code-built SAT streams.
//!
//! Inputs come from [`crate::tests`] builders. Snapshot regeneration never
//! writes input bytes.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::snapshot_text;

use crate::tests::BinaryFixtureKind;
use crate::SatCodec;

const TEXT_SPHERE_INSPECT: &str = include_str!("../tests/golden/inspect/text_sphere.json");
const TEXT_SPHERE_DECODE: &str = include_str!("../tests/golden/decode/text_sphere.json");
const BINARY_ASM_INSPECT: &str = include_str!("../tests/golden/inspect/binary_asm.json");
const BINARY_ASM_DECODE: &str = include_str!("../tests/golden/decode/binary_asm.json");
const BINARY_ACIS_INSPECT: &str = include_str!("../tests/golden/inspect/binary_acis.json");
const BINARY_ACIS_DECODE: &str = include_str!("../tests/golden/decode/binary_acis.json");

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

struct Case {
    stem: &'static str,
    bytes: Vec<u8>,
    inspect_golden: &'static str,
    decode_golden: &'static str,
}

fn cases() -> [Case; 3] {
    [
        Case {
            stem: "text_sphere",
            bytes: crate::tests::text_sphere_stream(1.0),
            inspect_golden: TEXT_SPHERE_INSPECT,
            decode_golden: TEXT_SPHERE_DECODE,
        },
        Case {
            stem: "binary_asm",
            bytes: crate::tests::binary_sphere_stream(BinaryFixtureKind::Asm),
            inspect_golden: BINARY_ASM_INSPECT,
            decode_golden: BINARY_ASM_DECODE,
        },
        Case {
            stem: "binary_acis",
            bytes: crate::tests::binary_sphere_stream(BinaryFixtureKind::Acis),
            inspect_golden: BINARY_ACIS_INSPECT,
            decode_golden: BINARY_ACIS_DECODE,
        },
    ]
}

#[test]
fn golden_snapshots_hold() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let rewrite = std::env::var_os("UPDATE_GOLDEN").is_some();
    for case in cases() {
        let inspect = inspect_snapshot(&case.bytes);
        let decode = decode_snapshot(&case.bytes);
        if rewrite {
            std::fs::write(root.join(format!("inspect/{}.json", case.stem)), &inspect)
                .expect("write inspect golden");
            std::fs::write(root.join(format!("decode/{}.json", case.stem)), &decode)
                .expect("write decode golden");
            continue;
        }
        assert_eq!(case.inspect_golden, inspect, "inspect {}", case.stem);
        assert_eq!(case.decode_golden, decode, "decode {}", case.stem);
    }
}

#[test]
fn golden_output_is_deterministic() {
    for case in cases() {
        assert_eq!(
            inspect_snapshot(&case.bytes),
            inspect_snapshot(&case.bytes),
            "inspect {}",
            case.stem
        );
        assert_eq!(
            decode_snapshot(&case.bytes),
            decode_snapshot(&case.bytes),
            "decode {}",
            case.stem
        );
    }
}
