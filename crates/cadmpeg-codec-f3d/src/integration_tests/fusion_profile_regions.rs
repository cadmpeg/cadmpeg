// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves whose Extrudes select regions through nested
//! profile-region selections. Fusion reported the selected profiles of each.

use std::io::Cursor;

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

/// The hole count of each region that the Extrude named `name` selects.
fn region_holes(bytes: &[u8], name: &str) -> Vec<usize> {
    let ir = decoded(bytes);
    let profile = &definition(&ir, name)["value"]["value"]["profile"]["value"];
    assert_eq!(profile["kind"], "sketch_regions", "{name}: {profile}");
    profile["value"]["regions"]
        .as_array()
        .expect("regions")
        .iter()
        .map(|region| region["hole_boundaries"].as_array().map_or(0, Vec::len))
        .collect()
}

#[test]
fn each_selected_profile_of_a_fusion_extrude_is_one_region() {
    let cases: [(&[u8], &str, &[usize]); 5] = [
        // One region that is one face of the sketch arrangement.
        (
            include_bytes!("../../../../corpus/f3d/region_A.f3d"),
            "Extrude 1",
            &[0],
        ),
        // One selected region of two arrangement faces: Fusion selects two profiles.
        (
            include_bytes!("../../../../corpus/f3d/region_B.f3d"),
            "Extrude 1",
            &[0, 0],
        ),
        // One region with a hole.
        (
            include_bytes!("../../../../corpus/f3d/region_C.f3d"),
            "Extrude 1",
            &[1],
        ),
        // Two separate regions.
        (
            include_bytes!("../../../../corpus/f3d/region_D.f3d"),
            "Extrude 1",
            &[0, 0],
        ),
        // Three of the five profiles of one sketch.
        (
            include_bytes!("../../../../corpus/f3d/strip_cut.f3d"),
            "Extrude 2",
            &[0, 0, 0],
        ),
    ];
    for (bytes, name, holes) in cases {
        assert_eq!(region_holes(bytes, name), holes, "{name}");
    }
}
