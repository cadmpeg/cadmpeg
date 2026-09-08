// SPDX-License-Identifier: Apache-2.0

use crate::geometry::VariableBlendValue;
use serde_json::json;

#[test]
fn variable_blend_value_derives_its_native_name_and_rejects_mismatches() {
    let wire = json!({
        "name": "two_ends", "modern_flag": true, "discriminator": 7, "calibrated": 3,
        "payload": {"kind": "two_ends", "parameters": [0.25, 0.75], "radii": [15.0, 25.0]}
    });
    let value: VariableBlendValue = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(value.payload.native_name(), "two_ends");
    assert_eq!(serde_json::to_value(value).unwrap(), wire);
    for name in [
        "unknown",
        "const",
        "interp",
        "fixed_width",
        "functional",
        "edge_offset",
    ] {
        let mut wrong = wire.clone();
        wrong["name"] = json!(name);
        let error = serde_json::from_value::<VariableBlendValue>(wrong).unwrap_err();
        assert!(error.to_string().contains("name"), "{error}");
    }
}

#[test]
fn variable_blend_value_keeps_every_payload_wire_and_its_outer_discriminator() {
    let function =
        json!({"kind": "line", "origin": {"u": 0.0, "v": 1.0}, "direction": {"u": 2.0, "v": 3.0}});
    let nested = json!({"name": "two_ends", "modern_flag": false, "discriminator": 9, "calibrated": 4, "payload": {"kind": "two_ends", "parameters": [0.0, 1.0], "radii": [2.0, 3.0]}});
    for (name, discriminator, payload) in [
        (
            "fixed_width",
            -3,
            json!({"kind": "fixed_width", "parameters": [0.0, 1.0], "width": 2.0}),
        ),
        (
            "edge_offset",
            0,
            json!({"kind": "edge_offset", "scalars": [0.0, 1.0], "lengths": [2.0]}),
        ),
        (
            "edge_offset",
            1,
            json!({"kind": "edge_offset", "scalars": [0.0, 1.0], "lengths": [2.0]}),
        ),
        (
            "functional",
            7,
            json!({"kind": "functional", "parameter": 0.0, "radius": 1.0, "function": function, "terminal": {"kind": "double", "value": 2.0}}),
        ),
        (
            "functional",
            7,
            json!({"kind": "functional", "parameter": 0.0, "radius": 1.0, "function": function, "terminal": {"kind": "text", "value": "end"}}),
        ),
        (
            "const",
            7,
            json!({"kind": "constant", "parameters": [0.0, 1.0], "radius": 2.0, "variable_chamfer": 3, "chamfer_type": 4, "nested": nested}),
        ),
        (
            "interp",
            7,
            json!({"kind": "interpolated", "parameter": 0.0, "radius": 1.0, "function": function, "enum_count": 3, "enum_tagged": true, "points": []}),
        ),
    ] {
        let wire = json!({"name": name, "modern_flag": true, "discriminator": discriminator, "calibrated": 5, "payload": payload});
        let value: VariableBlendValue = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(value.payload.discriminator(), discriminator);
        assert_eq!(serde_json::to_value(value).unwrap(), wire);
    }
}

#[test]
fn variable_blend_edge_offset_rejects_wrong_arity_and_discriminators() {
    let wire = json!({"name": "edge_offset", "modern_flag": true, "discriminator": 1, "calibrated": 0, "payload": {"kind": "edge_offset", "scalars": [0.0, 1.0], "lengths": [2.0]}});
    for code in [-1, 2, 7] {
        let mut invalid = wire.clone();
        invalid["discriminator"] = json!(code);
        let error = serde_json::from_value::<VariableBlendValue>(invalid).unwrap_err();
        assert!(error.to_string().contains("discriminator"), "{error}");
    }
    for (field, values) in [
        ("scalars", json!([0.0])),
        ("scalars", json!([0.0, 1.0, 2.0])),
        ("lengths", json!([])),
        ("lengths", json!([2.0, 3.0])),
    ] {
        let mut invalid = wire.clone();
        invalid["payload"][field] = values;
        assert!(serde_json::from_value::<VariableBlendValue>(invalid).is_err());
    }
}

#[test]
fn variable_blend_functional_terminal_rejects_unrelated_token_classes() {
    for terminal in [
        json!({"kind": "boolean", "value": true}),
        json!({"kind": "integer", "value": 2}),
        json!({"kind": "enum", "value": 3}),
    ] {
        let wire = json!({"name": "functional", "modern_flag": true, "discriminator": 1, "calibrated": 0, "payload": {"kind": "functional", "parameter": 0.0, "radius": 1.0, "function": {"kind": "line", "origin": {"u": 0.0, "v": 1.0}, "direction": {"u": 2.0, "v": 3.0}}, "terminal": terminal}});
        assert!(serde_json::from_value::<VariableBlendValue>(wire).is_err());
    }
}
