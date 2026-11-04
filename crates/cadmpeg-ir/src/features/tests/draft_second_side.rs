// SPDX-License-Identifier: Apache-2.0

#[test]
fn draft_second_side_round_trips_as_a_tagged_key() {
    use crate::features::{DraftSecondSide, FeatureDefinition, FeatureOperation};

    for (second_side, expected) in [
        (
            serde_json::json!({"kind": "symmetric"}),
            DraftSecondSide::Symmetric {},
        ),
        (
            serde_json::json!({"kind": "angle", "angle": -0.05}),
            DraftSecondSide::Angle {
                angle: crate::scalar::SlopeAngle::new(-0.05).unwrap(),
            },
        ),
    ] {
        let wire = serde_json::json!({
            "definition": "draft",
            "faces": {"kind": "native", "value": "draft:faces"},
            "anchor": {
                "kind": "neutral_plane",
                "plane": {"kind": "native", "value": "draft:plane"},
                "pull": {"direction": {"x": 0.0, "y": 0.0, "z": 1.0}}
            },
            "angle": 0.1,
            "outward": false,
            "second_side": second_side
        });
        let definition: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Draft {
            second_side: decoded,
            ..
        }) = &definition
        else {
            panic!("expected a Draft");
        };
        assert_eq!(*decoded, Some(expected));
        assert_eq!(serde_json::to_value(definition).unwrap(), wire);
    }

    let error = serde_json::from_value::<FeatureOperation>(serde_json::json!({
        "definition": "draft",
        "faces": {"kind": "native", "value": "draft:faces"},
        "anchor": {"kind": "neutral_plane", "plane": {"kind": "native", "value": "draft:plane"}},
        "angle": 0.1,
        "outward": false,
        "second_side": null
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("second_side"), "{error}");

    let error = serde_json::from_value::<DraftSecondSide>(
        serde_json::json!({"kind": "symmetric", "angle": 0.1}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("angle"), "{error}");
}
