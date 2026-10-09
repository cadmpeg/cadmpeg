// SPDX-License-Identifier: Apache-2.0

use super::super::{parse, Cur, FieldReader, Prim};
use crate::stream_error::StreamFailure;
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn manual_sat_typing_peeks_only_actual_primitives() {
    for count in [0_usize, 1, 64] {
        let prims = vec![Prim::Integer(7); count];
        let required = u64::try_from(count).unwrap();
        for cap in [0, required.saturating_sub(1), required] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_collection_items = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut cur = Cur { prims: &prims, pos: 0, scale: 1.0, failure: None, resource: None, ctx: &ctx };
            for _ in 0..count {
                if cur.bump().is_none() { break; }
            }
            if cap == required {
                assert_eq!(cur.pos, count);
                for _ in 0..64 { assert!(cur.peek().is_none()); }
                assert!(cur.resource.is_none());
                ctx.finish_session().unwrap();
            } else {
                let Some(CodecError::ResourceLimit(first)) = cur.resource.as_ref() else { panic!("actual primitive refusal"); };
                let first = *first;
                assert_eq!(first.operation, "read SAT typing primitive");
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                let mut empty = Cur { prims: &[], pos: 0, scale: 1.0, failure: None, resource: None, ctx: &ctx };
                assert!(empty.peek().is_none());
                assert!(matches!(empty.resource, Some(CodecError::ResourceLimit(last)) if last == first));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
    }
}

#[test]
fn manual_sat_absent_record_does_not_execute_a_record_step() {
    let bytes = b"0 0 0 0\n0 0 0 \n1 0 0\n";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "frame SAT record", None);
    assert!(matches!(parse(&ctx, bytes), Err(StreamFailure::Parse(error))
        if error.reason == "stream has no End-of-ASM-data or End-of-ACIS-data line"));
    drop(probe);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn manual_sat_absent_field_keeps_the_missing_terminator_error() {
    let bytes = b"0 0 0 0\n0 0 0 \n1 0 0\naudit \n";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "frame SAT field", None);
    assert!(matches!(parse(&ctx, bytes), Err(StreamFailure::Parse(error))
        if error.reason == "record `audit` has no `#` terminator"));
    drop(probe);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn manual_sat_empty_fields_are_free_and_preserve_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut reader = FieldReader { bytes: &[], pos: 0 };
    for _ in 0..64 { assert!(reader.next_field(&ctx).unwrap().is_none()); }
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original SAT scan refusal")
    else { panic!("original refusal"); };
    for _ in 0..64 {
        assert!(matches!(reader.next_field(&ctx), Err(StreamFailure::Resource(last)) if last == first));
        assert!(matches!(parse(&ctx, &[]), Err(StreamFailure::Resource(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}
