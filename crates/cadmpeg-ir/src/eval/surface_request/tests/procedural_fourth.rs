// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::analytic::{CircleCurve, LineCurve};
use crate::geometry::surface_payloads::{AxisRevolutionSurfaceConstruction, ExtrusionSurfaceConstruction,
    LinearSweepSurfaceConstruction, RevolutionSurfaceConstruction, SumSurfaceConstruction};
use crate::geometry::{CacheContract, Curve, CurveGeometry, SolvedCurveGeometry};
use crate::ids::CurveId;
use crate::units::UnitVector3;

const EPS_PROCEDURAL_FOURTH: f64 = 1.0e-12;

fn close(actual: Vector3, expected: Vector3) {
    assert!((actual - expected).norm() <= EPS_PROCEDURAL_FOURTH, "{actual:?} versus {expected:?}");
}

fn curve(ir: &mut CadIr, name: &str, geometry: SolvedCurveGeometry) -> CurveId {
    let id = CurveId::mint(format!("test:model:fourth-curve#{name}")).unwrap();
    ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Solved(geometry), source_object: None });
    id
}

fn circle(radius: f64) -> SolvedCurveGeometry {
    SolvedCurveGeometry::Circle(CircleCurve::try_new(Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), radius).unwrap())
}

fn requested(ir: &CadIr, surface: &SurfaceId, u: f64, v: f64, request: SurfaceRequest) -> super::super::RequestedJet {
    let index = ModelIndex::build(ir, StandardIndex);
    super::super::with_mapping(EvaluationAdmission::Standard, &index, surface, u, v, &mut |mapping| {
        mapping.evaluate(EvaluationAdmission::Standard, &index, request)
    }).unwrap()
}

fn direct_fourth(case: usize) {
    let mut ir = CadIr::empty();
    let first = curve(&mut ir, "circle2", circle(2.0));
    let second = curve(&mut ir, "circle4", circle(4.0));
    let line = curve(&mut ir, "axial-line", SolvedCurveGeometry::Line(LineCurve::try_new(
        Point3::new(2.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)).unwrap()));
    let origin = Point3::new(0.0, 0.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let (definition, u, v, expected, offset_third) = match case {
        0 => (ProceduralSurfaceDefinition::Extrusion(ExtrusionSurfaceConstruction::try_new(
            first, None, z, None, CacheContract::from_form(None)).unwrap()), 0.0, 0.25,
            [Vector3::new(2.0, 0.0, 0.0), zero, zero, zero, zero],
            [Vector3::new(0.0, -3.0, 0.0), zero, zero, zero]),
        1 => (ProceduralSurfaceDefinition::Revolution(RevolutionSurfaceConstruction::try_new(line,
            (FinitePoint3::new(origin).unwrap(), UnitVector3::new(z).unwrap()),
            [0.0, std::f64::consts::TAU], None, None, false, CacheContract::from_form(None)).unwrap()),
            0.25, 0.0, [zero, zero, zero, zero, Vector3::new(2.0, 0.0, 0.0)],
            [zero, zero, zero, Vector3::new(0.0, -1.0, 0.0)]),
        2 => (ProceduralSurfaceDefinition::Ruled { first, second, cache: None }, 0.0, 0.25,
            [Vector3::new(2.5, 0.0, 0.0), Vector3::new(0.0, -2.0, 0.0), zero, zero, zero],
            [Vector3::new(0.0, -2.5, 0.0), Vector3::new(-2.0, 0.0, 0.0), zero, zero]),
        3 => (ProceduralSurfaceDefinition::Sum(SumSurfaceConstruction::try_new(first, second,
            zero, CacheContract::from_form(None)).unwrap()), 0.0, std::f64::consts::FRAC_PI_2,
            [Vector3::new(2.0, 0.0, 0.0), zero, zero, zero, Vector3::new(0.0, 4.0, 0.0)],
            [Vector3::new(0.0, -2.0, 0.0), zero, zero, Vector3::new(4.0, 0.0, 0.0)]),
        4 => (ProceduralSurfaceDefinition::LinearSweep(LinearSweepSurfaceConstruction::try_new(first, z).unwrap()),
            0.0, 0.25, [Vector3::new(2.0, 0.0, 0.0), zero, zero, zero, zero],
            [Vector3::new(0.0, -3.0, 0.0), zero, zero, zero]),
        5 => (ProceduralSurfaceDefinition::AxisRevolution(AxisRevolutionSurfaceConstruction::try_new(line, origin, z).unwrap()),
            0.0, 0.25, [Vector3::new(2.0, 0.0, 0.0), zero, zero, zero, zero],
            [Vector3::new(0.0, -3.0, 0.0), zero, zero, zero]),
        _ => unreachable!("six actual construction owners"),
    };
    let surface = procedural(&mut ir, "direct-fourth", definition);
    let actual = requested(&ir, &surface, u, v, SurfaceRequest::Fourth);
    for (actual, expected) in actual.higher.fourth().unwrap().into_iter().zip(expected) { close(actual.get(), expected); }
    let lower = requested(&ir, &surface, u, v, SurfaceRequest::Third);
    assert_eq!(actual.jet.point, lower.jet.point); assert_eq!(actual.jet.first, lower.jet.first);
    assert_eq!(actual.jet.second, lower.jet.second); assert_eq!(actual.higher.third(), lower.higher.third());
    assert_eq!(lower.higher.fourth(), Err(EvaluationFailure::NoValue));
    let shifted = offset(&mut ir, "direct-fourth-offset", surface, 1.0);
    let shifted = requested(&ir, &shifted, u, v, SurfaceRequest::Third);
    for (actual, expected) in shifted.higher.third().unwrap().into_iter().zip(offset_third) { close(actual.get(), expected); }
}

#[test]
fn native_extrusion_fourth_supplies_true_offset_third() { direct_fourth(0); }
#[test]
fn native_revolution_fourth_supplies_true_offset_third() { direct_fourth(1); }
#[test]
fn ruled_surface_fourth_supplies_mixed_and_offset_orders() { direct_fourth(2); }
#[test]
fn sum_surface_fourth_supplies_independent_and_offset_orders() { direct_fourth(3); }
#[test]
fn linear_sweep_fourth_supplies_true_offset_third() { direct_fourth(4); }
#[test]
fn axis_revolution_fourth_supplies_true_offset_third() { direct_fourth(5); }

#[test]
fn native_revolution_fourth_maps_scale_and_oriented_offset_third_in_both_charts() {
    for transposed in [false, true] {
        let mut ir = CadIr::empty();
        let line = curve(&mut ir, "axial-line", SolvedCurveGeometry::Line(LineCurve::try_new(
            Point3::new(2.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)).unwrap()));
        let surface = procedural(&mut ir, "angular-fourth-chain", ProceduralSurfaceDefinition::Revolution(
            RevolutionSurfaceConstruction::try_new(line,
                (FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(), UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap()),
                [0.0, 2.0], Some([0.0, 1.0]), None, transposed, CacheContract::from_form(None)).unwrap()));
        let (u, v) = if transposed { (0.0, 0.25) } else { (0.25, 0.0) };
        let fourth = requested(&ir, &surface, u, v, SurfaceRequest::Fourth).higher.fourth().unwrap();
        let angular = usize::from(!transposed) * 4;
        // Radius2 * angular factor2^4 = 32; affine axial derivatives vanish.
        for (lane, actual) in fourth.into_iter().enumerate() {
            close(actual.get(), if lane == angular { Vector3::new(32.0, 0.0, 0.0) } else { Vector3::new(0.0, 0.0, 0.0) });
        }
        let shifted = offset(&mut ir, "angular-fourth-offset", surface, 1.0);
        let third = requested(&ir, &shifted, u, v, SurfaceRequest::Third);
        // u=angle has outward normal; u=axial has inward normal.
        let radius = if transposed { 3.0 } else { 1.0 };
        assert_eq!(third.jet.point.get(), Point3::new(radius, 0.0, 0.25));
        for (lane, actual) in third.higher.third().unwrap().into_iter().enumerate() {
            close(actual.get(), if lane == usize::from(!transposed) * 3 { Vector3::new(0.0, -8.0 * radius, 0.0) } else { Vector3::new(0.0, 0.0, 0.0) });
        }
    }
}

#[test]
fn degree_one_nurbs_curve_fourth_keeps_completed_third_and_all_lower_orders() {
    use crate::geometry::nurbs::NurbsCurve;
    let mut ir = CadIr::empty();
    let source = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 1.0, 1.0], vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 2.0]), false).unwrap().unwrap();
    let directrix = curve(&mut ir, "missing-curve-fourth", SolvedCurveGeometry::Nurbs(source));
    let surface = procedural(&mut ir, "nurbs-curve-fourth", ProceduralSurfaceDefinition::LinearSweep(
        LinearSweepSurfaceConstruction::try_new(directrix, Vector3::new(0.0, 0.0, 1.0)).unwrap()));
    let fourth = requested(&ir, &surface, 0.5, 0.25, SurfaceRequest::Fourth);
    let lower = requested(&ir, &surface, 0.5, 0.25, SurfaceRequest::Third);
    assert_eq!(fourth.jet.point, lower.jet.point); assert_eq!(fourth.jet.first, lower.jet.first);
    assert_eq!(fourth.jet.second, lower.jet.second); assert_eq!(fourth.higher.third(), lower.higher.third());
    close(fourth.higher.third().unwrap()[0].get(), Vector3::new(64.0 / 27.0, 0.0, 0.0));
    close(fourth.higher.fourth().unwrap()[0].get(), Vector3::new(-512.0 / 81.0, 0.0, 0.0));
    for lane in &fourth.higher.fourth().unwrap()[1..] { assert_eq!(*lane, FiniteVector3::ZERO); }
    let shifted = offset(&mut ir, "missing-fourth-offset", surface, 1.0);
    let result = requested(&ir, &shifted, 0.5, 0.25, SurfaceRequest::Third);
    assert!(result.jet.first.is_ok()); assert!(result.jet.second.is_ok());
    close(result.higher.third().unwrap()[0].get(), Vector3::new(64.0 / 27.0, 0.0, 0.0));
    for lane in &result.higher.third().unwrap()[1..] { assert_eq!(*lane, FiniteVector3::ZERO); }
}

#[test]
fn quadratic_rational_curve_fourth_keeps_its_true_third_and_lower_orders() {
    use crate::geometry::nurbs::NurbsCurve;
    let mut ir = CadIr::empty();
    // C=2t^2/(1+t^2). A zero homogeneous third is not a rational zero law.
    let source = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 1.0, 2.0]), false).unwrap().unwrap();
    let directrix = curve(&mut ir, "general-missing-fourth", SolvedCurveGeometry::Nurbs(source));
    let surface = procedural(&mut ir, "general-curve-fourth", ProceduralSurfaceDefinition::LinearSweep(
        LinearSweepSurfaceConstruction::try_new(directrix, Vector3::new(0.0, 0.0, 1.0)).unwrap()));
    let fourth = requested(&ir, &surface, 0.5, 0.25, SurfaceRequest::Fourth);
    let lower = requested(&ir, &surface, 0.5, 0.25, SurfaceRequest::Third);
    assert_eq!(fourth.jet.point, lower.jet.point); assert_eq!(fourth.jet.first, lower.jet.first);
    assert_eq!(fourth.jet.second, lower.jet.second); assert_eq!(fourth.higher.third(), lower.higher.third());
    close(fourth.higher.third().unwrap()[0].get(), Vector3::new(-4608.0 / 625.0, 0.0, 0.0));
    close(fourth.higher.fourth().unwrap()[0].get(), Vector3::new(58368.0 / 3125.0, 0.0, 0.0));
    let shifted = offset(&mut ir, "general-missing-fourth-offset", surface, 1.0);
    let result = requested(&ir, &shifted, 0.5, 0.25, SurfaceRequest::Third);
    assert!(result.jet.first.is_ok()); assert!(result.jet.second.is_ok());
    close(result.higher.third().unwrap()[0].get(), Vector3::new(-4608.0 / 625.0, 0.0, 0.0));
}
