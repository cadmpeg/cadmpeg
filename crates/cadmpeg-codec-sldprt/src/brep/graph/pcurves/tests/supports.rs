// SPDX-License-Identifier: Apache-2.0

use super::super::{CoedgeSupport, PcurveArenas, PcurveSupports};
use crate::brep::graph::Brep;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense, Vertex};

fn support_fixture() -> Brep {
    let surface = SurfaceId::mint("test:model:surface#supports").unwrap();
    let face = FaceId::mint("test:model:face#supports").unwrap();
    let lp = LoopId::mint("test:model:loop#supports").unwrap();
    let curve = CurveId::mint("test:model:curve#supports").unwrap();
    let edge = EdgeId::mint("test:model:edge#supports").unwrap();
    let vertex = VertexId::mint("test:model:vertex#supports").unwrap();
    let point = PointId::mint("test:model:point#supports").unwrap();
    let coedge = CoedgeId::mint("test:model:coedge#supports").unwrap();
    Brep {
        surfaces: vec![Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(Point3::new(0., 0., 0.), Vector3::new(0., 0., 1.), Vector3::new(1., 0., 0.)).unwrap(),
            )),
            source_object: None,
        }],
        faces: vec![Face {
            id: face.clone(),
            shell: ShellId::mint("test:model:shell#supports").unwrap(),
            surface,
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(vec![lp.clone()]),
            name: None, color: None, tolerance: None,
        }],
        loops: vec![Loop { id: lp.clone(), face, boundary: LoopBoundary::Ring(LoopRing::single(coedge.clone())) }],
        curves: vec![Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::try_new(Point3::new(0., 0., 0.), Vector3::new(1., 0., 0.)).unwrap())),
            source_object: None,
        }],
        edges: vec![Edge { id: edge.clone(), carrier: EdgeCarrier::new(Some(curve), Some([0., 1.])).unwrap(), start: vertex.clone(), end: vertex.clone(), tolerance: None }],
        coedges: vec![Coedge { id: coedge.clone(), owner_loop: lp, edge, radial_next: coedge, sense: Sense::Forward, use_curve: None, pcurves: Vec::new() }],
        vertices: vec![Vertex { id: vertex, point: point.clone(), tolerance: None }],
        points: vec![Point::new(point, Point3::new(2., 0., 0.).try_into().unwrap(), None)],
        ..Brep::default()
    }
}

fn arenas(out: &Brep) -> PcurveArenas<'_> {
    PcurveArenas { loops: &out.loops, faces: &out.faces, surfaces: &out.surfaces, edges: &out.edges, curves: &out.curves, vertices: &out.vertices, points: &out.points }
}

fn one_entry_map_bytes<T>() -> u64 {
    // Core's one-entry hash growth uses four buckets, alignment padding,
    // four bucket controls, and sixteen trailing controls.
    u64::try_from(4 * std::mem::size_of::<T>() + std::mem::align_of::<T>().max(16) - 1 + 4 + 16).unwrap()
}

fn support_live_bytes() -> u64 {
    // Core's first Vec slot has capacity4 for this three-reference value.
    u64::try_from(4 * std::mem::size_of::<Option<CoedgeSupport<'_>>>()).unwrap()
        + one_entry_map_bytes::<(&VertexId, Point3)>()
}

fn support_peak() -> u64 {
    support_live_bytes()
        + one_entry_map_bytes::<(&SurfaceId, &Surface)>()
        + one_entry_map_bytes::<(&FaceId, &Face)>()
        + one_entry_map_bytes::<(&LoopId, &Surface)>()
        + one_entry_map_bytes::<(&EdgeId, &Edge)>()
        + one_entry_map_bytes::<(&CurveId, &Curve)>()
        + one_entry_map_bytes::<(&PointId, Point3)>()
}

fn support_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = support_peak();
    policy.limits.max_retained_bytes = 0;
    policy
}

#[test]
fn pcurve_supports_keep_only_the_output_vector_and_vertex_map_live() {
    let out = support_fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &support_policy()).unwrap();
    let supports = PcurveSupports::new(&ctx, arenas(&out), &out.coedges).unwrap();
    let support = supports.coedges[0].as_ref().unwrap();
    assert!(std::ptr::eq(support.surface, &out.surfaces[0]));
    assert!(std::ptr::eq(support.edge, &out.edges[0]));
    assert!(std::ptr::eq(support.curve, &out.curves[0]));
    assert_eq!(supports.position(&ctx, &out.vertices[0].id).unwrap(), Some(Point3::new(2., 0., 0.)));
    drop(ctx.reserve_scoped(support_peak() - support_live_bytes(), "test support freed build maps").unwrap());
    let additional = support_peak() - support_live_bytes() + 1;
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(additional, "test support live backing") else { panic!("live support backing"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((limit.used, limit.additional, limit.limit), (support_live_bytes(), additional, support_peak()));
    drop(supports);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn pcurve_support_drop_releases_all_backing_with_zero_retention() {
    let out = support_fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &support_policy()).unwrap();
    drop(PcurveSupports::new(&ctx, arenas(&out), &out.coedges).unwrap());
    drop(ctx.reserve_scoped(support_peak(), "test dropped support backing").unwrap());
    assert_eq!(out.coedges[0].sense, Sense::Forward);
    assert!(out.coedges[0].pcurves.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn pcurve_support_build_refuses_before_its_first_allocation() {
    let out = support_fixture();
    let arena = DecodeArena::new();
    let mut policy = support_policy();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| PcurveSupports::new(&ctx, arenas(&out), &out.coedges));
    let Err(CodecError::ResourceLimit(limit)) = &result else { panic!("first surface map"); };
    let limit = *limit;
    assert_eq!(limit.operation, "index Parasolid pcurve surfaces");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((limit.used, limit.additional, limit.limit), (0, one_entry_map_bytes::<(&SurfaceId, &Surface)>(), 0));
    drop(result);
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn pcurve_vertex_support_refuses_at_the_source_derived_peak() {
    let out = support_fixture();
    let arena = DecodeArena::new();
    let mut policy = support_policy();
    policy.limits.max_materialized_bytes -= 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = PcurveSupports::new(&ctx, arenas(&out), &out.coedges);
    let Err(CodecError::ResourceLimit(limit)) = &result else { panic!("vertex support peak"); };
    let limit = *limit;
    let vertex_map = one_entry_map_bytes::<(&VertexId, Point3)>();
    assert_eq!(limit.operation, "index Parasolid pcurve vertex points");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((limit.used, limit.additional, limit.limit), (support_peak() - vertex_map, vertex_map, support_peak() - 1));
    drop(result);
    assert!(out.coedges[0].pcurves.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn pcurve_support_lookup_preserves_prior_refusal_without_allocation() {
    let out = support_fixture();
    let arena = DecodeArena::new();
    let mut policy = support_policy();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let supports = PcurveSupports::new(&ctx, arenas(&out), &out.coedges).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "test original support refusal", None);
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original support refusal") else { panic!("original refusal"); };
    drop(probe);
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| supports.position(&ctx, &out.vertices[0].id));
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert_eq!(allocations, 0);
    drop(supports);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
