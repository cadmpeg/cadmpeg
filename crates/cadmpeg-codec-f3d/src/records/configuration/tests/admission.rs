// SPDX-License-Identifier: Apache-2.0

use crate::records::configuration::{
    DesignConfiguration, DesignConfigurationKind, DesignConfigurationWire,
};
use crate::test_support::native_fixture::reject_changed_id;
use cadmpeg_core::serde_erase::json;

#[test]
fn configuration_identity_scope_is_independent_of_its_source_entry_name() {
    let name = "Design/Config #1.dsgcfg";
    let valid = cadmpeg_core::json!({
        "id": "f3d:component-0@configuration:entry#Design/Config%20%231.dsgcfg",
        "entry_name": name, "kind": "table", "payload": {}
    });
    let record: DesignConfiguration = json::from_value(valid.clone()).unwrap();
    assert_eq!(json::to_value(record).unwrap(), valid);
    for id in [
        "f3d::entry#Design/Config%20%231.dsgcfg",
        "f3d:scope with space:entry#Design/Config%20%231.dsgcfg",
        "f3d:scope:extra:entry#Design/Config%20%231.dsgcfg",
        "other:configuration:entry#Design/Config%20%231.dsgcfg",
        "f3d:configuration:other#Design/Config%20%231.dsgcfg",
        "f3d:component-0@configuration:entry#Other.dsgcfg",
    ] {
        let mut invalid = valid.clone();
        invalid["id"] = id.into();
        assert!(
            json::from_value::<DesignConfiguration>(invalid).is_err(),
            "{id}"
        );
    }
}

#[test]
fn configuration_admission_checks_the_wire_payload_and_kind() {
    use crate::records::configuration::DesignConfiguration;
    let wire = cadmpeg_core::json!({"id":crate::ids::configuration_entry_id("table.dsgcfg", &cadmpeg_ir::identity_component!("configuration")), "entry_name":"table.dsgcfg", "kind":"table",
        "payload":{"tables":[]}});
    let configuration: DesignConfiguration = json::from_value(wire.clone()).unwrap();
    assert_eq!(json::to_value(&configuration).unwrap(), wire);
    let mut invalid = wire.clone();
    invalid["payload"] = cadmpeg_core::json!([]);
    assert!(json::from_value::<DesignConfiguration>(invalid)
        .unwrap_err()
        .to_string()
        .contains("payload"));
    let mut invalid = wire;
    invalid["kind"] = "rule".into();
    assert!(json::from_value::<DesignConfiguration>(invalid).is_err());
}

#[test]
fn configuration_kind_requires_its_exact_entry_extension() {
    use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
    for (kind, valid, invalid) in [
        (
            DesignConfigurationKind::Table,
            "folder/table.dsgcfg",
            "folder/table.dsgcfgrule",
        ),
        (
            DesignConfigurationKind::Rule,
            "folder/rule.dsgcfgrule",
            "folder/rule.dsgcfg",
        ),
    ] {
        let admit =
            |name: &str| DesignConfiguration::try_new(name.into(), kind, serde_json::Map::new());
        let record = admit(valid).unwrap();
        let wire = json::to_value(&record).unwrap();
        assert_eq!(
            wire["id"],
            crate::ids::configuration_entry_id(
                valid,
                &cadmpeg_ir::identity_component!("configuration")
            )
        );
        assert_eq!(
            json::to_value(json::from_value::<DesignConfiguration>(wire.clone()).unwrap()).unwrap(),
            wire
        );
        for name in [invalid, "entry", "entry.DSGCFG", "entry.dsgcfg/suffix"] {
            let error = admit(name).unwrap_err().to_string();
            assert!(error.contains("entry_name"), "{error}");
            let mut malformed = wire.clone();
            malformed["entry_name"] = name.into();
            malformed["id"] = crate::ids::configuration_entry_id(
                name,
                &cadmpeg_ir::identity_component!("configuration"),
            )
            .into();
            assert!(json::from_value::<DesignConfiguration>(malformed).is_err());
        }
    }
}

#[test]
fn configuration_id_binds_the_escaped_entry_name() {
    let name = "Design/Config #1.dsgcfg";
    let wire = DesignConfigurationWire {
        id: crate::ids::configuration_entry_id(
            name,
            &cadmpeg_ir::identity_component!("configuration"),
        ),
        entry_name: name.into(),
        kind: DesignConfigurationKind::Table,
        payload: cadmpeg_core::json!({}),
        active_row: None,
    };
    for id in ["id", "f3d:configuration:entry#Design/Config #1.dsgcfg"] {
        assert!(DesignConfiguration::try_from(DesignConfigurationWire {
            id: id.into(),
            ..wire.clone()
        })
        .is_err());
    }
    let record = DesignConfiguration::try_from(wire).unwrap();
    let mut changed_name = json::to_value(&record).unwrap();
    changed_name["entry_name"] = cadmpeg_core::json!("Other.dsgcfg");
    assert!(json::from_value::<DesignConfiguration>(changed_name).is_err());
    reject_changed_id(
        record,
        &["id", "f3d:configuration:entry#Design/Config #1.dsgcfg"],
    );
}
