// SPDX-License-Identifier: Apache-2.0

#[test]
fn configuration_admission_checks_wire_and_variant_order() {
    use crate::records::DesignConfiguration;
    let wire = serde_json::json!({"id":"config", "entry_name":"table.dsgcfg", "kind":"table",
        "variant_order":["first","second"], "payload":{"configurations":{"first":{},"second":{}}}});
    let configuration: DesignConfiguration = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&configuration).unwrap(), wire);
    let mut invalid = wire.clone();
    invalid["variant_order"] = serde_json::json!(["first"]);
    assert!(serde_json::from_value::<DesignConfiguration>(invalid).is_err());
    let mut invalid = wire.clone();
    invalid["payload"] = serde_json::json!([]);
    assert!(serde_json::from_value::<DesignConfiguration>(invalid)
        .unwrap_err()
        .to_string()
        .contains("payload"));
    let mut invalid = wire;
    invalid["kind"] = "rule".into();
    assert!(serde_json::from_value::<DesignConfiguration>(invalid).is_err());
}
