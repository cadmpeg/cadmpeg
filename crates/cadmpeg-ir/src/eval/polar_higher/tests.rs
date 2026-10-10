// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;
use crate::eval::{decode::Scratch, pcurve_uv_differential, pcurve_uv_unsettled, ContactRequest,
    PcurveAcceleration, SurfaceRequest};
use crate::geometry::pcurve::{PcurveGeometry, PolarHarmonicPcurve};
use crate::math::{Point2, Point3, Vector3};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

const EPS_POLAR_HIGHER: f64 = 64.0 * f64::EPSILON;

fn harmonic(scale: f64) -> PcurveGeometry {
    PcurveGeometry::PolarHarmonic(PolarHarmonicPcurve::try_new(
        Point2::new(0.0, 0.0), Point2::new(2.0 * scale, 0.0), Point2::new(0.0, scale),
        0.0, 0.0, 0.0).unwrap())
}

#[test]
fn actual_harmonic_requested_orders_keep_lower_bits_subsets_scale_and_zero_resource_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        for scale in [f64::from_bits(1), 1e-200, 1.0, 1e200, f64::MAX / 4.0] {
            let curve = harmonic(scale);
            let scratch = Scratch::new(admission);
            let ordinary = pcurve_uv_differential(&scratch, &curve, FiniteReal::ZERO).unwrap();
            assert_eq!(ordinary.point.unwrap().get(), Point2::new(0.0, 0.0));
            assert!((ordinary.tangent.unwrap().u - 0.5).abs() <= EPS_POLAR_HIGHER);
            for order in [2, 3, 4, 5] {
                let mut higher = [Err(EvaluationFailure::NoValue); 3];
                let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::ZERO,
                    Some((order, &mut higher))).unwrap();
                assert_eq!(actual.point, ordinary.point);
                assert_eq!(actual.tangent, ordinary.tangent);
                match (actual.acceleration, ordinary.acceleration) {
                    (PcurveAcceleration::Finite(a), PcurveAcceleration::Finite(b)) => assert_eq!(a, b),
                    _ => panic!("this actual harmonic has finite acceleration"),
                }
                // atan((1/2)*tan t) has true orders3=.75,4=0,5=3.75 at0.
                for (at, expected) in [0.75, 0.0, 3.75].into_iter().enumerate() {
                    if at + 3 > order { assert_eq!(higher[at], Err(EvaluationFailure::NoValue)); }
                    else {
                        let actual = higher[at].unwrap().get();
                        assert!((actual.u - expected).abs() <= EPS_POLAR_HIGHER);
                        assert_eq!(actual.v, 0.0);
                    }
                }
            }
        }
    }
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn actual_harmonic_requested_axial_orders_follow_the_stored_coefficients() {
    let curve = PcurveGeometry::PolarHarmonic(PolarHarmonicPcurve::try_new(
        Point2::new(0.0, 0.0), Point2::new(2.0, 0.0), Point2::new(0.0, 1.0),
        7.0, 3.0, 5.0).unwrap());
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let old = pcurve_uv_differential(&scratch, &curve, FiniteReal::ZERO).unwrap();
    let mut higher = [Err(EvaluationFailure::NoValue); 3];
    let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::ZERO,
        Some((5, &mut higher))).unwrap();
    assert_eq!(actual.point, old.point);
    assert_eq!(actual.tangent, old.tangent);
    assert_eq!(actual.point.unwrap().v, 10.0);
    assert_eq!(actual.tangent.unwrap().v, 5.0);
    assert_eq!(actual.acceleration.finite().unwrap().v, -3.0);
    for (actual, expected) in higher.into_iter().zip([
        Point2::new(0.75, -5.0), Point2::new(0.0, 3.0), Point2::new(3.75, 5.0),
    ]) {
        let actual = actual.unwrap().get();
        assert!((actual.u - expected.u).abs() <= EPS_POLAR_HIGHER);
        assert_eq!(actual.v, expected.v);
    }
}

#[test]
fn actual_harmonic_requested_orders_keep_center_phase_and_orientation() {
    // At0, atan(sin(t)/(2+cos(t))) has third=-2/27 and fifth=-10/81.
    // Atpi/2, the centered anisotropic chart has third=-12 and fifth=480.
    // Both follow Taylor division and integration of the local angle law.
    for reversed in [false, true] {
        let sign = if reversed { -1.0 } else { 1.0 };
        for (center, cosine, at, expected, tolerance) in [
            (2.0, 1.0, 0.0, [-2.0 / 27.0, 0.0, -10.0 / 81.0], EPS_POLAR_HIGHER),
            (0.0, 2.0, std::f64::consts::FRAC_PI_2, [-12.0, 0.0, 480.0],
                EPS_POLAR_HIGHER * 512.0),
        ] {
            let curve = PcurveGeometry::PolarHarmonic(PolarHarmonicPcurve::try_new(
                Point2::new(center, 0.0), Point2::new(cosine, 0.0), Point2::new(0.0, sign),
                0.0, 0.0, 0.0).unwrap());
            let scratch = Scratch::new(EvaluationAdmission::Standard);
            let at = FiniteReal::new(at).unwrap();
            let old = pcurve_uv_differential(&scratch, &curve, at).unwrap();
            let mut higher = [Err(EvaluationFailure::NoValue); 3];
            let actual = pcurve_uv_unsettled(&scratch, &curve, at, Some((5, &mut higher))).unwrap();
            assert_eq!(actual.point, old.point);
            assert_eq!(actual.tangent, old.tangent);
            assert_eq!(actual.acceleration.finite(), old.acceleration.finite());
            for (actual, expected) in higher.into_iter().zip(expected) {
                let actual = actual.unwrap().get();
                assert!((actual.u - sign * expected).abs() <= tolerance);
                assert_eq!(actual.v, 0.0);
            }
        }
    }
}

#[test]
fn actual_harmonic_requested_orders_keep_undefined_and_nonfinite_point_routes() {
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    for (center, cosine, nonfinite) in [(-1.0, 1.0, false), (f64::MAX, f64::MAX, true)] {
        let curve = PcurveGeometry::PolarHarmonic(PolarHarmonicPcurve::try_new(
            Point2::new(center, 0.0), Point2::new(cosine, 0.0), Point2::new(0.0, 1.0),
            0.0, 0.0, 0.0).unwrap());
        let old = pcurve_uv_differential(&scratch, &curve, FiniteReal::ZERO);
        let mut higher = [Err(EvaluationFailure::NoValue); 3];
        let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::ZERO,
            Some((5, &mut higher)));
        if nonfinite {
            for evaluation in [old.unwrap(), actual.unwrap()] {
                let reached = evaluation.point.unwrap_err();
                assert!(reached.u.is_nan());
                assert_eq!(reached.v, 0.0);
                assert!(matches!(evaluation.tangent, Err(EvaluationFailure::NonFinite(_))));
                assert!(evaluation.acceleration.finite().is_none());
                assert_eq!(evaluation.resource, None);
            }
        } else { assert!(old.is_none() && actual.is_none()); }
        assert_eq!(higher, [Err(EvaluationFailure::NoValue); 3]);
    }
}

#[test]
fn actual_harmonic_contact_uses_true_plane_and_cylinder_composition() {
    use crate::geometry::{ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry};
    use crate::geometry::analytic::CylinderSurface;
    use crate::index::{ModelIndex, StandardIndex};
    let (mut ir, _) = crate::eval::tests::variable_blend::variable_blend_eval_fixture(
        Point3::new(0.0, 0.0, 0.0), [(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)); 2],
        [0.0; 2], None);
    let ProceduralSurfaceDefinition::VariableBlend(payload) = ir.model.procedural_surfaces[0].definition()
        else { panic!("actual blend fixture"); };
    let mut side = payload.construction().sides[0].clone();
    side.pcurve = Some(harmonic(1.0));
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for cylinder in [false, true] {
        if cylinder {
            ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                CylinderSurface::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0), 2.0).unwrap()));
        }
        let index = ModelIndex::build(&ir, StandardIndex);
        // For radius2: x=2cos(theta), y=2sin(theta), theta1=.5,theta3=.75,theta5=3.75.
        // Leibniz/composition gives x2=-.5,y3=1.25,x4=-23/8,y5=61/16.
        let expected = if cylinder {
            [Vector3::new(-0.5, 0.0, 0.0), Vector3::new(0.0, 1.25, 0.0),
                Vector3::new(-2.875, 0.0, 0.0), Vector3::new(0.0, 3.8125, 0.0)]
        } else {
            [Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.75, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 0.0), Vector3::new(3.75, 0.0, 0.0)]
        };
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let old = crate::eval::variable_blend_contact_track(admission, &index, &side, 0.0,
                ContactRequest::Tangent).unwrap();
            for (last, order) in [SurfaceRequest::Second, SurfaceRequest::Third,
                SurfaceRequest::Fourth, SurfaceRequest::Fifth].into_iter().enumerate() {
                let actual = crate::eval::variable_blend_contact_track(admission, &index, &side, 0.0,
                    ContactRequest::Higher(order)).unwrap();
                assert_eq!(actual.point(), old.point());
                assert_eq!(actual.tangent(), old.tangent());
                for (at, expected) in expected.into_iter().enumerate() {
                    if at <= last {
                        assert!((actual.higher[at].unwrap().get() - expected).norm() <= EPS_POLAR_HIGHER);
                    } else { assert_eq!(actual.higher[at], Err(EvaluationFailure::NoValue)); }
                }
            }
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn angular_higher_never_treats_missing_radial_orders_or_origin_as_exact_zero() {
    let zero = FinitePoint2::new(Point2::new(0.0, 0.0)).unwrap();
    let point = FinitePoint2::new(Point2::new(1.0, 0.0)).unwrap();
    let tangent = FinitePoint2::new(Point2::new(0.0, 1.0)).unwrap();
    // Actual radial r=(1,t) gives theta=atan(t), not a zero higher chart.
    let radial = [Ok(point), Ok(tangent), Ok(zero), Ok(zero), Ok(zero), Ok(zero)];
    assert_eq!(angular(radial, 5, None).map(|r| r.unwrap().get()), [-2.0, 0.0, 24.0]);
    let mut missing = radial;
    missing[3] = Err(EvaluationFailure::NoValue);
    assert_eq!(angular(missing, 5, None), [Err(EvaluationFailure::NoValue); 3]);
    let mut nonfinite = radial;
    nonfinite[5] = Err(EvaluationFailure::NonFinite(()));
    let actual = angular(nonfinite, 5, None);
    assert_eq!(actual[0].unwrap().get(), -2.0);
    assert_eq!(actual[1].unwrap().get(), 0.0);
    assert_eq!(actual[2], Err(EvaluationFailure::NonFinite(())));
    assert_eq!(angular([Ok(zero); 6], 5, None), [Err(EvaluationFailure::NoValue); 3]);
}

#[test]
fn actual_requested_harmonic_keeps_original_fuse_before_frame_and_finish() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx.charge_work_limit(1, "actual prior harmonic refusal").unwrap_err();
    let scratch = Scratch::new(&ctx);
    let mut higher = [Err(EvaluationFailure::NoValue); 3];
    let result = pcurve_uv_unsettled(&scratch, &harmonic(1.0), FiniteReal::ZERO,
        Some((5, &mut higher)));
    assert!(result.is_none());
    let settled: Result<(), EvaluationFailure<()>> = scratch.settle(Err(EvaluationFailure::NoValue));
    assert_eq!(settled, Err(EvaluationFailure::ResourceLimit(original)));
    assert_eq!(higher, [Err(EvaluationFailure::NoValue); 3]);
    drop(scratch);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}
