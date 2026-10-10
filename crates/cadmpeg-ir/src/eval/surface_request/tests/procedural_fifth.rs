// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::features::FinitePoint3;
use crate::geometry::analytic::{CircleCurve, LineCurve};
use crate::geometry::surface_payloads::{AxisRevolutionSurfaceConstruction, ExtrusionSurfaceConstruction,
    LinearSweepSurfaceConstruction, RevolutionSurfaceConstruction, SumSurfaceConstruction};
use crate::geometry::{CacheContract, Curve, CurveGeometry, SolvedCurveGeometry};
use crate::ids::CurveId;
use crate::units::UnitVector3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const EPS_PROCEDURAL_FIFTH: f64 = 1.0e-12;

fn close(actual: Vector3, expected: Vector3) {
    assert!((actual - expected).norm() <= EPS_PROCEDURAL_FIFTH, "{actual:?} versus {expected:?}");
}

fn curve(ir: &mut CadIr, name: &str, geometry: SolvedCurveGeometry) -> CurveId {
    let id = CurveId::mint(format!("test:model:fifth-curve#{name}")).unwrap();
    ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Solved(geometry), source_object: None });
    id
}

fn circle(radius: f64) -> SolvedCurveGeometry {
    SolvedCurveGeometry::Circle(CircleCurve::try_new(Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), radius).unwrap())
}

fn requested(admission: EvaluationAdmission<'_, '_>, index: &ModelIndex<'_>, surface: &SurfaceId,
    u: f64, v: f64, request: SurfaceRequest) -> super::super::RequestedJet
{
    super::super::with_mapping(admission, index, surface, u, v, &mut |mapping| {
        mapping.evaluate(admission, index, request)
    }).unwrap()
}

fn direct_fifth(case: usize) {
    let mut ir = CadIr::empty();
    let first = curve(&mut ir, "circle2", circle(2.0));
    let second = curve(&mut ir, "circle4", circle(4.0));
    let line = curve(&mut ir, "axial-line", SolvedCurveGeometry::Line(LineCurve::try_new(
        Point3::new(2.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)).unwrap()));
    let origin = Point3::new(0.0, 0.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let (definition, u, v, expected, offset_fourth) = match case {
        0 => (ProceduralSurfaceDefinition::Extrusion(ExtrusionSurfaceConstruction::try_new(
            first, None, z, None, CacheContract::from_form(None)).unwrap()), 0.0, 0.25,
            [Vector3::new(0.0, 2.0, 0.0), zero, zero, zero, zero, zero],
            [Vector3::new(3.0, 0.0, 0.0), zero, zero, zero, zero]),
        1 => (ProceduralSurfaceDefinition::Revolution(RevolutionSurfaceConstruction::try_new(line,
            (FinitePoint3::new(origin).unwrap(), UnitVector3::new(z).unwrap()),
            [0.0, std::f64::consts::TAU], None, None, false, CacheContract::from_form(None)).unwrap()),
            0.25, 0.0, [zero, zero, zero, zero, zero, Vector3::new(0.0, 2.0, 0.0)],
            [zero, zero, zero, zero, Vector3::new(1.0, 0.0, 0.0)]),
        2 => (ProceduralSurfaceDefinition::Ruled { first, second, cache: None }, 0.0, 0.25,
            [Vector3::new(0.0, 2.5, 0.0), Vector3::new(2.0, 0.0, 0.0), zero, zero, zero, zero],
            [Vector3::new(2.5, 0.0, 0.0), Vector3::new(0.0, -2.0, 0.0), zero, zero, zero]),
        3 => (ProceduralSurfaceDefinition::Sum(SumSurfaceConstruction::try_new(first, second,
            zero, CacheContract::from_form(None)).unwrap()), 0.0, std::f64::consts::FRAC_PI_2,
            [Vector3::new(0.0, 2.0, 0.0), zero, zero, zero, zero, Vector3::new(-4.0, 0.0, 0.0)],
            [Vector3::new(2.0, 0.0, 0.0), zero, zero, zero, Vector3::new(0.0, 4.0, 0.0)]),
        4 => (ProceduralSurfaceDefinition::LinearSweep(LinearSweepSurfaceConstruction::try_new(first, z).unwrap()),
            0.0, 0.25, [Vector3::new(0.0, 2.0, 0.0), zero, zero, zero, zero, zero],
            [Vector3::new(3.0, 0.0, 0.0), zero, zero, zero, zero]),
        5 => (ProceduralSurfaceDefinition::AxisRevolution(AxisRevolutionSurfaceConstruction::try_new(line, origin, z).unwrap()),
            0.0, 0.25, [Vector3::new(0.0, 2.0, 0.0), zero, zero, zero, zero, zero],
            [Vector3::new(3.0, 0.0, 0.0), zero, zero, zero, zero]),
        _ => unreachable!("six actual construction owners"),
    };
    let surface = procedural(&mut ir, "direct-fifth", definition);
    let shifted = offset(&mut ir, "direct-fifth-offset", surface.clone(), 1.0);
    let zero_offset = offset(&mut ir, "direct-fifth-zero", surface.clone(), 0.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let actual = requested(admission, &index, &surface, u, v, SurfaceRequest::Fifth);
        for (actual, expected) in actual.higher.fifth().unwrap().into_iter().zip(expected) { close(actual.get(), expected); }
        let lower = requested(admission, &index, &surface, u, v, SurfaceRequest::Fourth);
        assert_eq!(actual.jet.point, lower.jet.point); assert_eq!(actual.jet.first, lower.jet.first); assert_eq!(actual.jet.second, lower.jet.second);
        assert_eq!(actual.higher.third(), lower.higher.third()); assert_eq!(actual.higher.fourth(), lower.higher.fourth());
        assert_eq!(lower.higher.fifth(), Err(EvaluationFailure::NoValue));
        let zero_offset = requested(admission, &index, &zero_offset, u, v, SurfaceRequest::Fifth);
        assert_eq!(zero_offset.higher.fifth(), actual.higher.fifth()); assert_eq!(zero_offset.jet.point, actual.jet.point);
        let shifted_fourth = requested(admission, &index, &shifted, u, v, SurfaceRequest::Fourth);
        for (actual, expected) in shifted_fourth.higher.fourth().unwrap().into_iter().zip(offset_fourth) { close(actual.get(), expected); }
        let shifted_fifth = requested(admission, &index, &shifted, u, v, SurfaceRequest::Fifth);
        assert_eq!(shifted_fifth.higher.fifth(), Err(EvaluationFailure::NoValue));
        assert_eq!(shifted_fifth.higher.fourth(), shifted_fourth.higher.fourth());
    }
    ctx.finish_session().unwrap();
}

#[test]
fn native_extrusion_fifth_supplies_true_offset_fourth() { direct_fifth(0); }
#[test]
fn native_revolution_fifth_supplies_true_offset_fourth() { direct_fifth(1); }
#[test]
fn ruled_surface_fifth_supplies_true_mixed_and_offset_orders() { direct_fifth(2); }
#[test]
fn sum_surface_fifth_supplies_true_independent_and_offset_orders() { direct_fifth(3); }
#[test]
fn linear_sweep_fifth_supplies_true_offset_fourth() { direct_fifth(4); }
#[test]
fn axis_revolution_fifth_supplies_true_offset_fourth() { direct_fifth(5); }

#[test]
fn native_revolution_fifth_maps_actual_chain_factors_and_oriented_offset_in_both_charts() {
    for transposed in [false, true] {
        let mut ir = CadIr::empty();
        let line = curve(&mut ir, "axial-line", SolvedCurveGeometry::Line(LineCurve::try_new(
            Point3::new(2.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)).unwrap()));
        let surface = procedural(&mut ir, "angular-fifth-chain", ProceduralSurfaceDefinition::Revolution(
            RevolutionSurfaceConstruction::try_new(line,
                (FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(), UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap()),
                [0.0, 2.0], Some([0.0, 1.0]), None, transposed, CacheContract::from_form(None)).unwrap()));
        let shifted = offset(&mut ir, "angular-fifth-offset", surface.clone(), 1.0);
        let index = ModelIndex::build(&ir, StandardIndex);
        let (u, v) = if transposed { (0.0, 0.25) } else { (0.25, 0.0) };
        let fifth = requested(EvaluationAdmission::Standard, &index, &surface, u, v, SurfaceRequest::Fifth).higher.fifth().unwrap();
        for (lane, actual) in fifth.into_iter().enumerate() {
            close(actual.get(), if lane == usize::from(!transposed) * 5 { Vector3::new(0.0, 64.0, 0.0) } else { Vector3::new(0.0, 0.0, 0.0) });
        }
        let fourth = requested(EvaluationAdmission::Standard, &index, &shifted, u, v, SurfaceRequest::Fourth);
        let radius = if transposed { 3.0 } else { 1.0 };
        assert_eq!(fourth.jet.point.get(), Point3::new(radius, 0.0, 0.25));
        for (lane, actual) in fourth.higher.fourth().unwrap().into_iter().enumerate() {
            close(actual.get(), if lane == usize::from(!transposed) * 4 { Vector3::new(16.0 * radius, 0.0, 0.0) } else { Vector3::new(0.0, 0.0, 0.0) });
        }
    }
}
