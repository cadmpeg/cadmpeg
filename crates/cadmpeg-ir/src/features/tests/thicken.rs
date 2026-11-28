// SPDX-License-Identifier: Apache-2.0

use crate::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_core::serde_erase::json;

#[test]
fn thicken_result_and_chain_round_trip_independently() {
    let operation = cadmpeg_core::json!({"kind": "thicken", "value": {
        "faces": {"kind": "native", "value": "faces:1"},
        "thickness": 5.0, "side": "forward"
    }});
    let original = cadmpeg_core::json!({"kind": "operation", "value": operation});
    let legacy: FeatureDefinition = json::from_value(original.clone()).unwrap();
    assert!(matches!(
        legacy,
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            result: None,
            tangent_chain: None,
            ..
        })
    ));
    assert_eq!(json::to_value(legacy).unwrap(), original);
    for result in [
        cadmpeg_core::json!({"kind": "new_body"}),
        cadmpeg_core::json!({"kind": "boolean", "operation": "cut", "targets": {"kind": "bodies", "value": ["synthetic:test:body#2"]}}),
    ] {
        let mut wire = original.clone();
        wire["value"]["value"]["result"] = result;
        wire["value"]["value"]["tangent_chain"] = cadmpeg_core::json!(false);
        let decoded: FeatureDefinition = json::from_value(wire.clone()).unwrap();
        assert_eq!(json::to_value(decoded).unwrap(), wire);
    }
    for key in ["result", "tangent_chain"] {
        let mut wire = operation.clone();
        wire["value"][key] = serde_json::Value::Null;
        let error = json::from_value::<FeatureOperation>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
}
