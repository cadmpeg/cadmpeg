// SPDX-License-Identifier: Apache-2.0

#[test]
fn act_channels_reject_unpaired_keys_and_preserve_split_wire_maps() {
    let wire = serde_json::json!({
        "id": "stream:act-entity#7", "record_index": 7, "entity_id": "0_1",
        "in_table": false, "channel_class_tag": "261", "channel_record_index_offset": 100,
        "channel_entity_id_offset": 200,
        "channels": {"Appearance": "11111111-2222-3333-4444-555555555555"},
        "channel_guid_offsets": {"Appearance": 120}
    });
    let entity: crate::records::ActEntity = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(entity).unwrap(), wire);
    for offsets in [serde_json::json!({}), serde_json::json!({"Material": 120})] {
        let mut invalid = wire.clone();
        invalid["channel_guid_offsets"] = offsets;
        let error = serde_json::from_value::<crate::records::ActEntity>(invalid).unwrap_err();
        assert!(error
            .to_string()
            .contains("channels and channel_guid_offsets"));
    }
    for guid in ["", "11111111-2222-3333-4444-55555555555z"] {
        let mut invalid = wire.clone();
        invalid["channels"]["Appearance"] = serde_json::json!(guid);
        let error = serde_json::from_value::<crate::records::ActEntity>(invalid).unwrap_err();
        assert!(error.to_string().contains("GUID"));
    }
    let mut invalid = wire;
    invalid["channels"] = serde_json::json!({});
    assert!(serde_json::from_value::<crate::records::ActEntity>(invalid).is_err());
}

#[test]
fn act_class_tail_requires_nonpadding_bytes_and_a_bounded_offset() {
    let wire = serde_json::json!({
        "id": "stream:act-entity#7", "record_index": 7, "entity_id": "0_1",
        "in_table": false, "channel_class_tag": "261", "channel_record_index_offset": 100,
        "channel_entity_id_offset": 200,
        "channels": {"Appearance":"11111111-2222-3333-4444-555555555555"}, "channel_guid_offsets": {"Appearance":120},
        "channel_class_tail": [0, 1], "channel_class_tail_offset": 300
    });
    let entity: crate::records::ActEntity = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(entity).unwrap(), wire);
    for (bytes, offset) in [
        (serde_json::json!([]), serde_json::json!(300)),
        (serde_json::json!([0, 0]), serde_json::json!(300)),
        (serde_json::json!([1]), serde_json::Value::Null),
        (serde_json::json!([1]), serde_json::json!(u64::MAX)),
    ] {
        let mut invalid = wire.clone();
        invalid["channel_class_tail"] = bytes;
        invalid["channel_class_tail_offset"] = offset;
        let error = serde_json::from_value::<crate::records::ActEntity>(invalid).unwrap_err();
        assert!(error.to_string().contains("channel_class_tail"));
    }
}

#[test]
fn act_table_row_derives_the_entity_offset_and_rejects_wire_drift() {
    let wire = serde_json::json!({
        "id": "stream:act-entity#7", "record_index": 7, "entity_id": "0_1",
        "in_table": true, "table_record_index_offset": 20, "table_entity_id_offset": 34,
        "channel_class_tag": "261", "channel_record_index_offset": 100,
        "channels": {"Appearance":"11111111-2222-3333-4444-555555555555"}, "channel_guid_offsets": {"Appearance":120}
    });
    let entity: crate::records::ActEntity = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(entity.table_entity_id_offset(), Some(34));
    assert_eq!(serde_json::to_value(entity).unwrap(), wire);
    for offset in [20, 33, 35, u64::MAX] {
        let mut invalid = wire.clone();
        invalid["table_entity_id_offset"] = serde_json::json!(offset);
        let error = serde_json::from_value::<crate::records::ActEntity>(invalid).unwrap_err();
        assert!(error.to_string().contains("table_entity_id_offset"));
    }
    assert!(crate::records::ActTableRow::new(u64::MAX - 13).is_err());
    assert!(crate::records::ActTableRow::new(u64::MAX - 14).is_ok());
}

#[test]
fn act_root_component_rejects_nonroot_tracking_on_the_wire() {
    let wire = serde_json::json!({
        "id": "stream:act-root-component#0", "byte_offset": 0,
        "record_index": 1, "record_index_offset": 7, "class_tag": "261",
        "instance_root_record": 2, "instance_root_record_offset": 22,
        "tracked_entity_record": 3, "tracked_entity_record_offset": 43,
        "components_root_record": 4, "components_root_record_offset": 63,
        "registry_flag": 0, "registry_flag_offset": 53,
        "entity_id": "0_3", "entity_id_offset": 36,
        "display_name": "", "display_name_offset": 61
    });
    let root: crate::records::ActRootComponent = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(root).unwrap(), wire);
    for record in [0, 1, 2, 4, u32::MAX] {
        let mut invalid = wire.clone();
        invalid["tracked_entity_record"] = serde_json::json!(record);
        let error =
            serde_json::from_value::<crate::records::ActRootComponent>(invalid).unwrap_err();
        assert!(error.to_string().contains("tracked_entity_record"));
    }
    for field in [
        "record_index_offset",
        "instance_root_record_offset",
        "entity_id_offset",
        "tracked_entity_record_offset",
        "registry_flag_offset",
        "display_name_offset",
        "components_root_record_offset",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<crate::records::ActRootComponent>(invalid).is_err(),
            "{field}"
        );
    }
}

#[test]
fn act_root_layout_derives_utf16_offsets_and_bounds_padding() {
    use crate::records::ActRootLayout;
    let layout = ActRootLayout::new(100, "0_3".into(), "😀".into(), 8).unwrap();
    assert_eq!(layout.record_index_offset(), 107);
    assert_eq!(layout.instance_root_record_offset(), 122);
    assert_eq!(layout.entity_id_offset(), 136);
    assert_eq!(layout.tracked_entity_record_offset(), 143);
    assert_eq!(layout.registry_flag_offset(), 153);
    assert_eq!(layout.display_name_offset(), 161);
    assert_eq!(layout.components_root_record_offset(), 174);
    for padding in [0, 9, u64::MAX] {
        assert!(ActRootLayout::new(0, "0_3".into(), String::new(), padding).is_err());
    }
    assert!(ActRootLayout::new(u64::MAX - 63, "0_3".into(), String::new(), 1).is_ok());
    assert!(ActRootLayout::new(u64::MAX - 62, "0_3".into(), String::new(), 1).is_err());
    assert!(ActRootLayout::new(0, String::new(), String::new(), 1).is_err());
}

#[test]
fn act_table_reference_derives_target_offset_and_rejects_wire_drift() {
    let wire = serde_json::json!({
        "id": "stream:act-table-reference#20", "ordinal": 0,
        "byte_offset": 20, "target_record": 3, "target_record_offset": 21
    });
    let reference: crate::records::ActTableReference =
        serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(reference.byte_offset(), 20);
    assert_eq!(serde_json::to_value(reference).unwrap(), wire);
    for offset in [0, 20, 22, u64::MAX] {
        let mut invalid = wire.clone();
        invalid["target_record_offset"] = serde_json::json!(offset);
        let error =
            serde_json::from_value::<crate::records::ActTableReference>(invalid).unwrap_err();
        assert!(error.to_string().contains("target_record_offset"));
    }
    assert!(crate::records::ActTableReference::new(
        format!("stream:act-table-reference#{}", u64::MAX),
        0,
        u64::MAX,
        3
    )
    .is_err());
    assert!(crate::records::ActTableReference::new(
        format!("stream:act-table-reference#{}", u64::MAX - 1),
        0,
        u64::MAX - 1,
        3
    )
    .is_ok());
}

#[test]
fn act_guid_derives_payload_offset_and_rejects_wire_drift() {
    let wire = serde_json::json!({
        "id": "stream:act-guid#20", "byte_offset": 20, "guid_offset": 24,
        "ordinal": 0, "guid": "01234567-89ab-cdef-0123-456789abcdef"
    });
    let guid: crate::records::ActGuid = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(guid.byte_offset(), 20);
    assert_eq!(guid.guid_offset(), 24);
    assert_eq!(serde_json::to_value(guid).unwrap(), wire);
    for offset in [0, 20, 23, 25, u64::MAX] {
        let mut invalid = wire.clone();
        invalid["guid_offset"] = serde_json::json!(offset);
        let error = serde_json::from_value::<crate::records::ActGuid>(invalid).unwrap_err();
        assert!(error.to_string().contains("guid_offset"));
    }
    for text in ["", "01234567-89ab-cdef-0123-456789abcdeg"] {
        let mut invalid = wire.clone();
        invalid["guid"] = serde_json::json!(text);
        let error = serde_json::from_value::<crate::records::ActGuid>(invalid).unwrap_err();
        assert!(error.to_string().contains("GUID"));
    }
    let text = "01234567-89ab-cdef-0123-456789abcdef";
    assert!(crate::records::ActGuid::new(
        format!("stream:act-guid#{}", u64::MAX - 3),
        u64::MAX - 3,
        0,
        text.into()
    )
    .is_err());
    assert!(crate::records::ActGuid::new(
        format!("stream:act-guid#{}", u64::MAX - 4),
        u64::MAX - 4,
        0,
        text.into()
    )
    .is_ok());
}

#[test]
fn act_registry_channel_derives_offsets_and_rejects_invalid_wire() {
    let wire = serde_json::json!({
        "id": "stream:act-registry-channel#20", "ordinal": 0, "byte_offset": 20,
        "name": "abc", "name_offset": 24,
        "guid": "01234567-89ab-cdef-0123-456789abcdef", "guid_offset": 31
    });
    let channel: crate::records::ActRegistryChannel = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(channel.byte_offset(), 20);
    assert_eq!(channel.name(), "abc");
    assert_eq!(channel.name_offset(), 24);
    assert_eq!(channel.guid_offset(), 31);
    assert_eq!(serde_json::to_value(channel).unwrap(), wire);
    for field in ["name_offset", "guid_offset"] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!(0);
        assert!(serde_json::from_value::<crate::records::ActRegistryChannel>(invalid).is_err());
    }
    for name in [String::new(), "x".repeat(129), "é".into()] {
        let mut invalid = wire.clone();
        invalid["name"] = serde_json::json!(name);
        assert!(serde_json::from_value::<crate::records::ActRegistryChannel>(invalid).is_err());
    }
    let guid = "01234567-89ab-cdef-0123-456789abcdef";
    assert!(crate::records::ActRegistryChannel::new(
        format!("stream:act-registry-channel#{}", u64::MAX - 10),
        0,
        u64::MAX - 10,
        "abc".into(),
        guid.into()
    )
    .is_err());
    assert!(crate::records::ActRegistryChannel::new(
        format!("stream:act-registry-channel#{}", u64::MAX - 11),
        0,
        u64::MAX - 11,
        "abc".into(),
        guid.into()
    )
    .is_ok());
    assert!(crate::records::ActRegistryChannel::new(
        "stream:act-registry-channel#0".into(),
        0,
        0,
        "abc".into(),
        String::new()
    )
    .is_err());
}
