// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::decode::Scratch;
use crate::features::FiniteVector3;
use crate::geometry::analytic::{ConeSurface, SphereSurface, TorusSurface};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};

const EPS_FIFTH_PARTIALS: f64 = 1.0e-12;

fn close(actual: Vector3, expected: Vector3) {
    for (actual, expected) in [actual.x, actual.y, actual.z].into_iter().zip([expected.x, expected.y, expected.z]) {
        assert!((actual - expected).abs() <= EPS_FIFTH_PARTIALS, "{actual} vs {expected}");
    }
}

#[test]
fn actual_analytic_fifth_uses_complete_mixed_chart_laws() {
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let angle = std::f64::consts::FRAC_PI_4;
    let slope = angle.tan();
    let (u, v) = (0.3_f64, 0.4_f64);
    let (su, cu, sv, cv) = (u.sin(), u.cos(), v.sin(), v.cos());
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let cases = [
        (cylinder(), [Vector3::new(-2.0 * su, 2.0 * cu, 0.0), zero, zero, zero, zero, zero]),
        (SolvedSurfaceGeometry::Cone(ConeSurface::try_new(origin, axis, reference, 2.0, 2.0, angle).unwrap()), [
            Vector3::new(-(2.0 + v * slope) * su, 2.0 * (2.0 + v * slope) * cu, 0.0),
            Vector3::new(slope * cu, 2.0 * slope * su, 0.0), zero, zero, zero, zero,
        ]),
        (SolvedSurfaceGeometry::Sphere(SphereSurface::try_new(origin, axis, reference, 3.0).unwrap()), [
            Vector3::new(-3.0 * cv * su, 3.0 * cv * cu, 0.0),
            Vector3::new(-3.0 * sv * cu, -3.0 * sv * su, 0.0),
            Vector3::new(-3.0 * cv * su, 3.0 * cv * cu, 0.0),
            Vector3::new(-3.0 * sv * cu, -3.0 * sv * su, 0.0),
            Vector3::new(-3.0 * cv * su, 3.0 * cv * cu, 0.0),
            Vector3::new(-3.0 * sv * cu, -3.0 * sv * su, 3.0 * cv),
        ]),
        (SolvedSurfaceGeometry::Torus(TorusSurface::try_new(origin, axis, reference, 5.0, 2.0).unwrap()), [
            Vector3::new(-(5.0 + 2.0 * cv) * su, (5.0 + 2.0 * cv) * cu, 0.0),
            Vector3::new(-2.0 * sv * cu, -2.0 * sv * su, 0.0),
            Vector3::new(-2.0 * cv * su, 2.0 * cv * cu, 0.0),
            Vector3::new(-2.0 * sv * cu, -2.0 * sv * su, 0.0),
            Vector3::new(-2.0 * cv * su, 2.0 * cv * cu, 0.0),
            Vector3::new(-2.0 * sv * cu, -2.0 * sv * su, 2.0 * cv),
        ]),
    ];
    for (geometry, expected) in cases {
        let scratch = Scratch::new(EvaluationAdmission::Standard);
        let result = crate::eval::surface_requested_jet_solved(&scratch, &geometry, u, v, SurfaceRequest::Fifth).unwrap();
        for (actual, expected) in result.higher.fifth().unwrap().into_iter().zip(expected) { close(actual.get(), expected); }
        let lower = crate::eval::surface_requested_jet_solved(&scratch, &geometry, u, v, SurfaceRequest::Fourth).unwrap();
        assert_eq!(lower.higher.fifth(), Err(EvaluationFailure::NoValue));
        assert_eq!(lower.higher.third(), result.higher.third());
        assert_eq!(lower.higher.fourth(), result.higher.fourth());
        assert_eq!(lower.jet.first, result.jet.first);
        assert_eq!(lower.jet.second, result.jet.second);
    }
}

#[test]
fn actual_offset_fourth_follows_all_analytic_mixed_chart_laws() {
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let angle = std::f64::consts::FRAC_PI_4;
    let slope = angle.tan();
    let (u, v) = (0.3_f64, 0.4_f64);
    let (su, cu, sv, cv) = (u.sin(), u.cos(), v.sin(), v.cos());
    let cone_radius = 2.0 + v * slope + 1.0 / slope.hypot(1.0);
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let cases = [
        (cylinder(), [Vector3::new(3.0 * cu, 3.0 * su, 0.0), zero, zero, zero, zero]),
        (SolvedSurfaceGeometry::Cone(ConeSurface::try_new(origin, axis, reference, 2.0, 1.0, angle).unwrap()), [
            Vector3::new(cone_radius * cu, cone_radius * su, 0.0),
            Vector3::new(slope * su, -slope * cu, 0.0), zero, zero, zero,
        ]),
        (SolvedSurfaceGeometry::Sphere(SphereSurface::try_new(origin, axis, reference, 3.0).unwrap()), [
            Vector3::new(4.0 * cv * cu, 4.0 * cv * su, 0.0),
            Vector3::new(-4.0 * sv * su, 4.0 * sv * cu, 0.0),
            Vector3::new(4.0 * cv * cu, 4.0 * cv * su, 0.0),
            Vector3::new(-4.0 * sv * su, 4.0 * sv * cu, 0.0),
            Vector3::new(4.0 * cv * cu, 4.0 * cv * su, 4.0 * sv),
        ]),
        (SolvedSurfaceGeometry::Torus(TorusSurface::try_new(origin, axis, reference, 5.0, 2.0).unwrap()), [
            Vector3::new((5.0 + 3.0 * cv) * cu, (5.0 + 3.0 * cv) * su, 0.0),
            Vector3::new(-3.0 * sv * su, 3.0 * sv * cu, 0.0),
            Vector3::new(3.0 * cv * cu, 3.0 * cv * su, 0.0),
            Vector3::new(-3.0 * sv * su, 3.0 * sv * cu, 0.0),
            Vector3::new(3.0 * cv * cu, 3.0 * cv * su, 3.0 * sv),
        ]),
    ];
    for (geometry, expected) in cases {
        let mut ir = CadIr::empty();
        let id = stored(&mut ir, "analytic-fourth-base", geometry);
        let shifted = offset(&mut ir, "analytic-fourth-offset", id, 1.0);
        let index = ModelIndex::build(&ir, StandardIndex);
        super::super::with_mapping(EvaluationAdmission::Standard, &index, &shifted, u, v, &mut |mapping| {
            let result = mapping.evaluate(EvaluationAdmission::Standard, &index, SurfaceRequest::Fourth)?;
            for (actual, expected) in result.higher.fourth().unwrap().into_iter().zip(expected) { close(actual.get(), expected); }
            let lower = mapping.evaluate(EvaluationAdmission::Standard, &index, SurfaceRequest::Third)?;
            assert_eq!(lower.higher.third(), result.higher.third());
            assert_eq!(lower.jet.first, result.jet.first);
            assert_eq!(lower.jet.second, result.jet.second);
            assert_eq!(lower.higher.fourth(), Err(EvaluationFailure::NoValue));
            assert_eq!(result.higher.fifth(), Err(EvaluationFailure::NoValue));
            Ok(())
        }).unwrap();
    }
}

#[test]
fn actual_fifth_and_offset_fourth_fixed_grammar_are_free_in_both_modes() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let work = WorkBudget::new(0);
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        admission.within_work_slice(&work, |admission| {
            let scratch = Scratch::new(admission);
            let source = crate::eval::surface_requested_jet_solved(&scratch, &cylinder(), 0.0, 0.0, SurfaceRequest::Fifth)?;
            assert_eq!(source.higher.fifth().unwrap()[0].get(), Vector3::new(0.0, 2.0, 0.0));
            let result = super::super::offset(source, 1.0, SurfaceRequest::Fourth)?;
            assert_eq!(result.higher.fourth().unwrap()[0].get(), Vector3::new(3.0, 0.0, 0.0));
            assert_eq!(result.higher.third().unwrap()[0].get(), Vector3::new(0.0, -3.0, 0.0));
            Ok::<_, EvaluationFailure<Point3>>(())
        }).unwrap();
    }
    assert_eq!(work.consumed(), 0);
    ctx.finish_session().unwrap();
}

#[test]
fn actual_fifth_placement_and_zero_offset_preserve_separate_results() {
    let higher = HigherPartials::Fifth {
        third: Ok([FiniteVector3::ZERO; 4]), fourth: Ok([FiniteVector3::ZERO; 5]),
        fifth: Ok([FiniteVector3::new(Vector3::new(f64::MAX, 0.0, 0.0)).unwrap(); 6]),
    };
    let placed = higher.placed(Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap());
    assert_eq!(placed.third().unwrap(), [FiniteVector3::ZERO; 4]);
    assert_eq!(placed.fourth().unwrap(), [FiniteVector3::ZERO; 5]);
    assert_eq!(placed.fifth(), Err(EvaluationFailure::NonFinite(())));
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let mut source = crate::eval::surface_requested_jet_solved(&scratch, &cylinder(), 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
    source.higher = placed;
    let expected = (source.jet.point, source.jet.first, source.jet.second, source.higher.third(), source.higher.fourth(), source.higher.fifth());
    let result = super::super::offset(source, -0.0, SurfaceRequest::Fifth).unwrap();
    assert_eq!((result.jet.point, result.jet.first, result.jet.second, result.higher.third(), result.higher.fourth(), result.higher.fifth()), expected);
}

#[test]
fn actual_polynomial_fifth_zero_supplies_nonzero_fourth_normal() {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    // S=(u,v,u^2), so n=(-2u,0,1)/sqrt(1+4u^2).
    // n_z=1-2u^2+6u^4+O(u^6), with n_uuuu=(0,0,144).
    let poles = vec![
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        vec![Point3::new(0.5, 0.0, 0.0), Point3::new(0.5, 1.0, 0.0)],
        vec![Point3::new(1.0, 0.0, 1.0), Point3::new(1.0, 1.0, 1.0)],
    ];
    for rational in [false, true] {
        let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(poles.clone(), rational.then(|| vec![vec![1.0, 1.0]; 3])), false,
        ).unwrap().unwrap();
        let geometry = SolvedSurfaceGeometry::Nurbs(surface);
        let policy = DecodePolicy::service();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let scratch = Scratch::new(admission);
            let source = crate::eval::surface_requested_jet_solved(&scratch, &geometry, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
            assert_eq!(source.higher.third().unwrap(), [FiniteVector3::ZERO; 4]);
            assert_eq!(source.higher.fourth().unwrap(), [FiniteVector3::ZERO; 5]);
            if rational { assert_eq!(source.higher.fifth(), Err(EvaluationFailure::NoValue)); }
            else { assert_eq!(source.higher.fifth().unwrap(), [FiniteVector3::ZERO; 6]); }
            let result = super::super::offset(source, 1.0, SurfaceRequest::Fourth).unwrap();
            assert_eq!(result.jet.point.get(), Point3::new(0.0, 0.0, 1.0));
            assert_eq!(result.jet.first.unwrap()[0].get(), Vector3::new(-1.0, 0.0, 0.0));
            assert_eq!(result.jet.second.unwrap()[0].get(), Vector3::new(0.0, 0.0, -2.0));
            assert_eq!(result.higher.third().unwrap()[0].get(), Vector3::new(24.0, 0.0, 0.0));
            if rational { assert_eq!(result.higher.fourth(), Err(EvaluationFailure::NoValue)); }
            else {
                let fourth = result.higher.fourth().unwrap();
                assert_eq!(fourth[0].get(), Vector3::new(0.0, 0.0, 144.0));
                assert_eq!(&fourth[1..], &[FiniteVector3::ZERO; 4]);
            }
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn actual_mixed_quadratic_offset_fourth_uses_all_product_rule_lanes() {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    // S=(u,v,u^2+uv+v^2): |grad z|^2=5u^2+8uv+5v^2.
    // n_z fourth comes from (3/8)*(5u^2+8uv+5v^2)^2.
    let t = [0.0, 0.5, 1.0];
    let squared = [0.0, 0.0, 1.0];
    let poles = (0..3).map(|u| (0..3).map(|v| Point3::new(
        t[u], t[v], squared[u] + t[u] * t[v] + squared[v],
    )).collect()).collect();
    let axis = || NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false);
    let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), axis(), axis(),
        NurbsSurfaceLanes::new(poles, None), false).unwrap().unwrap();
    let geometry = SolvedSurfaceGeometry::Nurbs(surface);
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let source = crate::eval::surface_requested_jet_solved(&scratch, &geometry, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
    assert_eq!(source.higher.fifth().unwrap(), [FiniteVector3::ZERO; 6]);
    let result = super::super::offset(source, 1.0, SurfaceRequest::Fourth).unwrap();
    for (actual, z) in result.higher.fourth().unwrap().into_iter().zip([225.0, 180.0, 171.0, 180.0, 225.0]) {
        assert_eq!(actual.get(), Vector3::new(0.0, 0.0, z));
    }
}
