// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::geometry::{Surface, SurfaceGeometry, SolvedSurfaceGeometry};
use crate::geometry::pcurve::{LinePcurve, PcurveGeometry};
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::FiniteReal;

fn fixture() -> (crate::CadIr, crate::ids::SurfaceId, PcurveGeometry) {
    let id: crate::ids::SurfaceId = "test:model:surface#inverse-admission".try_into().unwrap();
    let mut ir = crate::CadIr::empty();
    ir.model.surfaces.push(Surface { id: id.clone(), source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            crate::geometry::analytic::PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        )),
    });
    (ir, id, PcurveGeometry::Line(LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap()))
}

fn refuses(cap: u64, operation: &str) {
    let (ir, id, geometry) = fixture();
    let index = crate::index::ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
    let context = super::super::SurfacePcurveContext { index: &index, surface_id: &id, geometry: &ir.model.surfaces[0].geometry };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let limit = super::super::mapped_pcurve_parameter_near_point(&ctx, &context, &geometry, Point3::new(1.0, 0.0, 0.0), FiniteReal::ZERO, 0.0).expect_err("iteration must refuse");
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, operation);
    assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn mapped_pcurve_newton_work_preserves_first_and_later_original_refusals() {
    // The domain visit costs one unit. The first Newton visit and the accepted
    // candidate comparison cost one each before the second Newton visit.
    refuses(1, "mapped pcurve Newton iteration");
    refuses(3, "mapped pcurve Newton iteration");
}

#[test]
fn mapped_pcurve_backtracking_work_refuses_before_candidate_comparison() {
    refuses(2, "mapped pcurve backtracking comparison");
}

#[test]
fn mapped_pcurve_inverse_admits_exact_linear_step_without_storage() {
    let (ir, id, geometry) = fixture();
    let index = crate::index::ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
    let context = super::super::SurfacePcurveContext { index: &index, surface_id: &id, geometry: &ir.model.surfaces[0].geometry };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(super::super::mapped_pcurve_parameter_near_point(&ctx, &context, &geometry, Point3::new(1.0, 0.0, 0.0), FiniteReal::ZERO, 0.0).unwrap(), Some(FiniteReal::ONE));
    ctx.finish_session().unwrap();
}

#[test]
fn geometric_checks_admit_model_index_with_live_context() {
    let (ir, _, _) = fixture();
    for route in 0..2 {
        for trigger in 0..3 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            let dimension = match trigger {
                0 => { policy.limits.max_materialized_bytes = 0; ResourceDimension::MaterializedBytes }
                1 => { policy.limits.max_collection_items = 0; ResourceDimension::CollectionItems }
                _ => { policy.limits.max_work_units = 0; ResourceDimension::WorkUnits }
            };
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            let result = if route == 0 {
                super::super::check_procedural_support_consistency(&ctx, &ir, &mut findings)
            } else {
                super::super::check_pcurve_surface_consistency(&ctx, &ir, &mut findings)
            };
            let cadmpeg_core::CodecError::ResourceLimit(first) = result.unwrap_err() else {
                panic!("index refusal must remain a resource limit");
            };
            assert_eq!(first.dimension, dimension);
            assert_eq!((first.limit, first.used), (0, 0));
            assert!(first.additional > 0);
            assert!(findings.is_empty());
            assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == first));
        }
    }
}
