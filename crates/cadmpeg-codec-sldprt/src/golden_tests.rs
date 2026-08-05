// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect` and `decode` over the committed
//! fixtures.
//!
//! `tests/golden/fixtures/*.sldprt` are the frozen inputs.
//! This harness never writes them: a snapshot test can only tell a decoder
//! change apart from an input change while the inputs hold still, so
//! regenerating an input destroys the evidence the snapshot exists to carry.
//! `UPDATE_GOLDEN=1` rewrites `tests/golden/decode/` and
//! `tests/golden/inspect/`, and nothing else.
//!
//! `tests/golden/inspect/` pins the container summary and
//! `tests/golden/decode/` pins the decoded document: the IR, the decode
//! report's losses, and source fidelity. A feature-typing or loss-accounting
//! change moves the decode branch and `inspect` cannot see it, because an
//! inspect summary describes the container, not what was transferred out of it.
//!
//! [`cadmpeg_codec_core::golden`] holds the enumeration, comparison, and
//! reporting shared with every other codec; this module supplies only this
//! codec's branches.

use std::io::Cursor;

use cadmpeg_codec_core::decode::InspectOptions;
use cadmpeg_codec_core::golden::{
    elide_local_digests, snapshot_text, snapshots_agree, Branch, Harness,
};
use cadmpeg_codec_core::CodecError;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
use cadmpeg_ir::roundtrip::{semantic_roundtrip, verbatim_replay_holds, SemanticOutcome};
use cadmpeg_ir::WritePath;

use super::SldprtCodec;

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "sldprt";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-sldprt golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
}

/// The branches this codec pins, in golden-directory order.
fn branches() -> [Branch; 2] {
    [
        Branch::new("inspect", inspect_snapshot),
        Branch::new("decode", decode_snapshot),
    ]
}

/// Serializes one container summary. An inspect error is frozen too: refusing a
/// container is contract-relevant behavior, so this never panics on codec
/// output.
fn inspect_snapshot(bytes: &[u8]) -> String {
    let value =
        match SldprtCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
            Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// Serializes one decoded document: the IR, the decode report, and source
/// fidelity. A decode error is frozen too: refusing a document is
/// contract-relevant behavior, so this never panics on codec output.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value =
        match SldprtCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
            Ok(mut result) => {
                if let Some(source) = result.ir.source.as_mut() {
                    // The `native` lane digests cover retained source bytes and
                    // stay pinned; a `_local_sha256` digest covers decoded
                    // content, so a platform's libm moves it.
                    elide_local_digests(&mut source.attributes);
                }
                serde_json::json!({
                    "ir": serde_json::to_value(&result.ir).expect("serialize ir"),
                    "report": serde_json::to_value(&result.report).expect("serialize report"),
                    "source_fidelity": serde_json::to_value(&result.source_fidelity)
                        .expect("serialize source_fidelity"),
                })
            }
            Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// Serializes the census of the document a byte string decodes to: how many
/// entities landed in each neutral arena, plus its units and tolerances.
///
/// This is what a rewritten container can be held to. Entity identity cannot be:
/// several `SolidWorks` identifiers embed the position of the record they came
/// from, so repacking the compound file renumbers them — rewriting
/// `body_display_list` moves one tessellation from
/// `sldprt:displaylist:record#247:0` to `#248:0` while the tessellation itself is
/// unchanged. A census still fails the moment the writer drops, duplicates, or
/// invents an entity.
///
/// The `native.*` rows are excluded for the same reason: they count records of
/// the container the writer just rebuilt, not entities of the document it
/// carried. Rewriting `analytic_cylinder` produces a container whose decode finds
/// one more unclassified top-level block than the input's, which is a fact about
/// the repack.
fn arena_census(bytes: &[u8]) -> String {
    let value =
        match SldprtCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
            Ok(result) => {
                let counts = cadmpeg_ir::validate(&result.ir, Vec::new())
                    .entity_counts
                    .into_iter()
                    .filter(|(arena, _)| !arena.starts_with("native."))
                    .collect::<std::collections::BTreeMap<_, _>>();
                serde_json::json!({
                    "counts": counts,
                    "units": serde_json::to_value(result.ir.units).expect("serialize units"),
                    "tolerances": serde_json::to_value(result.ir.tolerances)
                        .expect("serialize tolerances"),
                })
            }
            Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
        };
    snapshot_text(&value)
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

/// Every fixture survives a decode and comes back byte for byte through the
/// verbatim-replay path.
///
/// This covers container fidelity and nothing about the writer: on this path the
/// encoder copies the retained source image and no writer code runs.
/// [`cadmpeg_ir::roundtrip::verbatim_replay_holds`] asserts the path as well as
/// the bytes, so the day a fixture stops replaying, this fails rather than
/// quietly changing what it measures.
///
/// The comparison target is the fixture on disk. A stored copy of each expected
/// output would be byte-identical to its own input and would read as writer
/// evidence while carrying none.
#[test]
fn fixtures_replay_verbatim() {
    for (name, bytes) in harness().fixture_inputs() {
        verbatim_replay_holds(&SldprtCodec, &name, &bytes);
    }
}

/// The semantic write path either reproduces a fixture's document or declares
/// the edit it cannot make.
///
/// [`cadmpeg_ir::roundtrip::semantic_roundtrip`] removes the
/// `document_local_sha256` baseline first, so the encoder cannot show the
/// retained image is still current and must run the writer.
///
/// Two outcomes are contract-conforming and both are asserted. A write must
/// decode back to the same census — see [`arena_census`] for what a rewritten
/// container can and cannot be held to. A byte comparison is not the contract
/// here: the writer repacks a compound file, and a stored byte golden would pin
/// one packing forever.
///
/// A refusal must be `NotImplemented`, which names a capability this codec has
/// not built. Any other refusal means the writer misread or corrupted records it
/// retained itself, which is a defect rather than a gap. Which fixtures land in
/// which arm is not pinned here; pinning it would freeze today's gaps as the
/// specification.
#[test]
fn fixtures_survive_the_semantic_write_path() {
    let mut written_count = 0usize;
    for (name, bytes) in harness().fixture_inputs() {
        let expected = arena_census(&bytes);
        semantic_roundtrip(&SldprtCodec, &name, &bytes, |outcome| match outcome {
            SemanticOutcome::Written { report, bytes, .. } => {
                written_count += 1;
                assert_eq!(
                    report.write_path,
                    WritePath::Patched,
                    "fixture `{name}`: retained records fed the write, so it patched rather than synthesized"
                );
                if let Err(mismatch) = snapshots_agree(&expected, &arena_census(bytes)) {
                    panic!("fixture `{name}`: the semantically written document decodes to a different census: {mismatch}");
                }
            }
            SemanticOutcome::Refused { error } => {
                assert!(
                matches!(error, CodecError::NotImplemented(_)),
                "fixture `{name}`: the semantic writer may decline an edit it has not built, but this \
                 refusal reports a defect in what it already retains: {error}"
            );
            }
        });
    }
    assert!(
        written_count > 0,
        "no fixture reached the semantic writer, so this test exercised nothing; \
         add a fixture the writer can write, or this lane is dead"
    );
}
