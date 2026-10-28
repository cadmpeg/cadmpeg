use super::{derive, CatiaConsolidatedFourReferenceEdge};
use crate::native::CatiaAllocationReferenceEncoding as Encoding;
use crate::wire::records::{ConsolidatedFrameFlag, ConsolidatedFrameWidth};

fn framed(width: u8, flag: u8, header: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0xb1 + width,
        flag,
        0x5e,
        u8::try_from(payload.len()).expect("payload"),
    ];
    bytes.extend_from_slice(&header.to_le_bytes()[..usize::from(width)]);
    bytes.extend_from_slice(payload);
    bytes
}

fn decode(bytes: &[u8]) -> Vec<CatiaConsolidatedFourReferenceEdge> {
    let records = crate::wire::records::consolidated_records(bytes);
    crate::test_support::with_service_context(|ctx| derive(ctx, bytes, &records))
        .expect("four-reference admission")
}

#[test]
fn four_operands_keep_complete_frame_and_separate_terminal() {
    let bytes = framed(1, 3, 5, &[9, 15, 7, 11, 0x21]);
    let rows = decode(&bytes);
    let [row] = rows.as_slice() else {
        panic!("one four-reference frame")
    };
    assert_eq!(row.operands.each_ref().map(|r| r.value), [2, 3, 1, 2]);
    assert_eq!(
        row.operands.each_ref().map(|r| r.encoding),
        [
            Encoding::BackwardDistance,
            Encoding::OwnedChild,
            Encoding::OwnedChild,
            Encoding::OwnedChild,
        ]
    );
    assert_eq!(row.frame.header_token(), 5);
    assert_eq!(&*row.frame.payload, &[9, 15, 7, 11, 0x21]);
    assert_eq!(row.terminal, 0x21);
    assert_eq!(row.terminal_value, 8);
    assert_eq!(row.terminal_encoding, Encoding::BackwardDistance);
    assert_eq!(
        (row.source_index, row.source_offset, row.frame.pos),
        (0, 0, 0)
    );
}

#[test]
fn independent_header_width_and_flag_preserve_raw_values() {
    for (width, token, encoded_width) in [
        (1, 0x05, ConsolidatedFrameWidth::One),
        (2, 0x1234, ConsolidatedFrameWidth::Two),
        (3, 0x0012_3456, ConsolidatedFrameWidth::Three),
    ] {
        for (flag, encoded_flag) in [
            (3, ConsolidatedFrameFlag::Flag03),
            (19, ConsolidatedFrameFlag::Flag13),
            (131, ConsolidatedFrameFlag::Flag83),
        ] {
            let rows = decode(&framed(width, flag, token, &[9, 15, 7, 11, 0x21]));
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].frame.width(), encoded_width);
            assert_eq!(rows[0].frame.flag, encoded_flag);
            assert_eq!(rows[0].frame.header_token(), token);
            assert_eq!(rows[0].operands.each_ref().map(|r| r.value), [2, 3, 1, 2]);
        }
    }
}

#[test]
fn compact_terminal_bytes_retain_distinct_single_byte_forms() {
    for terminal in [1, 2, 0x21, 0x22, 0x25, 0x29, 0x2a] {
        let rows = decode(&framed(1, 3, 5, &[9, 15, 7, 11, terminal]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].terminal, terminal);
        assert_eq!(rows[0].terminal_value, u32::from(terminal >> 2));
        assert_eq!(
            rows[0].terminal_encoding,
            if terminal & 3 == 1 {
                Encoding::BackwardDistance
            } else {
                Encoding::Selector2
            }
        );
    }
}

#[test]
fn invalid_operands_terminals_and_other_reference_counts_withhold() {
    for slot in 0..4 {
        for invalid in [0, 2, 4, 6, 10] {
            let mut payload = [9, 15, 7, 11, 0x21];
            payload[slot] = invalid;
            assert!(decode(&framed(1, 3, 5, &payload)).is_empty());
        }
    }
    for terminal in [0, 3, 0x26, 0xff] {
        assert!(decode(&framed(1, 3, 5, &[9, 15, 7, 11, terminal])).is_empty());
    }
    for payload in [&[9, 15, 7, 0x21][..], &[3, 9, 15, 7, 11, 0x21][..]] {
        assert!(decode(&framed(1, 3, 5, payload)).is_empty());
    }
    let mut truncated = framed(1, 3, 5, &[9, 15, 7, 11, 0x21]);
    truncated.pop();
    assert!(decode(&truncated).is_empty());
}

#[test]
fn wire_self_consistency_checks_all_derived_fields() {
    let row = decode(&framed(1, 131, 5, &[9, 15, 7, 11, 0x21])).remove(0);
    let wire = serde_json::to_value(&row).expect("wire");
    assert_eq!(
        serde_json::from_value::<CatiaConsolidatedFourReferenceEdge>(wire.clone())
            .expect("checked wire"),
        row
    );
    for slot in 0..4 {
        for field in ["value", "encoding"] {
            let mut invalid = wire.clone();
            invalid["operands"][slot][field] = if field == "value" {
                serde_json::json!(63)
            } else {
                serde_json::json!("selector2")
            };
            assert!(serde_json::from_value::<CatiaConsolidatedFourReferenceEdge>(invalid).is_err());
        }
    }
    for field in [
        "terminal",
        "terminal_value",
        "terminal_encoding",
        "payload",
        "header_token",
    ] {
        let mut invalid = wire.clone();
        match field {
            "terminal" => invalid[field] = serde_json::json!(0x22),
            "terminal_value" => invalid[field] = serde_json::json!(0),
            "terminal_encoding" => invalid[field] = serde_json::json!("selector2"),
            "payload" => invalid["frame"][field] = serde_json::json!("090f070b26"),
            "header_token" => invalid["frame"][field] = serde_json::json!(256),
            _ => unreachable!(),
        }
        assert!(serde_json::from_value::<CatiaConsolidatedFourReferenceEdge>(invalid).is_err());
    }
}

#[test]
fn bounded_sources_keep_logical_offsets_and_physical_frame_identity() {
    let frame = framed(1, 3, 5, &[9, 15, 7, 11, 0x21]);
    let mut bytes = vec![0xff; 3];
    bytes.extend_from_slice(&frame);
    let second = bytes.len();
    bytes.extend_from_slice(&frame);
    let records = crate::wire::records::consolidated_records_in_range_sources(
        &bytes,
        [Some(3..second), Some(second..bytes.len())],
    );
    let rows = crate::test_support::with_service_context(|ctx| derive(ctx, &bytes, &records))
        .expect("source admission");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (
            rows[0].source_index,
            rows[0].source_offset,
            rows[0].frame.pos
        ),
        (0, 0, 3)
    );
    assert_eq!(
        (
            rows[1].source_index,
            rows[1].source_offset,
            rows[1].frame.pos
        ),
        (1, 0, u64::try_from(second).expect("offset"))
    );
    super::validate(&rows).expect("source identity order");
}

#[test]
fn spanning_frame_preserves_ordinal_accounting_without_typed_admission() {
    let mut bytes = vec![0xb2, 3, 0x24, 0, 5];
    bytes.extend_from_slice(&framed(1, 3, 5, &[9, 15, 7, 11, 0x21]));
    let split = 9;
    let records = crate::wire::records::consolidated_records_in_range_sources(
        &bytes,
        [vec![0..split, split..bytes.len()]],
    );
    assert!(records
        .iter()
        .any(|r| r.class() == 0x5e && r.range().is_none()));
    let rows = crate::test_support::with_service_context(|ctx| derive(ctx, &bytes, &records))
        .expect("bounded admission");
    assert!(rows.is_empty());
}
