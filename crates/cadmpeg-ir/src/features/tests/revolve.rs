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
        "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": 1.25}},
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
            "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": 1.25}},
            "solid": true,
            "face_maker_class": "Part::FaceMakerBullseye"
        })
    );
}
