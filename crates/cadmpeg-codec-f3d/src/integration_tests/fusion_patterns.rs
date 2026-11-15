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

/// The Compute option of every rectangular-pattern distribution block.
fn distribution_compute(ir: &serde_json::Value) -> Vec<u64> {
    ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .filter_map(|scope| scope["rectangular_pattern_construction"]["distribution"].as_object())
        .filter_map(|distribution| distribution["compute_option"]["value"].as_u64())
        .collect()
}

#[test]
fn the_compute_option_leaves_a_five_copy_pattern_unchanged() {
    // Fusion translates the copies by 0, 2, 4, 6 and 8 mm in all three saves.
    let files: [(&[u8], u64); 3] = [
        (
            include_bytes!("../../../../corpus/f3d/compute_adjust.f3d"),
            2,
        ),
        (
            include_bytes!("../../../../corpus/f3d/compute_optimized.f3d"),
            0,
        ),
        (
            include_bytes!("../../../../corpus/f3d/compute_identical.f3d"),
            1,
        ),
    ];
    for (bytes, compute) in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "Groove pattern")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        assert_eq!(pattern["spacing"], 2.0);
        assert_eq!(pattern["count"], 5);
        assert_eq!(distribution_compute(&ir), [compute]);
    }
}

#[test]
fn an_optimized_pattern_reads_its_extent_and_symmetric_distribution() {
    // Fusion translates the Extent copies by 0, 10 and 20 mm, and the
    // symmetric copies by -10, 0 and +10 mm.
    let extent = decoded(include_bytes!(
        "../../../../corpus/f3d/optimized_extent_pattern.f3d"
    ));
    let pattern = &definition(&extent, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["kind"], "linear", "{pattern}");
    assert_eq!(pattern["direction"], json!({"x": 1.0, "y": 0.0, "z": 0.0}));
    assert_eq!(pattern["spacing"], 10.0);
    assert_eq!(pattern["count"], 3);
    assert_eq!(pattern["symmetric"], false);
    assert_eq!(distribution_compute(&extent), [0]);

    let symmetric = decoded(include_bytes!(
        "../../../../corpus/f3d/optimized_symmetric_pattern.f3d"
    ));
    let pattern = &definition(&symmetric, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["spacing"], 10.0);
    assert_eq!(pattern["count"], 3);
    assert_eq!(pattern["symmetric"], true);
    assert_eq!(distribution_symmetry(&symmetric), [true]);
    assert_eq!(distribution_compute(&symmetric), [0]);
}

#[test]
fn a_pattern_of_a_cut_through_two_bodies_reads_its_distribution() {
    // Fusion translates the copies by 0, 10 and 20 mm along Y.
    let ir = decoded(include_bytes!(
        "../../../../corpus/f3d/two_target_pattern.f3d"
    ));
    let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["kind"], "linear", "{pattern}");
    assert_eq!(pattern["direction"], json!({"x": 0.0, "y": 1.0, "z": 0.0}));
    assert_eq!(pattern["spacing"], 10.0);
    assert_eq!(pattern["count"], 3);
    assert_eq!(distribution_compute(&ir), [2]);
}

#[test]
fn each_direction_of_a_rectangular_pattern_reads_its_own_symmetric_byte() {
    // Fusion repeats one slot along +x at 5 mm and along +y at 10 mm. A
    // symmetric direction of count 3 places one copy on each side of the seed;
    // count 4 places two copies along +y and one against it.
    let files: [(&[u8], u64, u64, bool, bool); 4] = [
        (
            include_bytes!("../../../../corpus/f3d/sym_q3_dir1only.f3d"),
            3,
            3,
            true,
            false,
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_q3_dir2only.f3d"),
            3,
            3,
            false,
            true,
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_q3_both.f3d"),
            3,
            3,
            true,
            true,
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_q4_dir2only.f3d"),
            2,
            4,
            false,
            true,
        ),
    ];
    for (bytes, count, second_count, symmetric, second_symmetric) in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        assert_eq!(pattern["direction"], json!({"x": 1.0, "y": 0.0, "z": 0.0}));
        assert_eq!(pattern["spacing"], 5.0);
        assert_eq!(pattern["count"], count);
        assert_eq!(pattern["symmetric"], symmetric, "{pattern}");
        let second = &pattern["second"];
        assert_eq!(second["direction"], json!({"x": 0.0, "y": 1.0, "z": 0.0}));
        assert_eq!(second["spacing"], 10.0);
        assert_eq!(second["count"], second_count);
        assert_eq!(second["symmetric"], second_symmetric, "{pattern}");
    }
}
