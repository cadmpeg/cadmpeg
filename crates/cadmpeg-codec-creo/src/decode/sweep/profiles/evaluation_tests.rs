// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;

#[test]
fn profile_sampling_propagates_evaluator_refusal() {
    let nurbs = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("line spline");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(
        matches!((|| { let mut evaluator = cadmpeg_ir::eval::decode::NurbsPointEvaluator::new(&ctx, &nurbs)?; super::nurbs_profile_point(&ctx, &mut evaluator, &nurbs, 0.5) })(), Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR B-spline basis")
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        (|| {
            let mut evaluator = cadmpeg_ir::eval::decode::NurbsPointEvaluator::new(&ctx, &nurbs)?;
            super::nurbs_profile_point(&ctx, &mut evaluator, &nurbs, 0.5)
        })()
        .expect("service"),
        Some([0.5, 0.0])
    );
}
