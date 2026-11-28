// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion saves of circular patterns: each Compute option, and
//! suppressed instances.

use cadmpeg_core::serde_erase::json;
use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

/// The IR as JSON and the loss codes of a corpus file, after checking that
/// the neutral model validates.
fn decoded(bytes: &[u8]) -> (serde_json::Value, Vec<String>) {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    let mut codes = Vec::new();
    loss_codes(
        &json::to_value(&result.report().losses).expect("serialize the losses"),
        &mut codes,
    );
    assert!(!codes.is_empty(), "the report names loss codes");
    (
        cadmpeg_test_support::wire::in_memory_json(result.ir()),
        codes,
    )
}

/// Every `code` string in a serialized loss list.
fn loss_codes(value: &serde_json::Value, codes: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                match value.as_str() {
                    Some(code) if key == "code" => codes.push(code.to_owned()),
                    _ => loss_codes(value, codes),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                loss_codes(item, codes);
            }
        }
        _ => {}
    }
}

/// The symmetric byte of every circular-pattern payload.
fn payload_symmetry(ir: &serde_json::Value) -> Vec<bool> {
    ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .filter_map(|scope| scope["circular_pattern_construction"]["symmetric"].as_object())
        .map(|symmetric| symmetric["value"] == true)
        .collect()
}

#[test]
fn every_compute_option_of_a_circular_pattern_reads_its_payload() {
    let files: [(&[u8], f64, bool); 3] = [
        (
            include_bytes!("../../../../corpus/f3d/circular_compute_optimized.f3d"),
            std::f64::consts::TAU,
            false,
        ),
        (
            include_bytes!("../../../../corpus/f3d/circular_compute_identical.f3d"),
            std::f64::consts::TAU,
            false,
        ),
        (
            include_bytes!("../../../../corpus/f3d/circular_symmetric_optimized.f3d"),
            std::f64::consts::FRAC_PI_2,
            true,
        ),
    ];
    for (bytes, angle, symmetric) in files {
        let (ir, codes) = decoded(bytes);
        assert_eq!(payload_symmetry(&ir), [symmetric]);
        assert!(
            !codes
                .iter()
                .any(|code| code == "feature.circular-pattern-distribution-assumed"),
            "{codes:?}"
        );
        // The neutral circular pattern has no symmetric field.
        assert_eq!(
            codes
                .iter()
                .any(|code| code == "feature.circular-pattern-placement-not-transferred"),
            symmetric,
            "{codes:?}"
        );
        let pattern = ir["model"]["features"]
            .as_array()
            .expect("feature list")
            .iter()
            .find(|feature| feature["name"] == "C-Pattern1")
            .expect("C-Pattern1");
        let pattern = &pattern["definition"]["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "circular", "{pattern}");
        assert_eq!(pattern["count"], 4);
        assert_eq!(pattern["angle"], angle);
    }
}

/// The pattern of the feature named `C-Pattern1`.
fn circular_pattern(ir: &serde_json::Value) -> &serde_json::Value {
    let feature = ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .find(|feature| feature["name"] == "C-Pattern1")
        .expect("C-Pattern1");
    &feature["definition"]["value"]["value"]["pattern"]
}

#[test]
fn a_one_sided_circular_pattern_leaves_out_its_suppressed_instances() {
    // The instances at 180 and 300 degrees of six over a full turn are
    // suppressed.
    let (ir, _) = decoded(include_bytes!(
        "../../../../corpus/f3d/circular_suppressed.f3d"
    ));
    let pattern = circular_pattern(&ir);
    assert_eq!(pattern["kind"], "circular_angles", "{pattern}");
    assert_eq!(
        pattern["axis_dir"],
        cadmpeg_core::json!({"x": 0.0, "y": 0.0, "z": 1.0})
    );
    let angles = pattern["angles"]
        .as_array()
        .expect("angles")
        .iter()
        .filter_map(serde_json::Value::as_f64)
        .map(f64::to_degrees)
        .collect::<Vec<_>>();
    assert_eq!(angles.len(), 4, "{angles:?}");
    for (angle, expected) in angles.iter().zip([0.0, 60.0, 120.0, 240.0]) {
        assert!((angle - expected).abs() < 1e-9, "{angles:?}");
    }
}

#[test]
fn a_symmetric_circular_pattern_keeps_and_charges_its_suppressed_instances() {
    let (ir, codes) = decoded(include_bytes!(
        "../../../../corpus/f3d/circular_suppressed_symmetric.f3d"
    ));
    let suppressed = ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .find_map(|scope| scope["circular_pattern_construction"]["suppressed"]["value"].as_array())
        .expect("suppressed keys");
    assert_eq!(
        suppressed,
        &[cadmpeg_core::json!(2), cadmpeg_core::json!(5)]
    );
    let pattern = circular_pattern(&ir);
    assert_eq!(pattern["kind"], "circular", "{pattern}");
    assert_eq!(pattern["count"], 5, "{pattern}");
    for code in [
        "feature.circular-pattern-placement-not-transferred",
        "feature.circular-pattern-suppression-not-transferred",
    ] {
        assert!(codes.iter().any(|found| found == code), "{codes:?}");
    }
}
