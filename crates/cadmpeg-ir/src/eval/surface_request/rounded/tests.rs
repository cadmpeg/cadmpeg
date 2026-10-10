// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::document::admission::StandardAdmission;
use crate::eval::surface_request::{model_requested_jet, model_jet};
use crate::eval::tests::variable_blend::variable_blend_eval_fixture;
use crate::geometry::analytic::CylinderSurface;
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::pcurve::{ParabolaPcurve, PcurveGeometry, PcurveNurbs};
use crate::geometry::surface_payloads::{OffsetSurfaceConstruction, SubsetSurfaceConstruction};
use crate::geometry::{LegacyExtensionFlags, OffsetExtension, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry, VariableBlendCrossSection};
use crate::ids::{ProceduralSurfaceId, SurfaceId};
use crate::index::StandardIndex;
use crate::math::Point2;
use crate::transform::Transform;
use crate::CadIr;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};
use cadmpeg_core::CodecError;

const EPS_ROUNDED_HIGHER: f64 = 1.0e-10;

fn cylinder() -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Cylinder(CylinderSurface::try_new(Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 2.0).unwrap())
}

fn fixture() -> (CadIr, SurfaceId) {
    // A(v)=(2cos v,2sin v,0), D(v)=(0,v,2).
    let (mut ir, id) = variable_blend_eval_fixture(Point3::new(0.0, 0.0, 0.0),
        [(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)),
            (Point2::new(0.0, 2.0), Point2::new(1.0, 0.0))],
        [0.0, 0.0], Some(VariableBlendCrossSection::RoundedChamfer { radius: None }));
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(cylinder());
    (ir, id)
}

fn attach(ir: &mut CadIr, name: &str, definition: ProceduralSurfaceDefinition) -> SurfaceId {
    let id = SurfaceId::mint(format!("test:model:rounded-higher#{name}")).unwrap();
    let construction = ProceduralSurfaceId::mint(format!("test:model:rounded-higher-construction#{name}")).unwrap();
    ir.model.surfaces.push(Surface { id: id.clone(), geometry: SurfaceGeometry::Solved(
        SolvedSurfaceGeometry::Unknown { record: None }), source_object: None });
    ir.model.add_procedural_surface(&StandardAdmission, &id,
        ProceduralSurface::new(construction, definition, None)).unwrap().unwrap();
    id
}

fn shifted(ir: &mut CadIr, name: &str, source: SurfaceId, distance: f64) -> SurfaceId {
    attach(ir, name, ProceduralSurfaceDefinition::Offset(OffsetSurfaceConstruction::try_new(
        source, distance, None, None, false,
        OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent {}, cache: None }).unwrap()))
}

fn pcurve(ir: &mut CadIr, geometry: PcurveGeometry) {
    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::VariableBlend(payload) = definition else { panic!("rounded fixture"); };
        let mut construction = payload.construction().to_raw();
        construction.sides[0].pcurve = Some(geometry);
        *payload = VariableBlendSurfacePayload::try_new(Box::new(construction)).unwrap();
    });
}

fn close<const N: usize>(actual: [FiniteVector3; N], expected: [Vector3; N]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual.get() - expected).norm() <= EPS_ROUNDED_HIGHER, "{actual:?} versus {expected:?}");
    }
}

#[test]
fn rounded_cylinder_contacts_supply_true_second_through_fifth_and_nonzero_offset_second() {
    let (mut ir, base) = fixture();
    let offset = shifted(&mut ir, "cylinder-offset", base.clone(), 1.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let zero = Vector3::new(0.0, 0.0, 0.0);
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let result = model_requested_jet(admission, &index, &base, 0.5, 0.0, SurfaceRequest::Fifth).unwrap();
        let old = crate::eval::model_surface_partials_by_id(admission, &index, &base, 0.5, 0.0).unwrap();
        assert_eq!(result.jet.first_order().partials().unwrap(), old);
        close(result.jet.second.unwrap(), [zero, Vector3::new(0.0, -1.0, 0.0), Vector3::new(-1.0, 0.0, 0.0)]);
        close(result.higher.third().unwrap(), [zero, zero, Vector3::new(2.0, 0.0, 0.0), Vector3::new(0.0, -1.0, 0.0)]);
        close(result.higher.fourth().unwrap(), [zero, zero, zero, Vector3::new(0.0, 2.0, 0.0), Vector3::new(1.0, 0.0, 0.0)]);
        close(result.higher.fifth().unwrap(), [zero, zero, zero, zero, Vector3::new(-2.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)]);
        // At(u,v)=(1/2,0), C=Bu cross Bv=(-3,0,-3),
        // Cv=(0,-2,0), Cvv=(2,0,3). Differentiate n=C/|C|.
        let root2 = 2.0_f64.sqrt();
        let offset = model_requested_jet(admission, &index, &offset, 0.5, 0.0, SurfaceRequest::Second).unwrap();
        close(offset.jet.second.unwrap(), [zero, Vector3::new(0.0, -1.0 + 8.0 / (9.0 * root2), 0.0),
            Vector3::new(-1.0 + 1.0 / (18.0 * root2), 0.0, 7.0 / (18.0 * root2))]);
        assert!(offset.jet.point.get().distance(Point3::new(1.0 - 1.0 / root2, 0.0, 1.0 - 1.0 / root2)) <= EPS_ROUNDED_HIGHER);
    }
    ctx.finish_session().unwrap();
}

#[test]
fn rounded_parabolic_contact_uses_true_nonlinear_chain_rule_third() {
    let (mut ir, base) = fixture();
    pcurve(&mut ir, PcurveGeometry::Parabola(ParabolaPcurve::try_new(Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0), Point2::new(0.0, 1.0), 1.0).unwrap()));
    let index = ModelIndex::build(&ir, StandardIndex);
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
    let result = model_requested_jet(admission, &index, &base, 0.5, 0.5, SurfaceRequest::Third).unwrap();
    // A=(2cos(v^2/4),2sin(v^2/4),v); p=v/2, p'=1/2.
    let (s, c) = (1.0_f64 / 16.0).sin_cos();
    let zero = Vector3::new(0.0, 0.0, 0.0);
    close(result.higher.third().unwrap(), [zero, zero,
        Vector3::new(c / 8.0 + s, s / 8.0 - c, 0.0),
        Vector3::new(s / 64.0 - 3.0 * c / 8.0, -c / 64.0 - 3.0 * s / 8.0, 0.0)]);
    }
    ctx.finish_session().unwrap();
}

fn rational_support() -> SolvedSurfaceGeometry {
    // S(a,b)=(a/(1+a),b,0), from actual linear weighted poles.
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = NurbsSurface::from_lanes(&ctx,
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(0.5, 0.0, 0.0), Point3::new(0.5, 1.0, 0.0)]], Some(vec![vec![1.0, 1.0], vec![2.0, 2.0]])),
        false).unwrap().unwrap();
    ctx.finish_session().unwrap(); SolvedSurfaceGeometry::Nurbs(result)
}

#[test]
fn rounded_rational_contacts_keep_all_odd_signs_placement_and_zero_identity() {
    for reversed in [[false, false], [true, false], [false, true], [true, true]] {
        let (mut ir, base) = fixture(); ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(rational_support());
        let subset = attach(&mut ir, "subset", ProceduralSurfaceDefinition::Subset(SubsetSurfaceConstruction::try_new(
            base, [[0.0, 1.0], [0.0, 1.0]], Some(!reversed[0]), Some(!reversed[1]), None).unwrap()));
        let placed = attach(&mut ir, "placed", ProceduralSurfaceDefinition::Replica { source: subset,
            transform: Transform::affine([[2.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0]]).unwrap() });
        let zero_offset = shifted(&mut ir, "zero", placed.clone(), -0.0);
        let index = ModelIndex::build(&ir, StandardIndex);
        let us = if reversed[0] { -1.0 } else { 1.0 }; let vs = if reversed[1] { -1.0 } else { 1.0 };
        let zero = Vector3::new(0.0, 0.0, 0.0);
        let policy = DecodePolicy::service(); let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let result = model_requested_jet(admission, &index, &placed, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
        close(result.higher.third().unwrap(), [zero, zero, Vector3::new(4.0 * us, 0.0, 0.0), Vector3::new(12.0 * vs, 0.0, 0.0)]);
        close(result.higher.fourth().unwrap(), [zero, zero, zero, Vector3::new(-12.0 * us * vs, 0.0, 0.0), Vector3::new(-48.0, 0.0, 0.0)]);
        close(result.higher.fifth().unwrap(), [zero, zero, zero, zero, Vector3::new(48.0 * us, 0.0, 0.0), Vector3::new(240.0 * vs, 0.0, 0.0)]);
        let identity = model_requested_jet(admission, &index, &zero_offset, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
        assert_eq!(identity.jet.point, result.jet.point); assert_eq!(identity.jet.first, result.jet.first);
        assert_eq!(identity.jet.second, result.jet.second); assert_eq!(identity.higher.third(), result.higher.third());
        assert_eq!(identity.higher.fourth(), result.higher.fourth()); assert_eq!(identity.higher.fifth(), result.higher.fifth());
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn rounded_missing_pcurve_higher_keeps_completed_point_first_and_second() {
    let (mut ir, base) = fixture();
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let nurbs = PcurveNurbs::from_lanes(&ctx, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(0.5, 0.0), Point2::new(1.0, 1.0)], None, false).unwrap().unwrap();
    ctx.finish_session().unwrap(); pcurve(&mut ir, PcurveGeometry::Nurbs { nurbs });
    let index = ModelIndex::build(&ir, StandardIndex);
    let second = model_requested_jet(EvaluationAdmission::Standard, &index, &base, 0.5, 0.5, SurfaceRequest::Second).unwrap();
    let third = model_requested_jet(EvaluationAdmission::Standard, &index, &base, 0.5, 0.5, SurfaceRequest::Third).unwrap();
    assert_eq!(third.jet.point, second.jet.point); assert_eq!(third.jet.first, second.jet.first);
    assert_eq!(third.jet.second, second.jet.second); assert_eq!(third.higher.third(), Err(EvaluationFailure::NoValue));
    let (s, c) = 0.5_f64.sin_cos();
    close(second.jet.second.unwrap(), [Vector3::new(0.0, 0.0, 0.0), Vector3::new(2.0 * s, 1.0 - 2.0 * c, -1.0),
        Vector3::new(-c, -s, 1.0)]);
}

#[test]
fn rounded_requested_orders_keep_actual_three_four_step_caps_and_original_fuse() {
    let (mut ir, base) = fixture(); let offset = shifted(&mut ir, "offset", base.clone(), 1.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    for (id, needed) in [(&base, 3), (&offset, 4)] {
        for cap in [needed - 1, needed] {
            let work = WorkBudget::new(cap);
            let actual = EvaluationAdmission::Standard.within_work_slice(&work, |mode| {
                model_jet(mode, &index, id, 0.5, 0.0, SurfaceRequest::Second)
            });
            if cap == needed { assert!(actual.unwrap().second.is_ok()); }
            else { assert!(matches!(actual, Err(EvaluationFailure::NoValue))); }
            assert_eq!(work.consumed(), cap);
        }
    }
    let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx.charge_work_limit(1, "actual prior rounded higher refusal").unwrap_err();
    for id in [&base, &offset] {
        for request in [SurfaceRequest::First, SurfaceRequest::Second, SurfaceRequest::Fifth] {
            assert!(matches!(model_jet(EvaluationAdmission::Decode(&ctx), &index, id, f64::NAN, f64::NAN, request),
                Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
        }
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn rounded_current_cache_selects_its_actual_complete_orders() {
    let (mut ir, base) = fixture();
    let SurfaceGeometry::Procedural { cache, .. } = &mut ir.model.surfaces[2].geometry else {
        panic!("rounded construction carrier");
    };
    *cache = Some(cylinder());
    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::VariableBlend(payload) = definition else { panic!("rounded fixture"); };
        let mut construction = payload.construction().to_raw();
        construction.cache = crate::geometry::VariableBlendCache::Current {
            shape_prefix: std::num::NonZeroI64::new(1).unwrap(),
            fit_tolerance: crate::geometry::FitTolerance::try_new(0.0).unwrap(),
        };
        *payload = VariableBlendSurfacePayload::try_new(Box::new(construction)).unwrap();
    });
    let index = ModelIndex::build(&ir, StandardIndex);
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let result = super::super::model::requested(admission, &index, &base, 0.0, 0.5, SurfaceRequest::Fifth).unwrap();
        let first = super::super::model::first_order(admission, &index, &base, 0.0, 0.5).unwrap();
        assert_eq!(result.jet.point, first.point); assert_eq!(result.jet.first, first.first);
        let (s, c) = 0.0_f64.sin_cos();
        let zero = Vector3::new(0.0, 0.0, 0.0);
        close(result.jet.second.unwrap(), [Vector3::new(-2.0 * c, -2.0 * s, 0.0), zero, zero]);
        close(result.higher.third().unwrap(), [Vector3::new(2.0 * s, -2.0 * c, 0.0), zero, zero, zero]);
        close(result.higher.fourth().unwrap(), [Vector3::new(2.0 * c, 2.0 * s, 0.0), zero, zero, zero, zero]);
        close(result.higher.fifth().unwrap(), [Vector3::new(-2.0 * s, 2.0 * c, 0.0), zero, zero, zero, zero, zero]);
        assert_eq!(result.jet.point.get(), Point3::new(2.0, 0.0, 0.5));
    }
    ctx.finish_session().unwrap();
}
