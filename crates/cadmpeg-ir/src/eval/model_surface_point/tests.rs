// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::document::admission::StandardAdmission;
use crate::geometry::analytic::{CircleCurve, LineCurve};
use crate::geometry::surface_payloads::{AxisRevolutionSurfaceConstruction,
    LinearSweepSurfaceConstruction, ExtrusionSurfaceConstruction, OffsetSurfaceConstruction,
    RevolutionSurfaceConstruction, SumSurfaceConstruction};
use crate::geometry::{CacheContract, Curve, CurveGeometry, LegacyExtensionFlags, OffsetExtension,
    ProceduralSurface, Surface, SolvedCurveGeometry};
use crate::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use crate::index::{ModelIndex, StandardIndex};
use crate::units::UnitVector3;
use crate::CadIr;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

const EPS_DIRECT_NORMAL_POINT: f64 = 1.0e-12;

fn fixture(case: usize) -> (CadIr, SurfaceId, SurfaceId, f64, f64, Point3, Point3) {
    let mut ir = CadIr::empty();
    let origin = Point3::new(0.0, 0.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    let first = CurveId::mint("test:model:normal-curve#first").unwrap();
    let second = CurveId::mint("test:model:normal-curve#second").unwrap();
    let axial = CurveId::mint("test:model:normal-curve#axial").unwrap();
    for (id, geometry) in [
        (first.clone(), SolvedCurveGeometry::Circle(CircleCurve::try_new(origin, z,
            Vector3::new(1.0, 0.0, 0.0), 2.0).unwrap())),
        (second.clone(), SolvedCurveGeometry::Circle(CircleCurve::try_new(origin, z,
            Vector3::new(1.0, 0.0, 0.0), 4.0).unwrap())),
        (axial.clone(), SolvedCurveGeometry::Line(LineCurve::try_new(Point3::new(2.0, 0.0, 0.0), z).unwrap())),
    ] {
        ir.model.curves.push(Curve { id, geometry: CurveGeometry::Solved(geometry), source_object: None });
    }
    let (definition, u, v, point, shifted) = match case {
        0 => (ProceduralSurfaceDefinition::Extrusion(ExtrusionSurfaceConstruction::try_new(
            first, None, z, None, CacheContract::from_form(None)).unwrap()), 0.0, 0.25,
            Point3::new(2.0, 0.0, 0.25), Point3::new(3.0, 0.0, 0.25)),
        1 => (ProceduralSurfaceDefinition::Revolution(RevolutionSurfaceConstruction::try_new(
            axial, (FinitePoint3::new(origin).unwrap(), UnitVector3::new(z).unwrap()),
            [0.0, std::f64::consts::TAU], None, None, false, CacheContract::from_form(None)).unwrap()),
            0.25, 0.0, Point3::new(2.0, 0.0, 0.25), Point3::new(1.0, 0.0, 0.25)),
        2 => (ProceduralSurfaceDefinition::Ruled { first, second, cache: None }, 0.0, 0.25,
            Point3::new(2.5, 0.0, 0.0), Point3::new(2.5, 0.0, -1.0)),
        3 => (ProceduralSurfaceDefinition::Sum(SumSurfaceConstruction::try_new(first, second,
            Vector3::new(0.0, 0.0, 0.0), CacheContract::from_form(None)).unwrap()),
            0.0, std::f64::consts::FRAC_PI_2, Point3::new(2.0, 4.0, 0.0), Point3::new(2.0, 4.0, 1.0)),
        4 => (ProceduralSurfaceDefinition::LinearSweep(LinearSweepSurfaceConstruction::try_new(first, z).unwrap()),
            0.0, 0.25, Point3::new(2.0, 0.0, 0.25), Point3::new(3.0, 0.0, 0.25)),
        5 => (ProceduralSurfaceDefinition::AxisRevolution(AxisRevolutionSurfaceConstruction::try_new(axial, origin, z).unwrap()),
            0.0, 0.25, Point3::new(2.0, 0.0, 0.25), Point3::new(3.0, 0.0, 0.25)),
        _ => unreachable!("six differential-selected direct owners"),
    };
    let mut publish = |name: &str, definition| {
        let surface = SurfaceId::mint(format!("test:model:normal-surface#{name}")).unwrap();
        let construction = ProceduralSurfaceId::mint(format!("test:model:normal-construction#{name}")).unwrap();
        ir.model.surfaces.push(Surface { id: surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }), source_object: None });
        ir.model.add_procedural_surface(&StandardAdmission, &surface,
            ProceduralSurface::new(construction, definition, None)).unwrap().unwrap();
        surface
    };
    let base = publish("base", definition);
    let offset = publish("offset", ProceduralSurfaceDefinition::Offset(OffsetSurfaceConstruction::try_new(
        base.clone(), 1.0, None, None, false,
        OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent {}, cache: None }).unwrap()));
    (ir, base, offset, u, v, point, shifted)
}

fn direct_normal(case: usize) {
    let (ir, base, offset, u, v, point, shifted) = fixture(case);
    let index = ModelIndex::build(&ir, StandardIndex);
    // Standard is the genuine ordinary evaluation route, with no DecodeContext.
    let actual = model_surface_point_by_id_inner(admission::EvaluationAdmission::Standard,
        &index, &offset, u, v).unwrap();
    assert!(actual.get().distance(shifted) <= EPS_DIRECT_NORMAL_POINT);
    let base_point = model_surface_point_by_id_inner(admission::EvaluationAdmission::Standard,
        &index, &base, u, v).unwrap();
    assert!(base_point.get().distance(point) <= EPS_DIRECT_NORMAL_POINT);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let contextual = crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
        &index, &offset, u, v).unwrap();
    assert_eq!(contextual, actual);
    ctx.finish_session().unwrap();
}

#[test]
fn native_extrusion_point_supplies_the_requested_support_normal() { direct_normal(0); }
#[test]
fn native_revolution_point_supplies_its_parameter_oriented_support_normal() { direct_normal(1); }
#[test]
fn ruled_point_supplies_the_requested_support_normal() { direct_normal(2); }
#[test]
fn sum_point_supplies_the_requested_support_normal() { direct_normal(3); }

#[test]
fn direct_procedural_normal_preserves_the_original_prefused_context() {
    for case in 0..4 {
        let (ir, _, offset, u, v, _, _) = fixture(case);
        let index = ModelIndex::build(&ir, StandardIndex);
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = ctx.charge_work_limit(1, "actual prior direct-normal refusal").unwrap_err();
        for (u, v) in [(u, v), (f64::NAN, f64::NAN)] {
            assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
                &index, &offset, u, v), Err(EvaluationFailure::ResourceLimit(original)));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    }
}

mod axis_linear;

mod zero_offset;

mod cache;
