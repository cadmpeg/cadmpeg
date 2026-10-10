// SPDX-License-Identifier: Apache-2.0
use crate::eval::admission::EvaluationAdmission;
use crate::eval::{decode::Scratch, pcurve_uv_differential, pcurve_uv_unsettled,
    ContactRequest, EvaluationFailure, PcurveAcceleration, PcurveEvaluation, SurfaceRequest};
use crate::geometry::pcurve::{PcurveGeometry, SphericalGreatCirclePcurve};
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const EPS_SPHERICAL_HIGHER: f64 = 512.0 * f64::EPSILON;

fn spherical(origin: f64, rate: f64, phase: f64, slope: f64) -> PcurveGeometry {
    PcurveGeometry::SphericalGreatCircle(
        SphericalGreatCirclePcurve::try_new(origin, rate, phase, slope).unwrap())
}

fn same_lower(actual: &PcurveEvaluation, old: &PcurveEvaluation) {
    let bits = |p: Point2| [p.u.to_bits(), p.v.to_bits()];
    assert_eq!(actual.point.map(|p| bits(p.get())).map_err(bits),
        old.point.map(|p| bits(p.get())).map_err(bits));
    assert_eq!(actual.tangent.map(|p| bits(p.get())).map_err(|failure| failure.map(bits)),
        old.tangent.map(|p| bits(p.get())).map_err(|failure| failure.map(bits)));
    match (actual.acceleration, old.acceleration) {
        (PcurveAcceleration::Finite(a), PcurveAcceleration::Finite(b)) => assert_eq!(bits(a.get()), bits(b.get())),
        (PcurveAcceleration::NonFinite, PcurveAcceleration::NonFinite)
            | (PcurveAcceleration::Unstated, PcurveAcceleration::Unstated) => {},
        _ => panic!("actual spherical lower acceleration state changed"),
    }
    assert_eq!(actual.resource, old.resource);
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() <= EPS_SPHERICAL_HIGHER * expected.abs(),
        "{actual} versus {expected}");
}

#[test]
fn requested_spherical_orders_keep_source_phase_laws_lower_bits_and_zero_resource_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        for slope in [-2.0, -1.0, 0.0, 1.0, 2.0] {
            for rate in [-2.0_f64, -1.0, 1.0, 2.0] {
                let curve = spherical(0.0, rate, 0.0, slope);
                let scratch = Scratch::new(admission);
                let old = pcurve_uv_differential(&scratch, &curve, FiniteReal::ZERO).unwrap();
                // theta=atan(s*cos(r*t)) is even at0. Differentiating its
                // source quotient gives theta4=s*r^4*(1-5s²)/(1+s²)².
                let expected = slope * rate.powi(4) * (1.0 - 5.0 * slope * slope)
                    / (1.0 + slope * slope).powi(2);
                for order in [2, 3, 4, 5] {
                    let mut higher = [Err(EvaluationFailure::NoValue); 3];
                    let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::ZERO,
                        Some((order, &mut higher))).unwrap();
                    same_lower(&actual, &old);
                    for (at, expected) in [0.0, expected, 0.0].into_iter().enumerate() {
                        if at + 3 > order { assert_eq!(higher[at], Err(EvaluationFailure::NoValue)); }
                        else {
                            let actual = higher[at].unwrap().get();
                            assert_eq!(actual.u, 0.0);
                            near(actual.v, expected);
                        }
                    }
                }
            }
        }
    }
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn requested_spherical_shifted_phase_and_odd_orientation_follow_independent_taylor_division() {
    for slope in [-2.0, 0.5, 1.0] {
        for rate in [-2.0_f64, 1.0, 2.0] {
            let phase = 0.5_f64;
            // Taylor coefficients of y=s*cos(phase+r*t), followed by the
            // independent recurrence (1+y²)*theta'=y'. No closed derivative
            // coefficient table or evaluated result supplies the oracle.
            let mut y = [0.0; 6];
            let mut factorial = 1.0;
            for (n, coefficient) in y.iter_mut().enumerate() {
                if n > 0 { factorial *= f64::from(u32::try_from(n).unwrap()); }
                let trigonometric = match n % 4 {
                    0 => phase.cos(), 1 => -phase.sin(), 2 => -phase.cos(), _ => phase.sin(),
                };
                *coefficient = slope * trigonometric * rate.powi(i32::try_from(n).unwrap()) / factorial;
            }
            let mut denominator = [0.0; 5];
            denominator[0] = 1.0;
            for (n, coefficient) in denominator.iter_mut().enumerate() {
                for k in 0..=n { *coefficient += y[k] * y[n - k]; }
            }
            let mut quotient = [0.0; 5];
            for n in 0..5 {
                let mut numerator = f64::from(u32::try_from(n + 1).unwrap()) * y[n + 1];
                for k in 1..=n { numerator -= denominator[k] * quotient[n - k]; }
                quotient[n] = numerator / denominator[0];
            }
            let curve = spherical(phase, rate, 0.0, slope);
            let scratch = Scratch::new(EvaluationAdmission::Standard);
            let old = pcurve_uv_differential(&scratch, &curve, FiniteReal::ZERO).unwrap();
            let mut higher = [Err(EvaluationFailure::NoValue); 3];
            let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::ZERO,
                Some((5, &mut higher))).unwrap();
            same_lower(&actual, &old);
            for (actual, expected) in higher.into_iter().zip([
                2.0 * quotient[2], 6.0 * quotient[3], 24.0 * quotient[4],
            ]) {
                let actual = actual.unwrap().get();
                assert_eq!(actual.u, 0.0);
                near(actual.v, expected);
            }
        }
    }
}

#[test]
fn requested_spherical_final_phase_ratio_recovers_finite_orders_and_reports_true_overflow() {
    for slope_sign in [-1.0, 1.0] {
        for rate_sign in [-1.0, 1.0] {
            for (slope, fourth) in [
                (2.0_f64.powi(600), -5.0 * 2.0_f64.powi(1000)),
                (2.0_f64.powi(-600), 2.0_f64.powi(1000)),
                (f64::from_bits(1), 2.0_f64.powi(526)),
            ] {
                let curve = spherical(0.0, rate_sign * 2.0_f64.powi(400), 0.0, slope_sign * slope);
                let policy = DecodePolicy::service();
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
                    let scratch = Scratch::new(admission);
                    let old = pcurve_uv_differential(&scratch, &curve, FiniteReal::ZERO).unwrap();
                    let mut higher = [Err(EvaluationFailure::NoValue); 3];
                    let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::ZERO,
                        Some((5, &mut higher))).unwrap();
                    same_lower(&actual, &old);
                    // Exact rational source Taylor expansion gives these
                    // binary64 limits, while forming rate^4 alone overflows.
                    for (actual, expected) in higher.into_iter().zip([0.0, slope_sign * fourth, 0.0]) {
                        let actual = actual.unwrap().get();
                        assert_eq!(actual.u, 0.0);
                        near(actual.v, expected);
                    }
                }
                ctx.finish_session().unwrap();
            }
        }
    }
    let curve = spherical(0.0, 2.0_f64.powi(400), 0.0, 1.0);
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let old = pcurve_uv_differential(&scratch, &curve, FiniteReal::ZERO).unwrap();
    let mut higher = [Err(EvaluationFailure::NoValue); 3];
    let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::ZERO,
        Some((5, &mut higher))).unwrap();
    same_lower(&actual, &old);
    assert_eq!(higher[0].unwrap().get(), Point2::new(0.0, 0.0));
    assert_eq!(higher[1], Err(EvaluationFailure::NonFinite(())));
    assert_eq!(higher[2].unwrap().get(), Point2::new(0.0, 0.0));
}

#[test]
fn requested_spherical_keeps_actual_unreached_phase_and_original_lower_overflow_routes() {
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    for (curve, parameter) in [
        (spherical(f64::MAX, 1.0, 0.0, 1.0), f64::MAX),
        (spherical(f64::MAX, 1.0, -f64::MAX, 1.0), 0.0),
    ] {
        let parameter = FiniteReal::new(parameter).unwrap();
        let old = pcurve_uv_differential(&scratch, &curve, parameter).unwrap();
        let mut higher = [Err(EvaluationFailure::NoValue); 3];
        let actual = pcurve_uv_unsettled(&scratch, &curve, parameter,
            Some((5, &mut higher))).unwrap();
        same_lower(&actual, &old);
        assert!(actual.point.unwrap_err().v.is_nan());
        assert_eq!(higher, [Err(EvaluationFailure::NoValue); 3]);
    }
    let curve = spherical(std::f64::consts::FRAC_PI_2, f64::MAX, 0.0, 2.0);
    let old = pcurve_uv_differential(&scratch, &curve, FiniteReal::ZERO).unwrap();
    let mut higher = [Err(EvaluationFailure::NoValue); 3];
    let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::ZERO,
        Some((5, &mut higher))).unwrap();
    same_lower(&actual, &old);
    assert!(actual.point.is_ok());
    assert_eq!(actual.tangent.unwrap_err(), EvaluationFailure::NonFinite(Point2::new(f64::MAX, f64::NEG_INFINITY)));
    assert_eq!(higher[1], Err(EvaluationFailure::NonFinite(())));
}

#[test]
fn requested_spherical_keeps_real_prior_collection_refusal_before_frame_and_finish() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.alloc_filled(1, 0_u8,
        "actual prior spherical byte collection").unwrap_err() else { panic!("actual collection refusal"); };
    assert_eq!(original.dimension, ResourceDimension::CollectionItems);
    assert_eq!((original.limit, original.used, original.additional), (0, 0, 1));
    let scratch = Scratch::new(&ctx);
    let mut higher = [Err(EvaluationFailure::NoValue); 3];
    assert!(pcurve_uv_unsettled(&scratch, &spherical(0.0, 1.0, 0.0, 1.0), FiniteReal::ZERO,
        Some((5, &mut higher))).is_none());
    let settled: Result<(), EvaluationFailure<()>> = scratch.settle(Err(EvaluationFailure::NoValue));
    assert_eq!(settled, Err(EvaluationFailure::ResourceLimit(original)));
    assert_eq!(higher, [Err(EvaluationFailure::NoValue); 3]);
    drop(scratch);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn requested_spherical_contact_uses_true_plane_and_sphere_requested_composition() {
    use crate::geometry::{ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry};
    use crate::geometry::analytic::SphereSurface;
    use crate::index::{ModelIndex, StandardIndex};
    let (mut ir, _) = super::variable_blend::variable_blend_eval_fixture(
        Point3::new(0.0, 0.0, 0.0), [(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)); 2],
        [0.0; 2], None);
    let ProceduralSurfaceDefinition::VariableBlend(payload) = ir.model.procedural_surfaces[0].definition()
        else { panic!("actual blend fixture"); };
    let mut side = payload.construction().sides[0].clone();
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for sphere in [false, true] {
        if sphere {
            ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                SphereSurface::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0), 2.0).unwrap()));
        }
        let index = ModelIndex::build(&ir, StandardIndex);
        for slope in [-1.0, 1.0] {
            for rate in [-1.0, 1.0] {
                side.pcurve = Some(spherical(0.0, rate, 0.0, slope));
                let a = std::f64::consts::FRAC_1_SQRT_2;
                // Actual radius2 sphere contact is
                // 2*(cos u,sin u,s*cos u)/sqrt(1+s²*cos²u), u=r*t.
                // Source series of (1-sin²u/2)^(-1/2) gives these orders.
                let expected = if sphere {
                    [Vector3::new(-a, 0.0, -slope * a), Vector3::new(0.0, rate * a, 0.0),
                        Vector3::new(-3.5 * a, 0.0, -3.5 * slope * a),
                        Vector3::new(0.0, -5.5 * rate * a, 0.0)]
                } else {
                    [Vector3::new(0.0, -0.5 * slope, 0.0), Vector3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, -slope, 0.0), Vector3::new(0.0, 0.0, 0.0)]
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
                                assert!((actual.higher[at].unwrap().get() - expected).norm() <= EPS_SPHERICAL_HIGHER);
                            } else { assert_eq!(actual.higher[at], Err(EvaluationFailure::NoValue)); }
                        }
                    }
                }
            }
        }
    }
    ctx.finish_session().unwrap();
}
