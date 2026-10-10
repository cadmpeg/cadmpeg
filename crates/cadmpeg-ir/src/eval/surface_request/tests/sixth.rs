// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::decode::Scratch;
use crate::eval::surface_request::{model_requested_jet, RequestedJet};
use crate::features::FiniteVector3;
use crate::geometry::analytic::{CircleCurve, ConeSurface, SphereSurface, TorusSurface};
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::surface_payloads::{LinearSweepSurfaceConstruction, SubsetSurfaceConstruction};
use crate::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use crate::ids::CurveId;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget};
use cadmpeg_core::CodecError;

const EPS_SIXTH_NORMAL: f64 = 4096.0 * f64::EPSILON;

fn near<const N: usize>(actual: [FiniteVector3; N], expected: [Vector3; N]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        let actual = actual.get();
        for (actual, expected) in [actual.x, actual.y, actual.z].into_iter().zip([expected.x, expected.y, expected.z]) {
            assert!((actual - expected).abs() <= EPS_SIXTH_NORMAL * expected.abs().max(1.0), "{actual} vs {expected}");
        }
    }
}

fn same_order<const N: usize>(a: Result<[FiniteVector3; N], EvaluationFailure<()>>,
    b: Result<[FiniteVector3; N], EvaluationFailure<()>>)
{
    let bits = |lanes: [FiniteVector3; N]| lanes.map(|lane| {
        let lane = lane.get(); [lane.x.to_bits(), lane.y.to_bits(), lane.z.to_bits()]
    });
    assert_eq!(a.map(bits), b.map(bits));
}

fn same_prefix(a: &RequestedJet, b: &RequestedJet) {
    let bits = |point: Point3| [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()];
    assert_eq!(bits(a.jet.point.get()), bits(b.jet.point.get()));
    same_order(a.jet.first, b.jet.first); same_order(a.jet.second, b.jet.second);
    same_order(a.higher.third(), b.higher.third());
    same_order(a.higher.fourth(), b.higher.fourth());
}

fn graph(degree: u32, mixed: bool, rational: bool) -> SolvedSurfaceGeometry {
    // Actual contextual fixture construction is separate from evaluation.
    // It does not certify Standard constructor admission.
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let v_degree = if mixed { 2 } else { 1 };
    let poles = (0..=degree).map(|i| (0..=v_degree).map(|j| {
        let u = f64::from(i) / f64::from(degree);
        let v = f64::from(j) / f64::from(v_degree);
        let z_u = if i == degree { 1.0 } else { 0.0 };
        let z_v = if j == v_degree { 1.0 } else { 0.0 };
        let z = if mixed { z_u + u * v + z_v } else { z_u };
        Point3::new(u, v, z)
    }).collect()).collect();
    let axis = |degree: u32| {
        let count = usize::try_from(degree + 1).unwrap();
        NurbsSurfaceAxis::new(degree, std::iter::repeat_n(0.0, count)
            .chain(std::iter::repeat_n(1.0, count)).collect::<Vec<_>>(), false)
    };
    let weights = rational.then(|| (0..=degree).map(|_| (0..=v_degree).map(|_| 1.0).collect()).collect());
    let surface = NurbsSurface::from_lanes(&ctx, axis(degree), axis(v_degree),
        NurbsSurfaceLanes::new(poles, weights), false).unwrap().unwrap();
    ctx.finish_session().unwrap(); SolvedSurfaceGeometry::Nurbs(surface)
}

#[test]
fn actual_analytic_sixth_uses_all_mixed_chart_laws_and_keeps_fifth_prefix_bits() {
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0); let reference = Vector3::new(1.0, 0.0, 0.0);
    let (u, v) = (0.3_f64, 0.4_f64);
    let (su, cu, sv, cv) = (u.sin(), u.cos(), v.sin(), v.cos());
    let slope = std::f64::consts::FRAC_PI_4.tan();
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let sphere_uu = Vector3::new(-3.0 * cv * cu, -3.0 * cv * su, 0.0);
    let sphere_uv = Vector3::new(3.0 * sv * su, -3.0 * sv * cu, 0.0);
    let torus_uv = Vector3::new(2.0 * sv * su, -2.0 * sv * cu, 0.0);
    let torus_mixed = Vector3::new(-2.0 * cv * cu, -2.0 * cv * su, 0.0);
    let cases = [
        (cylinder(), [Vector3::new(-2.0 * cu, -2.0 * su, 0.0), zero, zero, zero, zero, zero, zero]),
        (SolvedSurfaceGeometry::Cone(ConeSurface::try_new(origin, axis, reference, 2.0, 2.0,
            std::f64::consts::FRAC_PI_4).unwrap()), [
            Vector3::new(-(2.0 + v * slope) * cu, -2.0 * (2.0 + v * slope) * su, 0.0),
            Vector3::new(-slope * su, 2.0 * slope * cu, 0.0), zero, zero, zero, zero, zero,
        ]),
        (SolvedSurfaceGeometry::Sphere(SphereSurface::try_new(origin, axis, reference, 3.0).unwrap()),
            [sphere_uu, sphere_uv, sphere_uu, sphere_uv, sphere_uu, sphere_uv,
                Vector3::new(-3.0 * cv * cu, -3.0 * cv * su, -3.0 * sv)]),
        (SolvedSurfaceGeometry::Torus(TorusSurface::try_new(origin, axis, reference, 5.0, 2.0).unwrap()), [
            Vector3::new(-(5.0 + 2.0 * cv) * cu, -(5.0 + 2.0 * cv) * su, 0.0),
            torus_uv, torus_mixed, torus_uv, torus_mixed, torus_uv,
            Vector3::new(-2.0 * cv * cu, -2.0 * cv * su, -2.0 * sv),
        ]),
    ];
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        for (geometry, expected) in &cases {
            let fifth = crate::eval::surface_requested_jet_solved(&scratch, geometry, u, v, SurfaceRequest::Fifth).unwrap();
            let sixth = crate::eval::surface_requested_jet_solved(&scratch, geometry, u, v, SurfaceRequest::Sixth).unwrap();
            same_prefix(&sixth, &fifth); same_order(sixth.higher.fifth(), fifth.higher.fifth());
            assert_eq!(fifth.higher.sixth(), Err(EvaluationFailure::NoValue));
            near(sixth.higher.sixth().unwrap(), *expected);
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn actual_offset_fifth_uses_true_analytic_sixth_and_all_mixed_normal_laws() {
    let origin = Point3::new(0.0, 0.0, 0.0); let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0); let (u, v) = (0.3_f64, 0.4_f64);
    let (su, cu, sv, cv) = (u.sin(), u.cos(), v.sin(), v.cos());
    let angle = std::f64::consts::FRAC_PI_4; let slope = angle.tan();
    let radius = 2.0 + v * slope + 1.0 / slope.hypot(1.0);
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let cases = [
        (cylinder(), [Vector3::new(-3.0 * su, 3.0 * cu, 0.0), zero, zero, zero, zero, zero]),
        (SolvedSurfaceGeometry::Cone(ConeSurface::try_new(origin, axis, reference, 2.0, 1.0, angle).unwrap()),
            [Vector3::new(-radius * su, radius * cu, 0.0), Vector3::new(slope * cu, slope * su, 0.0), zero, zero, zero, zero]),
        (SolvedSurfaceGeometry::Sphere(SphereSurface::try_new(origin, axis, reference, 3.0).unwrap()), [
            Vector3::new(-4.0 * cv * su, 4.0 * cv * cu, 0.0), Vector3::new(-4.0 * sv * cu, -4.0 * sv * su, 0.0),
            Vector3::new(-4.0 * cv * su, 4.0 * cv * cu, 0.0), Vector3::new(-4.0 * sv * cu, -4.0 * sv * su, 0.0),
            Vector3::new(-4.0 * cv * su, 4.0 * cv * cu, 0.0), Vector3::new(-4.0 * sv * cu, -4.0 * sv * su, 4.0 * cv),
        ]),
        (SolvedSurfaceGeometry::Torus(TorusSurface::try_new(origin, axis, reference, 5.0, 2.0).unwrap()), [
            Vector3::new(-(5.0 + 3.0 * cv) * su, (5.0 + 3.0 * cv) * cu, 0.0), Vector3::new(-3.0 * sv * cu, -3.0 * sv * su, 0.0),
            Vector3::new(-3.0 * cv * su, 3.0 * cv * cu, 0.0), Vector3::new(-3.0 * sv * cu, -3.0 * sv * su, 0.0),
            Vector3::new(-3.0 * cv * su, 3.0 * cv * cu, 0.0), Vector3::new(-3.0 * sv * cu, -3.0 * sv * su, 3.0 * cv),
        ]),
    ];
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        for (geometry, expected) in &cases {
            let source = crate::eval::surface_requested_jet_solved(&scratch, geometry, u, v, SurfaceRequest::Sixth).unwrap();
            let result = super::super::offset(source, 1.0, SurfaceRequest::Fifth).unwrap();
            let old = crate::eval::surface_requested_jet_solved(&scratch, geometry, u, v, SurfaceRequest::Fifth).unwrap();
            let lower = super::super::offset(old, 1.0, SurfaceRequest::Fourth).unwrap();
            same_prefix(&result, &lower); near(result.higher.fifth().unwrap(), *expected);
            assert_eq!(result.higher.sixth(), Err(EvaluationFailure::NoValue));
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn actual_polynomial_sixth_zero_supplies_true_mixed_fifth_normal() {
    // Exact Taylor coefficient -3/8*(2u+v)*(5u^2+8uv+5v^2)^2;
    // each lane multiplies by (5-v)!v!, not by a sampled derivative.
    let geometry = graph(2, true, false);
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        let source = crate::eval::surface_requested_jet_solved(&scratch, &geometry, 0.0, 0.0, SurfaceRequest::Sixth).unwrap();
        assert_eq!(source.higher.sixth(), Ok([FiniteVector3::ZERO; 7]));
        let result = super::super::offset(source, 1.0, SurfaceRequest::Fifth).unwrap();
        let x = [-2250.0, -1665.0, -1386.0, -1233.0, -1170.0, -1125.0];
        let y = [-1125.0, -1170.0, -1233.0, -1386.0, -1665.0, -2250.0];
        near(result.higher.fifth().unwrap(), std::array::from_fn(|i| Vector3::new(x[i], y[i], 0.0)));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn actual_replica_offset_fifth_uses_ellipse_normal_and_subset_parity() {
    for reversed in [[false, false], [true, false], [false, true], [true, true]] {
        let mut ir = CadIr::empty(); let base = stored(&mut ir, "sixth-cylinder", cylinder());
        let placed = procedural(&mut ir, "sixth-ellipse", ProceduralSurfaceDefinition::Replica {
            source: base, transform: Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap(),
        });
        let subset = procedural(&mut ir, "sixth-subset", ProceduralSurfaceDefinition::Subset(
            SubsetSurfaceConstruction::try_new(placed, [[0.0, 1.0], [0.0, 1.0]], Some(!reversed[0]), Some(!reversed[1]), None).unwrap()));
        let shifted = offset(&mut ir, "sixth-offset", subset, 1.0);
        let zero = offset(&mut ir, "sixth-zero", shifted.clone(), -0.0);
        let index = ModelIndex::build(&ir, StandardIndex);
        let policy = DecodePolicy::service(); let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let fourth = model_requested_jet(admission, &index, &shifted, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
            let fifth = model_requested_jet(admission, &index, &shifted, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
            same_prefix(&fifth, &fourth);
            let distance = if reversed[0] == reversed[1] { 1.0 } else { -1.0 };
            let sign = if reversed[0] { -1.0 } else { 1.0 };
            assert_eq!(fifth.higher.fifth().unwrap()[0].get(), Vector3::new(0.0, sign * (2.0 + distance * 992.0), 0.0));
            assert_eq!(fifth.higher.fourth().unwrap()[0].get(), Vector3::new(4.0 + distance * 112.0, 0.0, 0.0));
            let identity = model_requested_jet(admission, &index, &zero, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
            same_prefix(&identity, &fifth); same_order(identity.higher.fifth(), fifth.higher.fifth());
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn actual_fifth_normal_keeps_final_fourth_overflow_separate_from_finite_fifth() {
    // S=(u,v,u^3): n_x=-3u^2+O(u^6), n_z=1-(9/2)u^4+O(u^8).
    // NormalFourth=(0,0,-108), NormalFifth=0. Offset at distance MAX
    // has a finite point and First, nonfinite Second/Fourth and finite Fifth.
    let geometry = graph(3, false, false);
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let source = crate::eval::surface_requested_jet_solved(&scratch, &geometry, 0.0, 0.0, SurfaceRequest::Sixth).unwrap();
    let fifth = super::super::offset(source, f64::MAX, SurfaceRequest::Fifth).unwrap();
    assert_eq!(fifth.jet.point.get(), Point3::new(0.0, 0.0, f64::MAX));
    assert!(fifth.jet.first.is_ok()); assert_eq!(fifth.jet.second, Err(EvaluationFailure::NonFinite(())));
    assert_eq!(fifth.higher.fourth(), Err(EvaluationFailure::NonFinite(())));
    assert_eq!(fifth.higher.fifth(), Ok([FiniteVector3::ZERO; 6]));
    let source = crate::eval::surface_requested_jet_solved(&scratch, &geometry, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
    let fourth = super::super::offset(source, f64::MAX, SurfaceRequest::Fourth).unwrap();
    same_prefix(&fifth, &fourth);
}

#[test]
fn actual_sixth_missing_owner_preserves_real_rational_and_procedural_fifth_prefix() {
    for geometry in [graph(2, true, true), graph(6, false, false)] {
        let scratch = Scratch::new(EvaluationAdmission::Standard);
        let fifth = crate::eval::surface_requested_jet_solved(&scratch, &geometry, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
        let sixth = crate::eval::surface_requested_jet_solved(&scratch, &geometry, 0.0, 0.0, SurfaceRequest::Sixth).unwrap();
        same_prefix(&sixth, &fifth); same_order(sixth.higher.fifth(), fifth.higher.fifth());
        assert_eq!(sixth.higher.sixth(), Err(EvaluationFailure::NoValue));
        assert_eq!(super::super::offset(sixth, 1.0, SurfaceRequest::Fifth).unwrap().higher.fifth(), Err(EvaluationFailure::NoValue));
    }
    let mut ir = CadIr::empty(); let id = CurveId::mint("test:model:sixth#circle").unwrap();
    ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 2.0).unwrap())), source_object: None });
    let surface = procedural(&mut ir, "sixth-sweep", ProceduralSurfaceDefinition::LinearSweep(
        LinearSweepSurfaceConstruction::try_new(id, Vector3::new(0.0, 0.0, 1.0)).unwrap()));
    let shifted = offset(&mut ir, "sixth-sweep-offset", surface.clone(), 1.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    let fifth = model_requested_jet(EvaluationAdmission::Standard, &index, &surface, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
    let sixth = model_requested_jet(EvaluationAdmission::Standard, &index, &surface, 0.0, 0.0, SurfaceRequest::Sixth).unwrap();
    same_prefix(&sixth, &fifth); same_order(sixth.higher.fifth(), fifth.higher.fifth());
    assert_eq!(sixth.higher.sixth(), Err(EvaluationFailure::NoValue));
    assert_eq!(model_requested_jet(EvaluationAdmission::Standard, &index, &shifted, 0.0, 0.0, SurfaceRequest::Fifth).unwrap().higher.fifth(), Err(EvaluationFailure::NoValue));
}

#[test]
fn actual_sixth_and_fifth_normal_fixed_work_keep_zero_caps_and_prior_real_refusal() {
    let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0; policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0; policy.limits.max_recursion_depth = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let work = WorkBudget::new(0);
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        admission.within_work_slice(&work, |admission| {
            let scratch = Scratch::new(admission);
            let source = crate::eval::surface_requested_jet_solved(&scratch, &cylinder(), 0.0, 0.0, SurfaceRequest::Sixth)?;
            assert_eq!(source.higher.sixth().unwrap()[0].get(), Vector3::new(-2.0, 0.0, 0.0));
            let result = super::super::offset(source, 1.0, SurfaceRequest::Fifth)?;
            assert_eq!(result.higher.fifth().unwrap()[0].get(), Vector3::new(0.0, 3.0, 0.0));
            let source = crate::eval::surface_requested_jet_solved(&scratch, &plane(), 0.0, 0.0, SurfaceRequest::Sixth)?;
            assert_eq!(super::super::offset(source, 2.0, SurfaceRequest::Sixth)?.higher.sixth(), Ok([FiniteVector3::ZERO; 7]));
            Ok::<_, EvaluationFailure<Point3>>(())
        }).unwrap();
    }
    assert_eq!(work.consumed(), 0); ctx.finish_session().unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = match ctx.alloc_filled(1, 0_u8, "actual prior sixth collection refusal") {
        Err(CodecError::ResourceLimit(limit)) => limit, _ => panic!("real collection operation must refuse"),
    };
    assert_eq!(original.dimension, ResourceDimension::CollectionItems);
    assert_eq!((original.limit, original.used, original.additional), (0, 0, 1));
    let scratch = Scratch::new(&ctx);
    assert!(matches!(crate::eval::surface_requested_jet_solved(&scratch, &cylinder(), f64::NAN, f64::NAN, SurfaceRequest::Sixth),
        Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
    drop(scratch);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn actual_fifth_final_range_failure_preserves_available_ellipse_fourth() {
    let mut ir = CadIr::empty(); let base = stored(&mut ir, "sixth-range-base", cylinder());
    let placed = procedural(&mut ir, "sixth-range-replica", ProceduralSurfaceDefinition::Replica {
        source: base, transform: Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap() });
    let shifted = offset(&mut ir, "sixth-range-offset", placed, f64::MAX / 512.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    let fifth = model_requested_jet(EvaluationAdmission::Standard, &index, &shifted, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
    let fourth = model_requested_jet(EvaluationAdmission::Standard, &index, &shifted, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
    same_prefix(&fifth, &fourth); assert!(fourth.higher.fourth().is_ok());
    // 992/512>1; Fourth's 112/512 remains finite at the same source input.
    assert_eq!(fifth.higher.fifth(), Err(EvaluationFailure::NonFinite(())));
}
