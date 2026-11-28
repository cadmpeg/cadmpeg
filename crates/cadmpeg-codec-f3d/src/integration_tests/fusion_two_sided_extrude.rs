// SPDX-License-Identifier: Apache-2.0
//! A CC0 Fusion save of two-sided distance Extrudes with every sign pair of
//! their side distances, wall-thickness Extrudes, and a drafted Extrude.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

fn decoded() -> serde_json::Value {
    let bytes = include_bytes!("../../../../corpus/f3d/extrude_two_sided_signs.f3d");
    let result = F3dCodec
        .decode(&mut Cursor::new(&bytes[..]), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    cadmpeg_test_support::wire::in_memory_json(result.ir())
}

/// The definition of the feature named `name`.
fn definition<'a>(ir: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    let mut named = ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["name"] == name);
    let feature = named.next().unwrap_or_else(|| panic!("feature {name}"));
    assert!(named.next().is_none(), "one feature named {name}");
    &feature["definition"]
}

/// The extent of the solid along the profile normal (the sketch lies on XY,
/// so this is the body's z range): the interval between the start plane and
/// each side's end.
fn span(extrude: &serde_json::Value) -> (f64, f64) {
    let sense = match extrude["direction"]["kind"].as_str() {
        Some("profile_normal") => 1.0,
        Some("reversed_profile_normal") => -1.0,
        other => panic!("direction {other:?}"),
    };
    let start = match extrude["start"]["kind"].as_str() {
        Some("profile_plane") => 0.0,
        Some("offset_profile_plane") => extrude["start"]["offset"].as_f64().expect("offset"),
        other => panic!("start {other:?}"),
    };
    let length = |side: &serde_json::Value| {
        assert_eq!(side["termination"]["kind"], "blind", "{side}");
        side["termination"]["length"].as_f64().expect("length")
    };
    let extent = &extrude["extent"];
    let (a, b) = match extent["kind"].as_str() {
        Some("one_sided") => (start, start + sense * length(&extent["side"])),
        Some("two_sided") => (
            start + sense * length(&extent["first"]),
            start - sense * length(&extent["second"]),
        ),
        other => panic!("extent {other:?}"),
    };
    (a.min(b), a.max(b))
}

#[test]
fn two_sided_distances_of_every_sign_pair_span_the_final_bodies() {
    let ir = decoded();
    // Side one / side two in millimetres, and the z range of the final body.
    for (name, expected) in [
        ("Extrude1", (5.0, 20.0)),   // +20 / -5
        ("Extrude2", (-20.0, -5.0)), // -20 / +5
        ("Extrude3", (-20.0, 5.0)),  // -20 / -5
        ("Extrude4", (-5.0, 20.0)),  // +20 / +5
    ] {
        let definition = definition(&ir, name);
        assert_eq!(definition["kind"], "operation", "{name}: {definition}");
        let extrude = &definition["value"]["value"];
        let (low, high) = span(extrude);
        assert!(
            (low - expected.0).abs() < 1e-9 && (high - expected.1).abs() < 1e-9,
            "{name}: {low}..{high}, expected {expected:?}: {extrude}"
        );
    }
}

#[test]
fn drafted_and_wall_thickness_extrudes_of_opposite_sign_stay_native() {
    let ir = decoded();
    for name in ["Extrude6", "Extrude7"] {
        let definition = definition(&ir, name);
        assert_ne!(
            definition["value"]["kind"], "extrude",
            "{name} is read as a neutral Extrude: {definition}"
        );
    }
}
