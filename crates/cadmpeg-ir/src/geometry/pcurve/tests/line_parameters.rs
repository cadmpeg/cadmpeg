// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::geometry::pcurve::{LinePcurve, OffsetPcurve, PcurveGeometry, PlacedPcurve, TrimmedPcurve};
use crate::math::Point2;
use crate::transform::Transform2;

fn nested_line() -> PcurveGeometry {
    let line = PcurveGeometry::Line(LinePcurve::try_new(Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)).unwrap());
    let trim = PcurveGeometry::Trimmed(TrimmedPcurve::try_new([0.0, 1.0], false, Box::new(line)).unwrap());
    PcurveGeometry::Transformed(PlacedPcurve::try_new(Box::new(trim), Transform2::affine([[2.0, 0.0, 5.0], [0.0, 3.0, 7.0]]).unwrap()).unwrap())
}

#[test]
fn pcurve_line_parameters_admit_first_later_and_active_session_frames() {
    let geometry = nested_line();
    for (dimension, cap, active) in [
        (ResourceDimension::RecursionDepth, 0, false),
        (ResourceDimension::RecursionDepth, 1, false),
        (ResourceDimension::RecursionDepth, 2, false),
        (ResourceDimension::RecursionDepth, 3, true),
        (ResourceDimension::WorkUnits, 0, false),
        (ResourceDimension::WorkUnits, 1, false),
        (ResourceDimension::WorkUnits, 2, false),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let caller = active.then(|| ctx.enter_nested_limit("active caller").unwrap());
        let limit = geometry.line_parameters(&ctx).expect_err("line walk must refuse");
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, if dimension == ResourceDimension::RecursionDepth {
            "pcurve line parameter nesting"
        } else { "pcurve line parameter visit" });
        drop(caller);
        assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn pcurve_line_parameters_preserve_affine_trim_and_non_line_semantics() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 3;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(nested_line().line_parameters(&ctx).unwrap(), Some((Point2::new(7.0, 13.0), Point2::new(6.0, 12.0))));
    let offset = PcurveGeometry::Offset(OffsetPcurve::try_new(2.0, Box::new(nested_line())).unwrap());
    assert_eq!(offset.line_parameters(&ctx).unwrap(), None);
    let placed_offset = PcurveGeometry::Transformed(PlacedPcurve::try_new(Box::new(offset), Transform2::identity()).unwrap());
    assert_eq!(placed_offset.line_parameters(&ctx).unwrap(), None);
    ctx.finish_session().unwrap();
}
