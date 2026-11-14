// SPDX-License-Identifier: Apache-2.0

use crate::features::{FeatureDefinition, FeatureOperation};

#[test]
fn thicken_result_and_chain_round_trip_independently() {
    let original = serde_json::json!({
        "definition": "thicken", "faces": {"kind": "native", "value": "faces:1"},
        "thickness": 5.0, "side": "forward"
    });
    let legacy: FeatureDefinition = serde_json::from_value(original.clone()).unwrap();
    assert!(matches!(
        legacy,
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            result: None,
            tangent_chain: None,
            ..
        })
    ));
    assert_eq!(serde_json::to_value(legacy).unwrap(), original);
    for result in [
        serde_json::json!({"kind": "new_body"}),
        serde_json::json!({"kind": "boolean", "operation": "cut", "targets": {"kind": "bodies", "value": ["synthetic:test:body#2"]}}),
    ] {
        let mut wire = original.clone();
        wire["result"] = result;
        wire["tangent_chain"] = serde_json::json!(false);
        let decoded: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    }
    for key in ["result", "tangent_chain"] {
        let mut wire = original.clone();
        wire[key] = serde_json::Value::Null;
        let error = serde_json::from_value::<FeatureOperation>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
}
