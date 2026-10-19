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

#[test]
fn replacing_revolve_operands_preserves_every_other_input() {
    use crate::features::{PartialRevolveConstruction, RevolveConstruction};
    let complete: RevolveConstruction = serde_json::from_value(serde_json::json!({
        "state": "resolved",
        "profile": {"kind": "native", "value": "test:profile"},
        "axis": {
            "origin": {"x": 1.0, "y": 2.0, "z": 3.0},
            "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
        },
        "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": 1.25}},
        "solid": true,
        "face_maker_class": "Part::FaceMakerBullseye",
        "fuse_order": "feature_first",
        "allow_multi_profile_faces": false
    }))
    .unwrap();
    for has_extent in [false, true] {
        let mut construction = complete.clone();
        if !has_extent {
            construction = RevolveConstruction::Unresolved(PartialRevolveConstruction::Extent {
                profile: complete.profile().unwrap().clone(),
                axis: complete.axis().unwrap().clone(),
                solid: complete.solid(),
                face_maker: complete.face_maker().cloned(),
                fuse_order: complete.fuse_order(),
                allow_multi_profile_faces: complete.allow_multi_profile_faces(),
            });
        }
        for has_profile in [false, true] {
            construction.set_profile(has_profile.then(|| complete.profile().unwrap().clone()));
            for has_axis in [false, true] {
                construction.set_axis(has_axis.then(|| complete.axis().unwrap().clone()));
                assert_eq!(
                    construction.profile(),
                    has_profile.then(|| complete.profile().unwrap())
                );
                assert_eq!(
                    construction.axis(),
                    has_axis.then(|| complete.axis().unwrap())
                );
                assert_eq!(
                    construction.extent(),
                    has_extent.then(|| complete.extent().unwrap())
                );
                assert_eq!(
                    construction.is_resolved(),
                    has_profile && has_axis && has_extent
                );
                assert_eq!(construction.solid(), complete.solid());
                assert_eq!(construction.face_maker(), complete.face_maker());
                assert_eq!(construction.fuse_order(), complete.fuse_order());
                assert_eq!(
                    construction.allow_multi_profile_faces(),
                    complete.allow_multi_profile_faces()
                );
            }
        }
    }
}
