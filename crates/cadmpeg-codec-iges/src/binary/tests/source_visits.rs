// SPDX-License-Identifier: Apache-2.0

use super::super::{normalize_directory_and_parameters, parameter_text, BinaryDirectory, BinaryParameter, BinaryValue, BitReader};
use super::{lengths, BitWriter};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn string_payload(value: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter::default();
    writer.string(value, lengths());
    writer.bytes()
}

#[test]
fn binary_string_source_refuses_before_reading_the_first_payload_byte() {
    let bytes = string_payload(b"abc");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut reader = BitReader::new(&bytes);
    let first = match reader.read_string(lengths(), &ctx) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected payload source refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges binary string payload");
    // The segment step is paid; the 32-bit count is fixed grammar.
    assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
    assert_eq!((reader.byte, reader.bit), (4, 0));
    for replay in [&bytes[..], &[]] {
        assert!(matches!(BitReader::new(replay).read_string(lengths(), &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn binary_non_ascii_first_byte_does_not_admit_the_payload_tail() {
    let bytes = string_payload(&[0x80, b'a', b'b']);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut reader = BitReader::new(&bytes);
    assert!(matches!(reader.read_string(lengths(), &ctx), Err(CodecError::Malformed(_))));
    assert_eq!((reader.byte, reader.bit), (5, 0));
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn binary_string_accepts_one_segment_and_three_payload_visits() {
    let bytes = string_payload(b"abc");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut reader = BitReader::new(&bytes);
    assert_eq!(reader.read_string(lengths(), &ctx).unwrap(), b"abc");
    assert!(reader.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn binary_macro_first_empty_statement_does_not_admit_later_statements() {
    let values = [BinaryValue::Default, BinaryValue::Integer(1), BinaryValue::Integer(2)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Four prefix bytes and one actual source visit precede the malformed value.
    policy.limits.max_work_units = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(parameter_text(306, &values, &ctx), Err(CodecError::Malformed(_))));
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn binary_parameter_first_empty_string_does_not_admit_later_values() {
    let values = [BinaryValue::String(Vec::new()), BinaryValue::Integer(1), BinaryValue::Integer(2)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Three prefix bytes, one source visit, three moved prefix bytes, one comma.
    policy.limits.max_work_units = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(parameter_text(116, &values, &ctx), Err(CodecError::Malformed(_))));
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn binary_parameter_sources_refuse_one_visit_after_the_prefix_copy() {
    for (entity_type, prefix_length) in [(116, 3), (306, 4)] {
        let values = [BinaryValue::Integer(1), BinaryValue::Integer(2), BinaryValue::Integer(3)];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = prefix_length;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let first = match parameter_text(entity_type, &values, &ctx) {
            Err(CodecError::ResourceLimit(first)) => first,
            other => panic!("expected parameter source refusal: {other:?}"),
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges binary parameter text");
        assert_eq!((first.limit, first.used, first.additional), (prefix_length, prefix_length, 1));
        for replay in [&values[..], &[]] {
            assert!(matches!(parameter_text(entity_type, replay, &ctx),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn binary_parameter_default_values_accept_exact_work_without_an_end_probe() {
    let values = [BinaryValue::Default, BinaryValue::Default, BinaryValue::Default];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Prefix copy3 + source3 + comma copies3 + growth move3 + terminator copy1.
    policy.limits.max_work_units = 13;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(parameter_text(116, &values, &ctx).unwrap(), b"116,,,;");
    ctx.finish_session().unwrap();
}

fn invalid_parameters() -> Vec<BinaryParameter> {
    (1..=3).map(|offset| BinaryParameter {
        offset,
        entity_type: 116,
        directory_pointer: -1,
        values: Vec::new(),
    }).collect()
}

fn directory(entity_type: i64, pointer: i64) -> [BinaryDirectory; 3] {
    std::array::from_fn(|index| {
        let mut values = std::array::from_fn(|_| BinaryValue::Default);
        values[0] = BinaryValue::Integer(entity_type);
        values[1] = BinaryValue::Pointer(pointer);
        BinaryDirectory::new(u32::try_from(index + 1).unwrap(), values).unwrap()
    })
}

fn normalization_source_refusal(
    directory: &[BinaryDirectory],
    parameters: fn() -> Vec<BinaryParameter>,
    work: u64,
    operation: &'static str,
) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut output = Vec::new();
    let first = match normalize_directory_and_parameters(&mut output, directory, parameters(), &ctx) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected normalization source refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    assert!(output.is_empty());
    for (entries, parameters) in [(directory, parameters()), (&[][..], Vec::new())] {
        assert!(matches!(normalize_directory_and_parameters(&mut output, entries, parameters, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn binary_normalized_parameter_source_refuses_before_its_first_pointer() {
    normalization_source_refusal(&[], invalid_parameters, 0, "iges binary normalized parameters");
}

#[test]
fn binary_first_invalid_parameter_pointer_does_not_admit_later_parameters() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut output = Vec::new();
    assert!(matches!(normalize_directory_and_parameters(&mut output, &[], invalid_parameters(), &ctx),
        Err(CodecError::Malformed(_))));
    assert!(output.is_empty());
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn binary_directory_pointer_source_refuses_after_exact_array_fills() {
    // Each of the two Directory-sized scalar arrays fills three slots.
    normalization_source_refusal(&directory(116, -1), Vec::new, 2 * 3,
        "iges binary Directory parameter pointers");
}

#[test]
fn binary_first_invalid_directory_pointer_does_not_admit_later_pointers() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2 * 3 + 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut output = Vec::new();
    assert!(matches!(normalize_directory_and_parameters(&mut output, &directory(116, -1), Vec::new(), &ctx),
        Err(CodecError::Malformed(_))));
    assert!(output.is_empty());
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn binary_directory_card_source_refuses_after_exact_pointer_visits() {
    // Two three-slot fills and three defaulted-pointer visits precede card rendering.
    normalization_source_refusal(&directory(123_456_789, 0), Vec::new, 2 * 3 + 3,
        "iges binary Directory cards");
}

#[test]
fn binary_first_unrenderable_directory_field_does_not_admit_later_cards() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2 * 3 + 3 + 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut output = Vec::new();
    // An IGES Directory field has eight columns; this entity discriminator needs nine.
    assert!(matches!(normalize_directory_and_parameters(&mut output, &directory(123_456_789, 0), Vec::new(), &ctx),
        Err(CodecError::Malformed(_))));
    assert!(output.is_empty());
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn binary_empty_normalization_executes_no_source_steps() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut output = Vec::new();
    assert_eq!(normalize_directory_and_parameters(&mut output, &[], Vec::new(), &ctx).unwrap(), (0, 0));
    assert!(output.is_empty());
    ctx.finish_session().unwrap();
}
