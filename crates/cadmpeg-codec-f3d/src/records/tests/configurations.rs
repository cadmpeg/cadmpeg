// SPDX-License-Identifier: Apache-2.0

#[test]
fn configuration_admission_checks_wire_and_variant_order() {
    use crate::records::DesignConfiguration;
    let wire = serde_json::json!({"id":crate::ids::configuration_entry_id("table.dsgcfg"), "entry_name":"table.dsgcfg", "kind":"table",
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

#[test]
fn configuration_parameter_overrides_require_scalar_values() {
    use crate::records::{DesignConfiguration, DesignConfigurationKind};
    let admit = |payload: serde_json::Value| {
        DesignConfiguration::try_new(
            crate::ids::configuration_entry_id("table.dsgcfg"),
            "table.dsgcfg".into(),
            DesignConfigurationKind::Table,
            vec!["variant".into()],
            payload.as_object().unwrap().clone(),
        )
    };
    assert!(admit(
        serde_json::json!({"configurations": {"variant": {"parameters": {
            "string": "25 mm", "number": 2.5, "boolean": true, "null": null
        }}}})
    )
    .is_ok());
    for value in [
        serde_json::json!(["25 mm"]),
        serde_json::json!({"value": "25 mm"}),
    ] {
        let error = admit(serde_json::json!({
            "configurations": {"variant": {"parameters": {"width": value}}}
        }))
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("parameter overrides must be JSON scalars"));
    }
}
