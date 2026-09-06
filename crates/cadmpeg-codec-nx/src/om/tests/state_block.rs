use super::*;

#[test]
fn operation_state_indices_retain_each_admitted_form() {
    let bytes = [
        0x7f, 0x83, 0xf9, 0x90, 0x12, 0x34, 0xa3, 0x1f, 0x85, 0xf1, 0x04, 0x2d, 0xff,
    ];
    let expected = [
        (Some(0x7f), 1),
        (Some(0x3f9), 2),
        (Some(0x1234), 3),
        (Some(0x31f85), 3),
        (Some(0x42d), 3),
        (None, 1),
    ];

    let mut at = 0;
    for (value, width) in expected {
        let token = crate::om::state_index::OperationStateIndex::read_at(&bytes, at, 0).expect("complete state index");
        assert_eq!(token.token().map(crate::om::state_index::StateIndexToken::value), value);
        assert_eq!(token.raw(), &bytes[at..at + width]);
        assert_eq!(token.offset(), at);
        at += width;
    }
    assert_eq!(at, bytes.len());
}

#[test]
fn operation_state_tagged_values_retain_width_and_value() {
    let cases = [
        ([0xaa, 0x60, 0x6b, 0, 0], 0x000a_606b, 3),
        ([0xc0, 0x1a, 0x3f, 0x40, 0], 0x001a_3f40, 4),
        ([0xe0, 0x01, 0x02, 0x03, 0x04], 0x0102_0304, 5),
        ([0xff, 0x80, 0x00, 0x00, 0x01], 0x8000_0001, 5),
    ];

    for (raw, value, width) in cases {
        let token =
            crate::om::state_tagged_value::StateTaggedValue::read_at(&raw, 0).expect("complete tagged value");
        assert_eq!(token.value(), value);
        assert_eq!(token.marker(), raw[0]);
        assert_eq!(token.raw(), &raw[..width]);
    }
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
    assert_eq!(messages[0].span.offset(), 500);
    assert_eq!(messages[0].text.declared_length(), 7);
    assert_eq!(messages[0].text.as_str(), "hello");
    assert_eq!(messages[0].value.raw().len(), 4);
    assert_eq!(messages[0].value.value(), 0x0001_0203);
    assert_eq!(messages[0].count_or_severity, 3);
    assert_eq!(messages[0].span.end_offset(), 500 + bytes.len());
}

#[test]
fn operation_state_messages_accept_terminal_count_shared_with_group_opener() {
    let mut bytes = message_bytes(b"terminal", &[0xaa, 0x39, 0x4e], [1, 0]);
    let group_start = bytes.len() - 2;
    bytes.extend([0x01, 0x02, 0x4a, 0x83, 0x20, 0x01, 0xff]);
    let table = super::operation_state_group_table(&bytes, group_start, bytes.len(), 500)
        .expect("group table");
    let messages = super::operation_state_block_before_boundary(&bytes, 0, group_start + 2, 500)
        .expect("terminal message")
        .messages;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text.as_str(), "terminal");
    assert_eq!(messages[0].span.end_offset(), 500 + group_start + 2);
    assert_eq!(table.offset, 500 + group_start);
    assert_eq!(table.groups[0].opener.bytes(), [0x01, 0x00]);
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
    assert_eq!(table.rows[0].status_code.value(), 0x41);
    assert!(matches!(
        table.rows[0].payload,
        OperationStateStatusPayload::Plain
    ));
    assert!(matches!(
        table.rows[1].payload,
        OperationStateStatusPayload::Linked {
            link_code,
            ..
        } if u8::from(link_code) == 0x45
    ));
    let OperationStateStatusPayload::Diagnostic { message } = table.rows[2].payload else {
        panic!("diagnostic row was not typed");
    };
    assert_eq!(message.text.as_str(), "bad curve");
    let OperationStateStatusPayload::Opaque { raw } = table.rows[3].payload else {
        panic!("opaque state lane was not retained");
    };
    assert_eq!(raw, &[0x1e, 0x01, 0x41, 0xff, 0x83, 0xad, 0xff, 0x02, 0x11]);
    assert_eq!(table.slot_lanes.len(), 1);
    assert_eq!(table.slot_lanes[0].slots.len(), 3);
    assert_eq!(table.slot_lanes[0].slots.as_slice()[1].token().map(crate::om::state_index::StateIndexToken::value), Some(0x3ad));
    assert_eq!(table.trailing_bytes, &b""[..]);
}

#[test]
fn operation_state_block_keeps_inline_diagnostics_out_of_standalone_messages() {
    let mut bytes = vec![0x3c, 0x81, 0x23];
    let diagnostic = message_bytes(b"inline", &[0xaa, 0x60, 0x6b], [0, 1]);
    bytes.extend_from_slice(&diagnostic);
    bytes.extend(message_bytes(b"standalone", &[0xaa, 0x39, 0x4e], [0, 2]));

    let block = super::operation_state_block_before_boundary(&bytes, 0, bytes.len(), 500)
        .expect("complete operation-state block");
    assert_eq!(block.rows.len(), 1);
    assert!(matches!(
        block.rows[0].payload,
        OperationStateStatusPayload::Diagnostic { .. }
    ));
    assert_eq!(block.messages.len(), 1);
    assert_eq!(block.messages[0].text.as_str(), "standalone");
    assert_eq!(block.status_end_offset, 500 + 3 + diagnostic.len());
}

#[test]
fn operation_state_status_table_ignores_incomplete_preceding_operation_lane() {
    let mut bytes = vec![
        0x41, 0x80, 0x01, 0x3f, 0x31, 0x80, 0x55, 0x87, 0xb3, 0xff, 0x81, 0x36, 0xff, 0x41, 0x80,
        0x20, 0x3f, 0x44, 0x80, 0x21, 0x4b, 0xff, 0x80, 0x22, 0xff,
    ];
    let message = message_bytes(b"boundary", &[0xaa, 0x01, 0x02], [0, 1]);
    let boundary = bytes.len();
    bytes.extend(message);

    let block = super::operation_state_block_before_boundary(&bytes, 0, boundary, 500)
        .expect("complete status chain");
    assert_eq!(block.offset, 500 + 13);
    assert_eq!(block.rows.len(), 2);
    assert_eq!(Some(block.rows[0].object_index.value()), Some(0x20));
    assert_eq!(block.rows[1].status_code.value(), 0x44);
    assert_eq!(block.status_end_offset, 500 + boundary);
}

#[test]
fn operation_state_block_stops_before_untyped_tail() {
    let mut bytes = vec![
        0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff,
    ];
    let status_end = bytes.len();
    bytes.extend([0x31, 0x80, 0x01, 0x01, 0x02, 0x55, 0x99]);

    let block = super::operation_state_block_before_boundary(&bytes, 0, bytes.len(), 500)
        .expect("status chain before bounded tail");
    assert_eq!(block.offset, 500);
    assert_eq!(block.rows.len(), 2);
    assert!(block.messages.is_empty());
    assert_eq!(block.status_end_offset, 500 + status_end);
}

#[test]
fn operation_state_block_keeps_a_large_opaque_prefix_sparse() {
    const OPAQUE_PREFIX_BYTES: usize = 128 * 1024;
    let mut bytes = vec![0xf0; OPAQUE_PREFIX_BYTES];
    let status_start = bytes.len();
    bytes.extend([
        0x41, 0x83, 0x20, 0x3f, 0x44, 0x83, 0x21, 0x4b, 0xff, 0x83, 0x22, 0xff,
    ]);
    let boundary = bytes.len();

    let block = super::operation_state_block_before_boundary(&bytes, 0, boundary, 500)
        .expect("status chain after large opaque prefix");
    assert_eq!(block.offset, 500 + status_start);
    assert_eq!(block.rows.len(), 2);
    assert!(block.messages.is_empty());
    assert_eq!(block.status_end_offset, 500 + boundary);
}

#[test]
fn operation_state_block_prefers_boundary_closed_path() {
    let mut bytes = vec![
        0x41, 0x80, 0x01, 0x3f, 0x41, 0x80, 0x02, 0x3f, 0x41, 0x80, 0x03, 0x3f, 0x31, 0x80, 0x04,
        0x01,
    ];
    let closed_path_start = bytes.len();
    bytes.extend([0x44, 0x80, 0x05, 0x3f]);
    bytes.extend(message_bytes(b"closed", &[0xaa, 0x01, 0x02], [0, 1]));

    let block = super::operation_state_block_before_boundary(&bytes, 0, bytes.len(), 500)
        .expect("boundary-closed state path");
    assert_eq!(block.offset, 500 + closed_path_start);
    assert_eq!(block.rows.len(), 1);
    assert_eq!(block.messages.len(), 1);
    assert_eq!(block.messages[0].text.as_str(), "closed");
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
    assert_eq!(table.groups[0].opener.bytes(), [0x01, 0x00]);
    assert_eq!(table.groups[0].members.count().prefix(), Some(1));
    assert_eq!(table.groups[0].members.rows().len(), 2);
    assert_eq!(table.groups[1].members.rows().len(), 1);
    let OperationStateGroupRow::Pair {
        tag, first, second, ..
    } = table.groups[1].members.rows()[0]
    else {
        panic!("pair group row was not typed");
    };
    assert_eq!(u8::from(tag), 0x4f);
    assert_eq!(Some(first.value()), Some(0x42d));
    assert_eq!(Some(second.value()), Some(0x3e1));
    assert_eq!(table.groups[2].members.count().declared_count(), 0);
    assert_eq!(table.groups[2].members.rows().len(), 0);
}

#[test]
fn operation_state_group_table_anchors_to_counter_map_boundary() {
    let mut bytes = vec![0xaa, 0xbb, 0xcc];
    bytes.extend([
        0x01, 0x00, 0x01, 0x03, 0x4a, 0x83, 0xba, 0x01, 0xff, 0x4a, 0x83, 0xb7, 0x02, 0xff, 0x01,
        0x01, 0x01, 0x02, 0x4f, 0xf1, 0x04, 0x2d, 0x83, 0xe1, 0xff, 0xff, 0x01, 0x01, 0x00, 0x01,
        0x01,
    ]);
    bytes.extend([
        0x05, 0x01, 0x83, 0x20, 0x01, 0x02, 0x4e, 0x05, 0x02, 0x90, 0x12, 0x34, 0x03, 0x04, 0x4e,
    ]);
    bytes.extend([0x99; 16]);

    let map = crate::om::state_counter::StateCounterMap::read(&bytes, 0).expect("counter map");
    let table = super::operation_state_group_table_before_counter_map(&bytes, map.offset(), 0)
        .expect("group table");
    assert_eq!(table.offset, 3);
    assert_eq!(table.end_offset, map.offset());
    assert_eq!(table.groups.len(), 3);
    assert_eq!(table.groups[0].members.rows().len(), 2);
    assert_eq!(table.groups[1].members.rows().len(), 1);
    assert_eq!(table.groups[2].members.count().declared_count(), 0);
    assert_eq!(table.trailing_bytes, &[0x01, 0x01]);
}

#[test]
fn operation_state_group_table_handles_a_long_adjacent_group_run() {
    const GROUP_COUNT: usize = 4096;
    let mut bytes = Vec::with_capacity(GROUP_COUNT * 3 + 12);
    for _ in 0..GROUP_COUNT {
        bytes.extend([0x01, 0x00, 0x00]);
    }
    let map_start = bytes.len();
    bytes.extend([0x05, 0x01, 0x00, 0x01, 0x01, 0x4e]);
    bytes.extend([0x05, 0x02, 0x01, 0x01, 0x01, 0x4e]);

    let table = super::operation_state_group_table_before_counter_map(&bytes, map_start, 0)
        .expect("long adjacent group run");
    assert_eq!(table.groups.len(), GROUP_COUNT);
    assert_eq!(table.offset, 0);
    assert_eq!(table.end_offset, map_start);
    assert!(table.trailing_bytes.is_empty());
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
    assert_eq!(row.value.value(), 0x0001_0203);
    assert_eq!(Some(row.schema_id.value()), Some(0x310));
    assert_eq!(Some(row.ordinal.value()), Some(0x2a));
}

#[test]
fn operation_state_journal_start_accepts_count_token_runs() {
    let prefix = [0x41, 0x00, 0x03, 0x05, 0x03, 0x03, 0x05, 0x03, 0x00];
    let group = [
        0x04, 0x01, 0x02, 0x00, 0x00, 0xe0, 0x65, 0x53, 0x4d, 0x20, 0xc0, 0x01, 0x02, 0x03, 0x83,
        0x10, 0x2a, 0x13,
    ];
    let mut bytes = prefix.to_vec();
    bytes.extend(group);

    let start = super::operation_state_journal_start(&bytes, 0).expect("journal prefix");
    assert_eq!(start, prefix.len());
    let groups =
        super::operation_state_journal_groups_before_boundary(&bytes, start, bytes.len(), 0)
            .expect("journal groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(Some(groups[0].rows[0].ordinal.value()), Some(0x2a));
}

#[test]
fn audit_trail_rows_retain_optional_selector_variable_value_width_and_raw_bytes() {
    let bytes = [
        0x41, 0x00, 0x03, 0x05, 0x01, 0x04, 0x00, 0x04, 0x02, 0x13, 0xe0, 0x65, 0x53, 0x4d, 0x20,
        0xe0, 0x01, 0x02, 0x03, 0x04, 0x04, 0x03, 0x13, 0x04, 0x05, 0x07, 0x00, 0xe0, 0x65, 0x53,
        0x4d, 0x21, 0xc0, 0x01, 0x02, 0x03, 0x04, 0x04, 0x04, 0x13, 0x04, 0x00,
    ];
    let rows = super::audit_trail_rows(&bytes, 2, bytes.len(), 900).expect("audit rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(Some(rows[0].record().ordinal.value()), Some(2));
    assert_eq!(rows[0].record().frame_selector, None);
    assert_eq!(rows[0].record().timestamp, 0x6553_4d20);
    assert_eq!(rows[0].record().value.raw().len(), 5);
    assert_eq!(rows[0].record().value.value(), 0x0102_0304);
    assert_eq!(rows[0].offset(), 900 + 7);
    assert_eq!(rows[0].record().raw(), &bytes[7..20]);
    assert_eq!(Some(rows[1].record().ordinal.value()), Some(3));
    assert_eq!(rows[1].record().frame_selector, Some(7));
    assert_eq!(rows[1].record().value.raw().len(), 4);
    assert_eq!(rows[1].record().value.value(), 0x0001_0203);
    assert_eq!(rows[1].record().raw(), &bytes[20..36]);
    assert_eq!(rows[1].end_offset(), 900 + 36);

    let truncated = super::audit_trail_rows(&bytes, 2, 35, 900).expect("bounded audit rows");
    assert_eq!(truncated.len(), 1);
    assert_eq!(truncated[0].record().raw(), &bytes[7..20]);
}
