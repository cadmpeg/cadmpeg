// SPDX-License-Identifier: Apache-2.0

use crate::geometry::RollingBallSide;
use serde_json::{json, Value};

fn empty_side_wire() -> Value {
    json!({
        "support_kind": "surface",
        "location": {"x": 1.0, "y": 2.0, "z": 3.0}
    })
}

#[test]
fn rolling_ball_side_keeps_flat_support_and_extension_fields() {
    let empty = empty_side_wire();
    let side: RollingBallSide = serde_json::from_value(empty.clone()).unwrap();
    assert!(side.surface.is_none());
    assert!(side.curve.is_none());
    assert!(side.extension.is_none());
    assert_eq!(serde_json::to_value(side).unwrap(), empty);

    let mut wire = empty_side_wire();
    wire["surface"] = json!({
        "surface": "test:model:surface#support",
        "parameter_ranges": [[1.0, null], [null, 4.0]]
    });
    wire["curve"] = json!({
        "curve": "test:model:curve#side",
        "parameter_range": [null, 6.0]
    });
    wire["extension"] = json!({"value": 7, "pcurve": null});
    let side: RollingBallSide = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        side.surface.as_ref().unwrap().parameter_ranges,
        [[Some(1.0), None], [None, Some(4.0)]]
    );
    assert_eq!(
        side.curve.as_ref().unwrap().parameter_range,
        [None, Some(6.0)]
    );
    assert!(side.extension.as_ref().unwrap().pcurve.is_none());
    assert_eq!(serde_json::to_value(side).unwrap(), wire);

    wire["extension"] = json!({
        "value": 7,
        "pcurve": {
            "kind": "line",
            "origin": {"u": 0.0, "v": 1.0},
            "direction": {"u": 2.0, "v": 3.0}
        }
    });
    let side: RollingBallSide = serde_json::from_value(wire.clone()).unwrap();
    assert!(side.extension.as_ref().unwrap().pcurve.is_some());
    assert_eq!(serde_json::to_value(side).unwrap(), wire);
}

#[test]
fn rolling_ball_side_rejects_orphaned_wire_payloads() {
    for (field, payload) in [
        ("surface_ranges", json!([[null, 2.0], [null, null]])),
        ("curve_range", json!([3.0, null])),
        (
            "tertiary_pcurve",
            json!({
                "kind": "line",
                "origin": {"u": 0.0, "v": 1.0},
                "direction": {"u": 2.0, "v": 3.0}
            }),
        ),
    ] {
        let mut wire = empty_side_wire();
        wire[field] = payload;
        let error = serde_json::from_value::<RollingBallSide>(wire).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("unknown field"), "{error}");
        assert!(message.contains(field), "{error}");
    }
}
