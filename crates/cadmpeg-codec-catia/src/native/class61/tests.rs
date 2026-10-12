use super::{records, CatiaConsolidatedClass61Record};
use crate::native::CatiaAllocationReferenceEncoding as Encoding;

fn framed(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0xb2,
        0x03,
        0x61,
        u8::try_from(payload.len()).expect("B payload"),
        0x05,
    ];
    bytes.extend_from_slice(payload);
    bytes
}

fn decode(bytes: &[u8]) -> Vec<CatiaConsolidatedClass61Record> {
    let frames = crate::wire::records::consolidated_records(bytes);
    crate::test_support::with_service_context(|ctx| records(ctx, bytes, &frames))
        .expect("service roster admission")
}

#[test]
fn reference_forms_and_occurrence_bits_remain_aligned() {
    let bytes = framed(&[
        0x86, 0x09, 0x03, 0x12, 0x04, 0x80, 0x06, 7, 0x0a, 0x2c, 1, 0xa8, 1, 3,
    ]);
    let rows = decode(&bytes);
    let [row] = rows.as_slice() else {
        panic!("one complete roster")
    };
    assert_eq!(row.roster_control, 8);
    assert!(!row.zero_prefix);
    assert_eq!(
        row.members.iter().map(|m| m.value).collect::<Vec<_>>(),
        [2, 0, 4, 128, 7, 300]
    );
    assert_eq!(
        row.members.iter().map(|m| m.encoding).collect::<Vec<_>>(),
        [
            Encoding::BackwardDistance,
            Encoding::OwnedChild,
            Encoding::Selector2,
            Encoding::WidthCoded,
            Encoding::TaggedU8,
            Encoding::TaggedU16
        ]
    );
    assert_eq!(
        row.members
            .iter()
            .map(|m| m.orientation_bit)
            .collect::<Vec<_>>(),
        [false, true, false, true, true, false]
    );
    assert_eq!(&*row.frame.payload, &bytes[5..]);
}

#[test]
fn ordinary_fixture_retains_references_header_and_packed_tail() {
    let bytes = crate::test_support::test_b2::b2_counted_61_stream();
    let rows = decode(&bytes);
    let [row] = rows.as_slice() else {
        panic!("one complete roster")
    };
    assert_eq!(row.frame.header_token(), 5);
    assert_eq!(
        row.members.iter().map(|m| m.value).collect::<Vec<_>>(),
        [1300, 1294, 30, 74]
    );
    assert_eq!(
        &row.frame.payload[row.frame.payload.len() - 2..],
        &[0x41, 0x03]
    );
    assert_eq!(row.roster_control, 1);
    assert_eq!(
        row.members
            .iter()
            .map(|m| m.orientation_bit)
            .collect::<Vec<_>>(),
        [false, false, true, false]
    );
}

#[test]
fn optional_zero_and_control_are_separate_from_member_bit() {
    for (prefix, payload) in [
        (false, vec![0x81, 3, 0x13, 3]),
        (true, vec![0x81, 3, 0, 0x13, 3]),
    ] {
        let rows = decode(&framed(&payload));
        let [row] = rows.as_slice() else {
            panic!("one one-member roster")
        };
        assert_eq!(row.zero_prefix, prefix);
        assert_eq!(row.roster_control, 3);
        assert!(row.members[0].orientation_bit);
    }
}

#[test]
fn maximum_count_uses_seventeen_packed_bytes_without_integer_truncation() {
    let mut payload = vec![0xff];
    payload.extend(std::iter::repeat_n(3, 127));
    payload.extend(std::iter::repeat_n(0xff, 16));
    payload.extend_from_slice(&[7, 3]);
    let rows = decode(&framed(&payload));
    let [row] = rows.as_slice() else {
        panic!("one maximal counted roster")
    };
    assert_eq!(row.members.len(), 127);
    assert_eq!(row.roster_control, 15);
    assert!(row
        .members
        .iter()
        .all(|m| m.value == 0 && m.orientation_bit));
}

#[test]
fn malformed_packed_width_high_bits_and_terminals_refuse() {
    for payload in [
        vec![0x80, 3],
        vec![0x81, 3, 0x33, 3],
        vec![0x81, 3, 1],
        vec![0x81, 3, 1, 0xff, 0xff],
        vec![0x81, 3, 0, 0, 1, 3],
        vec![0x81, 3, 5, 1, 3],
        vec![0x81, 0x0a, 1],
    ] {
        assert!(decode(&framed(&payload)).is_empty(), "{payload:02x?}");
    }
    assert!(decode(&crate::test_support::test_b2::class61_false_long_stream()).is_empty());
}

#[test]
fn clothes_record_has_one_counted_interpretation_and_no_scalar_lane() {
    let mut payload = vec![0xb5, 0x03, 0x2b, 0x47, 0x8f, 0xb3, 0xd7, 0xfb];
    for value in [
        74, 80, 86, 91, 94, 97, 99, 101, 104, 111, 114, 132, 137, 142, 144, 146, 148, 150, 152,
        158, 162, 168, 174, 190, 193, 199, 202, 215, 220, 225, 228, 231, 233, 235, 238, 241, 244,
        247, 249, 251, 254,
    ] {
        payload.extend_from_slice(&[6, value]);
    }
    for bytes in [[0, 1], [3, 1], [6, 1], [9, 1], [12, 1]] {
        payload.push(10);
        payload.extend_from_slice(&bytes);
    }
    assert_eq!(payload.len(), 105);
    payload.extend_from_slice(&[0xa1, 0xaf, 0xc4, 0x70, 0x9c, 0x95, 0x40, 0, 3]);
    let rows = decode(&framed(&payload));
    let [row] = rows.as_slice() else {
        panic!("one complete counted interpretation")
    };
    assert_eq!(row.members.len(), 53);
    assert_eq!(
        row.members
            .iter()
            .filter(|m| m.encoding == Encoding::OwnedChild)
            .count(),
        7
    );
    assert_eq!(
        row.members
            .iter()
            .filter(|m| m.encoding == Encoding::TaggedU8)
            .count(),
        41
    );
    assert_eq!(
        row.members
            .iter()
            .filter(|m| m.encoding == Encoding::TaggedU16)
            .count(),
        5
    );
    assert_eq!(row.members.last().expect("last member").value, 268);
    assert_eq!(row.roster_control, 1);
    assert!(!row.zero_prefix);
    assert_eq!(
        row.members
            .iter()
            .map(|m| if m.orientation_bit { '1' } else { '0' })
            .collect::<String>(),
        "01011111010100100011000011100011100110101001000000100"
    );
}

#[test]
fn wire_rejects_fields_that_disagree_with_retained_bytes() {
    let rows = decode(&framed(&[0x81, 3, 0x13, 3]));
    let wire = serde_json::to_value(&rows[0]).expect("roster wire");
    for field in ["zero_prefix", "roster_control", "value", "orientation_bit"] {
        let mut invalid = wire.clone();
        match field {
            "zero_prefix" => invalid[field] = serde_json::json!(true),
            "roster_control" => invalid[field] = serde_json::json!(1),
            "value" => invalid["members"][0][field] = serde_json::json!(1),
            "orientation_bit" => invalid["members"][0][field] = serde_json::json!(false),
            _ => unreachable!(),
        }
        assert!(serde_json::from_value::<CatiaConsolidatedClass61Record>(invalid).is_err());
    }
    assert_eq!(
        serde_json::from_value::<CatiaConsolidatedClass61Record>(wire)
            .expect("checked roster wire"),
        rows[0]
    );
}

#[test]
fn member_payload_record_and_identity_admission_refuse_limits() {
    let bytes = crate::test_support::test_b2::b2_counted_61_stream();
    let frames = crate::wire::records::consolidated_records(&bytes);
    let mut refused = std::collections::HashSet::new();
    for limit in 0..40 {
        if let Err(cadmpeg_core::CodecError::ResourceLimit(error)) =
            crate::test_support::with_collection_limit(limit, |ctx| records(ctx, &bytes, &frames))
        {
            refused.insert(error.operation);
        }
    }
    for operation in [
        "catia_class61_members",
        "catia_class61_payload",
        "catia_class61_records",
    ] {
        assert!(refused.contains(operation), "{operation} did not refuse");
    }
    let result = crate::test_support::with_retained_refusal(&[], "catia_class61_id", |ctx| {
        records(ctx, &bytes, &frames)
    });
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(error)) if error.operation == "catia_class61_id")
    );
}

#[test]
fn rosters_preserve_logical_order_across_reordered_extents() {
    let first = framed(&[0x81, 3, 0x13, 3]);
    let second = framed(&[0x81, 3, 0, 0x13, 3]);
    let mut bytes = first.clone();
    bytes.extend_from_slice(&second);
    let native = crate::test_support::with_service_context(|ctx| {
        let sources = [vec![
            crate::wire::records::SourceExtent::within(&bytes, first.len(), bytes.len())
                .expect("second physical extent"),
            crate::wire::records::SourceExtent::within(&bytes, 0, first.len())
                .expect("first physical extent"),
        ]];
        crate::native::CatiaNative::decode_with_record_sources(
            ctx,
            &bytes,
            &sources,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("logical source admission");
    let rows = &native.consolidated_class61_records;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].frame.pos,
        u64::try_from(first.len()).expect("offset")
    );
    assert_eq!(rows[1].frame.pos, 0);
    assert_eq!(rows[0].source_offset, 0);
    assert_eq!(
        rows[1].source_offset,
        u64::try_from(second.len()).expect("source offset")
    );
    assert!(rows[0].zero_prefix);
    assert!(!rows[1].zero_prefix);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store reordered source");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load logical roster order"),
        native
    );
}
