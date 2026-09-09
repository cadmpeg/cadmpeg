// SPDX-License-Identifier: Apache-2.0

use crate::native::om::roll_forward::{OmRollForwardStateGroup, OmRollForwardStateTable};
use crate::native::om::state_slot_lane::OmOperationStateSlotLane;
use crate::native::om::state_status::OmOperationStateStatus;
use crate::om::roll_forward::OperationStateGroupRow;
use crate::om::state_message::{StateMessage, StateMessageSeverity};
use crate::om::state_status::StateStatusPayload;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::native::features::FeatureOperationStateJournalUse;
use crate::native::om::journal_group::OmOperationStateJournalGroup;
use crate::native::om::{
    audit_trail_rows, operation_state_counters, operation_state_groups,
    operation_state_journal_groups, operation_state_messages, operation_state_slot_lanes,
    operation_state_statuses, OmAuditTrailRow, OmOperationStateCounter, OmOperationStateMessage,
};
use crate::test_support::{
    composed_feature_history_payload_with_operation_state_statuses,
    composed_feature_history_payload_with_state_journal, prt_with_named_payloads,
    segment_om_record_area_with_state_counter_map,
    segment_om_record_area_with_state_groups_and_counter_map,
    size_framed_audit_trail_section_with_record_area,
};
use crate::NxCodec;

#[test]
fn operation_state_message_severity_uses_only_known_high_bytes() {
    assert_eq!(
        StateMessageSeverity::from_word(0x01ff),
        Some(StateMessageSeverity::Alert)
    );
    assert_eq!(
        StateMessageSeverity::from_word(0x0300),
        Some(StateMessageSeverity::Failure)
    );
    assert_eq!(StateMessageSeverity::from_word(0x0003), None);
}

#[test]
fn native_catalog_emits_feature_history_state_counter_rows() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_om_record_area_with_state_counter_map(),
    )]);
    let container = container::scan_bytes(file).expect("required invariant");

    let rows = operation_state_counters(&container);
    assert_eq!(rows.len(), 2);
    assert_eq!(u8::from(rows[0].frame.kind()), 1);
    assert_eq!(rows[0].frame.object().value(), 0x320);
    assert_eq!(rows[0].frame.object().raw(), [0x83, 0x20]);
    assert_eq!(rows[0].frame.introduced(), 1);
    assert_eq!(rows[0].frame.modified(), 2);
    assert!(rows[0].frame.object_offset() > rows[0].frame.offset());
    assert_eq!(u8::from(rows[1].frame.kind()), 2);
    assert_eq!(rows[1].frame.object().value(), 0x1234);
    assert_eq!(rows[1].ordinal, 1);
    assert_eq!(rows[0].section_link, rows[1].section_link);

    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_om_record_area_with_state_counter_map(),
            )])),
            &DecodeOptions::default(),
        )
        .expect("native decode");
    let emitted = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<OmOperationStateCounter>("om_operation_state_counters")
        .expect("state-counter arena");
    assert_eq!(emitted, rows.as_slice());
}

#[test]
fn native_catalog_emits_role_gated_audit_trail_rows() {
    let payload = audit_trail_test_payload();
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let container = container::scan_bytes(file).expect("required invariant");

    let rows = audit_trail_rows(&container);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].record().ordinal.value(), 2);
    assert_eq!(rows[0].record().frame_selector, None);
    assert_eq!(rows[0].record().value.marker(), 0xe0);
    assert_eq!(rows[1].record().ordinal.value(), 3);
    assert_eq!(rows[1].record().frame_selector, Some(7));
    assert_eq!(rows[1].record().value.marker(), 0xc0);
    assert!(rows[1].source_offset() > rows[0].source_offset());

    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                audit_trail_test_payload(),
            )])),
            &DecodeOptions::default(),
        )
        .expect("native decode");
    let emitted = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<OmAuditTrailRow>("om_audit_trail_rows")
        .expect("audit-trail row arena");
    assert_eq!(emitted, rows.as_slice());
}

fn audit_trail_test_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&size_framed_audit_trail_section_with_record_area());
    payload
}

#[test]
fn native_catalog_emits_anchored_operation_state_journal_groups() {
    let payload = composed_feature_history_payload_with_state_journal();
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload.clone())]);
    let container = container::scan_bytes(file).expect("required invariant");

    let groups = operation_state_journal_groups(&container);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].frame.selector(), [0x01, 0x02]);
    assert_eq!(groups[0].frame.rows().len(), 1);
    assert_eq!(groups[0].frame.rows().first().value().marker(), 0xc0);
    assert_eq!(groups[0].frame.rows().first().value().value(), 0x0001_0203);
    assert_eq!(groups[0].frame.rows().first().schema().value(), 0x310);
    assert_eq!(groups[0].frame.rows().first().ordinal().value(), 2);
    assert_eq!(groups[1].frame.selector(), [0x05, 0x06]);
    assert_eq!(groups[1].frame.rows().first().value().marker(), 0xa0);
    assert_eq!(groups[1].frame.rows().first().value().value(), 0x0102);
    assert_eq!(groups[1].frame.rows().first().ordinal().value(), 3);
    assert!(groups[1].frame.offset() > groups[0].frame.offset());

    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                payload,
            )])),
            &DecodeOptions::default(),
        )
        .expect("native decode");
    let emitted = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<OmOperationStateJournalGroup>("om_operation_state_journal_groups")
        .expect("state-journal arena");
    assert_eq!(emitted, groups.as_slice());

    let uses = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<FeatureOperationStateJournalUse>("feature_operation_state_journal_uses")
        .expect("operation-state journal use arena");
    assert_eq!(uses.len(), 1);
    assert_eq!(
        serde_json::to_value(&uses[0]).unwrap()["operation_local_ordinal"],
        2
    );
    assert_eq!(
        serde_json::to_value(&uses[0]).unwrap()["journal_state_ordinal"],
        2
    );
    assert_eq!(uses[0].journal_row_ordinal, 0);
}

#[test]
fn native_catalog_emits_field_declared_roll_forward_groups() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_om_record_area_with_state_groups_and_counter_map(),
    )]);
    let container = container::scan_bytes(file).expect("required invariant");

    let tables = operation_state_groups(&container).unwrap();
    let groups = tables
        .iter()
        .flat_map(OmRollForwardStateTable::groups)
        .collect::<Vec<_>>();
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].frame.members().count().declared_count(), 3);
    assert_eq!(groups[0].frame.members().rows().len(), 2);
    assert!(matches!(
        groups[0].frame.members().rows()[0],
        OperationStateGroupRow::List {
            object_index,
            ..
        } if object_index.value() == 0x3ba
    ));
    assert!(matches!(
        groups[1].frame.members().rows()[0],
        OperationStateGroupRow::Pair {
            tag: crate::om::discriminators::OperationStatePairTag::Form4f,
            first,
            second,
            ..
        } if first.value() == 0x42d && second.value() == 0x3e1
    ));
    assert_eq!(groups[2].frame.members().count().declared_count(), 0);
    assert_eq!(groups[0].table_footer.bytes(), [0x01, 0x01]);
    assert!(groups[0].table_end_offset > groups[0].frame.offset());

    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_om_record_area_with_state_groups_and_counter_map(),
            )])),
            &DecodeOptions::default(),
        )
        .expect("native decode");
    let emitted = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<OmRollForwardStateGroup>("om_roll_forward_state_groups")
        .expect("roll-forward group arena");
    assert_eq!(emitted.iter().collect::<Vec<_>>(), groups);
}

#[test]
fn native_catalog_emits_bounded_operation_state_messages() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_om_record_area_with_state_groups_and_counter_map(),
    )]);
    let container = container::scan_bytes(file).expect("required invariant");

    let messages = operation_state_messages(&container);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].body.text.as_str(), "state warning");
    assert_eq!(messages[0].body.value.marker(), 0xaa);
    assert_eq!(messages[0].body.value.value(), 0x000a_606b);
    assert_eq!(messages[0].body.count_or_severity, 0x0100);
    assert_eq!(
        messages[0].body.severity(),
        Some(StateMessageSeverity::Alert)
    );

    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_om_record_area_with_state_groups_and_counter_map(),
            )])),
            &DecodeOptions::default(),
        )
        .expect("native decode");
    let emitted = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<OmOperationStateMessage>("om_operation_state_messages")
        .expect("state-message arena");
    assert_eq!(emitted, messages.as_slice());
}

#[test]
fn native_catalog_emits_bounded_operation_state_statuses_and_slot_lanes() {
    let payload = composed_feature_history_payload_with_operation_state_statuses();
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload.clone())]);
    let container = container::scan_bytes(file).expect("required invariant");

    let statuses = operation_state_statuses(&container);
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].body().status_code.value(), 0x41);
    assert_eq!(statuses[0].body().object_index.value(), 0x20);
    assert_eq!(statuses[0].body().status_code.raw(), [0x41]);
    assert!(matches!(
        statuses[0].body().payload,
        StateStatusPayload::Plain
    ));
    assert_eq!(statuses[1].body().status_code.value(), 0x44);
    assert_eq!(statuses[1].body().object_index.value(), 0x21);
    assert!(matches!(
        statuses[1].body().payload,
        StateStatusPayload::Linked {
            link_code,
            object_index,
            ..
        } if u8::from(link_code) == 0x4b && object_index.value() == 0x22
    ));

    let lanes = operation_state_slot_lanes(&container);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].frame.slots().len(), 3);
    assert_eq!(
        lanes[0].frame.slots().as_slice()[0].map(crate::om::state_index::StateIndexToken::value),
        None
    );
    assert_eq!(
        lanes[0].frame.slots().as_slice()[1].map(crate::om::state_index::StateIndexToken::value),
        Some(0x3ad)
    );
    assert_eq!(
        lanes[0].frame.slots().as_slice()[2].map(crate::om::state_index::StateIndexToken::value),
        None
    );

    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                payload,
            )])),
            &DecodeOptions::default(),
        )
        .expect("native decode");
    let namespace = result.ir().native.namespace("nx").expect("NX namespace");
    let emitted_statuses = namespace
        .arena_as::<OmOperationStateStatus>("om_operation_state_statuses")
        .expect("state-status arena");
    let emitted_lanes = namespace
        .arena_as::<OmOperationStateSlotLane>("om_operation_state_slot_lanes")
        .expect("state-slot-lane arena");
    assert_eq!(emitted_statuses, statuses.as_slice());
    assert_eq!(emitted_lanes, lanes.as_slice());
}

#[test]
fn message_body_preserves_flat_tagged_value_wire() {
    let json = r#"{"declared_length":3,"text":"A","value_marker":160,"value":0,"raw_value":[160,0,0],"count_or_severity":0}"#;
    let body: StateMessage<String> = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&body).unwrap(), json);
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    wire["value"] = 1.into();
    assert!(serde_json::from_value::<StateMessage<String>>(wire)
        .unwrap_err()
        .to_string()
        .contains("value"));
}

#[test]
fn message_body_rejects_text_length_mismatch() {
    let json = r#"{"declared_length":4,"text":"A","value_marker":160,"value":0,"raw_value":[160,0,0],"count_or_severity":0}"#;
    assert!(serde_json::from_str::<StateMessage<String>>(json)
        .unwrap_err()
        .to_string()
        .contains("declared_length"));
}

#[test]
fn roll_forward_groups_preserve_zero_row_headers_and_reject_count_mismatch() {
    for (prefix, count) in [("null", 0), ("1", 0), ("1", 1)] {
        let json = format!(
            r#"{{"id":"group","section_link":"section","ordinal":0,"opener":[1,0],"count_prefix":{prefix},"declared_count":{count},"rows":[],"table_trailing_bytes":[],"source_entry":"om","source_offset":0,"table_end_offset":4}}"#
        );
        let group: OmRollForwardStateGroup = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&group).unwrap(), json);
    }
    let json = r#"{"id":"group","section_link":"section","ordinal":0,"opener":[1,0],"count_prefix":1,"declared_count":2,"rows":[],"table_trailing_bytes":[],"source_entry":"om","source_offset":0,"table_end_offset":4}"#;
    assert!(serde_json::from_str::<OmRollForwardStateGroup>(json)
        .unwrap_err()
        .to_string()
        .contains("declared_count/rows"));
}

#[test]
fn roll_forward_group_derives_row_ordinals() {
    let json = r#"{"id":"group","section_link":"section","ordinal":0,"opener":[1,0],"count_prefix":1,"declared_count":2,"rows":[{"List":{"ordinal":0,"object_index":1,"raw_object_index":[1],"position":1,"raw_position":[1],"source_offset":4}}],"table_trailing_bytes":[],"source_entry":"om","source_offset":0,"table_end_offset":8}"#;
    let group: OmRollForwardStateGroup = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&group).unwrap(), json);
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    wire["rows"][0]["List"]["ordinal"] = 1.into();
    assert!(serde_json::from_value::<OmRollForwardStateGroup>(wire)
        .unwrap_err()
        .to_string()
        .contains("rows.ordinal"));
}

#[test]
fn state_slot_lane_derives_ordinals_and_preserves_null_tokens() {
    let json = r#"{"id":"lane","section_link":"section","ordinal":0,"slots":[{"ordinal":0,"object_index":null,"raw_object_index":[255]},{"ordinal":1,"object_index":255,"raw_object_index":[144,0,255]}],"source_entry":"om","source_offset":0,"end_offset":9}"#;
    let lane: OmOperationStateSlotLane = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&lane).unwrap(), json);
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    wire["slots"][1]["ordinal"] = 0.into();
    assert!(serde_json::from_value::<OmOperationStateSlotLane>(wire)
        .unwrap_err()
        .to_string()
        .contains("slots.ordinal"));
}
