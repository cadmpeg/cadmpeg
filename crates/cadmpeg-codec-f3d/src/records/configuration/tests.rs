// SPDX-License-Identifier: Apache-2.0

use super::{encode_configuration_payload, DesignConfiguration};
use serde_json::{json, Value};

fn wire(kind: &str, order: &[&str], payload: Value) -> Value {
    let name = if kind == "rule" {
        "entry.dsgcfgrule"
    } else {
        "entry.dsgcfg"
    };
    Value::Object(
        [
            ("id".into(), crate::ids::configuration_entry_id(name).into()),
            ("entry_name".into(), name.into()),
            ("kind".into(), kind.into()),
            ("variant_order".into(), json!(order)),
            ("payload".into(), payload),
        ]
        .into_iter()
        .collect(),
    )
}

#[test]
fn configuration_payload_retains_absence_empty_fields_and_extension_values() {
    for (order, payload) in [
        (vec![], json!({})),
        (vec![], json!({"configurations": {}})),
        (vec![], json!({"configurations": {"v": {}}})),
        (
            vec!["v"],
            json!({"configurations": {"v": {"parameters": {}, "suppressed": [], "material": ""}}}),
        ),
        (
            vec![""],
            json!({"active": "", "configurations": {"": {"suppressed": ["", "x", "x"]}}}),
        ),
    ] {
        let expected = wire("table", &order, payload.clone());
        let admitted: DesignConfiguration = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(&admitted).unwrap(), expected);
        assert_eq!(
            serde_json::from_slice::<Value>(&encode_configuration_payload(&admitted).unwrap())
                .unwrap(),
            payload
        );
    }
    for value in [
        Value::Null,
        json!(false),
        json!(2.5),
        json!(""),
        json!([]),
        json!({"nested": [1, null]}),
    ] {
        let payload = json!({"configurations": {"v": {"unknown": value}}, "unknown": value});
        let expected = wire("table", &["v"], payload.clone());
        let admitted: DesignConfiguration = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(admitted.unknown_member_count(), 2);
        assert_eq!(serde_json::to_value(&admitted).unwrap(), expected);
        assert_eq!(
            serde_json::from_slice::<Value>(&encode_configuration_payload(&admitted).unwrap())
                .unwrap(),
            payload
        );

        let rule = wire(
            "rule",
            &[],
            json!({"when": value, "activate": value, "configurations": value}),
        );
        let admitted: DesignConfiguration = serde_json::from_value(rule.clone()).unwrap();
        assert!(admitted.variants().is_empty());
        assert_eq!(serde_json::to_value(&admitted).unwrap(), rule);
    }
}

#[test]
fn configuration_scalar_projection_preserves_exact_text() {
    let admitted: DesignConfiguration = serde_json::from_value(wire(
        "table",
        &["v"],
        json!({
            "configurations": {"v": {"parameters": {
                "boolean": false, "null": null, "number": u64::MAX,
                "text": " 25 mm ", "zero": -0.0
            }}}
        }),
    ))
    .unwrap();
    let (_, variant) = &admitted.variants()[0];
    let actual: Vec<_> = variant
        .parameters()
        .map(|(key, value)| (key.as_str(), value.text()))
        .collect();
    assert_eq!(
        actual,
        [
            ("boolean", "false".into()),
            ("null", "null".into()),
            ("number", "18446744073709551615".into()),
            ("text", " 25 mm ".into()),
            ("zero", "-0.0".into()),
        ]
    );
}

#[test]
fn configuration_order_owns_each_member_once() {
    let payload = json!({"active": "b", "configurations": {"a": {}, "b": {}, "c": {}}});
    for order in [
        vec![],
        vec!["a"],
        vec!["a", "a", "c"],
        vec!["a", "b", "missing"],
        vec!["a", "b", "c", "d"],
    ] {
        assert!(serde_json::from_value::<DesignConfiguration>(wire(
            "table",
            &order,
            payload.clone()
        ))
        .is_err());
    }
    let admitted: DesignConfiguration =
        serde_json::from_value(wire("table", &["c", "a", "b"], payload)).unwrap();
    assert_eq!(
        admitted
            .variants()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["c", "a", "b"]
    );
    assert_eq!(admitted.active(), Some("b"));
    assert_eq!(
        encode_configuration_payload(&admitted).unwrap(),
        br#"{"active":"b","configurations":{"c":{},"a":{},"b":{}}}"#
    );
}

#[test]
fn configuration_known_fields_reject_wrong_json_kinds() {
    for value in [
        Value::Null,
        json!(false),
        json!(1),
        json!("s"),
        json!([]),
        json!({}),
    ] {
        for field in ["parameters", "suppressed", "material"] {
            let payload = json!({"configurations": {"v": {field: value}}});
            let valid = match field {
                "parameters" => value.is_object(),
                "suppressed" => value.is_array(),
                "material" => value.is_string(),
                _ => unreachable!(),
            };
            assert_eq!(
                serde_json::from_value::<DesignConfiguration>(wire("table", &["v"], payload))
                    .is_ok(),
                valid,
                "{field}: {value}"
            );
        }
    }
    for value in [json!({}), json!([])] {
        assert!(serde_json::from_value::<DesignConfiguration>(wire(
            "table",
            &["v"],
            json!({"configurations": {"v": {"parameters": {"p": value}}}})
        ))
        .is_err());
    }
    for value in [Value::Null, json!(false), json!(1), json!({}), json!([])] {
        assert!(serde_json::from_value::<DesignConfiguration>(wire(
            "table",
            &["v"],
            json!({"configurations": {"v": {"suppressed": [value]}}})
        ))
        .is_err());
    }
}

mod admission;
