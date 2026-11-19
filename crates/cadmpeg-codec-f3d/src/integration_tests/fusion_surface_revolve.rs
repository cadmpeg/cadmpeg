// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of symmetric surface Revolves about the Z origin axis.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};
use serde_json::json;

use crate::F3dCodec;

/// The resolved construction of the one Revolve, after checking that the
/// neutral model validates.
fn revolve_construction(bytes: &[u8]) -> serde_json::Value {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    let ir = serde_json::to_value(result.ir()).expect("serialize the IR");
    let mut revolves = ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["source_tag"] == "Revolve");
    let revolve = revolves.next().expect("one Revolve");
    assert!(revolves.next().is_none(), "one Revolve");
    let operation = &revolve["definition"]["value"];
    assert_eq!(operation["kind"], "revolve");
    assert_eq!(operation["value"]["op"], "new_body");
    let construction = &operation["value"]["construction"];
    assert_eq!(construction["kind"], "resolved", "{construction:#}");
    construction["value"].clone()
}

/// Checks the fields that both saves share: the Z origin axis, a symmetric
/// extent of twice the stored 90-degree angle, and a surface result.
fn assert_symmetric_surface_about_z(construction: &serde_json::Value) {
    let axis = &construction["axis"];
    assert_eq!(axis["origin"], json!({"x": 0.0, "y": 0.0, "z": 0.0}));
    let direction = &axis["direction"];
    assert_eq!(
        (&direction["x"], &direction["y"]),
        (&json!(0.0), &json!(0.0))
    );
    assert_eq!(direction["z"].as_f64().map(f64::abs), Some(1.0));
    let extent = &construction["extent"];
    assert_eq!(extent["kind"], "symmetric");
    assert_eq!(extent["termination"]["kind"], "angle");
    let angle = extent["termination"]["angle"].as_f64().expect("angle");
    assert!((angle - std::f64::consts::PI).abs() < 1e-12, "{angle}");
    assert_eq!(construction["solid"], false);
}

#[test]
fn a_symmetric_surface_revolve_of_a_closed_profile_resolves() {
    // Fusion: a closed rectangle on XZ revolved about Z, isSolid false,
    // Symmetric 90 deg; one surface body of four faces.
    let construction = revolve_construction(include_bytes!(
        "../../../../corpus/f3d/symmetric_surface_revolve.f3d"
    ));
    assert_symmetric_surface_about_z(&construction);
    let profile = &construction["profile"];
    assert_eq!(profile["kind"], "sketch_profiles");
    assert_eq!(profile["value"]["profiles"], json!([0]));
}

#[test]
fn a_symmetric_surface_revolve_of_an_open_line_resolves_its_curve() {
    // Fusion: one open line on XZ, selected with createOpenProfile, revolved
    // about Z, isSolid false, Symmetric 90 deg; one cylindrical face.
    let construction = revolve_construction(include_bytes!(
        "../../../../corpus/f3d/open_profile_surface_revolve.f3d"
    ));
    assert_symmetric_surface_about_z(&construction);
    let profile = &construction["profile"];
    assert_eq!(profile["kind"], "sketch_entities");
    assert_eq!(
        profile["value"]["entities"].as_array().map(Vec::len),
        Some(1)
    );
}
