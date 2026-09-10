// SPDX-License-Identifier: Apache-2.0

use crate::geometry::RollingBallSupportCurve;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct SecondaryCurveWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_curve: Option<RollingBallSupportCurve>,
}

#[test]
fn the_secondary_curve_carries_its_own_bounds() {
    for wire in [
        json!({}),
        json!({"secondary_curve": {
            "curve": "test:model:curve#secondary",
            "parameter_range": [2.0, null]
        }}),
        json!({"secondary_curve": {
            "curve": "test:model:curve#secondary",
            "parameter_range": [null, 4.0]
        }}),
    ] {
        let decoded: SecondaryCurveWire = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    }
}

#[test]
fn secondary_bounds_without_a_curve_have_no_encoding() {
    let error = serde_json::from_value::<SecondaryCurveWire>(json!({
        "secondary_range": [2.0, null]
    }))
    .unwrap();
    assert!(error.secondary_curve.is_none());

    let error = serde_json::from_value::<SecondaryCurveWire>(json!({
        "secondary_curve": {"parameter_range": [2.0, null]}
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("curve"), "{error}");
}
