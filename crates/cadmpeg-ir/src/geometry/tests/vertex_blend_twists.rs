// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use crate::geometry::VertexBlendTwists;
use crate::math::Point3;

#[test]
fn vertex_blend_twists_preserve_exact_forms_and_reject_count_conflicts() {
    let point = Point3::new(2.0, 3.0, 4.0);
    for (form, value, count) in [
        (0, VertexBlendTwists::None, 0),
        (1, VertexBlendTwists::One(point), 1),
        (3, VertexBlendTwists::Two([point; 2]), 2),
    ] {
        let expected = serde_json::json!({ "form": form, "twists": vec![point; count] });
        assert_eq!(serde_json::to_value(&value).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<VertexBlendTwists>(expected).unwrap(),
            value
        );
        for wrong_count in (0..=3).filter(|other| *other != count) {
            let invalid = serde_json::json!({ "form": form, "twists": vec![point; wrong_count] });
            assert!(serde_json::from_value::<VertexBlendTwists>(invalid).is_err());
        }
    }
    assert!(serde_json::from_value::<VertexBlendTwists>(
        serde_json::json!({ "form": 2, "twists": [point, point] })
    )
    .is_err());
}

#[test]
fn vertex_blend_circle_keeps_the_flat_form_and_twists_fields() {
    use crate::geometry::VertexBlendBoundaryGeometry;
    let wire = serde_json::json!({
        "kind": "circle",
        "curve": "test:model:curve#0",
        "curve_endpoints": [null, null],
        "twists": {"form": 1, "twists": [{"x": 2.0, "y": 3.0, "z": 4.0}]},
        "parameters": [0.0, 1.0],
        "sense": true,
    });
    let definition: VertexBlendBoundaryGeometry = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);
    let mut invalid = wire;
    invalid["twists"]["form"] = serde_json::json!(3);
    assert!(serde_json::from_value::<VertexBlendBoundaryGeometry>(invalid).is_err());
}
