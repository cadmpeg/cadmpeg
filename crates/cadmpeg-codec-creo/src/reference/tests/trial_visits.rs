// SPDX-License-Identifier: Apache-2.0
use super::super::arc_z_fields;
use crate::scalar::ScalarCache;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn empty_arc_trials_are_free_and_keep_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let cache = ScalarCache::default();
    assert!(arc_z_fields(&ctx, &[], &cache, 7).expect("no source offset").is_none());
    let original = ctx.charge_work_limit(1, "seed empty arc trial refusal").expect_err("zero work cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(matches!(arc_z_fields(&ctx, &[], &cache, 7),
        Err(CodecError::ResourceLimit(r)) if r == original));
}

#[test]
fn arc_trials_admit_two_present_offset_passes_without_a_terminal_visit() {
    let valid = b"\x01\xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f".as_slice();
    for (body, valid) in [(b"\x00\x00\x00".as_slice(), false), (valid, true)] {
        let need = 2 * cadmpeg_core::decode::u64_from_index(body.len());
        let cache = ScalarCache::default();
        for allowed in 0..=need {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowed;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let result = arc_z_fields(&ctx, body, &cache, 7);
            let original = if allowed < need {
                let CodecError::ResourceLimit(r) = result.expect_err("next present offset") else {
                    panic!("work refusal");
                };
                assert_eq!(r.dimension, ResourceDimension::WorkUnits);
                assert_eq!(r.operation, "creo arc-z numeric trials");
                assert_eq!((r.used, r.additional), (allowed, 1));
                r
            } else {
                let circle = result.expect("both source passes admitted");
                if valid {
                    let circle = circle.expect("diameter image");
                    assert_eq!(circle.entity_id, 7);
                    assert_eq!(circle.offset, 1);
                    assert_eq!(<[f64; 3]>::from(circle.center().get()), [0.0; 3]);
                    assert_eq!(circle.radius().get(), 1.0);
                    assert_eq!(<[f64; 3]>::from(circle.start().get()), [1.0, 0.0, 0.0]);
                    assert_eq!(<[f64; 3]>::from(circle.end().get()), [-1.0, 0.0, 0.0]);
                    assert!(!circle.center_stored());
                } else {
                    assert!(circle.is_none());
                }
                let r = ctx.charge_work_limit(1, "after arc trial passes").expect_err("exact cap");
                assert_eq!((r.used, r.additional), (need, 1));
                r
            };
            assert!(matches!(arc_z_fields(&ctx, &[], &cache, 7),
                Err(CodecError::ResourceLimit(r)) if r == original));
        }
    }
}
