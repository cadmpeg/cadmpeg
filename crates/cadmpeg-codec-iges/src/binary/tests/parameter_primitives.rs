// SPDX-License-Identifier: Apache-2.0

use super::super::{read_directory, read_parameters, BinaryDirectory, BinaryValue, ValueStream};
use super::{lengths, BitWriter};
use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;

fn parameter_body(count: usize, repeated: bool) -> Vec<u8> {
    let mut writer = BitWriter::default();
    writer.control(1);
    writer.integer(116, lengths().single_integer);
    writer.control(5);
    writer.pointer(1);
    if repeated {
        writer.control_repeat(
            false,
            u8::try_from(count).expect("valid test input and admitted budget"),
            0,
        );
    } else {
        for _ in 0..count {
            writer.control(0);
        }
    }
    let body = writer.bytes();
    let mut bytes = u32::try_from(body.len())
        .expect("valid test input and admitted budget")
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(&body);
    bytes
}

fn directory() -> [BinaryDirectory; 1] {
    let mut fields = std::array::from_fn(|_| BinaryValue::Default);
    fields[0] = BinaryValue::Integer(116);
    fields[1] = BinaryValue::Pointer(1);
    [BinaryDirectory::new(1, fields).expect("valid test input and admitted budget")]
}

#[test]
fn binary_parameter_primitives_accept_actual_values_without_an_exhausted_probe() {
    // One record step, n primitive steps, and live-slot moves at 4/8/16/32.
    // A singleton offset search compares two u32 pairs: 2 * (1 + 4 + 4).
    for (count, moved) in [(0, 0), (1, 0), (64, 4 + 8 + 16 + 32)] {
        let bytes = parameter_body(count, false);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1
            + u64_from_index(count)
            + u64_from_index(moved * std::mem::size_of::<BinaryValue>())
            + 18;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("valid test input and admitted budget");
        let records = read_parameters(&bytes, lengths(), &directory(), &ctx)
            .expect("valid test input and admitted budget");
        assert_eq!(records.len(), 1);
        assert_eq!(
            (
                records[0].offset,
                records[0].entity_type,
                records[0].directory_pointer
            ),
            (1, 116, 1)
        );
        assert_eq!(records[0].values.len(), count);
        assert!(records[0]
            .values
            .iter()
            .all(|value| *value == BinaryValue::Default));
        ctx.finish_session()
            .expect("valid test input and admitted budget");
    }
}

#[test]
fn binary_parameter_primitives_refuse_the_first_or_last_real_value_and_replay() {
    for (count, moved) in [(1, 0), (64, 4 + 8 + 16 + 32)] {
        let bytes = parameter_body(count, false);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let work = u64_from_index(count + moved * std::mem::size_of::<BinaryValue>());
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("valid test input and admitted budget");
        let CodecError::ResourceLimit(first) =
            read_parameters(&bytes, lengths(), &directory(), &ctx)
                .expect_err("the test requires a refusal")
        else {
            panic!("expected parameter primitive refusal");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges binary parameter primitives");
        assert_eq!((first.used, first.additional, first.limit), (work, 1, work));
        assert!(
            matches!(read_parameters(&bytes, lengths(), &directory(), &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first)
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
        );
    }
}

#[test]
fn binary_parameter_keeps_pending_values_after_the_byte_source_ends() {
    for count in [4, 16] {
        let bytes = parameter_body(count, true);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("valid test input and admitted budget");
        let records = read_parameters(&bytes, lengths(), &directory(), &ctx)
            .expect("valid test input and admitted budget");
        assert_eq!(records[0].values.len(), count);
        assert!(records[0]
            .values
            .iter()
            .all(|value| *value == BinaryValue::Default));
        ctx.finish_session()
            .expect("valid test input and admitted budget");
    }
}

#[test]
fn binary_stream_and_empty_record_readers_need_no_work_or_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("valid test input and admitted budget");
    let mut stream = ValueStream::new(&[0], lengths(), &ctx);
    assert_eq!(
        stream.next().expect("valid test input and admitted budget"),
        Some(BinaryValue::Default)
    );
    assert_eq!(
        stream.next().expect("valid test input and admitted budget"),
        None
    );
    stream
        .finish()
        .expect("valid test input and admitted budget");
    assert!(read_directory(&[], lengths(), &ctx)
        .expect("valid test input and admitted budget")
        .is_empty());
    assert!(read_parameters(&[], lengths(), &[], &ctx)
        .expect("valid test input and admitted budget")
        .is_empty());
    ctx.finish_session()
        .expect("valid test input and admitted budget");
}

#[test]
fn binary_stream_repeated_integer_preserves_pending_values_and_cursor() {
    let mut writer = BitWriter::default();
    writer.control_repeat(false, 3, 1);
    writer.integer(7, lengths().single_integer);
    let bytes = writer.bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("valid test input and admitted budget");
    let mut stream = ValueStream::new(&bytes, lengths(), &ctx);
    assert_eq!(
        stream.next().expect("valid repeated integer"),
        Some(BinaryValue::Integer(7))
    );
    assert_eq!(stream.pending.len(), 2);
    assert!(stream.bits.is_empty());
    let cursor = (stream.bits.byte, stream.bits.bit);
    for remaining in [1, 0] {
        assert_eq!(
            stream.next().expect("valid pending integer"),
            Some(BinaryValue::Integer(7))
        );
        assert_eq!(stream.pending.len(), remaining);
        assert_eq!((stream.bits.byte, stream.bits.bit), cursor);
    }
    assert_eq!(stream.next().expect("exhausted stream"), None);
    assert_eq!((stream.bits.byte, stream.bits.bit), cursor);
    stream.finish().expect("valid repeated integer stream");
    ctx.finish_session().expect("stream uses no source work");
}

#[test]
fn binary_directory_records_refuse_before_reading_the_first_count() {
    let mut writer = BitWriter::default();
    writer.control_repeat(false, 3, 1);
    writer.integer(7, lengths().single_integer);
    let bytes = writer.bytes();
    let operation = "iges binary section records";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("valid test input and admitted budget");
            let Err(CodecError::ResourceLimit(first)) = read_directory(&bytes, lengths(), &ctx)
            else {
                panic!("expected first Directory record refusal");
            };
            for replay in [&[0][..], &bytes[..]] {
                assert!(matches!(read_directory(replay, lengths(), &ctx),
                    Err(CodecError::ResourceLimit(last)) if last == first));
            }
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
            Err::<(), CodecError>(CodecError::ResourceLimit(first))
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal");
    };
    assert_eq!(
        (
            limit.dimension,
            limit.operation,
            limit.used,
            limit.additional
        ),
        (ResourceDimension::WorkUnits, operation, 0, 1)
    );
}
