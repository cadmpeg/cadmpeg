// SPDX-License-Identifier: Apache-2.0

use crate::geometry::tests::budget::with_limit;
use cadmpeg_core::decode::ResourceDimension;

#[test]
fn pcurve_pole_validation_does_not_charge_unvisited_mutation() {
    use crate::geometry::pcurve::PcurveNurbs;
    use crate::math::Point2;
    for rational in [false, true] {
        let mut curve = PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 1,
            vec![0., 0., 1., 1.], vec![Point2::new(0., 0.), Point2::new(1., 0.)],
            rational.then(|| vec![1., 2.]), false).expect("admitted fixture or edit").expect("admitted fixture or edit");
        let original = curve.clone();
        assert_eq!(with_limit(ResourceDimension::WorkUnits, 1, |ctx| curve.try_map_control_points(
            |_, _| Err("first pole refused"), ctx)).expect("admitted fixture or edit"), Err("first pole refused"));
        assert_eq!(curve, original);
        assert_eq!(with_limit(ResourceDimension::WorkUnits, 1, |ctx| curve.try_map_control_points_in_place(
            |_| Err("first pole refused"), ctx)).expect("admitted fixture or edit"), Err("first pole refused"));
        assert_eq!(curve, original);
    }
}

#[test]
fn analytic_pcurve_operations_are_free_and_preserve_sticky_refusal() {
    use crate::geometry::pcurve::{LinePcurve, PcurveGeometry};
    use crate::geometry::tests::budget::with_policy;
    use crate::math::Point2;
    use cadmpeg_core::decode::DecodePolicy;
    use cadmpeg_core::CodecError;
    let pcurve = PcurveGeometry::Line(LinePcurve::try_new(Point2::new(1., 2.), Point2::new(1., 0.)).expect("line"));
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    with_policy(ResourceDimension::WorkUnits, 0, policy, |ctx| {
        assert_eq!(pcurve.try_clone_for_decode(ctx, "free pcurve copy")?, pcurve);
        assert_eq!(pcurve.clone().scaled_coordinates_owned(ctx, [2., 2.])?.expect("scale"),
            PcurveGeometry::Line(LinePcurve::try_new(Point2::new(2., 4.), Point2::new(2., 0.)).expect("scaled line")));
        let original = ctx.charge_work_limit(1, "first refusal").expect_err("zero budget");
        for result in [pcurve.try_clone_for_decode(ctx, "later pcurve copy").map(|_| ()),
            pcurve.scaled_coordinates_owned(ctx, [2., 2.]).map(|_| ())] {
            assert!(matches!(result, Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        }
        Err::<(), _>(original.into())
    }).expect_err("original refusal stays sticky");
}
