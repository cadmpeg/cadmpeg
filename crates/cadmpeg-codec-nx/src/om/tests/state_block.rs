use super::*;

#[test]
fn operation_state_indices_retain_each_admitted_form() {
    let bytes = [
        0x7f, 0x83, 0xf9, 0x90, 0x12, 0x34, 0xa3, 0x1f, 0x85, 0xf1, 0x04, 0x2d, 0xff,
    ];
    let expected = [
        (Some(0x7f), OperationStateIndexForm::Direct, 1),
        (Some(0x3f9), OperationStateIndexForm::Compact, 2),
        (Some(0x1234), OperationStateIndexForm::Wide16, 3),
        (Some(0x31f85), OperationStateIndexForm::Wide20, 3),
        (Some(0x42d), OperationStateIndexForm::Extended16, 3),
        (None, OperationStateIndexForm::Null, 1),
    ];

    let mut at = 0;
    for (value, form, width) in expected {
        let token = super::operation_state_index(&bytes, at).expect("complete state index");
        assert_eq!(token.value, value);
        assert_eq!(token.form, form);
        assert_eq!(token.raw, &bytes[at..at + width]);
        assert_eq!(token.offset, at);
        at += width;
    }
    assert_eq!(at, bytes.len());
}

#[test]
fn operation_state_tagged_values_retain_width_and_value() {
    let cases = [
        (
            [0xaa, 0x60, 0x6b, 0, 0],
            OperationStateTaggedValueForm::Two,
            0x000a_606b,
            3,
        ),
        (
            [0xc0, 0x1a, 0x3f, 0x40, 0],
            OperationStateTaggedValueForm::Three,
            0x001a_3f40,
            4,
        ),
        (
            [0xe0, 0x01, 0x02, 0x03, 0x04],
            OperationStateTaggedValueForm::Four,
            0x0102_0304,
            5,
        ),
        (
            [0xff, 0x80, 0x00, 0x00, 0x01],
            OperationStateTaggedValueForm::Four,
            0x8000_0001,
            5,
        ),
    ];

    for (raw, form, value, width) in cases {
        let token = super::operation_state_tagged_value(&raw, 0).expect("complete tagged value");
        assert_eq!(token.form, form);
        assert_eq!(token.value, value);
        assert_eq!(token.marker, raw[0]);
        assert_eq!(token.raw, &raw[..width]);
        assert_eq!(token.offset, 0);
    }
}

#[test]
fn operation_state_counter_map_anchors_to_the_longest_bounded_suffix() {
    let mut bytes = vec![0x41, 0x83, 0x20, 0x3f];
    bytes.extend([
        0x05, 0x01, 0x90, 0x12, 0x34, 0x56, 0x57, 0x4e, 0x05, 0x02, 0xa3, 0x1f, 0x85, 0x2a, 0x2b,
        0x4e, 0x05, 0x01, 0x7d, 0x63, 0x63, 0x4e,
    ]);
    bytes.extend([
        0xb8, 0x6e, 0x58, 0x81, 0xd8, 0xb9, 0x96, 0x62, 0xdf, 0x59, 0xb8, 0x59, 0xc0, 0xd1, 0xf1,
        0xed,
    ]);

    let map = super::operation_state_counter_map(&bytes, 1000).expect("counter-map suffix");
    assert_eq!(map.offset, 1004);
    assert_eq!(map.rows.len(), 3);
    assert_eq!(map.end_offset, 1004 + 8 + 8 + 6);
    assert_eq!(map.trailing_bytes.len(), 16);
    assert_eq!(map.rows[0].row_kind, 1);
    assert_eq!(map.rows[0].object_index.value, Some(0x1234));
    assert_eq!(map.rows[0].introduced_state, 0x56);
    assert_eq!(map.rows[0].modified_state, 0x57);
    assert_eq!(map.rows[1].row_kind, 2);
    assert_eq!(map.rows[1].object_index.value, Some(0x31f85));
    assert_eq!(map.rows[1].introduced_state, 0x2a);
    assert_eq!(map.rows[1].modified_state, 0x2b);
    assert_eq!(
        map.rows[2].object_index.form,
        OperationStateIndexForm::Direct
    );
}

#[test]
fn operation_state_counter_map_rejects_a_short_non_suffix_lane() {
    let bytes = [
        0x05, 0x01, 0x12, 0x34, 0x56, 0x4e, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
        0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
        0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
        0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
        0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
    ];
    assert!(super::operation_state_counter_map(&bytes, 0).is_none());
}

fn message_bytes(text: &[u8], value: &[u8], count_or_severity: [u8; 2]) -> Vec<u8> {
    let declared_length = u8::try_from(text.len() + 2).expect("short synthesized message");
    let mut bytes = vec![0x03, declared_length];
    bytes.extend_from_slice(text);
    bytes.extend([0, 0, 0, 0, 0]);
    bytes.extend_from_slice(value);
    bytes.extend(count_or_severity);
    bytes
}

#[test]
fn operation_state_messages_decode_text_value_and_severity() {
    let bytes = message_bytes(b"hello", &[0xc0, 0x01, 0x02, 0x03], [0, 3]);
    let messages = super::operation_state_messages(&bytes, 500);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].offset, 500);
    assert_eq!(messages[0].declared_length, 7);
    assert_eq!(messages[0].text, "hello");
    assert_eq!(messages[0].value.form, OperationStateTaggedValueForm::Three);
    assert_eq!(messages[0].value.value, 0x0001_0203);
    assert_eq!(messages[0].count_or_severity, 3);
    assert_eq!(messages[0].end_offset, 500 + bytes.len());
}

#[test]
fn operation_state_status_table_retains_plain_link_diagnostic_and_opaque_rows() {
    let mut bytes = vec![
        0x41, 0x83, 0x20, 0x3f, 0x3e, 0x80, 0xac, 0x45, 0xff, 0x82, 0x52, 0xff, 0x3c, 0x81, 0x23,
    ];
    bytes.extend(message_bytes(b"bad curve", &[0xaa, 0x60, 0x6b], [0, 1]));
    bytes.extend([
        0x36, 0x83, 0xcf, 0x1e, 0x01, 0x41, 0xff, 0x83, 0xad, 0xff, 0x02, 0x11,
    ]);
    bytes.extend([0x02, 0x01, 0x11, 0xff, 0x83, 0xad, 0xff, 0x02, 0x11]);

    let table =
        super::operation_state_status_table(&bytes, 0, bytes.len(), 700).expect("status table");
    assert_eq!(table.rows.len(), 4);
    assert_eq!(table.rows[0].status_code.value, Some(0x41));
    assert!(matches!(
        table.rows[0].payload,
        OperationStateStatusPayload::Plain
    ));
    assert!(matches!(
        table.rows[1].payload,
        OperationStateStatusPayload::Linked {
            link_code: 0x45,
            ..
        }
    ));
    let OperationStateStatusPayload::Diagnostic { message } = table.rows[2].payload else {
        panic!("diagnostic row was not typed");
    };
    assert_eq!(message.text, "bad curve");
    let OperationStateStatusPayload::Opaque { raw } = table.rows[3].payload else {
        panic!("opaque state lane was not retained");
    };
    assert_eq!(raw, &[0x1e, 0x01, 0x41, 0xff, 0x83, 0xad, 0xff, 0x02, 0x11]);
    assert_eq!(table.slot_lanes.len(), 1);
    assert_eq!(table.slot_lanes[0].slots.len(), 3);
    assert_eq!(table.slot_lanes[0].slots[1].value, Some(0x3ad));
    assert_eq!(table.trailing_bytes, &b""[..]);
}

#[test]
fn operation_state_group_table_decodes_list_pair_and_empty_groups() {
    let bytes = [
        0x01, 0x00, 0x01, 0x03, 0x4a, 0x83, 0xba, 0x01, 0xff, 0x4a, 0x83, 0xb7, 0x02, 0xff, 0x01,
        0x01, 0x01, 0x02, 0x4f, 0xf1, 0x04, 0x2d, 0x83, 0xe1, 0xff, 0xff, 0x01, 0x01, 0x00,
    ];
    let table =
        super::operation_state_group_table(&bytes, 0, bytes.len(), 900).expect("group table");
    assert_eq!(table.groups.len(), 3);
    assert_eq!(table.groups[0].opener, [0x01, 0x00]);
    assert_eq!(table.groups[0].count_prefix, Some(1));
    assert_eq!(table.groups[0].rows.len(), 2);
    assert_eq!(table.groups[1].rows.len(), 1);
    let OperationStateGroupRow::Pair { tag, first, second } = table.groups[1].rows[0] else {
        panic!("pair group row was not typed");
    };
    assert_eq!(tag, 0x4f);
    assert_eq!(first.value, Some(0x42d));
    assert_eq!(second.value, Some(0x3e1));
    assert_eq!(table.groups[2].declared_count, 0);
    assert_eq!(table.groups[2].rows.len(), 0);
}

#[test]
fn operation_state_journal_decodes_timestamp_value_schema_and_ordinal() {
    let bytes = [
        0x04, 0x01, 0x02, 0x00, 0x00, 0xe0, 0x65, 0x53, 0x4d, 0x20, 0xc0, 0x01, 0x02, 0x03, 0x83,
        0x10, 0x2a, 0x13,
    ];
    let groups = super::operation_state_journal(&bytes, 0, bytes.len(), 1100).expect("journal");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].selector, [0x01, 0x02]);
    assert_eq!(groups[0].rows.len(), 1);
    let row = groups[0].rows[0];
    assert_eq!(row.timestamp, 0x6553_4d20);
    assert_eq!(row.value.value, 0x0001_0203);
    assert_eq!(row.schema_id.value, Some(0x310));
    assert_eq!(row.ordinal.value, Some(0x2a));
}
