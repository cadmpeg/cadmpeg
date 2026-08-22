// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::native::om::{
    operation_state_counters, operation_state_groups, operation_state_messages,
    operation_state_slot_lanes, operation_state_statuses, OmOperationStateCounter,
    OmOperationStateMessage, OmOperationStateSlotLane, OmOperationStateStatus,
    OmRollForwardStateGroup, OmRollForwardStateRow,
};
use crate::test_support::{
    composed_feature_history_payload_with_operation_state_statuses, prt_with_named_payloads,
    segment_om_record_area_with_state_counter_map,
    segment_om_record_area_with_state_groups_and_counter_map,
};
use crate::NxCodec;

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
