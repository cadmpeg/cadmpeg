// SPDX-License-Identifier: Apache-2.0
use super::super::PrimitiveTriangleStrip;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn empty_strip_validation_is_free_and_keeps_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(PrimitiveTriangleStrip::new(&ctx, 17, Vec::new(), None, Vec::new())
        .expect("no span").is_none());
    let original = ctx.charge_work_limit(1, "seed empty primitive strip refusal")
        .expect_err("zero work cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(matches!(PrimitiveTriangleStrip::new(&ctx, 17, Vec::new(), None, Vec::new()),
        Err(CodecError::ResourceLimit(r)) if r == original));
}

#[test]
fn strip_validation_admits_present_spans_and_stops_at_the_first_invalid_span() {
    for (spans, vertices, visited, valid) in [
        ([2, 3], 3, 1_u64, false),
        ([3, 2], 6, 2, false),
        ([3, 3], 6, 2, true),
    ] {
        for allowed in 0..=visited {
            let positions = super::finite_points(vec![[0.0; 3]; vertices]);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowed;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let result = PrimitiveTriangleStrip::new(&ctx, 17, positions, None, spans.to_vec());
            let original = if allowed < visited {
                let CodecError::ResourceLimit(r) = result.expect_err("next present span") else {
                    panic!("work refusal");
                };
                assert_eq!(r.dimension, ResourceDimension::WorkUnits);
                assert_eq!(r.operation, "creo primitive strip validation");
                assert_eq!((r.used, r.additional), (allowed, 1));
                r
            } else {
                let result = result.expect("all visited spans admitted");
                if valid {
                    let strip = result.expect("complete source strip");
                    assert_eq!(strip.offset, 17);
                    assert_eq!(strip.strip_lengths(), &spans);
                    assert_eq!(strip.positions().len(), vertices);
                    assert!(strip.positions().all(|position| position.get() == [0.0; 3]));
                    assert!(strip.normals().is_none());
                } else {
                    assert!(result.is_none());
                }
                let r = ctx.charge_work_limit(1, "after primitive strip spans").expect_err("exact cap");
                assert_eq!((r.used, r.additional), (visited, 1));
                r
            };
            assert!(matches!(PrimitiveTriangleStrip::new(&ctx, 17, Vec::new(), None, Vec::new()),
                Err(CodecError::ResourceLimit(r)) if r == original));
        }
    }
}
