// SPDX-License-Identifier: Apache-2.0
use super::*;
use super::super::super::{linear_boundary_geometry, BoundaryItem, BoundarySegment,
    BoundarySurfaceKind, LinearBoundaryGeometry};
use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles;
use cadmpeg_ir::units::FinitePoint2;
use cadmpeg_ir::topology::EdgeCarrier;

const ITEMS: usize = 64;
const CAP: u64 = 65536;

fn fixture(segment: &BoundarySegment) -> (CadIr, SurfaceGeometry, Vec<BoundaryItem<'_>>) {
    let id = CurveId::mint("test:model:curve#linear").unwrap();
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0),
        ).unwrap())),
        source_object: None,
    });
    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(
        Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    ).unwrap()));
    let items = (0..ITEMS).map(|i| {
        let start = f64::from(u32::try_from(i).unwrap());
        let end = start + 1.0;
        let nurbs = crate::test_support::with_service_context(&[], |setup| {
            PcurveNurbs::new(setup, 1, vec![0.0, 0.0, 1.0, 1.0],
                PcurveNurbsPoles::Polynomial {
                    points: [Point2::new(start, 0.0), Point2::new(end, 0.0)]
                        .map(|point| FinitePoint2::new(point).unwrap()).to_vec(),
                }, false).unwrap().unwrap()
        });
        BoundaryItem {
            segment, model_curve: id.clone(),
            source_edge: Edge {
                id: EdgeId::mint("test:model:edge#linear").unwrap(),
                carrier: EdgeCarrier::new(Some(id.clone()), Some([0.0, 1.0])).unwrap(),
                start: VertexId::mint("test:model:vertex#start").unwrap(),
                end: VertexId::mint("test:model:vertex#end").unwrap(), tolerance: None,
            },
            start: FinitePoint3::new(Point3::new(start, 0.0, 0.0)).unwrap(),
            end: FinitePoint3::new(Point3::new(end, 0.0, 0.0)).unwrap(),
            pcurves: vec![(PcurveGeometry::Nurbs { nurbs }, [0.0, 1.0])],
        }
    }).collect();
    (ir, plane, items)
}

fn segment(authoritative: bool) -> BoundarySegment {
    BoundarySegment { model_curve: 1, pcurves: vec![3], sense: Sense::Forward,
        parameter_curves_authoritative: authoritative }
}

fn assert_output_custody(parameter: bool, empty: bool) {
    let segment = segment(parameter);
    let (ir, plane, mut items) = fixture(&segment);
    if empty { items.clear(); }
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    // Both aggregates use core's four-slot minimum and doubling growth.
    // 64 joined two-point segments produce 65 values in 128 slots.
    let live = if empty { 0 } else { u64_from_index(128 * size_of::<[f64; 2]>()) };
    for refuse_extra in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = CAP;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut outer = ctx.reserve_scoped(0, "test linear boundary ambient storage").unwrap();
        let candidate = outer.with_storage(|| linear_boundary_geometry(
            &items, &index, &plane, 0.0, 0.0, BoundarySurfaceKind::Trimmed, &ctx,
        )).unwrap().unwrap();
        let points = match &candidate.geometry {
            LinearBoundaryGeometry::Parameter(points) => { assert!(parameter && !empty); points }
            LinearBoundaryGeometry::Model(points) => { assert!(!parameter || empty); points }
        };
        assert_eq!(points.len(), if empty { 0 } else { ITEMS + 1 });
        for (i, point) in points.iter().enumerate() {
            let x = f64::from(u32::try_from(i).unwrap());
            // A +Z plane chooses +Y as u and -X as v. Parameter poles use +X.
            assert_eq!(*point, if parameter { [x, 0.0] } else { [0.0, -x] });
        }
        let free = ctx.reserve_scoped(CAP - live, "test exact surviving boundary geometry").unwrap();
        drop(free);
        if refuse_extra {
            let first = match ctx.reserve_scoped(CAP - live + 1, "test linear boundary backing remains live").err().unwrap() {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected original materialized refusal"),
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "test linear boundary backing remains live");
            assert_eq!((first.limit, first.used, first.additional), (CAP, live, CAP - live + 1));
            drop(candidate); drop(outer);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            drop(candidate);
            let free = ctx.reserve_scoped(CAP, "test linear boundary backing destroyed").unwrap();
            drop(free); drop(outer);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn model_boundary_keeps_only_surviving_projected_coordinates() {
    assert_output_custody(false, false);
}
#[test]
fn parameter_boundary_releases_unused_model_and_per_curve_points() {
    assert_output_custody(true, false);
}
#[test]
fn empty_linear_boundary_retains_no_output_storage() {
    assert_output_custody(false, true);
}

#[test]
fn absent_last_linear_boundary_curve_releases_partial_aggregate_per_attempt() {
    let segment = segment(false);
    let (ir, plane, mut items) = fixture(&segment);
    items[ITEMS - 1].model_curve = CurveId::mint("test:model:curve#absent").unwrap();
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = CAP;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut outer = ctx.reserve_scoped(0, "test partial boundary ambient storage").unwrap();
    for _ in 0..16 {
        assert!(outer.with_storage(|| linear_boundary_geometry(
            &items, &index, &plane, 0.0, 0.0, BoundarySurfaceKind::Trimmed, &ctx,
        )).unwrap().is_none());
        let free = ctx.reserve_scoped(CAP, "test partial boundary backing destroyed").unwrap();
        drop(free);
    }
    drop(outer);
    ctx.finish_session().unwrap();
}

#[test]
fn linear_boundary_first_point_allocation_keeps_original_refusal() {
    let segment = segment(false);
    let (ir, plane, items) = fixture(&segment);
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let allocation = u64_from_index(2 * size_of::<Point3>());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = allocation - 1;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = match linear_boundary_geometry(
        &items, &index, &plane, 0.0, 0.0, BoundarySurfaceKind::Trimmed, &ctx,
    ).err().expect("expected actual first point allocation refusal") {
        CodecError::ResourceLimit(first) => first,
        _ => panic!("expected original resource refusal"),
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "iges linear model boundary line");
    assert_eq!((first.limit, first.used, first.additional), (allocation - 1, 0, allocation));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn unsupported_linear_boundary_preserves_original_refusal_and_fresh_absence() {
    let ir = CadIr::empty();
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let unsupported = SurfaceGeometry::Procedural {
        construction: cadmpeg_ir::ids::ProceduralSurfaceId::mint("test:model:procedural-surface#unsupported").unwrap(),
        cache: None,
    };
    for fused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if fused {
            let first = match ctx.charge_work(1, "test original unsupported boundary refusal").unwrap_err() {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected original resource refusal"),
            };
            assert!(matches!(linear_boundary_geometry(
                &[], &index, &unsupported, 0.0, 0.0, BoundarySurfaceKind::Trimmed, &ctx,
            ), Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            assert!(linear_boundary_geometry(
                &[], &index, &unsupported, 0.0, 0.0, BoundarySurfaceKind::Trimmed, &ctx,
            ).unwrap().is_none());
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn empty_linear_boundary_rings_preserve_original_refusal_and_zero_work_absence() {
    for fused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if fused {
            let first = match ctx.charge_work(1, "test original empty ring refusal").unwrap_err() {
                CodecError::ResourceLimit(first) => first,
                _ => panic!("expected original resource refusal"),
            };
            assert!(matches!(linear_boundary_rings(
                std::iter::empty(), BoundarySpace::Model, &ctx,
            ), Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            assert!(linear_boundary_rings(std::iter::empty(), BoundarySpace::Model, &ctx).unwrap().is_none());
            ctx.finish_session().unwrap();
        }
    }
}
