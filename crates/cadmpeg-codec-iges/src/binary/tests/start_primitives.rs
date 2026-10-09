// SPDX-License-Identifier: Apache-2.0

use super::{lengths, BitWriter};
use super::super::normalize_start;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn text_primitives(count: usize) -> Vec<u8> {
    let mut writer = BitWriter::default();
    for _ in 0..count {
        writer.control(6);
        writer.string(b"a", lengths());
    }
    writer.bytes()
}

#[test]
fn binary_start_primitives_accept_only_actual_visits_and_text_moves() {
    // Each one-byte string executes primitive, segment, payload and append work.
    // At 64 bytes the output growth copies its live 8-, 16- and 32-byte prefixes.
    for (count, work) in [(1, 4), (64, 4 * 64 + 8 + 16 + 32)] {
        let bytes = text_primitives(count);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let text = normalize_start(&bytes, lengths(), &ctx).unwrap();
        assert_eq!(text.len(), count);
        assert!(text.iter().all(|byte| *byte == b'a'));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn binary_start_primitives_refuse_the_first_or_last_real_value_and_replay_exactly() {
    for (count, work) in [(1, 0), (64, 4 * 63 + 8 + 16 + 32)] {
        let bytes = text_primitives(count);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(first) = normalize_start(&bytes, lengths(), &ctx).unwrap_err()
        else { panic!("expected primitive refusal"); };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges binary start primitives");
        assert_eq!((first.used, first.additional, first.limit), (work, 1, work));
        for replay in [&bytes[..], &[]] {
            assert!(matches!(normalize_start(replay, lengths(), &ctx),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn empty_binary_start_returns_its_grammar_refusal_without_source_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(normalize_start(&[], lengths(), &ctx), Err(CodecError::Malformed(_))));
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn binary_start_keeps_pending_repeated_text_after_the_byte_source_ends() {
    let mut writer = BitWriter::default();
    writer.control_repeat(false, 4, 6);
    writer.string(b"a", lengths());
    let bytes = writer.bytes();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(normalize_start(&bytes, lengths(), &ctx).unwrap(), b"aaaa");
    ctx.finish_session().unwrap();
}

#[test]
fn binary_start_nontext_first_value_does_not_admit_later_primitives() {
    let bytes = [0; 64];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(normalize_start(&bytes, lengths(), &ctx), Err(CodecError::Malformed(_))));
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}
