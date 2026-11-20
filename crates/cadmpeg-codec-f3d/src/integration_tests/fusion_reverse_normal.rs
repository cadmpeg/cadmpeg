// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of surface Revolves whose bodies a Reverse Normal selects.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};
use serde_json::{json, Value};

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
            == crate::loss::F3dLossCode::FeatureReverseNormalBodyProducerUnresolved.kind()),
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
