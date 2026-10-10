// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::decode::Scratch;
use crate::geometry::analytic::{ConeSurface, SphereSurface, TorusSurface};
use crate::features::FiniteVector3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};

const EPS_FOURTH_PARTIALS: f64 = 1.0e-12;

fn close(actual: Vector3, expected: Vector3) {
    for (actual, expected) in [actual.x, actual.y, actual.z].into_iter().zip([expected.x, expected.y, expected.z]) {
        assert!((actual - expected).abs() <= EPS_FOURTH_PARTIALS, "{actual} vs {expected}");
    }
}

#[test]
fn actual_analytic_fourth_uses_chart_laws_and_minor_only_torus_mixed_term() {
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let angle = std::f64::consts::FRAC_PI_4;
    let slope = angle.tan();
    let (u, v) = (0.3_f64, 0.4_f64);
    let (su, cu, sv, cv) = (u.sin(), u.cos(), v.sin(), v.cos());
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let cases = [
        (cylinder(), [Vector3::new(2.0 * cu, 2.0 * su, 0.0), zero, zero, zero, zero]),
        (SolvedSurfaceGeometry::Cone(ConeSurface::try_new(origin, axis, reference, 2.0, 2.0, angle).unwrap()), [
            Vector3::new((2.0 + v * slope) * cu, 2.0 * (2.0 + v * slope) * su, 0.0),
            Vector3::new(slope * su, -2.0 * slope * cu, 0.0), zero, zero, zero,
        ]),
        (SolvedSurfaceGeometry::Sphere(SphereSurface::try_new(origin, axis, reference, 3.0).unwrap()), [
            Vector3::new(3.0 * cv * cu, 3.0 * cv * su, 0.0),
            Vector3::new(-3.0 * sv * su, 3.0 * sv * cu, 0.0),
            Vector3::new(3.0 * cv * cu, 3.0 * cv * su, 0.0),
            Vector3::new(-3.0 * sv * su, 3.0 * sv * cu, 0.0),
            Vector3::new(3.0 * cv * cu, 3.0 * cv * su, 3.0 * sv),
        ]),
        (SolvedSurfaceGeometry::Torus(TorusSurface::try_new(origin, axis, reference, 5.0, 2.0).unwrap()), [
            Vector3::new((5.0 + 2.0 * cv) * cu, (5.0 + 2.0 * cv) * su, 0.0),
            Vector3::new(-2.0 * sv * su, 2.0 * sv * cu, 0.0),
            Vector3::new(2.0 * cv * cu, 2.0 * cv * su, 0.0),
            Vector3::new(-2.0 * sv * su, 2.0 * sv * cu, 0.0),
            Vector3::new(2.0 * cv * cu, 2.0 * cv * su, 2.0 * sv),
        ]),
    ];
    for (geometry, expected) in cases {
        let scratch = Scratch::new(EvaluationAdmission::Standard);
        let requested = crate::eval::surface_requested_jet_solved(&scratch, &geometry, u, v, SurfaceRequest::Fourth).unwrap();
        assert!(requested.higher.third().is_ok());
        for (actual, expected) in requested.higher.fourth().unwrap().into_iter().zip(expected) {
            close(actual.get(), expected);
        }
        let third_only = crate::eval::surface_requested_jet_solved(&scratch, &geometry, u, v, SurfaceRequest::Third).unwrap();
        assert_eq!(third_only.higher.fourth(), Err(EvaluationFailure::NoValue));
        assert_eq!(third_only.higher.third(), requested.higher.third());
        assert_eq!(third_only.jet.second, requested.jet.second);
    }
}

#[test]
fn actual_offset_third_follows_cylinder_cone_sphere_and_torus_equations() {
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
        (cylinder(), [Vector3::new(3.0 * su, -3.0 * cu, 0.0), zero, zero, zero]),
        (SolvedSurfaceGeometry::Cone(ConeSurface::try_new(origin, axis, reference, 2.0, 1.0, angle).unwrap()), [
            Vector3::new(cone_radius * su, -cone_radius * cu, 0.0),
            Vector3::new(-slope * cu, -slope * su, 0.0), zero, zero,
        ]),
        (SolvedSurfaceGeometry::Sphere(SphereSurface::try_new(origin, axis, reference, 3.0).unwrap()), [
            Vector3::new(4.0 * cv * su, -4.0 * cv * cu, 0.0),
            Vector3::new(4.0 * sv * cu, 4.0 * sv * su, 0.0),
            Vector3::new(4.0 * cv * su, -4.0 * cv * cu, 0.0),
            Vector3::new(4.0 * sv * cu, 4.0 * sv * su, -4.0 * cv),
        ]),
        (SolvedSurfaceGeometry::Torus(TorusSurface::try_new(origin, axis, reference, 5.0, 2.0).unwrap()), [
            Vector3::new((5.0 + 3.0 * cv) * su, -(5.0 + 3.0 * cv) * cu, 0.0),
            Vector3::new(3.0 * sv * cu, 3.0 * sv * su, 0.0),
            Vector3::new(3.0 * cv * su, -3.0 * cv * cu, 0.0),
            Vector3::new(3.0 * sv * cu, 3.0 * sv * su, -3.0 * cv),
        ]),
    ];
    for (geometry, expected) in cases {
        let mut ir = CadIr::empty();
        let id = stored(&mut ir, "analytic-third", geometry);
        let shifted = offset(&mut ir, "analytic-offset-third", id, 1.0);
        let index = ModelIndex::build(&ir, StandardIndex);
        super::super::with_mapping(EvaluationAdmission::Standard, &index, &shifted, u, v, &mut |mapping| {
            let result = mapping.evaluate(EvaluationAdmission::Standard, &index, SurfaceRequest::Third)?;
            assert!(result.jet.first.is_ok() && result.jet.second.is_ok());
            for (actual, expected) in result.higher.third().unwrap().into_iter().zip(expected) { close(actual.get(), expected); }
            assert_eq!(result.higher.fourth(), Err(EvaluationFailure::NoValue));
            Ok(())
        }).unwrap();
    }
}

#[test]
fn actual_offset_third_and_fourth_fixed_grammar_need_no_decode_or_independent_work() {
    let geometry = cylinder();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let work = WorkBudget::new(0);
    for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
        admission.within_work_slice(&work, |admission| {
            let scratch = Scratch::new(admission);
            let source = crate::eval::surface_requested_jet_solved(&scratch, &geometry, 0.0, 0.0, SurfaceRequest::Fourth)?;
            let result = super::super::offset(source, 1.0, SurfaceRequest::Third)?;
            assert_eq!(result.higher.third().unwrap()[0].get(), Vector3::new(0.0, -3.0, 0.0));
            assert_eq!(result.jet.second.unwrap()[0].get(), Vector3::new(-3.0, 0.0, 0.0));
            Ok::<_, EvaluationFailure<Point3>>(())
        }).unwrap();
    }
    assert_eq!(work.consumed(), 0);
    ctx.finish_session().unwrap();
}

#[test]
fn actual_zero_offset_keeps_all_requested_orders_without_higher_access() {
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let source = crate::eval::surface_requested_jet_solved(&scratch, &cylinder(), 0.3, 0.4, SurfaceRequest::Fourth).unwrap();
    let expected = (source.jet.point, source.jet.first, source.jet.second, source.higher.third(), source.higher.fourth());
    let actual = super::super::offset(source, -0.0, SurfaceRequest::Fourth).unwrap();
    assert_eq!((actual.jet.point, actual.jet.first, actual.jet.second, actual.higher.third(), actual.higher.fourth()), expected);
}

#[test]
fn actual_higher_placement_keeps_available_order_when_another_overflows() {
    let fourth = HigherPartials::Fourth {
        third: Ok([FiniteVector3::ZERO; 4]),
        fourth: Ok([FiniteVector3::new(Vector3::new(f64::MAX, 0.0, 0.0)).unwrap(); 5]),
    };
    let placed = fourth.placed(Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap());
    assert_eq!(placed.third().unwrap(), [FiniteVector3::ZERO; 4]);
    assert_eq!(placed.fourth(), Err(EvaluationFailure::NonFinite(())));
}

#[test]
fn actual_missing_fifth_keeps_nested_offset_point_first_and_second() {
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "fifth-base", cylinder());
    let inner = offset(&mut ir, "fifth-inner", base, 1.0);
    let placed = procedural(&mut ir, "fifth-placement", ProceduralSurfaceDefinition::Replica {
        source: inner, transform: Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap(),
    });
    let outer = offset(&mut ir, "fifth-outer", placed, 1.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    super::super::with_mapping(EvaluationAdmission::Standard, &index, &outer, 0.0, 0.0, &mut |mapping| {
        let result = mapping.evaluate(EvaluationAdmission::Standard, &index, SurfaceRequest::Third)?;
        assert_eq!(result.jet.point.get(), Point3::new(7.0, 0.0, 0.0));
        assert_eq!(result.jet.first.unwrap()[0].get(), Vector3::new(0.0, 5.0, 0.0));
        assert_eq!(result.jet.second.unwrap()[0].get(), Vector3::new(-10.0, 0.0, 0.0));
        // The inner analytic offset now supplies its genuine fourth order.
        // For the ellipse a=6,b=3, n_y=2u-(10/3)u^3+O(u^5).
        let third = result.higher.third().unwrap();
        assert_eq!(third[0].get(), Vector3::new(0.0, -23.0, 0.0));
        assert_eq!(&third[1..], &[FiniteVector3::ZERO; 3]);
        Ok(())
    }).unwrap();
}

#[test]
fn actual_stored_placement_maps_third_and_fourth_without_repeating_source() {
    let transform = Transform::affine([[2.0, 0.0, 0.0, 1.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0]]).unwrap();
    let geometry = SolvedSurfaceGeometry::Transformed(crate::geometry::PlacedSurface::try_new(Box::new(cylinder()), transform).unwrap());
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let result = crate::eval::surface_requested_jet_solved(&scratch, &geometry, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
    assert_eq!(result.jet.point.get(), Point3::new(5.0, 0.0, 0.0));
    assert_eq!(result.higher.third().unwrap()[0].get(), Vector3::new(0.0, -6.0, 0.0));
    assert_eq!(result.higher.fourth().unwrap()[0].get(), Vector3::new(4.0, 0.0, 0.0));
    let budget = WorkBudget::new(1);
    EvaluationAdmission::Standard.within_work_slice(&budget, |admission| {
        let scratch = Scratch::new(admission);
        crate::eval::surface_requested_jet_solved(&scratch, &geometry, 0.0, 0.0, SurfaceRequest::Fourth)
    }).unwrap();
    assert_eq!(budget.consumed(), 1);
}

#[test]
fn actual_quartic_offset_third_follows_polynomial_normal_law() {
    let surface = crate::eval::surface_nurbs::tests::quartic();
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "quartic", SolvedSurfaceGeometry::Nurbs(surface));
    let shifted = offset(&mut ir, "quartic-offset", base, 1.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    super::super::with_mapping(EvaluationAdmission::Standard, &index, &shifted, 0.0, 0.0, &mut |mapping| {
        let result = mapping.evaluate(EvaluationAdmission::Standard, &index, SurfaceRequest::Third)?;
        assert_eq!(result.jet.point.get(), Point3::new(0.0, 0.0, 1.0));
        assert_eq!(result.jet.second.unwrap(), [FiniteVector3::ZERO; 3]);
        for (actual, (x, y)) in result.higher.third().unwrap().into_iter().zip([
            (-24.0, -12.0), (-12.0, -12.0), (-12.0, -24.0), (-24.0, -120.0),
        ]) { close(actual.get(), Vector3::new(x, y, 0.0)); }
        Ok(())
    }).unwrap();
}
