// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;

#[test]
fn profile_sampling_propagates_evaluator_refusal() {
    let nurbs = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
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
    .expect("fixture constructor admission")
    .expect("line spline");
    let run = |ctx: &DecodeContext<'_>| {
        let mut evaluator = cadmpeg_ir::eval::decode::NurbsPointEvaluator::new(ctx, &nurbs)?;
        super::nurbs_profile_point(ctx, &mut evaluator, &nurbs, 0.5)
    };
    let error = crate::test_support::last_refusal_at(&[], cadmpeg_core::decode::ResourceDimension::CollectionItems, "IR B-spline basis", &run);
    assert!(matches!(error, CodecError::ResourceLimit(resource) if resource.operation == "IR B-spline basis"));
    assert_eq!(crate::decode::with_test_decode_ctx(run).expect("service"), Some([0.5, 0.0]));
}
