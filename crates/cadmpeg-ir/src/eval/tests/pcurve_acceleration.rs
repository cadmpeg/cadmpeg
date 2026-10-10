// SPDX-License-Identifier: Apache-2.0

use crate::eval::admission::EvaluationAdmission;
use crate::eval::{pcurve_tangent, EvaluationFailure};
use crate::geometry::pcurve::{
    LinePcurve, OffsetPcurve, ParabolaPcurve, PcurveGeometry, PlacedPcurve, TrimmedPcurve,
};
use crate::math::Point2;
use crate::transform::Transform2;

fn placed_trimmed(basis: PcurveGeometry) -> PcurveGeometry {
    PcurveGeometry::Transformed(PlacedPcurve::try_new(
        Box::new(PcurveGeometry::Trimmed(TrimmedPcurve::try_new(
            [0.0, 1.0], true, Box::new(basis),
        ).unwrap())),
        Transform2::identity(),
    ).unwrap())
}

fn nested_offset() -> PcurveGeometry {
    let basis = PcurveGeometry::Line(LinePcurve::try_new(
        Point2::new(0.0, 0.0), Point2::new(0.0, 1.0),
    ).unwrap());
    let inner = PcurveGeometry::Offset(OffsetPcurve::try_new(1.0, Box::new(basis)).unwrap());
    PcurveGeometry::Offset(OffsetPcurve::try_new(1.0, Box::new(placed_trimmed(inner))).unwrap())
}

#[test]
fn placed_trimmed_offset_preserves_unstated_acceleration() {
    let curve = nested_offset();
    assert_eq!(pcurve_tangent(EvaluationAdmission::Standard, &curve, 0.0), Err(EvaluationFailure::NoValue));
    assert_eq!(
        crate::eval::decode::pcurve_uv(EvaluationAdmission::Standard, &curve, 0.0)
            .map(crate::units::FinitePoint2::get),
        Ok(Point2::new(-2.0, 0.0)),
    );
}

#[test]
fn placed_trimmed_offset_preserves_nonfinite_declared_acceleration() {
    // At zero the parabola has point (0,0) and tangent (0,1). Its axial
    // second derivative is 1/(2*focal), beyond the finite f64 range.
    let basis = PcurveGeometry::Parabola(ParabolaPcurve::try_new(
        Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), Point2::new(0.0, 1.0),
        f64::from_bits(1),
    ).unwrap());
    let curve = PcurveGeometry::Offset(OffsetPcurve::try_new(1.0, Box::new(placed_trimmed(basis))).unwrap());
    let Err(EvaluationFailure::NonFinite(tangent)) = pcurve_tangent(EvaluationAdmission::Standard, &curve, 0.0) else {
        panic!("the declared acceleration cannot form a finite offset tangent");
    };
    assert!(tangent.u.is_nan() && tangent.v.is_nan());
    assert_eq!(
        crate::eval::decode::pcurve_uv(EvaluationAdmission::Standard, &curve, 0.0)
            .map(crate::units::FinitePoint2::get),
        Ok(Point2::new(-1.0, 0.0)),
    );
}

#[test]
fn nested_offset_admits_only_actual_carrier_frames_and_preserves_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let curve = nested_offset();
    // Outer offset, placement, trim, inner offset, line: five active frames.
    for cap in 0..=5 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = cap;
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        {
            let result = pcurve_tangent(&ctx, &curve, 0.0);
            if cap == 5 {
                assert_eq!(result, Err(EvaluationFailure::NoValue));
                ctx.finish_session().unwrap();
            } else {
                let Err(EvaluationFailure::ResourceLimit(first)) = result else {
                    panic!("the next actual carrier frame exceeds the depth cap");
                };
                assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                assert_eq!(first.operation, "geometry evaluation nesting");

                assert_eq!(ctx.resource_refusal(), Some(first));
                assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
            }
        }
    }
}
