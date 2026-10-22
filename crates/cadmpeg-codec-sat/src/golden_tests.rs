// SPDX-License-Identifier: Apache-2.0
//! Golden inspect and decode snapshots over code-built SAT streams.
//!
//! Inputs come from [`crate::test_support`] builders. Shared harness:
//! [`cadmpeg_test_support::golden`]. `UPDATE_GOLDEN=1` rewrites goldens only.

use cadmpeg_test_support::EditableDecodeResult;

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};

use crate::test_support::test_streams::BinaryFixtureKind;
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
            "attributed_sphere".to_string(),
            crate::test_support::test_streams::attributed_sphere_stream(),
        ),
        (
            "text_sphere".to_string(),
            crate::test_support::test_streams::text_sphere_stream(1.0),
        ),
        (
            "binary_asm".to_string(),
            crate::test_support::test_streams::binary_sphere_stream(BinaryFixtureKind::Asm),
        ),
        (
            "binary_acis".to_string(),
            crate::test_support::test_streams::binary_sphere_stream(BinaryFixtureKind::Acis),
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
    let value = match cadmpeg_test_support::golden::decode(
        &SatCodec,
        &mut Cursor::new(bytes),
        &DecodeOptions::default(),
    ) {
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

#[test]
fn golden_snapshots_hold() {
    harness().check_inputs(&inputs(), &branches());
}

#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism_inputs(&inputs(), &branches());
}

/// A geometry-depth decode agrees with the full decode outside the design
/// arenas. The decoder derives the delivered scope from the request (§1.1):
/// a geometry request excludes design, so every geometry decode is a cut.
#[test]
fn geometry_depth_agrees_with_full_depth() {
    let summary = cadmpeg_test_support::depth::assert_geometry_agrees_with_full_with(
        &SatCodec,
        &inputs(),
        |result| {
            let expected = crate::loss::unrepresented(
                &result.report().losses,
                result.ir().native.namespace("sat"),
            );
            assert!(expected.iter().all(|class| result.source_fidelity().unrepresented().contains(class)), "every source-only loss must have retained source evidence carrying its arena class");
        },
    );
    assert_eq!(summary.cut, summary.inputs, "{summary:?}");
}

/// Attributes have a neutral home; direct ASM colors remain display colors.
#[test]
fn asm_attributes_and_display_color_have_distinct_neutral_homes() {
    let bytes = crate::test_support::test_streams::attributed_sphere_stream();
    let result = cadmpeg_test_support::golden::decode(
        &SatCodec,
        &mut Cursor::new(bytes),
        &DecodeOptions::default(),
    )
    .expect("attributed sphere decodes");
    assert!(!result.ir().model.attributes.is_empty());
    assert!(result.ir().model.appearances.is_empty());
    assert!(result.ir().model.appearance_bindings.is_empty());
    assert!(result
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.color.is_some()));
}

/// Check 8 (#246): no native row of a decoded golden holds the id of a
/// neutral record, and every neutral `native_ref` names an actual native row.
#[test]
fn goldens_hold_no_native_back_links() {
    cadmpeg_test_support::native_linkage::assert_golden_native_linkage(&SatCodec, &inputs());
}
