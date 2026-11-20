// SPDX-License-Identifier: Apache-2.0
//! Reverse Normal of surface Revolve bodies, and over surface bodies of one
//! component and of several components.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};
use serde_json::{json, Value};

use crate::loss::F3dLossCode;
use crate::F3dCodec;

/// The decoded IR of a corpus file as JSON, after checking that the neutral
/// model validates.
fn decoded(bytes: &[u8]) -> Value {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(
        !result.report().losses.iter().any(|loss| loss.code
            == F3dLossCode::FeatureReverseNormalBodyProducerUnresolved.kind()),
        "{:#?}",
        result.report().losses
    );
    serde_json::to_value(result.ir()).expect("serialize the IR")
}

fn features<'a>(ir: &'a Value, tag: &str) -> Vec<&'a Value> {
    ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["source_tag"] == tag)
        .collect()
}

/// Checks that the one Reverse Normal selects the output bodies of the
/// Revolves, in Revolve order, and depends on every Revolve.
fn assert_reverse_selects_revolve_bodies(ir: &Value, revolve_count: usize) {
    let revolves = features(ir, "Revolve");
    assert_eq!(revolves.len(), revolve_count);
    let [reverse] = features(ir, "Reverse Normal")[..] else {
        panic!("one Reverse Normal");
    };
    let operation = &reverse["definition"]["value"];
    assert_eq!(operation["kind"], "reverse_shape");
    let source = &operation["value"]["source"];
    assert_eq!(source["kind"], "resolved", "{source:#}");
    let produced = revolves
        .iter()
        .flat_map(|revolve| revolve["outputs"].as_array().expect("outputs").clone())
        .collect::<Vec<_>>();
    assert_eq!(produced.len(), revolve_count);
    assert_eq!(source["value"]["bodies"], json!(produced));
    let dependencies = reverse["dependencies"].as_array().expect("dependencies");
    for revolve in revolves {
        assert!(dependencies.contains(&revolve["id"]), "{dependencies:#?}");
    }
}

#[test]
fn a_reverse_normal_of_one_revolved_body_selects_that_body() {
    // Fusion: Revolve1 of a closed rectangle as a surface body, then Reverse
    // Normal1 of Body1. One surface body of four faces.
    let ir = decoded(include_bytes!(
        "../../../../corpus/f3d/reverse_normal_revolve.f3d"
    ));
    assert_reverse_selects_revolve_bodies(&ir, 1);
}

#[test]
fn a_reverse_normal_of_two_revolved_bodies_selects_both_bodies() {
    // Fusion: Revolve1 and Revolve2 each make one surface body, then Reverse
    // Normal1 of Body1 and Body2. Two surface bodies of eight faces.
    let ir = decoded(include_bytes!(
        "../../../../corpus/f3d/reverse_normal_two_revolves.f3d"
    ));
    assert_reverse_selects_revolve_bodies(&ir, 2);
}

/// The IR of a corpus file as JSON, and whether the report charges
/// `history.body-recipe-history-inferred`, after checking that the neutral
/// model validates.
fn decoded_with_inferred_history(bytes: &[u8]) -> (Value, bool) {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    let inferred = result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::HistoricalBodyRecipeHistoryInferred.kind());
    (
        serde_json::to_value(result.ir()).expect("serialize the IR"),
        inferred,
    )
}

/// The features named `Reverse Normal 1`, in timeline order.
fn reverse_normals(ir: &Value) -> Vec<&Value> {
    let mut features = ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["name"] == "Reverse Normal 1")
        .collect::<Vec<_>>();
    features.sort_by_key(|feature| feature["ordinal"].as_u64());
    features
}

/// The model bodies that a Reverse Normal feature reverses.
fn reversed_bodies(feature: &Value) -> Vec<&str> {
    let definition = &feature["definition"]["value"];
    assert_eq!(definition["kind"], "reverse_shape", "{definition}");
    let source = &definition["value"]["source"];
    assert_eq!(source["kind"], "resolved", "{source}");
    source["value"]["bodies"]
        .as_array()
        .expect("resolved bodies")
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

#[test]
fn a_reverse_normal_of_one_component_reverses_both_of_its_bodies() {
    let (ir, inferred) = decoded_with_inferred_history(include_bytes!(
        "../../../../corpus/f3d/reverse_normal_two_bodies.f3d"
    ));
    let operations = ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .map(|scope| &scope["reverse_normal_operation"])
        .filter(|operation| !operation.is_null())
        .collect::<Vec<_>>();
    assert!(!inferred);
    assert_eq!(operations.len(), 1);
    assert!(operations[0]["link"].is_null(), "{}", operations[0]);
    let features = reverse_normals(&ir);
    assert_eq!(features.len(), 1);
    assert_eq!(reversed_bodies(features[0]).len(), 2);
}

#[test]
fn each_linked_scope_of_a_two_component_reverse_normal_reverses_its_own_body() {
    let (ir, inferred) = decoded_with_inferred_history(include_bytes!(
        "../../../../corpus/f3d/reverse_normal_two_components.f3d"
    ));
    assert!(inferred);
    let linked = ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .filter(|scope| scope["reverse_normal_operation"]["link"].is_object())
        .collect::<Vec<_>>();
    let links = linked
        .iter()
        .filter_map(|scope| scope["reverse_normal_operation"]["link"]["value"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0], links[1]);
    for field in ["history_state_id", "previous_history_state_id"] {
        assert!(linked[0][field].is_i64(), "{field}");
        assert_eq!(linked[0][field], linked[1][field], "{field}");
    }
    assert_ne!(links[0], "00000000-0000-0000-0000-000000000000");
    let features = reverse_normals(&ir);
    assert_eq!(features.len(), 2);
    assert_eq!(
        features[1]["ordinal"].as_u64(),
        features[0]["ordinal"].as_u64().map(|ordinal| ordinal + 1)
    );
    assert_ne!(
        features[0]["product_definition"],
        features[1]["product_definition"]
    );
    let model_bodies = ir["geometry"]["bodies"]
        .as_array()
        .expect("bodies")
        .iter()
        .filter_map(|body| body["id"].as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut reversed = std::collections::HashSet::new();
    for feature in features {
        let bodies = reversed_bodies(feature);
        assert_eq!(bodies.len(), 1, "{feature}");
        // Each linked scope reverses a different model body.
        assert!(model_bodies.contains(bodies[0]), "{bodies:?}");
        assert!(reversed.insert(bodies[0]), "{bodies:?}");
    }
}

#[test]
fn each_component_of_a_linked_reverse_normal_reverses_its_own_bodies() {
    // Two bodies in the first component and one in the second; three
    // components with one body each.
    for (bytes, counts) in [
        (
            &include_bytes!("../../../../corpus/f3d/reverse_normal_two_plus_one.f3d")[..],
            &[2, 1][..],
        ),
        (
            &include_bytes!("../../../../corpus/f3d/reverse_normal_three_components.f3d")[..],
            &[1, 1, 1],
        ),
    ] {
        let (ir, _) = decoded_with_inferred_history(bytes);
        let linked = ir["native"]["f3d"]["design_parameter_scopes"]
            .as_array()
            .expect("scopes")
            .iter()
            .filter(|scope| scope["reverse_normal_operation"]["link"].is_object())
            .collect::<Vec<_>>();
        assert_eq!(linked.len(), counts.len());
        for scope in &linked[1..] {
            assert_eq!(
                scope["reverse_normal_operation"]["link"]["value"],
                linked[0]["reverse_normal_operation"]["link"]["value"]
            );
        }
        let features = reverse_normals(&ir);
        assert_eq!(
            features
                .iter()
                .map(|feature| reversed_bodies(feature).len())
                .collect::<Vec<_>>(),
            counts
        );
        let components = features
            .iter()
            .map(|feature| feature["product_definition"].as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(components.len(), counts.len());
        let first = features[0]["ordinal"].as_u64().expect("ordinal");
        for (at, feature) in features.iter().enumerate() {
            assert_eq!(
                feature["ordinal"].as_u64(),
                Some(first + u64::try_from(at).unwrap())
            );
        }
    }
}

#[test]
fn the_linked_scopes_of_a_two_plus_one_reverse_normal_charge_the_inferred_history() {
    let (_, inferred) = decoded_with_inferred_history(include_bytes!(
        "../../../../corpus/f3d/reverse_normal_two_plus_one.f3d"
    ));
    assert!(inferred);
}

#[test]
fn each_linked_scope_after_another_follower_charges_the_assumed_order() {
    for (bytes, count) in [
        (
            &include_bytes!("../../../../corpus/f3d/reverse_normal_two_plus_one.f3d")[..],
            0,
        ),
        (
            &include_bytes!("../../../../corpus/f3d/reverse_normal_three_components.f3d")[..],
            2,
        ),
    ] {
        let result = F3dCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode the corpus file");
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code == F3dLossCode::FeatureReverseNormalLinkOrderAssumed.kind())
                .count(),
            count
        );
    }
}
