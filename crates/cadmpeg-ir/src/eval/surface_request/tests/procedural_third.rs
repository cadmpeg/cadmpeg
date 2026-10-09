// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::features::FinitePoint3;
use crate::geometry::analytic::{CircleCurve, LineCurve};
use crate::geometry::surface_payloads::{AxisRevolutionSurfaceConstruction,
    ExtrusionSurfaceConstruction, LinearSweepSurfaceConstruction,
    RevolutionSurfaceConstruction, SumSurfaceConstruction};
use crate::geometry::{CacheContract, Curve, CurveGeometry, SolvedCurveGeometry};
use crate::ids::CurveId;
use crate::units::UnitVector3;

const EPS_PROCEDURAL_THIRD: f64 = 1.0e-12;

fn close(actual: Vector3, expected: Vector3) {
    assert!((actual - expected).norm() <= EPS_PROCEDURAL_THIRD, "{actual:?} versus {expected:?}");
}

fn add_curve(ir: &mut CadIr, name: &str, geometry: SolvedCurveGeometry) -> CurveId {
    let id = CurveId::mint(format!("test:model:requested-curve#{name}")).unwrap();
    ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Solved(geometry), source_object: None });
    id
}

fn circle(radius: f64) -> SolvedCurveGeometry {
    SolvedCurveGeometry::Circle(CircleCurve::try_new(Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), radius).unwrap())
}

fn requested(ir: &CadIr, surface: &SurfaceId, u: f64, v: f64) -> crate::eval::surface_request::RequestedJet {
    let index = ModelIndex::build(ir, StandardIndex);
    super::super::with_mapping(EvaluationAdmission::Standard, &index, surface, u, v, &mut |mapping| {
        mapping.evaluate(EvaluationAdmission::Standard, &index, SurfaceRequest::Third)
    }).unwrap()
}

fn direct_third(case: usize) {
    let mut ir = CadIr::empty();
    let first = add_curve(&mut ir, "circle2", circle(2.0));
    let second = add_curve(&mut ir, "circle4", circle(4.0));
    let line = add_curve(&mut ir, "axial-line", SolvedCurveGeometry::Line(LineCurve::try_new(
        Point3::new(2.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)).unwrap()));
    let origin = Point3::new(0.0, 0.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let (definition, u, v, expected, offset_point, offset_second) = match case {
        0 => (ProceduralSurfaceDefinition::Extrusion(ExtrusionSurfaceConstruction::try_new(
            first, None, z, None, CacheContract::from_form(None)).unwrap()), 0.0, 0.25,
            [Vector3::new(0.0, -2.0, 0.0), zero, zero, zero], Point3::new(3.0, 0.0, 0.25),
            [Vector3::new(-3.0, 0.0, 0.0), zero, zero]),
        1 => (ProceduralSurfaceDefinition::Revolution(RevolutionSurfaceConstruction::try_new(line,
            (FinitePoint3::new(origin).unwrap(), UnitVector3::new(z).unwrap()),
            [0.0, std::f64::consts::TAU], None, None, false, CacheContract::from_form(None)).unwrap()),
            0.25, 0.0, [zero, zero, zero, Vector3::new(0.0, -2.0, 0.0)], Point3::new(1.0, 0.0, 0.25),
            [zero, zero, Vector3::new(-1.0, 0.0, 0.0)]),
        2 => (ProceduralSurfaceDefinition::Ruled { first, second, cache: None }, 0.0, 0.25,
            [Vector3::new(0.0, -2.5, 0.0), Vector3::new(-2.0, 0.0, 0.0), zero, zero],
            Point3::new(2.5, 0.0, -1.0), [Vector3::new(-2.5, 0.0, 0.0), Vector3::new(0.0, 2.0, 0.0), zero]),
        3 => (ProceduralSurfaceDefinition::Sum(SumSurfaceConstruction::try_new(first, second,
            zero, CacheContract::from_form(None)).unwrap()), 0.0, std::f64::consts::FRAC_PI_2,
            [Vector3::new(0.0, -2.0, 0.0), zero, zero, Vector3::new(4.0, 0.0, 0.0)],
            Point3::new(2.0, 4.0, 1.0), [Vector3::new(-2.0, 0.0, 0.0), zero, Vector3::new(0.0, -4.0, 0.0)]),
        4 => (ProceduralSurfaceDefinition::LinearSweep(LinearSweepSurfaceConstruction::try_new(first, z).unwrap()),
            0.0, 0.25, [Vector3::new(0.0, -2.0, 0.0), zero, zero, zero], Point3::new(3.0, 0.0, 0.25),
            [Vector3::new(-3.0, 0.0, 0.0), zero, zero]),
        5 => (ProceduralSurfaceDefinition::AxisRevolution(AxisRevolutionSurfaceConstruction::try_new(line, origin, z).unwrap()),
            0.0, 0.25, [Vector3::new(0.0, -2.0, 0.0), zero, zero, zero], Point3::new(3.0, 0.0, 0.25),
            [Vector3::new(-3.0, 0.0, 0.0), zero, zero]),
        _ => unreachable!("six actual construction owners"),
    };
    let surface = procedural(&mut ir, "direct-third", definition);
    for (actual, expected) in requested(&ir, &surface, u, v).higher.third().unwrap().into_iter().zip(expected) {
        close(actual.get(), expected);
    }
    let shifted = offset(&mut ir, "direct-offset", surface, 1.0);
    let result = evaluate(&ir, &shifted, u, v, SurfaceRequest::Second);
    close(result.point.get().vector_from(offset_point), zero);
    for (actual, expected) in result.second.unwrap().into_iter().zip(offset_second) {
        close(actual.get(), expected);
    }
}

#[test]
fn native_extrusion_third_supplies_true_offset_second() { direct_third(0); }
#[test]
fn native_revolution_third_supplies_true_offset_second() { direct_third(1); }
#[test]
fn ruled_surface_third_supplies_mixed_and_offset_orders() { direct_third(2); }
#[test]
fn sum_surface_third_supplies_independent_and_offset_orders() { direct_third(3); }
#[test]
fn linear_sweep_third_supplies_true_offset_second() { direct_third(4); }
#[test]
fn axis_revolution_third_supplies_true_offset_second() { direct_third(5); }

#[test]
fn native_revolution_third_maps_angular_scale_and_transposition() {
    for transposed in [false, true] {
        let mut ir = CadIr::empty();
        let line = add_curve(&mut ir, "axial-line", SolvedCurveGeometry::Line(LineCurve::try_new(
            Point3::new(2.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)).unwrap()));
        let surface = procedural(&mut ir, "angular-chain", ProceduralSurfaceDefinition::Revolution(
            RevolutionSurfaceConstruction::try_new(line,
                (FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(), UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap()),
                [0.0, 2.0], Some([0.0, 1.0]), None, transposed, CacheContract::from_form(None)).unwrap()));
        let (u, v) = if transposed { (0.0, 0.25) } else { (0.25, 0.0) };
        let third = requested(&ir, &surface, u, v).higher.third().unwrap();
        let angular = usize::from(!transposed) * 3;
        for (lane, actual) in third.into_iter().enumerate() {
            close(actual.get(), if lane == angular { Vector3::new(0.0, -16.0, 0.0) } else { Vector3::new(0.0, 0.0, 0.0) });
        }
    }
}

#[test]
fn degree_one_nurbs_curve_third_keeps_real_surface_lower_orders() {
    use crate::geometry::nurbs::NurbsCurve;
    let mut ir = CadIr::empty();
    let curve = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 1.0, 1.0], vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 2.0]), false).unwrap().unwrap();
    let directrix = add_curve(&mut ir, "rational-line", SolvedCurveGeometry::Nurbs(curve));
    let surface = procedural(&mut ir, "rational-extrusion", ProceduralSurfaceDefinition::LinearSweep(
        LinearSweepSurfaceConstruction::try_new(directrix, Vector3::new(0.0, 0.0, 1.0)).unwrap()));
    let third = requested(&ir, &surface, 0.5, 0.25);
    close(third.jet.point.get().vector_from(Point3::new(2.0 / 3.0, 0.0, 0.25)), Vector3::new(0.0, 0.0, 0.0));
    assert!(third.jet.first.is_ok());
    assert!(third.jet.second.is_ok());
    // C(u)=2u/(1+u), so C'''(1/2)=12/(3/2)^4=64/27.
    let [uuu, uuv, uvv, vvv] = third.higher.third().unwrap();
    close(uuu.get(), Vector3::new(64.0 / 27.0, 0.0, 0.0));
    for mixed in [uuv, uvv, vvv] {
        close(mixed.get(), Vector3::new(0.0, 0.0, 0.0));
    }
    let shifted = offset(&mut ir, "rational-offset", surface, 1.0);
    let result = evaluate(&ir, &shifted, 0.5, 0.25, SurfaceRequest::Second);
    close(result.point.get().vector_from(Point3::new(2.0 / 3.0, -1.0, 0.25)), Vector3::new(0.0, 0.0, 0.0));
    assert!(result.first.is_ok());
    // The chart normal is constant. Offset retains C''(1/2)=-4/(3/2)^3.
    let [uu, uv, vv] = result.second.unwrap();
    close(uu.get(), Vector3::new(-32.0 / 27.0, 0.0, 0.0));
    for mixed in [uv, vv] {
        close(mixed.get(), Vector3::new(0.0, 0.0, 0.0));
    }
}

#[test]
fn quadratic_rational_third_supplies_actual_offset_second_and_preserves_lower_orders() {
    use crate::geometry::nurbs::NurbsCurve;
    let mut ir = CadIr::empty();
    let curve = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 1.0, 2.0]), false).unwrap().unwrap();
    let directrix = add_curve(&mut ir, "higher-rational-line", SolvedCurveGeometry::Nurbs(curve));
    let surface = procedural(&mut ir, "higher-rational-extrusion", ProceduralSurfaceDefinition::LinearSweep(
        LinearSweepSurfaceConstruction::try_new(directrix, Vector3::new(0.0, 0.0, 1.0)).unwrap()));
    let third = requested(&ir, &surface, 0.5, 0.25);
    let second = evaluate(&ir, &surface, 0.5, 0.25, SurfaceRequest::Second);
    assert_eq!(third.jet.point, second.point);
    assert_eq!(third.jet.first, second.first);
    assert_eq!(third.jet.second, second.second);
    assert!(third.jet.first.is_ok());
    assert!(third.jet.second.is_ok());
    // C=2u^2/(1+u^2), so C'''(.5)=-4608/625.
    let [uuu, uuv, uvv, vvv] = third.higher.third().unwrap();
    close(uuu.get(), Vector3::new(-4608.0 / 625.0, 0.0, 0.0));
    for mixed in [uuv, uvv, vvv] { close(mixed.get(), Vector3::new(0.0, 0.0, 0.0)); }
    let shifted = offset(&mut ir, "higher-rational-offset", surface, 1.0);
    let result = evaluate(&ir, &shifted, 0.5, 0.25, SurfaceRequest::Second);
    assert!(result.first.is_ok());
    // The normal is constant. C''(.5)=4*(1-3/4)/(5/4)^3=64/125.
    let [uu, uv, vv] = result.second.unwrap();
    close(uu.get(), Vector3::new(64.0 / 125.0, 0.0, 0.0));
    for mixed in [uv, vv] { close(mixed.get(), Vector3::new(0.0, 0.0, 0.0)); }
}

#[test]
fn unavailable_cubic_rational_third_preserves_actual_surface_lower_orders() {
    use crate::geometry::nurbs::NurbsCurve;
    let mut ir = CadIr::empty();
    let curve = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 1.0, 1.0, 2.0]), false).unwrap().unwrap();
    let directrix = add_curve(&mut ir, "cubic-rational-line", SolvedCurveGeometry::Nurbs(curve));
    let surface = procedural(&mut ir, "cubic-rational-extrusion", ProceduralSurfaceDefinition::LinearSweep(
        LinearSweepSurfaceConstruction::try_new(directrix, Vector3::new(0.0, 0.0, 1.0)).unwrap()));
    let third = requested(&ir, &surface, 0.5, 0.25);
    let second = evaluate(&ir, &surface, 0.5, 0.25, SurfaceRequest::Second);
    assert_eq!(third.jet.point, second.point);
    assert_eq!(third.jet.first, second.first);
    assert_eq!(third.jet.second, second.second);
    assert!(third.jet.first.is_ok());
    assert!(third.jet.second.is_ok());
    assert_eq!(third.higher.third(), Err(EvaluationFailure::NoValue));
    let shifted = offset(&mut ir, "cubic-rational-offset", surface, 1.0);
    let result = evaluate(&ir, &shifted, 0.5, 0.25, SurfaceRequest::Second);
    assert!(result.first.is_ok());
    assert_eq!(result.second, Err(EvaluationFailure::NoValue));
}
