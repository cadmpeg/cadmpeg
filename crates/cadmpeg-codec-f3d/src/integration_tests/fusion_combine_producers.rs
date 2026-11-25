// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of a Join Combine whose target a later Fillet
//! updates, and of the same Combine without the Fillet.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

/// The neutral model of a corpus file as JSON and its decode losses.
fn decoded(bytes: &[u8]) -> (serde_json::Value, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    (
        cadmpeg_test_support::wire::in_memory_json(result.ir()),
        result.report().losses.clone(),
    )
}

/// The features of the model, in order.
fn features(ir: &serde_json::Value) -> &[serde_json::Value] {
    ir["model"]["features"].as_array().expect("feature list")
}

/// The one feature whose definition carries Combine operands.
fn combine(ir: &serde_json::Value) -> &serde_json::Value {
    let mut combines = features(ir)
        .iter()
        .filter(|feature| !feature["definition"]["value"]["value"]["operands"].is_null());
    let combine = combines.next().expect("a Combine");
    assert!(combines.next().is_none(), "one Combine");
    combine
}

/// The ids of the features that a generated body selection names.
fn generated_features(selection: &serde_json::Value) -> Vec<String> {
    assert_eq!(selection["kind"], "generated", "{selection}");
    selection["value"]["bodies"]
        .as_array()
        .expect("generated bodies")
        .iter()
        .map(|body| body["feature"].as_str().expect("feature id").to_owned())
        .collect()
}

/// The id of the feature named `name`.
fn feature_id(ir: &serde_json::Value, name: &str) -> String {
    let mut named = features(ir).iter().filter(|feature| feature["name"] == name);
    let feature = named.next().unwrap_or_else(|| panic!("feature {name}"));
    assert!(named.next().is_none(), "one feature named {name}");
    feature["id"].as_str().expect("feature id").to_owned()
}

fn producer_unresolved(losses: &[cadmpeg_ir::report::loss::LossNote]) -> usize {
    let code = crate::loss::F3dLossCode::FeatureCombineBodyProducerUnresolved.kind();
    losses.iter().filter(|loss| loss.code == code).count()
}

#[test]
fn a_join_target_that_a_later_fillet_updates_is_generated_by_its_extrude() {
    let (ir, losses) = decoded(include_bytes!(
        "../../../../corpus/f3d/combine_join_then_fillet.f3d"
    ));
    let operands = &combine(&ir)["definition"]["value"]["value"]["operands"];
    let target = generated_features(&operands["target"]);
    let tools = generated_features(&operands["tools"]);
    assert_eq!(target, [feature_id(&ir, "Extrude 1")], "{operands}");
    assert_eq!(tools, [feature_id(&ir, "Extrude 2")], "{operands}");
    assert_eq!(producer_unresolved(&losses), 0);
}

#[test]
fn a_join_target_that_no_later_feature_updates_selects_its_model_body() {
    let (ir, losses) = decoded(include_bytes!(
        "../../../../corpus/f3d/combine_join_control.f3d"
    ));
    let operands = &combine(&ir)["definition"]["value"]["value"]["operands"];
    let bodies = ir["geometry"]["bodies"].as_array().expect("bodies");
    assert_eq!(bodies.len(), 1);
    assert_eq!(operands["target"]["kind"], "resolved", "{operands}");
    assert_eq!(
        operands["target"]["value"]["bodies"],
        serde_json::json!([bodies[0]["id"]])
    );
    assert_eq!(
        generated_features(&operands["tools"]),
        [feature_id(&ir, "Extrude 2")]
    );
    assert_eq!(producer_unresolved(&losses), 0);
}
