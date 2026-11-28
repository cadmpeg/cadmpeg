// SPDX-License-Identifier: Apache-2.0
//! A CC0 Fusion API save that mirrors a body and removes the original.

use std::io::Cursor;

use cadmpeg_core::json;
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

/// The decoded IR of a corpus file as JSON, after checking that the neutral
/// model validates.
fn decoded(bytes: &[u8]) -> serde_json::Value {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    cadmpeg_test_support::wire::in_memory_json(result.ir())
}

/// The definition of the feature named `name`.
fn definition<'a>(ir: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    let mut features = ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["name"] == name);
    let feature = features.next().unwrap_or_else(|| panic!("feature {name}"));
    assert!(features.next().is_none(), "one feature named {name}");
    &feature["definition"]
}

#[test]
fn a_mirrored_then_removed_body_is_generated_by_the_extrude_that_made_it() {
    // Fusion: Extrude1, a cut, Mirror1 of the body across YZ as a new body,
    // then RemoveBody of the original. The final model holds the copy.
    let ir = decoded(include_bytes!("../../../../corpus/f3d/mirror_remove.f3d"));
    assert_eq!(ir["geometry"]["bodies"].as_array().map(Vec::len), Some(1));
    let extrude = ir["model"]["features"]
        .as_array()
        .expect("features")
        .iter()
        .find(|feature| feature["name"] == "Extrude1")
        .expect("Extrude1")["id"]
        .clone();
    let generated =
        json!({"kind": "generated", "value": {"bodies": [{"feature": extrude, "local_id": "1"}]}});

    let mirror = &definition(&ir, "Mirror1")["value"]["value"];
    assert_eq!(mirror["pattern"]["kind"], "mirror");
    assert_eq!(
        mirror["pattern"]["plane_origin"],
        json!({"x": 0.0, "y": 0.0, "z": 0.0})
    );
    assert_eq!(
        mirror["pattern"]["plane_normal"],
        json!({"x": 1.0, "y": 0.0, "z": 0.0})
    );
    let seed = &mirror["seeds"][0]["value"];
    assert_eq!(seed["kind"], generated["kind"]);
    assert_eq!(seed["value"]["bodies"], generated["value"]["bodies"]);

    let removed = &definition(&ir, "RemoveBody-Body1")["value"]["value"]["bodies"];
    assert_eq!(removed["kind"], generated["kind"]);
    assert_eq!(removed["value"]["bodies"], generated["value"]["bodies"]);
}
