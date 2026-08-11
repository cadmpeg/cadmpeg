// SPDX-License-Identifier: Apache-2.0
//! Lossless write round-trip invariant over the committed IGES fixtures.
//!
//! An export whose report carries no losses must decode back to an IR that
//! [`cadmpeg_ir::diff`] reports as empty against the pre-write document.

use std::io::Cursor;

use cadmpeg_core::golden::Harness;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::SourceFidelity;

use super::IgesCodec;

/// Extension of the committed fixture inputs (matches `golden_tests`).
const FIXTURE_EXTENSION: &str = "igs";

/// Crate-relative regeneration hint used by the shared harness constructor.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
}

fn try_lossless_round_trip(
    stem: &str,
    original: &CadIr,
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
) -> bool {
    let Ok(plan) = Encoder::plan(&IgesCodec, EncodeInput { ir, fidelity }) else {
        return false;
    };
    let mut produced = Vec::new();
    let Ok(report) = plan.write_to(&mut produced) else {
        return false;
    };
    if !report.losses.is_empty() {
        return false;
    }
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(produced), &DecodeOptions::default())
        .unwrap_or_else(|e| panic!("{stem}: written file failed to decode: {e}"));
    let validation = cadmpeg_ir::validate(&round_trip.ir, Vec::new());
    assert!(validation.is_ok(), "{stem}: {:#?}", validation.findings);
    let d = cadmpeg_ir::diff::diff(original, &round_trip.ir);
    assert!(d.is_empty(), "{stem}: no-loss export drifted: {d:#?}");
    true
}

#[test]
fn lossless_exports_round_trip_to_identical_ir() {
    let harness = harness();
    let mut written_any = false;
    for (stem, bytes) in harness.fixture_inputs() {
        let Ok(decoded) =
            IgesCodec.decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        else {
            continue;
        };
        if try_lossless_round_trip(&stem, &decoded.ir, &decoded.ir, None)
            || try_lossless_round_trip(
                &stem,
                &decoded.ir,
                &decoded.ir,
                Some(&decoded.source_fidelity),
            )
        {
            written_any = true;
        }
    }
    assert!(
        written_any,
        "no fixture took the lossless write path — test is vacuous"
    );
}
