// SPDX-License-Identifier: Apache-2.0
use crate::eval::admission::EvaluationAdmission;
use crate::eval::{decode::Scratch, pcurve_uv_differential, pcurve_uv_unsettled,
    ContactRequest, EvaluationFailure, PcurveAcceleration, PcurveEvaluation, SurfaceRequest};
use crate::geometry::pcurve::{PcurveGeometry, PolarNurbsPole, PolarPcurveNurbs};
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const EPS_POLAR_NURBS_HIGHER: f64 = 512.0 * f64::EPSILON;

fn source(degree: u32, knots: Vec<f64>, poles: Vec<PolarNurbsPole>, weights: Option<Vec<f64>>)
    -> PcurveGeometry
{
    // Real contextual construction precedes the evaluation session. These
    // fixtures do not certify an unavailable Standard constructor.
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let nurbs = PolarPcurveNurbs::from_lanes(&ctx, degree, knots, poles, weights, false).unwrap().unwrap();
    ctx.finish_session().unwrap();
    PcurveGeometry::PolarNurbs { nurbs }
}

fn clamped(degree: u32, poles: Vec<PolarNurbsPole>, weights: Option<Vec<f64>>) -> PcurveGeometry {
    let count = usize::try_from(degree + 1).unwrap();
    source(degree, std::iter::repeat_n(0.0, count).chain(std::iter::repeat_n(1.0, count)).collect(),
        poles, weights)
}

fn large_line(rational: bool) -> PcurveGeometry {
    let count = 1024_u32;
    let end = f64::from(count - 2);
    let knots: Vec<_> = [0.0; 3].into_iter().chain((1..count - 2).map(f64::from))
        .chain([end; 3]).collect();
    let poles = (0..count).map(|i| {
        let i = usize::try_from(i).unwrap();
        let t = knots[i + 1].midpoint(knots[i + 2]);
        PolarNurbsPole { radial: Point2::new(1.0, t), axial: t }
    }).collect();
    source(2, knots, poles, rational.then(|| (0..count).map(|_| 1.0).collect()))
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
        _ => panic!("actual lower acceleration state changed"),
    }
    assert_eq!(actual.resource, old.resource);
}

fn near(actual: Point2, expected: Point2) {
    assert!((actual.u - expected.u).abs() <= EPS_POLAR_NURBS_HIGHER * (1.0 + expected.u.abs()),
        "{actual:?} versus {expected:?}");
    assert!((actual.v - expected.v).abs() <= EPS_POLAR_NURBS_HIGHER * (1.0 + expected.v.abs()),
        "{actual:?} versus {expected:?}");
}

#[test]
fn requested_polar_nurbs_large_source_keeps_original_combined_caps_and_true_chart_orders() {
    for rational in [false, true] {
        let curve = large_line(rational);
        for parameter in [0.0, 0.5] {
            let t = FiniteReal::new(parameter).unwrap();
            let standard = Scratch::new(EvaluationAdmission::Standard);
            let old = pcurve_uv_differential(&standard, &curve, t).unwrap();
            let d = 1.0 + parameter * parameter;
            // Differentiation of theta=atan(t), with the real axial v=t.
            let expected = [-2.0 * (1.0 - 3.0 * parameter * parameter) / d.powi(3),
                24.0 * parameter * (1.0 - parameter * parameter) / d.powi(4),
                24.0 * (1.0 - 10.0 * parameter * parameter + 5.0 * parameter.powi(4)) / d.powi(5)];
            for order in [2, 3, 4, 5] {
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = 22;
                policy.limits.max_materialized_bytes = 512;
                policy.limits.max_work_units = 256;
                policy.limits.max_retained_bytes = 0;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
                    let scratch = Scratch::new(admission);
                    let mut higher = [Err(EvaluationFailure::NoValue); 3];
                    let actual = pcurve_uv_unsettled(&scratch, &curve, t, Some((order, &mut higher))).unwrap();
                    same_lower(&actual, &old);
                    for (at, expected) in expected.into_iter().enumerate() {
                        if at + 3 > order { assert_eq!(higher[at], Err(EvaluationFailure::NoValue)); }
                        else { near(higher[at].unwrap().get(), Point2::new(expected, 0.0)); }
                    }
                }
                drop(ctx.reserve_scoped_limit(512, "actual requested polar backing destroyed").unwrap());
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn requested_polar_nurbs_rational_linear_and_cubic_use_actual_source_quotients() {
    for sign in [-1.0, 1.0] {
        let linear = clamped(1, vec![
            PolarNurbsPole { radial: Point2::new(1.0, 0.0), axial: 0.0 },
            PolarNurbsPole { radial: Point2::new(1.0, 0.5), axial: 0.5 },
        ], Some(vec![sign, 2.0 * sign]));
        let cubic = clamped(3, vec![
            PolarNurbsPole { radial: Point2::new(1.0, 0.0), axial: 0.0 },
            PolarNurbsPole { radial: Point2::new(1.0, 0.0), axial: 0.0 },
            PolarNurbsPole { radial: Point2::new(1.0, 0.0), axial: 0.0 },
            PolarNurbsPole { radial: Point2::new(1.0, 0.5), axial: 1.5 },
        ], Some(vec![sign, sign, sign, 2.0 * sign]));
        // For linear: q=t/(1+t), theta'=1/(1+2t+2t²).
        // For cubic: q=t³/(1+t³), theta=q+O(t^9), axial=3q.
        for (curve, expected) in [
            (linear, [Point2::new(4.0, 6.0), Point2::new(0.0, -24.0), Point2::new(-96.0, 120.0)]),
            (cubic, [Point2::new(6.0, 18.0), Point2::new(0.0, 0.0), Point2::new(0.0, 0.0)]),
        ] {
            let policy = DecodePolicy::service();
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
                let scratch = Scratch::new(admission);
                let old = pcurve_uv_differential(&scratch, &curve, FiniteReal::ZERO).unwrap();
                for order in [3, 4, 5] {
                    let mut higher = [Err(EvaluationFailure::NoValue); 3];
                    let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::ZERO,
                        Some((order, &mut higher))).unwrap();
                    same_lower(&actual, &old);
                    for (at, expected) in expected.into_iter().enumerate() {
                        if at + 3 > order { assert_eq!(higher[at], Err(EvaluationFailure::NoValue)); }
                        else { near(higher[at].unwrap().get(), expected); }
                    }
                }
            }
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn requested_polar_nurbs_quartic_uses_real_inline_capture_and_geometric_scaling() {
    for rational in [false, true] {
        for scale in [2.0_f64.powi(-600), 1.0, 2.0_f64.powi(600)] {
            let poles = (0..=4).map(|i| {
                let q = 2.0_f64.powi(i) - 1.0;
                PolarNurbsPole { radial: Point2::new(scale, scale * q), axial: q }
            }).collect();
            let weights = rational.then(|| (0..=4).map(|i| 1.0 + f64::from(i) / 4.0).collect());
            let curve = clamped(4, poles, weights);
            // Polynomial q=4t+6t²+4t³+t⁴; rational q=5t+7t²+3t³.
            // Exact Taylor products of atan(q)=q-q³/3+q⁵/5 give:
            let (angular, axial) = if rational { ([-232.0, -4200.0, 36600.0], [18.0, 0.0, 0.0]) }
                else { ([-104.0, -2280.0, -384.0], [24.0, 24.0, 0.0]) };
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
                for (at, value) in higher.into_iter().enumerate() {
                    near(value.unwrap().get(), Point2::new(angular[at], axial[at]));
                }
            }
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn requested_polar_nurbs_keeps_actual_radial_axial_and_higher_resource_error_order() {
    let curve = large_line(true);
    for cap in [2, 5, 10, 11] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        policy.limits.max_materialized_bytes = 512;
        policy.limits.max_work_units = 256;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let mut higher = [Err(EvaluationFailure::NoValue); 3];
        let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::HALF,
            Some((5, &mut higher))).unwrap();
        let original = actual.resource.unwrap();
        assert_eq!(original.dimension, ResourceDimension::CollectionItems);
        assert_eq!(original.operation, match cap {
            2 | 11 => "IR B-spline basis", 5 => "IR B-spline derivative basis",
            10 => "IR B-spline second derivative basis", _ => unreachable!(),
        });
        if cap == 11 { assert_eq!((original.limit, original.used, original.additional), (11, 11, 3)); }
        assert_eq!(higher, [Err(EvaluationFailure::NoValue); 3]);
        let settled: Result<(), EvaluationFailure<()>> = scratch.settle(Err(EvaluationFailure::NoValue));
        assert_eq!(settled, Err(EvaluationFailure::ResourceLimit(original)));
        drop(scratch);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
    }
    let curve = clamped(4, (0..=4).map(|i| PolarNurbsPole {
        radial: Point2::new(1.0, 2.0_f64.powi(i) - 1.0), axial: 0.0,
    }).collect(), None);
    let mut policy = DecodePolicy::service();
    // Each old lower degree4 lane collects5+4+5+3+4+5=26 items.
    // Both lower operations fit52. The first actual Third row then needs
    // one collected item, before any Fourth/Fifth row or axial higher.
    policy.limits.max_collection_items = 52;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = Scratch::new(&ctx);
    let mut higher = [Err(EvaluationFailure::NoValue); 3];
    let actual = pcurve_uv_unsettled(&scratch, &curve, FiniteReal::ZERO,
        Some((5, &mut higher))).unwrap();
    let original = actual.resource.unwrap();
    assert_eq!(original.dimension, ResourceDimension::CollectionItems);
    assert_eq!(original.operation, "IR scaled B-spline derivative basis");
    assert_eq!((original.limit, original.used, original.additional), (52, 52, 1));
    assert_eq!(higher, [Err(EvaluationFailure::NoValue); 3]);
    drop(scratch);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn requested_polar_nurbs_keeps_origin_overflow_and_missing_heap_capture_lower_outcomes() {
    let origin = clamped(2, vec![PolarNurbsPole { radial: Point2::new(0.0, 0.0), axial: 1.0 }; 3], None);
    let overflow = clamped(1, vec![
        PolarNurbsPole { radial: Point2::new(1.0, 0.0), axial: 0.0 },
        PolarNurbsPole { radial: Point2::new(1.0, 0.0), axial: f64::MAX },
    ], None);
    let missing = clamped(5, (0..=5).map(|i| {
        let t = f64::from(i) / 5.0;
        PolarNurbsPole { radial: Point2::new(1.0, t), axial: t }
    }).collect(), None);
    for curve in [&origin, &overflow, &missing] {
        let parameter = if std::ptr::eq(curve, &overflow) { FiniteReal::new(2.0).unwrap() }
            else { FiniteReal::ZERO };
        let policy = DecodePolicy::service();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let scratch = Scratch::new(admission);
            let old = pcurve_uv_differential(&scratch, curve, parameter);
            let mut higher = [Err(EvaluationFailure::NoValue); 3];
            let actual = pcurve_uv_unsettled(&scratch, curve, parameter, Some((5, &mut higher)));
            if std::ptr::eq(curve, &origin) { assert!(old.is_none() && actual.is_none()); }
            else {
                let (old, actual) = (old.unwrap(), actual.unwrap());
                same_lower(&actual, &old);
                if std::ptr::eq(curve, &overflow) {
                    assert_eq!(actual.point.unwrap_err(), Point2::new(0.0, f64::INFINITY));
                } else {
                    assert_eq!(actual.point.unwrap().get(), Point2::new(0.0, 0.0));
                    near(actual.tangent.unwrap().get(), Point2::new(1.0, 1.0));
                }
            }
            assert_eq!(higher, [Err(EvaluationFailure::NoValue); 3]);
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn requested_polar_nurbs_contact_uses_real_plane_and_cylinder_composition() {
    use crate::geometry::{ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry};
    use crate::geometry::analytic::CylinderSurface;
    use crate::index::{ModelIndex, StandardIndex};
    let (mut ir, _) = super::variable_blend::variable_blend_eval_fixture(
        Point3::new(0.0, 0.0, 0.0), [(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)); 2],
        [0.0; 2], None);
    let ProceduralSurfaceDefinition::VariableBlend(payload) = ir.model.procedural_surfaces[0].definition()
        else { panic!("actual blend fixture"); };
    let mut side = payload.construction().sides[0].clone();
    side.pcurve = Some(clamped(2, [0.0, 0.5, 1.0].map(|t| PolarNurbsPole {
        radial: Point2::new(1.0, t), axial: 0.0,
    }).to_vec(), None));
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
        // theta=atan(t). The actual radius2 contact is
        // (2/sqrt(1+t²),2t/sqrt(1+t²),0).
        let expected = if cylinder {
            [Vector3::new(-2.0, 0.0, 0.0), Vector3::new(0.0, -6.0, 0.0),
                Vector3::new(18.0, 0.0, 0.0), Vector3::new(0.0, 90.0, 0.0)]
        } else {
            [Vector3::new(0.0, 0.0, 0.0), Vector3::new(-2.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 0.0), Vector3::new(24.0, 0.0, 0.0)]
        };
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let old = crate::eval::variable_blend_contact_track(admission, &index, &side, 0.0,
                ContactRequest::Tangent).unwrap();
            let actual = crate::eval::variable_blend_contact_track(admission, &index, &side, 0.0,
                ContactRequest::Higher(SurfaceRequest::Fifth)).unwrap();
            assert_eq!(actual.point(), old.point());
            assert_eq!(actual.tangent(), old.tangent());
            for (actual, expected) in actual.higher.into_iter().zip(expected) {
                assert!((actual.unwrap().get() - expected).norm() <= EPS_POLAR_NURBS_HIGHER);
            }
        }
    }
    ctx.finish_session().unwrap();
}
