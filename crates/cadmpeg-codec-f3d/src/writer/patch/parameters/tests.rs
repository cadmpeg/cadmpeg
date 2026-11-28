// SPDX-License-Identifier: Apache-2.0

use super::*;

const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

#[test]
fn a_record_with_an_unreadable_header_is_left_alone() {
    let mut frame = crate::design::test_support::parameter_record(
        None,
        "3 mm",
        "User Parameter",
        Some("mm"),
        "Width",
        0.3,
    );
    let record = decode_parameter_frame(&frame, STREAM, 0).unwrap().unwrap();
    let id = neutral_parameter_id(&record);
    frame[0] = 0;
    let changed = Change {
        expression: Some("19 mm".into()),
        ..Default::default()
    };
    assert!(replacement(
        &frame,
        STREAM,
        &record,
        &id,
        &changed,
        &BTreeMap::new(),
        None,
    )
    .is_none());
}

#[test]
fn a_resize_moves_outer_and_nested_index_offsets_after_the_expression() {
    let mut frame = crate::design::test_support::parameter_record(
        None,
        "3 mm",
        "User Parameter",
        Some("mm"),
        "Width",
        0.3,
    );
    let end = u64::try_from(frame.len()).unwrap();
    crate::test_support::indexed_header(&mut frame, *b"257", 19);
    let record = decode_parameter_frame(&frame[..usize::try_from(end).unwrap()], STREAM, 0)
        .unwrap()
        .unwrap();
    let id = neutral_parameter_id(&record);
    let changed = Change {
        expression: Some("143 mm".into()),
        ..Default::default()
    };
    let edit = replacement(
        &frame,
        STREAM,
        &record,
        &id,
        &changed,
        &BTreeMap::new(),
        None,
    )
    .unwrap();
    let nested = record.evaluated_value_offset();
    let mut meta = crate::test_support::streams_test::design_metastream_with_records(
        &[],
        &[(71, 0), (19, end)],
    );
    let second_index = meta.len() - 20;
    let mut secondary = Vec::new();
    secondary.extend_from_slice(&1u32.to_le_bytes());
    secondary.extend_from_slice(&71u64.to_le_bytes());
    secondary.extend_from_slice(&nested.to_le_bytes());
    meta.splice(second_index..second_index + 4, secondary);
    let after = crate::metastream::relocate_records(&meta, "MetaStream", frame.len(), |at| {
        relocate(at, std::slice::from_ref(&edit))
    })
    .unwrap();
    let parsed = crate::metastream::parse(&after, "MetaStream").unwrap();
    assert_eq!(parsed.records[0].bulk_offset, 0);
    assert_eq!(parsed.records[1].bulk_offset, end + 4);
    assert_eq!(parsed.secondary_records[0].bulk_offset, nested + 4);
    assert_eq!(
        relocate(record.expression_offset(), std::slice::from_ref(&edit)),
        None
    );
}
