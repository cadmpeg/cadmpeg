// SPDX-License-Identifier: Apache-2.0
//! Golden inspect and decode snapshots over field-built CFB declarations.
//!
//! Inputs are constructed in code by [`crate::test_support::test_fixtures::fixture`] and
//! [`crate::test_support::test_fixtures::primary_envelope_fixture`]. Shared harness:
//! [`cadmpeg_test_support::golden`]. `UPDATE_GOLDEN=1` rewrites goldens only.

use cadmpeg_test_support::EditableDecodeResult;

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
        (
            "model_states".to_string(),
            crate::test_support::test_fixtures::model_state_envelope_fixture(),
        ),
        (
            "structural".to_string(),
            crate::test_support::test_fixtures::fixture(true),
        ),
        (
            "primary".to_string(),
            crate::test_support::test_fixtures::primary_envelope_fixture(),
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

#[test]
fn primary_golden_snapshots_hold() {
    harness().check_inputs(&inputs(), &branches());
}

#[test]
fn primary_golden_output_is_deterministic() {
    harness().check_determinism_inputs(&inputs(), &branches());
}

/// A geometry-depth decode agrees with the full decode outside the design
/// arenas. This codec has no geometry-depth cut, so every input delivers full.
#[test]
fn geometry_depth_agrees_with_full_depth() {
    let summary = cadmpeg_test_support::depth::assert_geometry_agrees_with_full_with(
        &InventorCodec,
        &inputs(),
        |result| {
            let expected = crate::loss::unrepresented(
                &result.report().losses,
                result.ir().native.namespace("inventor"),
            );
            assert!(expected.iter().all(|class| result.source_fidelity().unrepresented().contains(class)), "every source-only loss must have retained source evidence carrying its arena class");
        },
    );
    assert_eq!(summary.cut, 0, "{summary:?}");
}

#[test]
fn source_model_state_has_a_neutral_configuration() {
    let result = InventorCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_fixtures::model_state_envelope_fixture()),
            &DecodeOptions::default(),
        )
        .expect("model-state document decodes");
    let configuration = result
        .ir()
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Master"))
        .expect("typed model state projects");
    assert!(configuration.active);
    assert!(configuration.parameter_overrides.is_empty());
    assert!(configuration.parameter_values.is_empty());
    assert!(configuration.feature_states.is_empty());
    assert!(configuration.native_ref.is_some());

    use cadmpeg_ir::codec::write::{target::TargetRequest, CadirEncoder, EncodeInput, Encoder};
    let mut encoded = Vec::new();
    CadirEncoder
        .plan(
            EncodeInput::new(result.ir(), Some(result.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("the projected model state exports to CADIR")
        .write_to(&mut encoded)
        .expect("CADIR configuration bytes write");
    let restored = cadmpeg_ir::CadIr::from_json(std::str::from_utf8(&encoded).unwrap())
        .expect("CADIR parses independently of a decode sidecar");
    assert!(restored.scope.design);
    assert_eq!(
        restored.model.configurations,
        result.ir().model.configurations
    );
    assert_eq!(restored.model.parameters, result.ir().model.parameters);
}
