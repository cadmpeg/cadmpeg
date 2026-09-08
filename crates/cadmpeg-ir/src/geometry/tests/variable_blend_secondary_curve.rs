// SPDX-License-Identifier: Apache-2.0

use crate::geometry::RollingBallSupportCurve;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct SecondaryCurveWire {
    #[serde(flatten, with = "crate::geometry::variable_blend_secondary_curve_wire")]
    secondary_curve: Option<RollingBallSupportCurve>,
}

#[test]
fn variable_blend_secondary_curve_keeps_flat_wire_and_nullable_bounds() {
    for wire in [
        json!({"secondary_range": [null, null]}),
        json!({"secondary_curve": "test:model:curve#secondary", "secondary_range": [2.0, null]}),
        json!({"secondary_curve": "test:model:curve#secondary", "secondary_range": [null, 4.0]}),
    ] {
        let decoded: SecondaryCurveWire = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    }
    let omitted: SecondaryCurveWire = serde_json::from_value(json!({})).unwrap();
    assert!(omitted.secondary_curve.is_none());
}

#[test]
fn variable_blend_secondary_curve_rejects_bounds_without_a_curve() {
    for range in [json!([2.0, null]), json!([null, 4.0])] {
        let error = serde_json::from_value::<SecondaryCurveWire>(json!({"secondary_range": range}))
            .unwrap_err();
        assert!(error.to_string().contains("secondary_range"), "{error}");
    }
}
