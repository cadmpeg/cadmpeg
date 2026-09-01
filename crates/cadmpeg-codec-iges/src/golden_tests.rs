// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect`, `decode`, and `encode` over the
//! committed fixtures.
//!
//! `tests/golden/fixtures/*.igs` are the frozen inputs.
//! Fixtures stay frozen; `UPDATE_GOLDEN=1` rewrites goldens only.
//! `inspect` pins the container summary; `decode` pins the IR, losses, and
//! source fidelity; `encode` pins writer output and deliberate refusals.
//! Shared harness: [`cadmpeg_test_support::golden`].

use crate::IgesVersion;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{elide_local_digests, snapshot_text, Branch, Harness};

use super::IgesCodec;

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "igs";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
}

/// The branches this codec pins, in golden-directory order.
fn branches() -> [Branch; 3] {
    [
        Branch::new("inspect", inspect_snapshot),
        Branch::new("decode", decode_snapshot),
        Branch::new("encode", encode_snapshot),
    ]
}

/// Serializes one container summary. An inspect error is frozen too: refusing a
/// container is contract-relevant behavior, so this never panics on codec
/// output.
fn inspect_snapshot(bytes: &[u8]) -> String {
    let value =
        match IgesCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
            Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// Serializes one decoded document: the IR, the decode report, and source
/// fidelity. A decode error is frozen too: refusing a document is
/// contract-relevant behavior, so this never panics on codec output.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match IgesCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
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

/// Decodes `bytes`, then re-encodes through the semantic writer path.
///
/// `fidelity: None` forces synthesis. Decode and encode refusals freeze as
/// `decode_error` / `encode_error`. The writer stamps `SystemTime::now()` into
/// G-section field 18; that wall-clock value is replaced before freeze.
fn encode_snapshot(bytes: &[u8]) -> String {
    let decoded =
        match IgesCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
            Ok(result) => result,
            Err(error) => {
                return snapshot_text(&serde_json::json!({
                    "decode_error": error.to_string()
                }))
            }
        };
    let outcome = Encoder::plan(
        &IgesCodec,
        EncodeInput::new(decoded.ir(), None),
        TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
    )
    .and_then(|plan| {
        let mut produced = Vec::new();
        plan.write_to(&mut produced)
            .map(|report| (report, produced))
    });
    match outcome {
        Ok((report, produced)) => {
            let output = elide_generation_timestamps(&String::from_utf8_lossy(&produced));
            snapshot_text(&serde_json::json!({
                "report": report,
                "output": output,
            }))
        }
        Err(error) => snapshot_text(&serde_json::json!({
            "encode_error": error.to_string()
        })),
    }
}

/// Replaces each writer generation timestamp `15HYYYYMMDD.HHMMSS` with a fixed
/// placeholder of the same Hollerith length.
fn elide_generation_timestamps(output: &str) -> String {
    const PREFIX: &str = "15H";
    const STAMP_LEN: usize = 15;
    const PLACEHOLDER: &str = "YYYYMMDD.HHMMSS";
    let mut result = output.to_string();
    let mut search_from = 0;
    while let Some(rel) = result[search_from..].find(PREFIX) {
        let start = search_from + rel;
        let stamp_start = start + PREFIX.len();
        let stamp_end = stamp_start + STAMP_LEN;
        if stamp_end <= result.len() {
            let stamp = &result.as_bytes()[stamp_start..stamp_end];
            if stamp[0..8].iter().all(u8::is_ascii_digit)
                && stamp[8] == b'.'
                && stamp[9..15].iter().all(u8::is_ascii_digit)
            {
                result.replace_range(stamp_start..stamp_end, PLACEHOLDER);
                search_from = stamp_end;
                continue;
            }
        }
        search_from = start + PREFIX.len();
    }
    result
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
