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
