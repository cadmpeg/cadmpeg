// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Codec as _;

use super::{BinaryValue, BitWriter, PrimitiveLengths, ValueStream};

fn lengths(width: u8) -> PrimitiveLengths {
    PrimitiveLengths {
        single_integer: width,
        double_integer: width,
        single_exponent: 8,
        single_fraction: 23,
        double_exponent: 11,
        double_fraction: 52,
    }
}

fn consume(stream: &mut ValueStream<'_>) -> Result<Vec<BinaryValue>, CodecError> {
    let mut values = Vec::new();
    while let Some(value) = stream.next()? {
        values.push(value);
    }
    stream.finish()?;
    Ok(values)
}

#[test]
fn physical_repeat_navigation_is_lazy_and_only_original_coordinate_bytes_are_read() {
    let mut writer = BitWriter::default();
    writer.control_repeat(true, 8, 1);
    for value in [110, 1, 1, 2, 3, 4, 5, 6] {
        writer.integer(value, 8);
    }
    let bytes = writer.bytes();
    let mut navigation = ValueStream::new(&bytes, lengths(8));
    assert_eq!(
        navigation.next().expect("entity type"),
        Some(BinaryValue::Integer(110))
    );
    assert_eq!(navigation.bits.byte, 2);
    assert_eq!(
        navigation.next().expect("owner"),
        Some(BinaryValue::Integer(1))
    );
    assert_eq!(navigation.bits.byte, 3);
    let mut prefixed = b"head".to_vec();
    prefixed.extend_from_slice(&bytes);
    prefixed.extend_from_slice(b"tail");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&prefixed, &arena, &DecodePolicy::service())
        .expect("source");
    let source = root
        .child(4, 4 + bytes.len())
        .expect("original value stream");
    let actual = ctx
        .record("iges:entity:directory#1", source, || {
            consume(&mut ValueStream::source_line(source, lengths(8)))
        })
        .expect("actual source values");
    assert_eq!(actual, [110, 1, 1, 2, 3, 4, 5, 6].map(BinaryValue::Integer));
    let captures = ctx.take_record_captures().expect("one drain");
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".to_owned(), vec![7..13])])
    );
}

#[test]
fn a_shared_primitive_is_read_once_for_its_kept_uses_and_prefix_only_use_gets_no_credit() {
    for repeat in [2, 8] {
        let mut writer = BitWriter::default();
        writer.control_repeat(false, repeat, 1);
        writer.integer(110, 8);
        let bytes = writer.bytes();
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("source");
        let values = ctx
            .record("iges:entity:directory#1", source, || {
                consume(&mut ValueStream::source_line(source, lengths(8)))
            })
            .expect("same physical primitive produces its actual repeated values");
        assert_eq!(values, vec![BinaryValue::Integer(110); usize::from(repeat)]);
        let captures = ctx.take_record_captures().expect("one drain");
        assert_eq!(captures.len(), 1);
        if repeat == 2 {
            assert!(captures[0].reads().is_empty());
        } else {
            assert_eq!(
                captures[0].reads(),
                &BTreeMap::from([("file".to_owned(), vec![1..2])])
            );
        }
    }
}

#[test]
fn nonbyte_integer_padding_is_checked_and_failure_rolls_back_before_the_peer() {
    let mut writer = BitWriter::default();
    for value in [110, 1, 1, 2, 3, 4, 5, 6] {
        writer.control(1);
        writer.integer(value, 12);
    }
    let good = writer.bytes();
    let mut bad = good.clone();
    // Each fixture field has its own control and two primitive bytes.
    let first_coordinate_padding = 2 * 3 + 2;
    bad[first_coordinate_padding] |= 1;
    let mut bytes = bad.clone();
    bytes.extend_from_slice(&good);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("source");
    let failed = root.child(0, bad.len()).expect("actual failed body");
    let error = ctx
        .record("iges:entity:directory#1", failed, || {
            consume(&mut ValueStream::source_line(failed, lengths(12)))
        })
        .expect_err("nonzero actual alignment padding");
    assert!(error.to_string().contains("padding contains a one bit"));
    let peer = root
        .child(bad.len(), bytes.len())
        .expect("actual peer body");
    let values = ctx
        .record("iges:entity:directory#3", peer, || {
            consume(&mut ValueStream::source_line(peer, lengths(12)))
        })
        .expect("independent peer");
    assert_eq!(values, [110, 1, 1, 2, 3, 4, 5, 6].map(BinaryValue::Integer));
    let captures = ctx
        .take_record_captures()
        .expect("one drain after failure and peer");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "iges:entity:directory#3");
    let expected: Vec<_> = (2..8)
        .map(|field| {
            let start = u64::try_from(bad.len() + field * 3 + 1).expect("actual field offset");
            start..start + 2
        })
        .collect();
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".to_owned(), expected)])
    );
}

#[test]
fn source_line_normalization_refuses_a_foreign_session_and_keeps_the_public_input_limit() {
    let bytes = crate::test_support::binary_parts::plain_line_then_point(1, false);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("caller session");
    let (_other, foreign) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("different session");
    let error = crate::binary::parts::normalize(foreign, &ctx)
        .err()
        .expect("foreign provenance is refused");
    assert!(error
        .to_string()
        .contains("another or detached decode session"));
    assert!(ctx.take_record_captures().expect("one drain").is_empty());
    let mut options = cadmpeg_ir::codec::DecodeOptions::default();
    let limit = u64::try_from(bytes.len() - 1).expect("actual bounded input limit");
    options.policy.limits.max_input_bytes = limit;
    let error = crate::IgesCodec
        .decode(&mut std::io::Cursor::new(bytes.clone()), &options)
        .expect_err("the real caller input allowance is not reset");
    let cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(resource)) = error else {
        panic!("actual caller resource refusal");
    };
    assert_eq!(resource.dimension, ResourceDimension::InputBytes);
    assert_eq!(resource.limit, limit);
}

#[test]
fn default_coordinate_repetitions_do_not_credit_their_control_byte() {
    let mut writer = BitWriter::default();
    for value in [110, 1] {
        writer.control(1);
        writer.integer(value, 8);
    }
    writer.control_repeat(false, 6, 0);
    let bytes = writer.bytes();
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("source");
    let values = ctx
        .record("iges:entity:directory#1", source, || {
            consume(&mut ValueStream::source_line(source, lengths(8)))
        })
        .expect("actual omitted-coordinate values");
    assert_eq!(
        &values[..2],
        &[BinaryValue::Integer(110), BinaryValue::Integer(1)]
    );
    assert_eq!(
        &values[2..],
        &[
            BinaryValue::Default,
            BinaryValue::Default,
            BinaryValue::Default,
            BinaryValue::Default,
            BinaryValue::Default,
            BinaryValue::Default
        ]
    );
    let captures = ctx.take_record_captures().expect("one drain");
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
}

#[test]
fn a_truncated_physical_repeat_cannot_consume_the_neighboring_source_window() {
    let mut writer = BitWriter::default();
    writer.control_repeat(true, 8, 1);
    for value in [110, 1, 1] {
        writer.integer(value, 8);
    }
    let bad = writer.bytes();
    let mut writer = BitWriter::default();
    writer.control_repeat(true, 8, 1);
    for value in [110, 1, 1, 2, 3, 4, 5, 6] {
        writer.integer(value, 8);
    }
    let good = writer.bytes();
    let mut bytes = bad.clone();
    bytes.extend_from_slice(&good);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("source");
    let failed = root
        .child(0, bad.len())
        .expect("the original failed body boundary");
    let error = ctx
        .record("iges:entity:directory#1", failed, || {
            consume(&mut ValueStream::source_line(failed, lengths(8)))
        })
        .expect_err("a repeat has no payload outside its actual window");
    assert!(error.to_string().contains("primitive is truncated"));
    let peer = root
        .child(bad.len(), bytes.len())
        .expect("the original peer body boundary");
    let values = ctx
        .record("iges:entity:directory#3", peer, || {
            consume(&mut ValueStream::source_line(peer, lengths(8)))
        })
        .expect("independent source peer");
    assert_eq!(values, [110, 1, 1, 2, 3, 4, 5, 6].map(BinaryValue::Integer));
    let captures = ctx
        .take_record_captures()
        .expect("one drain after both attempts");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "iges:entity:directory#3");
    let start = u64::try_from(bad.len() + 3).expect("actual peer coordinate start");
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".to_owned(), vec![start..start + 6])])
    );
}

#[test]
fn unproved_global_integer_capacity_keeps_the_existing_uncredited_handoff() {
    let mut bytes = crate::test_support::binary_parts::plain_line_then_point(1, false);
    let range = {
        let arena = DecodeArena::new();
        let (_ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("original source");
        let sections = crate::binary::parse_sections(root).expect("actual Global section");
        let mut values = ValueStream::new(sections.global.window(), sections.lengths);
        for _ in 0..6 {
            values
                .next()
                .expect("actual preceding Global field")
                .expect("field exists");
        }
        assert_eq!(
            values.next().expect("sender integer capacity"),
            Some(BinaryValue::Integer(32))
        );
        assert_eq!(values.value_extent.len(), 4);
        sections.global.start() + values.value_extent.start
            ..sections.global.start() + values.value_extent.end
    };
    let neutral = bytes.clone();
    bytes[range].copy_from_slice(&8_i32.to_be_bytes());
    // The Binary Flag Section supersedes Global field 7 (IGES 5.3 Appendix
    // H.2.3): the declared 8-bit capacity hands off as the neutral 32 does.
    let normalize = |bytes: &[u8]| {
        let arena = DecodeArena::new();
        let (ctx, source) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("edited source");
        let (normalized, _) = crate::binary::parts::normalize(source, &ctx)
            .expect("unchanged admitted source semantics");
        let captures = ctx.take_record_captures().expect("one drain");
        (normalized.admission.parameters, captures)
    };
    let (records, captures) = normalize(&bytes);
    let (_, neutral_captures) = normalize(&neutral);
    assert_eq!(records.len(), 2);
    for (index, expected) in [110, 1, 2, 3, 4, 5, 6].into_iter().enumerate() {
        assert_eq!(records[0].integer(index), Some(expected));
    }
    assert_eq!(captures.len(), 2);
    assert_eq!(
        captures
            .iter()
            .map(|capture| capture.reads().clone())
            .collect::<Vec<_>>(),
        neutral_captures
            .iter()
            .map(|capture| capture.reads().clone())
            .collect::<Vec<_>>()
    );
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.curves.len(), 1);
    assert_eq!(decoded.ir().model.points.len(), 3);
}
