// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::surface::{opaque_spans, scalar_frames, SurfaceParameterOpaqueSpan, SurfaceParameterScalar, SurfaceParameterScalarFrame};

#[test]
fn opaque_scalar_spans_visit_each_token_once_without_a_terminal_probe() {
    for count in [0, 1, 7, 17, 257] {
        let body = vec![0xe4; count];
        let tokens: Vec<_> = (0..count).map(|offset| SurfaceParameterScalar {
            value: Some(1.0), raw: vec![0xe4], offset,
        }).collect();
        let work = u64_from_index(count);
        for cap in 0..=work + 1 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || opaque_spans(&ctx, &body, &tokens);
            let original = if cap < work {
                let CodecError::ResourceLimit(refusal) = run().expect_err("token visit Work cap") else {
                    panic!("resource refusal");
                };
                assert_eq!(refusal.operation, "creo surface opaque token spans");
                assert_eq!((refusal.used, refusal.additional), (cap, 1));
                refusal
            } else {
                assert!(run().expect("all bytes are token-owned").is_empty());
                let refusal = ctx.charge_work_limit(cap - work + 1, "after opaque token visits")
                    .expect_err("exact remaining Work");
                assert_eq!((refusal.used, refusal.additional), (work, cap - work + 1));
                refusal
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn opaque_scalar_spans_preserve_prefix_middle_and_terminal_gap_identity() {
    for gap in [1, 7, 17, 257] {
        let mut body = vec![0xed; gap];
        let mut tokens = Vec::new();
        for _ in 0..3 {
            tokens.push(SurfaceParameterScalar { value: Some(1.0), raw: vec![0xe4], offset: body.len() });
            body.push(0xe4);
            body.extend(std::iter::repeat_n(0xed, gap));
        }
        let work = 3 + u64_from_index(4 * gap); // Three visits and four retained gap copies.
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_collection_items = 4;
        policy.limits.max_retained_bytes = u64_from_index(4 * gap + 4 * std::mem::size_of::<SurfaceParameterOpaqueSpan>());
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || opaque_spans(&ctx, &body, &tokens);
        let spans = run().expect("exact visit, copy and initial backing costs");
        assert_eq!(spans.len(), 4);
        for (index, span) in spans.iter().enumerate() {
            assert_eq!(span.offset, index * (gap + 1));
            assert_eq!(span.raw, vec![0xed; gap]);
        }
        let original = ctx.charge_work_limit(1, "after opaque gap copies").expect_err("exact Work");
        assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, work, 1));
        assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn scalar_frames_visit_each_start_adjacent_pair_and_copied_token_once() {
    for count in [0_usize, 1, 2, 4] {
        for separated in [false, true] {
            let tokens: Vec<_> = (0..count).map(|index| SurfaceParameterScalar {
                value: Some(1.0), raw: vec![0xe4], offset: index * if separated { 2 } else { 1 },
            }).collect();
            let frame_count = if separated { count } else { usize::from(count != 0) };
            let work = u64_from_index(frame_count + count.saturating_sub(1) + 2 * count);
            let retained = if count == 0 { 0 } else {
                count + 4 * frame_count * std::mem::size_of::<SurfaceParameterScalar>()
                    + 4 * std::mem::size_of::<SurfaceParameterScalarFrame>()
            };
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            policy.limits.max_retained_bytes = u64_from_index(retained);
            policy.limits.max_collection_items = u64_from_index(count + frame_count);
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || scalar_frames(&ctx, &tokens);
            let frames = run().expect("exact starts, comparisons, visits, copies and initial backing costs");
            assert_eq!(frames.len(), frame_count);
            if separated {
                for (index, frame) in frames.iter().enumerate() {
                    assert_eq!(frame.offset, 2 * index);
                    assert_eq!(frame.slots, tokens[index..=index]);
                }
            } else if count != 0 {
                assert_eq!(frames[0].offset, 0);
                assert_eq!(frames[0].slots, tokens);
            }
            let original = ctx.charge_work_limit(1, "after scalar frame visits").expect_err("exact Work");
            assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, work, 1));
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn scalar_frame_start_and_adjacency_refuse_before_slot_storage() {
    let tokens = [
        SurfaceParameterScalar { value: Some(1.0), raw: vec![0xe4], offset: 0 },
        SurfaceParameterScalar { value: Some(1.0), raw: vec![0xe4], offset: 1 },
    ];
    for cap in [0, 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || scalar_frames(&ctx, &tokens);
        let CodecError::ResourceLimit(original) = run().expect_err("visit before storage") else {
            panic!("resource refusal");
        };
        assert_eq!(original.operation, if cap == 0 { "creo surface scalar frame starts" } else { "creo surface scalar frame adjacency" });
        assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, cap, 1));
        assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}
