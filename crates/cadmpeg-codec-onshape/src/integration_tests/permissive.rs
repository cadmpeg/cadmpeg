// SPDX-License-Identifier: Apache-2.0
//! One malformed record is a named loss and the rest of the file decodes.
use crate::test_support::fixture;
use crate::test_support::review::{decode, edit, wire};
use serde_json::Value;

fn feature_count(wire: &Value) -> usize {
    wire["model"]["features"].as_array().unwrap().len()
}

fn malformed_losses(result: &cadmpeg_ir::codec::DecodeResult) -> usize {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "onshape/record.malformed")
        .count()
}

#[test]
fn a_feature_without_an_id_is_omitted_with_one_named_loss() {
    let whole = wire(&decode(&fixture("plate-fillet")));
    let total = feature_count(&whole);
    assert!(total >= 3);
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if path.ends_with("features.json") {
            let last = response["features"].as_array().unwrap().len() - 1;
            response["features"][last]
                .as_object_mut()
                .unwrap()
                .remove("featureId");
        }
    });
    let result = decode(&bytes);
    assert_eq!(malformed_losses(&result), 1, "{:?}", result.report().losses);
    assert_eq!(feature_count(&wire(&result)), total - 1);
}

#[test]
fn a_repeated_feature_id_omits_the_repeat_and_keeps_the_rest() {
    let whole = wire(&decode(&fixture("plate-fillet")));
    let total = feature_count(&whole);
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if path.ends_with("features.json") {
            let first = response["features"][0]["featureId"].clone();
            let last = response["features"].as_array().unwrap().len() - 1;
            response["features"][last]["featureId"] = first;
        }
    });
    let result = decode(&bytes);
    assert_eq!(malformed_losses(&result), 1, "{:?}", result.report().losses);
    assert_eq!(feature_count(&wire(&result)), total - 1);
}

#[test]
fn a_missing_rollback_index_keeps_the_features_with_one_named_loss() {
    let whole = wire(&decode(&fixture("plate-fillet")));
    let total = feature_count(&whole);
    let bytes = crate::test_support::review::members(&fixture("plate-fillet"), |members| {
        members.retain(|name, _| !name.ends_with("bodydetails/index.json"));
    });
    let result = decode(&bytes);
    assert_eq!(malformed_losses(&result), 1, "{:?}", result.report().losses);
    assert_eq!(feature_count(&wire(&result)), total);
}
