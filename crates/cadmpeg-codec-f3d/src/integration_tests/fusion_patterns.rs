// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of rectangular patterns. Fusion reported each copy's
//! translation.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};
use serde_json::json;

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
    serde_json::to_value(result.ir()).expect("serialize the IR")
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

/// The symmetric byte of every rectangular-pattern distribution block.
fn distribution_symmetry(ir: &serde_json::Value) -> Vec<bool> {
    ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .filter_map(|scope| scope["rectangular_pattern_construction"]["distribution"].as_object())
        .map(|distribution| distribution["symmetric"]["value"] == true)
        .collect()
}

#[test]
fn a_symmetric_pattern_with_a_count_one_second_direction_reads_its_spacing() {
    // Fusion places the four copies at x = -5, 0, +5 and +10 mm. Direction two
    // has quantity 1 and keeps its 5 mm distance.
    let files: [&[u8]; 2] = [
        include_bytes!("../../../../corpus/f3d/symmetric_pattern.f3d"),
        include_bytes!("../../../../corpus/f3d/symmetric_pattern_body.f3d"),
    ];
    for bytes in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        assert_eq!(pattern["direction"], json!({"x": 1.0, "y": 0.0, "z": 0.0}));
        assert_eq!(pattern["spacing"], 5.0);
        assert_eq!(pattern["count"], 4);
        assert_eq!(pattern["symmetric"], true);
        assert_eq!(distribution_symmetry(&ir), [true]);
    }
}

#[test]
fn the_compute_option_leaves_a_five_copy_pattern_unchanged() {
    // Fusion translates the copies by 0, 2, 4, 6 and 8 mm in all three saves.
    let files: [&[u8]; 3] = [
        include_bytes!("../../../../corpus/f3d/compute_adjust.f3d"),
        include_bytes!("../../../../corpus/f3d/compute_optimized.f3d"),
        include_bytes!("../../../../corpus/f3d/compute_identical.f3d"),
    ];
    for bytes in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "Groove pattern")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        assert_eq!(pattern["spacing"], 2.0);
        assert_eq!(pattern["count"], 5);
    }
}
