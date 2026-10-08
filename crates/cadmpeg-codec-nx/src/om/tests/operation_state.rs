// SPDX-License-Identifier: Apache-2.0
//! Bounded operation-state group-table, journal, audit-trail and message entry points.

use super::message_bytes;
use crate::om::roll_forward::OperationStateGroupRow;
use crate::om::{
    operation_state_group_table, operation_state_group_table_before_counter_map,
    operation_state_journal, operation_state_journal_groups_before_boundary,
    operation_state_journal_start, operation_state_messages,
};

#[test]
fn operation_state_messages_decode_text_value_and_severity() {
    let bytes = message_bytes(b"hello", &[0xc0, 0x01, 0x02, 0x03], [0, 3]);
    let messages = operation_state_messages(&bytes, 500);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].offset(), 500);
    assert_eq!(messages[0].body().text.declared_length(), 7);
    assert_eq!(messages[0].body().text.as_str(), "hello");
    assert_eq!(messages[0].body().value.raw().len(), 4);
    assert_eq!(messages[0].body().value.value(), 0x0001_0203);
    assert_eq!(messages[0].body().count_or_severity, 3);
    assert_eq!(messages[0].end_offset(), 500 + bytes.len());
}

#[test]
fn operation_state_group_table_decodes_list_pair_and_empty_groups() {
    let bytes = [
        0x01, 0x00, 0x01, 0x03, 0x4a, 0x83, 0xba, 0x01, 0xff, 0x4a, 0x83, 0xb7, 0x02, 0xff, 0x01,
        0x01, 0x01, 0x02, 0x4f, 0xf1, 0x04, 0x2d, 0x83, 0xe1, 0xff, 0xff, 0x01, 0x01, 0x00,
    ];
    let table = operation_state_group_table(&bytes, 0, bytes.len(), 900).expect("group table");
    assert_eq!(table.groups().len(), 3);
    assert_eq!(table.groups().first().opener().bytes(), [0x01, 0x00]);
    assert_eq!(table.groups().first().members().count().prefix(), Some(1));
    assert_eq!(table.groups().first().members().rows().len(), 2);
    assert_eq!(
        table.groups().iter().nth(1).unwrap().members().rows().len(),
        1
    );
    let OperationStateGroupRow::Pair {
        tag, first, second, ..
    } = table.groups().iter().nth(1).unwrap().members().rows()[0]
    else {
        panic!("pair group row was not typed");
    };
    assert_eq!(u8::from(tag), 0x4f);
    assert_eq!(Some(first.value()), Some(0x42d));
    assert_eq!(Some(second.value()), Some(0x3e1));
    assert_eq!(
        table
            .groups()
            .iter()
            .nth(2)
            .unwrap()
            .members()
            .count()
            .declared_count(),
        0
    );
    assert_eq!(
        table.groups().iter().nth(2).unwrap().members().rows().len(),
        0
    );
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

    let map = crate::test_support::with_decode_context(|ctx| {
        crate::om::state_counter::StateCounterMap::read(ctx, &bytes, 0)
    })
    .unwrap()
    .expect("counter map");
    let table = crate::test_support::with_decode_context(|ctx| {
        operation_state_group_table_before_counter_map(ctx, &bytes, map.offset(), 0)
    })
    .unwrap()
    .expect("group table");
    assert_eq!(table.offset(), 3);
    assert_eq!(table.end_offset(), map.offset());
    assert_eq!(table.groups().len(), 3);
    assert_eq!(table.groups().first().members().rows().len(), 2);
    assert_eq!(
        table.groups().iter().nth(1).unwrap().members().rows().len(),
        1
    );
    assert_eq!(
        table
            .groups()
            .iter()
            .nth(2)
            .unwrap()
            .members()
            .count()
            .declared_count(),
        0
    );
    assert_eq!(table.trailing_bytes(), &[0x01, 0x01]);
}

#[test]
fn operation_state_group_table_handles_a_long_adjacent_group_run_and_refuses_collection_limit() {
    const GROUP_COUNT: usize = 4096;
    let mut bytes = Vec::with_capacity(GROUP_COUNT * 3 + 12);
    for _ in 0..GROUP_COUNT {
        bytes.extend([0x01, 0x00, 0x00]);
    }
    let map_start = bytes.len();
    bytes.extend([0x05, 0x01, 0x00, 0x01, 0x01, 0x4e]);
    bytes.extend([0x05, 0x02, 0x01, 0x01, 0x01, 0x4e]);

    let table = crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| operation_state_group_table_before_counter_map(ctx, &bytes, map_start, 0),
    )
    .unwrap()
    .expect("long adjacent group run");
    assert_eq!(table.groups().len(), GROUP_COUNT);
    assert_eq!(table.offset(), 0);
    assert_eq!(table.end_offset(), map_start);
    assert!(table.trailing_bytes().is_empty());

    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            policy.limits.max_collection_items =
                cadmpeg_core::decode::u64_from_index(GROUP_COUNT - 1);
        },
        |ctx| {
            let error = operation_state_group_table_before_counter_map(ctx, &bytes, map_start, 0)
                .expect_err("the final group exceeds the admitted collection count");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            ));
        },
    );
}

#[test]
fn operation_state_group_prefix_stops_before_unvisited_suffix() {
    let bytes = [0x01, 0x00, 0x00, 0x01, 0x00, 0x00];
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX selected state group path traversal",
        |ctx| {
            operation_state_group_table_before_counter_map(
                ctx,
                &bytes,
                bytes.len(),
                usize::MAX - 1,
            )
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("selected group traversal must be admitted per candidate");
    };
    assert_eq!(limit.additional, 1);

    let work_limit = limit.used + limit.additional;
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_work_units = work_limit,
        |ctx| {
            assert!(operation_state_group_table_before_counter_map(
                ctx,
                &bytes,
                bytes.len(),
                usize::MAX - 1,
            )
            .unwrap()
            .is_none());
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn operation_state_journal_decodes_timestamp_value_schema_and_ordinal() {
    let bytes = [
        0x04, 0x01, 0x02, 0x00, 0x00, 0xe0, 0x65, 0x53, 0x4d, 0x20, 0xc0, 0x01, 0x02, 0x03, 0x83,
        0x10, 0x2a, 0x13,
    ];
    let groups = operation_state_journal(&bytes, 0, bytes.len(), 1100).expect("journal");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].selector(), [0x01, 0x02]);
    assert_eq!(groups[0].rows().len(), 1);
    let row = groups[0].rows().first();
    assert_eq!(row.timestamp(), 0x6553_4d20);
    assert_eq!(row.value().value(), 0x0001_0203);
    assert_eq!(Some(row.schema().value()), Some(0x310));
    assert_eq!(Some(row.ordinal().value()), Some(0x2a));
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

    let start = crate::test_support::with_decode_context(|ctx| {
        operation_state_journal_start(ctx, &bytes, 0)
    })
    .unwrap()
    .expect("journal prefix");
    assert_eq!(start, prefix.len());
    let groups = crate::test_support::with_decode_context(|ctx| {
        operation_state_journal_groups_before_boundary(ctx, &bytes, start, bytes.len(), 0)
    })
    .unwrap()
    .expect("journal groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(Some(groups[0].rows().first().ordinal().value()), Some(0x2a));
}

fn state_journal_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = [
        0x04, 0x01, 0x02, 0x00, 0x00, 0xe0, 0x65, 0x53, 0x4d, 0x20, 0xc0, 0x01, 0x02, 0x03, 0x83,
        0x10, 0x2a, 0x13,
    ];

    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            configure(policy);
        },
        |ctx| {
            crate::om::operation_state_journal_groups_before_boundary(
                ctx,
                &bytes,
                0,
                bytes.len(),
                0,
            )
            .unwrap_err()
        },
    )
}

#[test]
fn state_journal_groups_refuse_collection_limit() {
    let error = state_journal_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn state_journal_groups_refuse_retained_limit() {
    let error = state_journal_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn state_journal_groups_refuse_work_limit() {
    let error = state_journal_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn audit_trail_rows_retain_optional_selector_variable_value_width_and_raw_bytes() {
    let bytes = [
        0x41, 0x00, 0x03, 0x05, 0x01, 0x04, 0x00, 0x04, 0x02, 0x13, 0xe0, 0x65, 0x53, 0x4d, 0x20,
        0xe0, 0x01, 0x02, 0x03, 0x04, 0x04, 0x03, 0x13, 0x04, 0x05, 0x07, 0x00, 0xe0, 0x65, 0x53,
        0x4d, 0x21, 0xc0, 0x01, 0x02, 0x03, 0x04, 0x04, 0x04, 0x13, 0x04, 0x00,
    ];
    let rows = crate::test_support::with_decode_context(|ctx| {
        crate::om::audit_trail_rows(ctx, &bytes, 2, bytes.len(), 900)
    })
    .unwrap()
    .expect("audit rows");
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

    let truncated = crate::test_support::with_decode_context(|ctx| {
        crate::om::audit_trail_rows(ctx, &bytes, 2, 35, 900)
    })
    .unwrap()
    .expect("bounded audit rows");
    assert_eq!(truncated.len(), 1);
    assert_eq!(truncated[0].record().raw(), &bytes[7..20]);
}

fn audit_trail_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = [
        0x41, 0x00, 0x03, 0x05, 0x01, 0x04, 0x00, 0x04, 0x02, 0x13, 0xe0, 0x65, 0x53, 0x4d, 0x20,
        0xe0, 0x01, 0x02, 0x03, 0x04, 0x04, 0x03, 0x13, 0x04, 0x05, 0x07, 0x00, 0xe0, 0x65, 0x53,
        0x4d, 0x21, 0xc0, 0x01, 0x02, 0x03, 0x04, 0x04, 0x04, 0x13, 0x04, 0x00,
    ];

    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            configure(policy);
        },
        |ctx| crate::om::audit_trail_rows(ctx, &bytes, 2, bytes.len(), 900).unwrap_err(),
    )
}

#[test]
fn audit_trail_rows_refuse_collection_limit() {
    let error = audit_trail_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn audit_trail_rows_refuse_retained_limit() {
    let error = audit_trail_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn audit_trail_rows_refuse_work_limit() {
    let error = audit_trail_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn state_group_path_reversal_refusal_propagates() {
    let bytes = [0x01, 0x00, 0x00, 0x01, 0x00, 0x00];
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX operation-state group path reversal",
        |ctx| operation_state_group_table_before_counter_map(ctx, &bytes, bytes.len(), 0),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "NX operation-state group path reversal" && limit.additional == 2 + 3 * 2 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>()))
    );
}
