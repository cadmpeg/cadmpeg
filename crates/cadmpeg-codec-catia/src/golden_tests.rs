// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect` and `decode` over the committed
//! fixtures.
//!
//! `tests/golden/fixtures/*.catpart` are the frozen inputs.
//! Fixtures stay frozen; `UPDATE_GOLDEN=1` rewrites goldens only.
//! `inspect` pins the container summary; `decode` pins the IR, losses, and
//! source fidelity. Shared harness: [`cadmpeg_test_support::golden`].

use cadmpeg_test_support::EditableDecodeResult;

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};

use super::CatiaCodec;

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "catpart";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-catia golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
}

fn inputs() -> Vec<(String, Vec<u8>)> {
    let mut inputs = harness().fixture_inputs();
    let mut body = crate::test_support::test_container::summary_preview_segment();
    body.extend(crate::test_support::test_container::external_reference_segment("Support.CATPart"));
    inputs.push((
        "external_preview".into(),
        crate::test_support::test_container::outer_body_catpart(&body),
    ));
    inputs
}

/// The branches this codec pins, in golden-directory order.
fn branches() -> [Branch; 2] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
    ]
}

/// Serializes one container summary. An inspect error is frozen too: refusing a
/// container is contract-relevant behavior, so this never panics on codec
/// output.
fn inspect_snapshot(bytes: &[u8]) -> String {
    let value =
        match CatiaCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
            Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// Serializes one decoded document: the IR, the decode report, and source
/// fidelity. A decode error is frozen too: refusing a document is
/// contract-relevant behavior, so this never panics on codec output.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match CatiaCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
    {
        Ok(result) => {
            let result = EditableDecodeResult::from(result);
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

/// Every committed golden still matches what the codec produces.
#[test]
fn golden_snapshots_hold() {
    harness().check_inputs(&inputs(), &branches());
}

/// Putting the same bytes through a branch twice produces identical text.
#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism_inputs(&inputs(), &branches());
}

/// A geometry-depth decode agrees with the full decode outside the design
/// arenas. This codec has no geometry-depth cut, so every input delivers full.
#[test]
fn geometry_depth_agrees_with_full_depth() {
    let summary = cadmpeg_test_support::depth::assert_geometry_agrees_with_full_with(
        &CatiaCodec,
        &inputs(),
        |result| {
            // #209 §1.3: B1 is concrete retained source state, independent of losses.
            let fidelity = result.source_fidelity();
            let expected: cadmpeg_ir::ArenaClasses = fidelity
                .retained_records()
                .values()
                .flat_map(|record| record.untyped().iter().copied())
                .collect();
            assert_eq!(fidelity.unrepresented(), expected);
            let restored: cadmpeg_ir::SourceFidelity = serde_json::from_value(
                serde_json::to_value(fidelity).expect("fixture value is valid"),
            )
            .expect("fixture value is valid");
            assert_eq!(restored.retained_records(), fidelity.retained_records());
            assert_eq!(restored.unrepresented(), fidelity.unrepresented());
        },
    );
    // The external-preview source has model content and no design part (§1.1).
    assert_eq!(summary.cut, 1, "{summary:?}");
}

#[test]
fn independently_decoded_preview_has_a_neutral_asset_home() {
    let (_, bytes) = inputs()
        .into_iter()
        .find(|(name, _)| name == "external_preview")
        .expect("fixture value is valid");
    let result = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("authored preview document decodes");
    assert!(result.ir().model.assets.iter().any(|asset| asset
        .media_type
        .as_ref()
        .map(cadmpeg_core::text::NonBlankString::as_str)
        == Some("image/jpeg")));
    assert!(result
        .ir()
        .native
        .namespace("catia")
        .expect("fixture value is valid")
        .arenas()
        .get("external_references")
        .is_some_and(|records| !records.is_empty()));
    assert!(result.ir().scope.model);
    assert!(!result.ir().scope.design);
}
