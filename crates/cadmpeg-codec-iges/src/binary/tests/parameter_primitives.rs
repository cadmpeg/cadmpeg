// SPDX-License-Identifier: Apache-2.0

use super::super::{read_directory, read_parameters, BinaryDirectory, BinaryValue, ValueStream};
use super::{lengths, BitWriter};
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn parameter_body(count: usize, repeated: bool) -> Vec<u8> {
    let mut writer = BitWriter::default();
    writer.control(1);
    writer.integer(116, lengths().single_integer);
    writer.control(5);
    writer.pointer(1);
    if repeated {
        writer.control_repeat(false, u8::try_from(count).unwrap(), 0);
    } else {
        for _ in 0..count {
            writer.control(0);
        }
    }
    let body = writer.bytes();
    let mut bytes = u32::try_from(body.len()).unwrap().to_be_bytes().to_vec();
    bytes.extend_from_slice(&body);
    bytes
}

fn directory() -> [BinaryDirectory; 1] {
    let mut fields = std::array::from_fn(|_| BinaryValue::Default);
    fields[0] = BinaryValue::Integer(116);
    fields[1] = BinaryValue::Pointer(1);
    [BinaryDirectory::new(1, fields).unwrap()]
}

#[test]
fn binary_parameter_primitives_accept_actual_values_without_an_exhausted_probe() {
    // One record step, n primitive steps, and live-slot moves at 4/8/16/32.
    // A singleton offset search compares two u32 pairs: 2 * (1 + 4 + 4).
    for (count, moved) in [(0, 0), (1, 0), (64, 4 + 8 + 16 + 32)] {
        let bytes = parameter_body(count, false);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1 + u64_from_index(count)
            + u64_from_index(moved * std::mem::size_of::<BinaryValue>()) + 18;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let records = read_parameters(&bytes, lengths(), &directory(), &ctx).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!((records[0].offset, records[0].entity_type, records[0].directory_pointer), (1, 116, 1));
        assert_eq!(records[0].values.len(), count);
        assert!(records[0].values.iter().all(|value| *value == BinaryValue::Default));
        ctx.finish_session().unwrap();
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
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(first) = read_parameters(&bytes, lengths(), &directory(), &ctx).unwrap_err()
        else { panic!("expected parameter primitive refusal"); };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges binary parameter primitives");
        assert_eq!((first.used, first.additional, first.limit), (work, 1, work));
        for replay in [&bytes[..], &[]] {
            assert!(matches!(read_parameters(replay, lengths(), &directory(), &ctx),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn binary_parameter_keeps_pending_values_after_the_byte_source_ends() {
    for count in [4, 16] {
        let bytes = parameter_body(count, true);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let records = read_parameters(&bytes, lengths(), &directory(), &ctx).unwrap();
        assert_eq!(records[0].values.len(), count);
        assert!(records[0].values.iter().all(|value| *value == BinaryValue::Default));
        ctx.finish_session().unwrap();
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
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut stream = ValueStream::new(&[0], lengths(), &ctx);
    assert_eq!(stream.next().unwrap(), Some(BinaryValue::Default));
    assert_eq!(stream.next().unwrap(), None);
    stream.finish().unwrap();
    assert!(read_directory(&[], lengths(), &ctx).unwrap().is_empty());
    assert!(read_parameters(&[], lengths(), &[], &ctx).unwrap().is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn binary_stream_refusal_preserves_pending_values_cursor_and_first_error() {
    let mut writer = BitWriter::default();
    writer.control_repeat(false, 3, 1);
    writer.integer(7, lengths().single_integer);
    let bytes = writer.bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut stream = ValueStream::new(&bytes, lengths(), &ctx);
    assert_eq!(stream.next().unwrap(), Some(BinaryValue::Integer(7)));
    assert_eq!(stream.pending.len(), 2);
    assert!(stream.bits.is_empty());
    let cursor = (stream.bits.byte, stream.bits.bit);
    let CodecError::ResourceLimit(first) = ctx.charge_work(1, "test original Binary stream refusal").unwrap_err()
    else { panic!("expected original refusal"); };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!((first.used, first.additional, first.limit), (0, 1, 0));
    assert!(matches!(stream.next(), Err(CodecError::ResourceLimit(last)) if last == first));
    assert_eq!(stream.pending.len(), 2);
    assert_eq!((stream.bits.byte, stream.bits.bit), cursor);
    assert!(matches!(stream.finish(), Err(CodecError::ResourceLimit(last)) if last == first));
    for replay in [&[][..], &[0][..], &bytes[..]] {
        let mut stream = ValueStream::new(replay, lengths(), &ctx);
        assert!(matches!(stream.next(), Err(CodecError::ResourceLimit(last)) if last == first));
        assert_eq!((stream.bits.byte, stream.bits.bit), (0, 0));
        assert!(matches!(stream.finish(), Err(CodecError::ResourceLimit(last)) if last == first));
        assert!(matches!(read_directory(replay, lengths(), &ctx), Err(CodecError::ResourceLimit(last)) if last == first));
        assert!(matches!(read_parameters(replay, lengths(), &[], &ctx), Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}
