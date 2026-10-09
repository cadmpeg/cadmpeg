// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::tests::law_sweep::law_sweep_model;
use crate::eval::tests::variable_blend::{constant_rolling_ball_fixture, variable_blend_eval_fixture};
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::{FitTolerance, LawExpression, RevisionCacheForm, VariableBlendCache,
    VariableBlendCrossSection};
use crate::math::Point2;
use cadmpeg_core::decode::{ResourceDimension, WorkBudget};

fn fixture(case: usize, current: bool, width: f64, x: f64) -> (CadIr, SurfaceId, SurfaceId) {
    let (mut ir, base) = match case {
        0 => law_sweep_model(LawExpression::Double { value: 1.0 }),
        1 => variable_blend_eval_fixture(Point3::new(0.0, 0.0, 0.0),
            [(Point2::new(3.0, 0.0), Point2::new(0.0, 1.0)),
                (Point2::new(0.0, 3.0), Point2::new(1.0, 0.0))],
            [3.0, 3.0], Some(VariableBlendCrossSection::G2Round { parameters: [1.0, 1.0] })),
        2 => constant_rolling_ball_fixture(CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(3.0, 0.0, 3.0), Vector3::new(0.0, 1.0, 0.0)).unwrap()))),
        _ => unreachable!("three native current-cache fallback owners"),
    };
    // Missing construction curves force the genuine non-resource failure route.
    // Current-cache forms reject construction evaluation before these reads.
    ir.model.curves.clear();
    if current {
        ir.model.procedural_surfaces[0].edit_definition(|definition| match definition {
            ProceduralSurfaceDefinition::Sweep(payload) => {
                let mut raw = payload.native().as_deref().unwrap().to_raw();
                raw.cache.form_mut().unwrap().cache = RevisionCacheForm::SolvedCache { fit_tolerance: FitTolerance::try_new(0.0).unwrap() };
                *payload = crate::geometry::surface_payloads::SweepSurfacePayload::try_new(
                    payload.profile().clone(), payload.spine().clone(), Some(Box::new(raw))).unwrap();
            }
            ProceduralSurfaceDefinition::VariableBlend(payload) => {
                let mut raw = payload.construction().to_raw();
                raw.cache = VariableBlendCache::Current {
                    shape_prefix: std::num::NonZeroI64::new(1).unwrap(),
                    fit_tolerance: FitTolerance::try_new(0.0).unwrap(),
                };
                *payload = crate::geometry::surface_payloads::VariableBlendSurfacePayload::try_new(Box::new(raw)).unwrap();
            }
            ProceduralSurfaceDefinition::Blend(payload) => {
                let mut raw = payload.native().unwrap().to_raw();
                raw.cache = RevisionCacheForm::SolvedCache { fit_tolerance: FitTolerance::try_new(0.0).unwrap() };
                *payload = crate::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    payload.supports().clone(), payload.spine().clone(), payload.radius().clone(),
                    payload.cross_section().clone(), crate::geometry::CacheContract::from_form(Some(Box::new(raw)))).unwrap();
            }
            _ => unreachable!("fixture carries the selected native construction"),
        });
    }
    let cache = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, width, width], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(x, 0.0, 0.0), Point3::new(x, 1.0, 0.0)]], None), false).unwrap().unwrap();
    let SurfaceGeometry::Procedural { cache: selected, .. } = &mut ir.model.surfaces
        .iter_mut().find(|surface| surface.id == base).unwrap().geometry else { unreachable!() };
    *selected = Some(SolvedSurfaceGeometry::Nurbs(cache));
    let offset = SurfaceId::mint("test:model:cache-fallback#offset").unwrap();
    ir.model.surfaces.push(Surface { id: offset.clone(), geometry: SurfaceGeometry::Solved(
        SolvedSurfaceGeometry::Unknown { record: None }), source_object: None });
    ir.model.add_procedural_surface(&StandardAdmission, &offset, ProceduralSurface::new(
        ProceduralSurfaceId::mint("test:model:cache-fallback#offset-construction").unwrap(),
        ProceduralSurfaceDefinition::Offset(OffsetSurfaceConstruction::try_new(base.clone(), 1.0,
            None, None, false, OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent {}, cache: None }).unwrap()),
        None)).unwrap().unwrap();
    (ir, base, offset)
}

fn current_point(case: usize) {
    let (ir, base, offset) = fixture(case, true, 1.0, 1.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    let expected = FinitePoint3::new(Point3::new(0.25, 0.5, 0.0)).unwrap();
    let work = WorkBudget::new(13);
    // One model step plus the existing 2*2 + 2*2 + 2*2 point cost.
    let actual = admission::EvaluationAdmission::Standard.within_work_slice(&work, |admission|
        crate::eval::model_surface_point_by_id(admission, &index, &base, 0.25, 0.5));
    assert_eq!(actual, Ok(expected)); assert_eq!(work.consumed(), 13);
    let short = WorkBudget::new(12);
    assert_eq!(admission::EvaluationAdmission::Standard.within_work_slice(&short, |admission|
        crate::eval::model_surface_point_by_id(admission, &index, &base, 0.25, 0.5)), Err(EvaluationFailure::NoValue));
    let path_slot = u64::try_from(std::mem::size_of::<Option<ModelEvaluationIdentity>>()).unwrap();
    let mut policy = DecodePolicy::service(); policy.limits.max_materialized_bytes = path_slot;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
        &index, &base, 0.25, 0.5), Ok(expected));
    ctx.finish_session().unwrap();
    // An actual offset reader still obtains the cached first-order normal.
    assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
        &index, &offset, 0.25, 0.5).unwrap().get(), Point3::new(0.25, 0.5, 1.0));
    // Root path holds one slot; nested support holds two until it returns.
    policy.limits.max_materialized_bytes = 3 * path_slot;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(EvaluationFailure::ResourceLimit(original)) = crate::eval::model_surface_point_by_id(
        admission::EvaluationAdmission::Decode(&ctx), &index, &offset, 0.25, 0.5) else {
        panic!("requested normal must reserve its derivative basis");
    };
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(original.operation, "IR B-spline derivative basis");
    assert_eq!(original.limit, 3 * path_slot); assert_eq!(original.used, 3 * path_slot);
    // The existing amortized f64 Vec reserves its minimum four elements.
    assert_eq!(original.additional, u64::try_from(4 * std::mem::size_of::<f64>()).unwrap());
    assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
        &index, &base, f64::NAN, f64::NAN), Err(EvaluationFailure::ResourceLimit(original)));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn current_sweep_cache_point_does_not_request_an_unused_normal() { current_point(0); }
#[test]
fn current_variable_blend_cache_point_does_not_request_an_unused_normal() { current_point(1); }
#[test]
fn current_rolling_ball_cache_point_does_not_request_an_unused_normal() { current_point(2); }

#[test]
fn retained_cache_rows_do_not_override_native_currentness() {
    for case in 0..3 {
        let (ir, base, _) = fixture(case, false, 1.0, 1.0);
        let index = ModelIndex::build(&ir, StandardIndex);
        assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
            &index, &base, 0.25, 0.5), Err(EvaluationFailure::NoValue));
    }
}

#[test]
fn current_cache_point_survives_a_genuinely_nonfinite_requested_normal() {
    for case in 0..3 {
        let (ir, base, offset) = fixture(case, true, 0.5, f64::MAX);
        let index = ModelIndex::build(&ir, StandardIndex);
        // C_u = MAX/0.5 overflows, while C(0,v)=(0,v,0) is finite.
        assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
            &index, &base, 0.0, 0.5).unwrap().get(), Point3::new(0.0, 0.5, 0.0));
        assert!(matches!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
            &index, &offset, 0.0, 0.5), Err(EvaluationFailure::NonFinite(_))));
    }
}
