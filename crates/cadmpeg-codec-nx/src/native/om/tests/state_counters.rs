// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::native::features::FeatureOperationStateJournalUse;
use crate::native::om::{
    audit_trail_rows, operation_state_counters, operation_state_groups,
    operation_state_journal_groups, operation_state_messages, operation_state_slot_lanes,
    operation_state_statuses, OmAuditTrailRow, OmOperationStateCounter,
    OmOperationStateJournalGroup, OmOperationStateMessage, OmOperationStateMessageSeverity,
    OmOperationStateSlotLane, OmOperationStateStatus, OmRollForwardStateGroup,
    OmRollForwardStateRow,
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
        super::super::operation_state_message_severity(0x01ff),
        Some(OmOperationStateMessageSeverity::Alert)
    );
    assert_eq!(
        super::super::operation_state_message_severity(0x0300),
        Some(OmOperationStateMessageSeverity::Failure)
    );
    assert_eq!(super::super::operation_state_message_severity(0x0003), None);
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
    assert_eq!(u8::from(rows[0].row_kind), 1);
    assert_eq!(rows[0].object_index.value(), 0x320);
    assert_eq!(rows[0].object_index.raw(), [0x83, 0x20]);
    assert_eq!(rows[0].introduced_state, 1);
    assert_eq!(rows[0].modified_state, 2);
    assert!(rows[0].object_index_source_offset > rows[0].source_offset);
    assert_eq!(u8::from(rows[1].row_kind), 2);
    assert_eq!(rows[1].object_index.value(), 0x1234);
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
    assert_eq!(rows[0].ordinal.value(), 2);
    assert_eq!(rows[0].frame_selector, None);
    assert_eq!(rows[0].value.marker(), 0xe0);
    assert_eq!(rows[1].ordinal.value(), 3);
    assert_eq!(rows[1].frame_selector, Some(7));
    assert_eq!(rows[1].value.marker(), 0xc0);
    assert!(rows[1].source_offset > rows[0].source_offset);

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
    assert_eq!(groups[0].selector, [0x01, 0x02]);
    assert_eq!(groups[0].rows.len(), 1);
    assert_eq!(groups[0].rows[0].value.marker(), 0xc0);
    assert_eq!(groups[0].rows[0].value.value(), 0x0001_0203);
    assert_eq!(groups[0].rows[0].schema_id.value(), 0x310);
    assert_eq!(groups[0].rows[0].state_ordinal.value(), 2);
    assert_eq!(groups[1].selector, [0x05, 0x06]);
    assert_eq!(groups[1].rows[0].value.marker(), 0xa0);
    assert_eq!(groups[1].rows[0].value.value(), 0x0102);
    assert_eq!(groups[1].rows[0].state_ordinal.value(), 3);
    assert!(groups[1].source_offset > groups[0].source_offset);

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
    assert_eq!(uses[0].operation_local_ordinal, 2);
    assert_eq!(uses[0].journal_state_ordinal, 2);
    assert_eq!(uses[0].journal_row_ordinal, 0);
}

#[test]
fn native_catalog_emits_field_declared_roll_forward_groups() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_om_record_area_with_state_groups_and_counter_map(),
    )]);
    let container = container::scan_bytes(file).expect("required invariant");

    let groups = operation_state_groups(&container);
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].members.count().declared_count(), 3);
    assert_eq!(groups[0].members.rows().len(), 2);
    assert!(matches!(
        groups[0].members.rows()[0],
        OmRollForwardStateRow::List {
            object_index,
            ..
        } if object_index.value() == 0x3ba
    ));
    assert!(matches!(
        groups[1].members.rows()[0],
        OmRollForwardStateRow::Pair {
            tag: crate::om::discriminators::OperationStatePairTag::Form4f,
            first,
            second,
            ..
        } if first.value() == 0x42d && second.value() == 0x3e1
    ));
    assert_eq!(groups[2].members.count().declared_count(), 0);
    assert_eq!(groups[0].table_trailing_bytes, [0x01, 0x01]);
    assert!(groups[0].table_end_offset > groups[0].source_offset);

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
    assert_eq!(emitted, groups.as_slice());
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
        Some(OmOperationStateMessageSeverity::Alert)
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
    assert_eq!(statuses[0].status_code.value(), 0x41);
    assert_eq!(statuses[0].object_index.value(), 0x20);
    assert_eq!(statuses[0].status_code.raw(), [0x41]);
    assert!(matches!(
        statuses[0].payload,
        crate::native::om::OmOperationStateStatusPayload::Plain
    ));
    assert_eq!(statuses[1].status_code.value(), 0x44);
    assert_eq!(statuses[1].object_index.value(), 0x21);
    assert!(matches!(
        statuses[1].payload,
        crate::native::om::OmOperationStateStatusPayload::Linked {
            link_code: 0x4b,
            object_index,
            ..
        } if object_index.value() == 0x22
    ));

    let lanes = operation_state_slot_lanes(&container);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].slots.len(), 3);
    assert_eq!(lanes[0].slots[0].object_index.map(crate::om::state_index::StateIndexToken::value), None);
    assert_eq!(lanes[0].slots[1].object_index.map(crate::om::state_index::StateIndexToken::value), Some(0x3ad));
    assert_eq!(lanes[0].slots[2].object_index.map(crate::om::state_index::StateIndexToken::value), None);

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
    let body: super::OmOperationStateMessageBody = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&body).unwrap(), json);
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    wire["value"] = 1.into();
    assert!(serde_json::from_value::<super::OmOperationStateMessageBody>(wire)
        .unwrap_err().to_string().contains("value"));
}

#[test]
fn message_body_rejects_text_length_mismatch() {
    let json = r#"{"declared_length":4,"text":"A","value_marker":160,"value":0,"raw_value":[160,0,0],"count_or_severity":0}"#;
    assert!(serde_json::from_str::<super::OmOperationStateMessageBody>(json)
        .unwrap_err().to_string().contains("declared_length"));
}

#[test]
fn roll_forward_groups_preserve_zero_row_headers_and_reject_count_mismatch() {
    for (prefix, count) in [("null", 0), ("1", 0), ("1", 1)] {
        let json = format!(r#"{{"id":"group","section_link":"section","ordinal":0,"opener":[1,0],"count_prefix":{prefix},"declared_count":{count},"rows":[],"table_trailing_bytes":[],"source_entry":"om","source_offset":0,"table_end_offset":4}}"#);
        let group: OmRollForwardStateGroup = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&group).unwrap(), json);
    }
    let json = r#"{"id":"group","section_link":"section","ordinal":0,"opener":[1,0],"count_prefix":1,"declared_count":2,"rows":[],"table_trailing_bytes":[],"source_entry":"om","source_offset":0,"table_end_offset":4}"#;
    assert!(serde_json::from_str::<OmRollForwardStateGroup>(json)
        .unwrap_err().to_string().contains("declared_count/rows"));
}
