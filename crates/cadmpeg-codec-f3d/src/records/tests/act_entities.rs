// SPDX-License-Identifier: Apache-2.0

use crate::records::act::{ActChannelGroup, ActClassTail, ActEntity, ActTableRow};
use std::collections::BTreeMap;

fn group_wire() -> serde_json::Value {
    serde_json::json!({
        "id":"stream:act-entity#7", "record_index":7, "entity_id":"0_1", "in_table":false,
        "channel_class_tag":"261",
        "channels":{"Appearance":"11111111-2222-3333-4444-555555555555"}
    })
}

fn channels(
    offset: u64,
) -> BTreeMap<String, crate::records::identity::Located<crate::records::mesh::DesignGuidText>> {
    BTreeMap::from([(
        "Appearance".into(),
        crate::records::identity::Located {
            value: "11111111-2222-3333-4444-555555555555"
                .to_owned()
                .try_into()
                .unwrap(),
            offset,
        },
    )])
}

#[test]
fn act_group_admission_rejects_invalid_names_counts_and_locations() {
    let channel = crate::records::identity::Located {
        value: "11111111-2222-3333-4444-555555555555"
            .to_owned()
            .try_into()
            .unwrap(),
        offset: 120,
    };
    for name in [String::new(), "x".repeat(129), "é".into()] {
        assert!(ActChannelGroup::try_new(
            100,
            Some(200),
            "261".to_owned().try_into().unwrap(),
            BTreeMap::from([(name, channel.clone())]),
            None
        )
        .is_err());
    }
    for count in [0, 1, 8, 9] {
        let channels = (0..count)
            .map(|index| {
                (
                    format!("Channel{index}"),
                    crate::records::identity::Located {
                        value: channel.value.clone(),
                        offset: 120 + 80 * index,
                    },
                )
            })
            .collect();
        assert_eq!(
            ActChannelGroup::try_new(
                100,
                Some(1000),
                "261".to_owned().try_into().unwrap(),
                channels,
                None
            )
            .is_ok(),
            (1..=8).contains(&count)
        );
    }
    for (field, value) in [
        ("channel_record_index_offset", 120),
        ("channel_entity_id_offset", 191),
    ] {
        let record = if field == "channel_record_index_offset" {
            value
        } else {
            100
        };
        let entity = if field == "channel_entity_id_offset" {
            value
        } else {
            200
        };
        assert!(ActChannelGroup::try_new(
            record,
            Some(entity),
            "261".to_owned().try_into().unwrap(),
            channels(120),
            None
        )
        .is_err());
    }
    for offset in [100, 129, u64::MAX] {
        assert!(ActChannelGroup::try_new(
            100,
            Some(200),
            "261".to_owned().try_into().unwrap(),
            channels(offset),
            None
        )
        .is_err());
    }
    for tail_offset in [100, 191, 200] {
        let tail = ActClassTail::new(vec![1], tail_offset).unwrap();
        assert!(ActChannelGroup::try_new(
            100,
            Some(200),
            "261".to_owned().try_into().unwrap(),
            channels(120),
            Some(tail)
        )
        .is_err());
    }
}

#[test]
fn act_entity_requires_a_group_and_a_table_or_inline_key() {
    let group = || {
        ActChannelGroup::try_new(
            100,
            None,
            "261".to_owned().try_into().unwrap(),
            channels(120),
            None,
        )
        .unwrap()
    };
    assert!(
        ActEntity::try_new("stream:act-entity#7".into(), 7, "0_1".into(), None, group()).is_err()
    );
    let mut wire = group_wire();
    wire["in_table"] = serde_json::json!(true);
    let entity = ActEntity::try_new(
        "stream:act-entity#7".into(),
        7,
        "0_1".into(),
        Some(ActTableRow::new(20).unwrap()),
        group(),
    )
    .unwrap();
    assert_eq!(entity.table_record_index_offset(), Some(20));
    assert_eq!(entity.table_entity_id_offset(), Some(34));
    assert_eq!(serde_json::to_value(entity).unwrap(), wire);
    for field in ["channel_class_tag", "channel_record_index_offset"] {
        wire.as_object_mut().unwrap().remove(field);
    }
    wire["channels"] = serde_json::json!({});
    wire["channel_guid_offsets"] = serde_json::json!({});
    assert!(serde_json::from_value::<ActEntity>(wire).is_err());
}

#[test]
fn act_entity_edits_keep_keys_and_channel_locations_valid() {
    let group = ActChannelGroup::try_new(
        100,
        Some(200),
        "261".to_owned().try_into().unwrap(),
        channels(120),
        None,
    )
    .unwrap();
    let mut entity =
        ActEntity::try_new("stream:act-entity#7".into(), 7, "0_1".into(), None, group).unwrap();
    for key in ["", "_1", "1_", "a_1", "1_2_3", "1_+2"] {
        let before = entity.clone();
        assert!(entity.try_set_entity_id(key.into()).is_err());
        assert_eq!(entity, before);
        let mut wire = group_wire();
        wire["entity_id"] = serde_json::json!(key);
        assert!(serde_json::from_value::<ActEntity>(wire).is_err());
    }
    entity.try_set_entity_id("000_001".into()).unwrap();
    assert_eq!(entity.entity_id(), "000_001");
    let offset = entity.channel_group().channels()["Appearance"].offset;
    entity
        .set_channel_guid(
            "Appearance",
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
                .to_owned()
                .try_into()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        entity.channel_group().channels()["Appearance"].offset,
        offset
    );
    assert!(entity
        .set_channel_guid(
            "Other",
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
                .to_owned()
                .try_into()
                .unwrap()
        )
        .is_err());
}
