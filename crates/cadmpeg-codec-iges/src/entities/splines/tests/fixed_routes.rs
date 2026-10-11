// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::CadIr;
use crate::entities::geometry::SourceSequences;

fn with_zero_context(mut run: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>)) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 { run(&ctx); }
    ctx.finish_session().unwrap();
}

#[test]
fn spline_invalid_edge_interval_adds_no_edge() {
    let nurbs = crate::test_support::with_service_context(&[], |ctx| {
        NurbsCurve::from_lanes(ctx, 1, vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)], None, false)
            .expect("fixture admission").expect("valid fixture")
    });
    // Prebuild all consumed arguments outside the measured caller sessions.
    // The fresh session calls the route sixty-four times.
    let mut fixtures: Vec<_> = (0..64).map(|_| nurbs.clone()).collect();
    let entry = crate::test_support::directory_target(1, 112);
    let mut ir = CadIr::empty();
    let before = ir.model.clone();
    let mut sequences = SourceSequences::default();
    with_zero_context(|ctx| {
        let result = super::super::add_edge(&mut ir, &entry,
            fixtures.pop().expect("prebuilt fixture"),
            [FiniteReal::ZERO, FiniteReal::ZERO], &mut sequences, ctx);
        assert!(matches!(result, Ok(None)));
        assert_eq!(ir.model, before);
    });
    assert!(fixtures.is_empty());
}
