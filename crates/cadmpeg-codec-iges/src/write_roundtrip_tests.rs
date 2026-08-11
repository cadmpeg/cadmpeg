// SPDX-License-Identifier: Apache-2.0
//! Lossless write round-trip invariant over the committed IGES fixtures.
//!
//! An export whose report carries no losses must decode back to an IR that
//! [`cadmpeg_ir::diff`] reports as empty against the pre-write document.
//!
//! Fixture-decoded documents always carry an IGES source baseline. Planning
//! with `fidelity: None` therefore always records
//! `PreservedSourceUnavailable` (and typically native passthrough warnings),
//! so the no-loss path here is retained-fidelity export — usually
//! [`WritePath::VerbatimReplay`].

use std::io::Cursor;

use cadmpeg_core::golden::Harness;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions, EncodeInput, Encoder};

use super::IgesCodec;

/// Extension of the committed fixture inputs (matches `golden_tests`).
const FIXTURE_EXTENSION: &str = "igs";

/// Crate-relative regeneration hint used by the shared harness constructor.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
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
        // Retained fidelity is required for a no-loss export on these fixtures:
        // `fidelity: None` always emits PreservedSourceUnavailable for an IGES
        // source baseline (see writer.rs plan()). Refusals and lossy semantic
        // exports stay pinned by the encode golden.
        let plan = Encoder::plan(
            &IgesCodec,
            EncodeInput {
                ir: &decoded.ir,
                fidelity: Some(&decoded.source_fidelity),
            },
        );
        let Ok(plan) = plan else {
            continue;
        };
        let mut produced = Vec::new();
        let Ok(report) = plan.write_to(&mut produced) else {
            continue;
        };
        if !report.losses.is_empty() {
            continue;
        }
        written_any = true;
        let round_trip = IgesCodec
            .decode(&mut Cursor::new(produced), &DecodeOptions::default())
            .unwrap_or_else(|e| panic!("{stem}: written file failed to decode: {e}"));
        let validation = cadmpeg_ir::validate(&round_trip.ir, Vec::new());
        assert!(validation.is_ok(), "{stem}: {:#?}", validation.findings);
        let d = cadmpeg_ir::diff::diff(&decoded.ir, &round_trip.ir);
        assert!(d.is_empty(), "{stem}: no-loss export drifted: {d:#?}");
    }
    assert!(
        written_any,
        "no fixture took the lossless write path — test is vacuous"
    );
}
