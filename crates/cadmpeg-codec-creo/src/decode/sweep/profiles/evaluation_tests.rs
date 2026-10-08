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
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "IR B-spline basis",
        run,
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(resource) if resource.operation == "IR B-spline basis")
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(run).expect("service"),
        Some([0.5, 0.0])
    );
}

#[test]
fn successive_profile_evaluations_release_lifted_curve_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
    use cadmpeg_ir::math::Point2;
    let geometry = cadmpeg_ir::sketches::SketchGeometry::nurbs(
        PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(1.0, 0.0), Point2::new(1.0, 1.0)],
            None,
            false,
        )
        .expect("fixture admission")
        .expect("linear NURBS"),
    );
    let segment = super::ProfileEntity::new(
        &cadmpeg_test_support::service_decode_context(),
        geometry,
        false,
    )
    .expect("fixture admission")
    .expect("profile entity");
    let limit = |repeats| {
        crate::test_support::allocation_limit_at(
            ResourceDimension::MaterializedBytes,
            None,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                for _ in 0..repeats {
                    let area =
                        super::extrusion_profile_signed_area(&ctx, std::slice::from_ref(&segment))?
                            .expect("finite area");
                    assert!((area.get() - 0.5).abs() <= f64::EPSILON);
                    let polyline = super::profile_nurbs_polyline(&ctx, &segment, 0.01)?
                        .expect("linear polyline");
                    assert_eq!(polyline.points, [[1.0, 0.0], [1.0, 1.0]]);
                }
                Ok::<_, CodecError>(())
            },
        )
    };
    assert_eq!(limit(1), limit(2));
}
