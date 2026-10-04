// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use crate::geometry::VertexBlendTwists;
use crate::math::Point3;

#[test]
fn vertex_blend_twists_preserve_exact_forms_and_reject_count_conflicts() {
    let point = Point3::new(2.0, 3.0, 4.0);
    for (value, expected) in [
        (
            VertexBlendTwists::None {},
            serde_json::json!({ "form": "none" }),
        ),
        (
            VertexBlendTwists::One { twist: point },
            serde_json::json!({ "form": "one", "twist": point }),
        ),
        (
            VertexBlendTwists::Two { twists: [point; 2] },
            serde_json::json!({ "form": "two", "twists": [point, point] }),
        ),
    ] {
        assert_eq!(serde_json::to_value(&value).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<VertexBlendTwists>(expected).unwrap(),
            value
        );
    }
    // A count that disagrees with the form has no spelling: each form names
    // exactly the entries it carries, and the native integer forms 0, 1 and 3
    // are the names none, one and two.
    for invalid in [
        serde_json::json!({ "form": "none", "twist": point }),
        serde_json::json!({ "form": "one", "twists": [point] }),
        serde_json::json!({ "form": "one" }),
        serde_json::json!({ "form": "two", "twists": [point] }),
        serde_json::json!({ "form": "two", "twists": [point, point, point] }),
        serde_json::json!({ "form": 2, "twists": [point, point] }),
        serde_json::json!({ "form": 1, "twists": [point] }),
    ] {
        assert!(serde_json::from_value::<VertexBlendTwists>(invalid).is_err());
    }
}

#[test]
fn vertex_blend_circle_keeps_the_flat_form_and_twists_fields() {
    use crate::geometry::VertexBlendBoundaryGeometry;
    let wire = serde_json::json!({
        "kind": "circle",
        "curve": "test:model:curve#0",
        "curve_endpoints": [null, null],
        "twists": {"form": "one", "twist": {"x": 2.0, "y": 3.0, "z": 4.0}},
        "parameters": [0.0, 1.0],
        "sense": true,
    });
    let definition: VertexBlendBoundaryGeometry = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);
    let mut invalid = wire;
    invalid["twists"]["form"] = serde_json::json!("two");
    assert!(serde_json::from_value::<VertexBlendBoundaryGeometry>(invalid).is_err());
}
