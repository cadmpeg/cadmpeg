// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;

fn global() -> crate::global::ProjectedGlobal {
    let bytes = crate::test_support::test_cards::fixed_ascii_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,3,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    );
    let scan = crate::test_support::scan(&bytes).unwrap();
    crate::test_support::parse_global(&scan).unwrap().0.length_context().unwrap()
}

fn policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

#[test]
fn coordinate_quantum_charges_only_actual_first_and_last_points() {
    let global = global();
    assert_eq!(global.single_precision_significance(), 3);
    for count in [1_usize, 64] {
        let total = u64::try_from(count).unwrap();
        for (work, accepts) in [(0, false), (total - 1, false), (total, true)] {
            let arena = DecodeArena::new();
            let policy = policy(work);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let points = (0..count).map(|_| Point3::new(100.0, 0.0, 0.0));
            let result = super::super::coordinate_quantum(&global, points, &ctx);
            if accepts {
                // Three significant digits at magnitude100 give a quantum1.
                assert_eq!(result.unwrap(), 1.0);
                ctx.finish_session().unwrap();
            } else {
                let first = match result {
                    Err(CodecError::ResourceLimit(first)) => first,
                    Err(error) => panic!("unexpected coordinate source error: {error:?}"),
                    Ok(_) => panic!("expected actual coordinate source refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, "iges face coordinate magnitude");
                assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
                for end in [count, 0] {
                    assert!(matches!(super::super::coordinate_quantum(&global,
                        (0..end).map(|_| Point3::new(100.0, 0.0, 0.0)), &ctx),
                        Err(CodecError::ResourceLimit(last)) if last == first));
                }
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
    }
}

#[test]
fn empty_coordinate_source_is_free_and_preserves_original_refusal() {
    let global = global();
    for fused in [false, true] {
        let arena = DecodeArena::new();
        let policy = policy(0);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if fused {
            let CodecError::ResourceLimit(first) = ctx.charge_work(1,
                "test original empty coordinate refusal").unwrap_err() else { panic!("expected original refusal"); };
            assert!(matches!(super::super::coordinate_quantum(&global,
                std::iter::empty(), &ctx), Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            assert_eq!(super::super::coordinate_quantum(&global, std::iter::empty(), &ctx).unwrap(), 0.0);
            ctx.finish_session().unwrap();
        }
    }
}
