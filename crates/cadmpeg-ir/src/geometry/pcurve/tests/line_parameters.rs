// SPDX-License-Identifier: Apache-2.0

use crate::geometry::pcurve::{
    LinePcurve, OffsetPcurve, PcurveGeometry, PlacedPcurve, TrimmedPcurve,
};
use crate::math::Point2;
use crate::transform::Transform2;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn nested_line() -> PcurveGeometry {
    let line = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)).unwrap(),
    );
    let trim =
        PcurveGeometry::Trimmed(TrimmedPcurve::try_new([0.0, 1.0], false, Box::new(line)).unwrap());
    PcurveGeometry::Transformed(
        PlacedPcurve::try_new(
            Box::new(trim),
            Transform2::affine([[2.0, 0.0, 5.0], [0.0, 3.0, 7.0]]).unwrap(),
        )
        .unwrap(),
    )
}

#[test]
fn pcurve_line_parameters_admit_first_later_and_active_session_frames() {
    let geometry = nested_line();
    for active in [false, true] {
        for (dimension, operation) in [
            (ResourceDimension::RecursionDepth, "pcurve line parameter nesting"),
            (ResourceDimension::WorkUnits, "pcurve line parameter visit"),
        ] {
            cadmpeg_test_support::refusal::resource_limit_at(dimension, operation,
                |cap| crate::geometry::tests::budget::with_limit(dimension, cap, |ctx| {
                    let caller = active.then(|| ctx.enter_nested_limit("active caller").expect("caller frame"));
                    let result = geometry.line_parameters(ctx).map_err(Into::into);
                    drop(caller);
                    result
                }));
        }
    }
}

#[test]
fn pcurve_line_parameters_preserve_affine_trim_and_non_line_semantics() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 2;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        nested_line().line_parameters(&ctx).unwrap(),
        Some((Point2::new(7.0, 13.0), Point2::new(6.0, 12.0)))
    );
    let offset =
        PcurveGeometry::Offset(OffsetPcurve::try_new(2.0, Box::new(nested_line())).unwrap());
    assert_eq!(offset.line_parameters(&ctx).unwrap(), None);
    let placed_offset = PcurveGeometry::Transformed(
        PlacedPcurve::try_new(Box::new(offset), Transform2::identity()).unwrap(),
    );
    assert_eq!(placed_offset.line_parameters(&ctx).unwrap(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn pcurve_line_parameters_do_not_charge_unvisited_bases_or_fixed_leaves() {
    use cadmpeg_core::CodecError;
    let line = PcurveGeometry::Line(LinePcurve::U_AXIS);
    let offset = PcurveGeometry::Offset(OffsetPcurve::try_new(2., Box::new(nested_line())).expect("offset"));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    crate::geometry::tests::budget::with_policy(ResourceDimension::WorkUnits, 0, policy, |ctx| {
        assert_eq!(line.line_parameters(ctx)?, Some((Point2::new(0., 0.), Point2::new(1., 0.))));
        assert_eq!(offset.line_parameters(ctx)?, None);
        let original = ctx.charge_work_limit(1, "first refusal").expect_err("zero budget");
        assert!(matches!(line.line_parameters(ctx), Err(sticky) if sticky == original));
        Err::<(), CodecError>(original.into())
    }).expect_err("sticky refusal");
}
