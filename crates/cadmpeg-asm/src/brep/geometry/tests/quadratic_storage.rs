// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::mem::size_of;

#[test]
fn homogeneous_degree_reduction_has_the_source_sized_peak_and_releases_scratch() {
    // Cubic degree elevation of [0,0,0,1], [3,6,0,1], [6,0,0,1].
    let input = [
        [0.0, 0.0, 0.0, 1.0], [2.0, 4.0, 0.0, 1.0],
        [4.0, 4.0, 0.0, 1.0], [6.0, 0.0, 0.0, 1.0],
    ];
    let expected = [[0.0, 0.0, 0.0, 1.0], [3.0, 6.0, 0.0, 1.0], [6.0, 0.0, 0.0, 1.0]];
    let copied = u64_from_index(input.len() * size_of::<[f64; 4]>());
    let reduced = u64_from_index((input.len() - 1) * size_of::<[f64; 4]>());
    let peak = copied + reduced;
    for cap in [peak - 1, peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::reduce_homogeneous_bezier_to_quadratic(&ctx, &input);
        if cap < peak {
            let Some(Err(CodecError::ResourceLimit(first))) = result else {
                panic!("expected degree-reduction allocation refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "ASM rational four-arc degree reduction");
            assert_eq!((first.limit, first.used, first.additional), (cap, copied, reduced));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let quadratic = result.unwrap().unwrap();
            for (actual, expected) in quadratic.into_iter().flatten().zip(expected.into_iter().flatten()) {
                assert!((actual - expected).abs() <= super::super::EPS_DEGREE_REDUCTION * expected.abs().max(1.0));
            }
            let released = ctx.reserve_scoped(peak, "all degree-reduction scratch released").unwrap();
            drop(released);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn rejected_homogeneous_degree_reduction_releases_scratch() {
    // The first three cubic controls require [6,0,0,1] as the endpoint;
    // [9,0,0,1] cannot be the degree elevation of the same quadratic.
    let input = [
        [0.0, 0.0, 0.0, 1.0], [2.0, 4.0, 0.0, 1.0],
        [4.0, 4.0, 0.0, 1.0], [9.0, 0.0, 0.0, 1.0],
    ];
    let peak = u64_from_index((input.len() + input.len() - 1) * size_of::<[f64; 4]>());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = peak;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(super::super::reduce_homogeneous_bezier_to_quadratic(&ctx, &input).is_none());
    let released = ctx.reserve_scoped(peak, "rejected degree-reduction scratch released").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}
