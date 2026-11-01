// SPDX-License-Identifier: Apache-2.0
//! Revolve-construction ownership transitions.

#[test]
fn revolve_set_axis_preserves_owned_profile_extent_and_selections() {
    use crate::features::{RevolutionAxis, RevolveConstruction};

    let axis = serde_json::json!({
        "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
    });
    let mut construction: RevolveConstruction = serde_json::from_value(serde_json::json!({
        "state": "unresolved",
        "missing": "axis",
        "profile": {"kind": "native", "value": "test:profile"},
        "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": {"kind":"literal","value":1.25}}},
        "solid": true,
        "face_maker_class": "Part::FaceMakerBullseye"
    }))
    .unwrap();
    construction.set_axis(Some(
        serde_json::from_value::<RevolutionAxis>(axis.clone()).unwrap(),
    ));
    assert_eq!(
        serde_json::to_value(construction).unwrap(),
        serde_json::json!({
            "state": "resolved",
            "profile": {"kind": "native", "value": "test:profile"},
            "axis": axis,
            "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": {"kind":"literal","value":1.25}}},
            "solid": true,
            "face_maker_class": "Part::FaceMakerBullseye"
        })
    );
}

#[test]
fn revolve_fixed_angles_have_positive_sign_and_one_turn_limit() {
    use crate::features::RevolveExtent;
    use serde_json::json;
    use std::f64::consts::{PI, TAU};

    for kind in ["one_sided", "symmetric"] {
        for angle in [-PI, 0.0] {
            assert!(serde_json::from_value::<RevolveExtent>(json!({
                "kind": kind,
                "termination": {"kind": "angle", "angle": {"kind": "literal", "value": angle}}
            }))
            .is_err());
        }
        for (angle, valid) in [
            (PI, true),
            (TAU, true),
            (TAU + 1.0e-13, true),
            (TAU + 1.0e-9, false),
            (TAU + PI, false),
        ] {
            let extent: RevolveExtent = serde_json::from_value(json!({
                "kind": kind,
                "termination": {"kind": "angle", "angle": {"kind": "literal", "value": angle}}
            }))
            .unwrap();
            assert_eq!(extent.angles_within_turn(), valid);
        }
    }
    for (first, second, valid) in [
        (PI, PI, true),
        (PI, PI / 2.0, true),
        (PI + 7.0e-15, PI + 7.0e-15, true),
        (TAU, PI, false),
    ] {
        let extent: RevolveExtent = serde_json::from_value(json!({
            "kind": "two_sided",
            "first": {"kind": "angle", "angle": {"kind": "literal", "value": first}},
            "second": {"kind": "angle", "angle": {"kind": "literal", "value": second}}
        }))
        .unwrap();
        assert_eq!(extent.angles_within_turn(), valid);
    }
    for side in ["first", "second"] {
        let mut extent = json!({
            "kind": "two_sided",
            "first": {"kind": "angle", "angle": {"kind": "literal", "value": PI}},
            "second": {"kind": "angle", "angle": {"kind": "literal", "value": PI}}
        });
        extent[side]["angle"] = json!({"kind": "literal", "value": -PI});
        assert!(serde_json::from_value::<RevolveExtent>(extent).is_err());
    }
}
