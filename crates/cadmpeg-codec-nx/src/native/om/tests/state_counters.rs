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
    assert_eq!(rows[0].row_kind, 1);
    assert_eq!(rows[0].object_index, 0x320);
    assert_eq!(rows[0].raw_object_index, [0x83, 0x20]);
    assert_eq!(rows[0].introduced_state, 1);
    assert_eq!(rows[0].modified_state, 2);
    assert!(rows[0].object_index_source_offset > rows[0].source_offset);
    assert_eq!(rows[1].row_kind, 2);
    assert_eq!(rows[1].object_index, 0x1234);
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
    assert_eq!(rows[0].ordinal, 2);
    assert_eq!(rows[0].frame_selector, None);
    assert_eq!(rows[0].value_marker, 0xe0);
    assert_eq!(rows[1].ordinal, 3);
    assert_eq!(rows[1].frame_selector, Some(7));
    assert_eq!(rows[1].value_marker, 0xc0);
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
    assert_eq!(groups[0].rows[0].value_marker, 0xc0);
    assert_eq!(groups[0].rows[0].value, 0x0001_0203);
    assert_eq!(groups[0].rows[0].schema_id, 0x310);
    assert_eq!(groups[0].rows[0].state_ordinal, 2);
    assert_eq!(groups[1].selector, [0x05, 0x06]);
    assert_eq!(groups[1].rows[0].value_marker, 0xa0);
    assert_eq!(groups[1].rows[0].value, 0x0102);
    assert_eq!(groups[1].rows[0].state_ordinal, 3);
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
    assert_eq!(groups[0].declared_count, 3);
    assert_eq!(groups[0].rows.len(), 2);
    assert!(matches!(
        groups[0].rows[0],
        OmRollForwardStateRow::List {
            object_index: 0x3ba,
            ..
        }
    ));
    assert!(matches!(
        groups[1].rows[0],
        OmRollForwardStateRow::Pair {
            tag: 0x4f,
            first: 0x42d,
            second: 0x3e1,
            ..
        }
    ));
    assert_eq!(groups[2].declared_count, 0);
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
    assert_eq!(messages[0].text, "state warning");
    assert_eq!(messages[0].value_marker, 0xaa);
    assert_eq!(messages[0].value, 0x000a_606b);
    assert_eq!(messages[0].count_or_severity, 0x0100);
    assert_eq!(
        messages[0].severity,
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
    assert_eq!(statuses[0].status_code, 0x41);
    assert_eq!(statuses[0].object_index, 0x20);
    assert_eq!(statuses[0].raw_status_code, [0x41]);
    assert!(matches!(
        statuses[0].payload,
        crate::native::om::OmOperationStateStatusPayload::Plain
    ));
    assert_eq!(statuses[1].status_code, 0x44);
    assert_eq!(statuses[1].object_index, 0x21);
    assert!(matches!(
        statuses[1].payload,
        crate::native::om::OmOperationStateStatusPayload::Linked {
            link_code: 0x4b,
            object_index: 0x22,
            ..
        }
    ));

    let lanes = operation_state_slot_lanes(&container);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].slots.len(), 3);
    assert_eq!(lanes[0].slots[0].object_index, None);
    assert_eq!(lanes[0].slots[1].object_index, Some(0x3ad));
    assert_eq!(lanes[0].slots[2].object_index, None);

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
