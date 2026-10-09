// SPDX-License-Identifier: Apache-2.0
//! Raw circular boundary rosters have no final trim ownership.

use crate::tessellation::{planar_trim, CircularHole, PlanarHole, PlanarOuter, PlanarTrim, TrimTopology};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, LoopBoundary, LoopRing, Point, Sense, Vertex};

fn with_fixture(
    circles: &[(f64, f64)],
    invalidate_last: bool,
    check: impl FnOnce(&Face, &SurfaceGeometry, &TrimTopology<'_>),
) {
    let mut model = Model::default();
    let face_id = FaceId::mint("synthetic:test:face#circle-workspace").unwrap();
    let mut loop_ids = Vec::new();
    let fixture_ctx = cadmpeg_test_support::service_decode_context();
    for (index, &(center_x, radius)) in circles.iter().enumerate() {
        let loop_id = LoopId::mint(format!("synthetic:test:loop#circle-{index}")).unwrap();
        let coedge_id = CoedgeId::mint(format!("synthetic:test:coedge#circle-{index}")).unwrap();
        let edge_id = EdgeId::mint(format!("synthetic:test:edge#circle-{index}")).unwrap();
        let curve_id = CurveId::mint(format!("synthetic:test:curve#circle-{index}")).unwrap();
        let vertex_id = VertexId::mint(format!("synthetic:test:vertex#circle-{index}")).unwrap();
        let point_id = PointId::mint(format!("synthetic:test:point#circle-{index}")).unwrap();
        model.points.push(Point::new(point_id.clone(),
            FinitePoint3::new(Point3::new(center_x + radius, 0., 0.)).unwrap(), None));
        model.vertices.push(Vertex { id: vertex_id.clone(), point: point_id, tolerance: None });
        let geometry = if invalidate_last && index + 1 == circles.len() {
            SolvedCurveGeometry::Line(cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(center_x, 0., 0.), Vector3::new(1., 0., 0.)).unwrap())
        } else {
            SolvedCurveGeometry::Circle(cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(center_x, 0., 0.), Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.), radius).unwrap())
        };
        model.curves.push(Curve { id: curve_id.clone(), geometry: CurveGeometry::Solved(geometry), source_object: None });
        model.edges.push(Edge { id: edge_id.clone(), carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
            start: vertex_id.clone(), end: vertex_id, tolerance: None });
        model.coedges.push(Coedge { id: coedge_id.clone(), owner_loop: loop_id.clone(), edge: edge_id,
            radial_next: coedge_id.clone(), sense: Sense::Forward, pcurves: Vec::new(), use_curve: None });
        model.loops.push(Loop { id: loop_id.clone(), face: face_id.clone(),
            boundary: LoopBoundary::Ring(LoopRing::new(&fixture_ctx, vec![coedge_id], Vec::new()).unwrap().unwrap()) });
        loop_ids.push(loop_id);
    }
    let face = Face { id: face_id, shell: ShellId::mint("synthetic:test:shell#circle-workspace").unwrap(),
        surface: SurfaceId::mint("synthetic:test:surface#circle-workspace").unwrap(), sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(loop_ids), name: None, color: None, tolerance: None };
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.), Vector3::new(1., 0., 0.)).unwrap()));
    // Source fixtures and topology indexes precede the measured caller session.
    let loops = model.loops.iter().map(|value| (&value.id, value)).collect();
    let coedges = model.coedges.iter().map(|value| (&value.id, value)).collect();
    let edges = model.edges.iter().map(|value| (&value.id, value)).collect();
    let vertices = model.vertices.iter().map(|value| (&value.id, value)).collect();
    let points = model.points.iter().map(|value| (&value.id, value.position().get())).collect();
    let curves = model.curves.iter().map(|value| (&value.id, &value.geometry)).collect();
    check(&face, &surface, &TrimTopology { loops: &loops, coedges: &coedges, edges: &edges,
        vertices: &vertices, points: &points, curves: &curves, coordinate_scale: 16. });
}

fn raw_circle_bytes() -> u64 {
    u64::try_from(4 * std::mem::size_of::<CircularHole>()).unwrap()
}

fn final_hole_bytes() -> u64 {
    u64::try_from(4 * std::mem::size_of::<PlanarHole>()).unwrap()
}

fn policy(materialized: u64, retained: u64, items: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = materialized;
    policy.limits.max_retained_bytes = retained;
    policy.limits.max_collection_items = items;
    policy
}

fn assert_trim(trim: &PlanarTrim, hole_count: usize) {
    let Some(PlanarOuter::Circle(outer)) = &trim.outer else { panic!("circular outer"); };
    assert_eq!(outer.center, cadmpeg_ir::math::Point2::new(0., 0.));
    assert_eq!(outer.radius, 10.);
    assert_eq!(trim.holes.len(), hole_count);
    if hole_count == 2 {
        let [PlanarHole::Circle(first), PlanarHole::Circle(second)] = trim.holes.as_slice()
        else { panic!("circular holes"); };
        assert_eq!(first.center, cadmpeg_ir::math::Point2::new(6., 0.));
        assert_eq!(second.center, cadmpeg_ir::math::Point2::new(-6., 0.));
        assert_eq!((first.radius, second.radius), (2., 2.));
    }
}

#[test]
fn circular_outer_uses_no_retained_roster_and_releases_its_backing() {
    with_fixture(&[(0., 10.)], false, |face, surface, topology| {
        let arena = DecodeArena::new();
        let bytes = raw_circle_bytes();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(bytes, 0, 1)).unwrap();
        let trim = planar_trim(&ctx, face, surface, topology).unwrap().unwrap();
        assert_trim(&trim, 0);
        drop(ctx.reserve_scoped(bytes, "test raw outer circle expiry").unwrap());
        ctx.finish_session().unwrap();
    });
}

#[test]
fn circular_outer_refuses_raw_roster_before_any_allocation() {
    with_fixture(&[(0., 10.)], false, |face, surface, topology| {
        let arena = DecodeArena::new();
        let bytes = raw_circle_bytes();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(bytes - 1, 0, 1)).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| planar_trim(&ctx, face, surface, topology));
        let Err(CodecError::ResourceLimit(limit)) = result else { panic!("raw roster refusal"); };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(limit.operation, "collect SLDPRT planar trim circles");
        assert_eq!((limit.used, limit.additional, limit.limit), (0, bytes, bytes - 1));
        assert_eq!(allocations, 0);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    });
}

#[test]
fn circular_holes_retain_only_final_hole_slots_at_exact_scoped_peak() {
    with_fixture(&[(0., 10.), (6., 2.), (-6., 2.)], false, |face, surface, topology| {
        let arena = DecodeArena::new();
        let peak = 2 * raw_circle_bytes();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(peak, final_hole_bytes(), 7)).unwrap();
        let trim = planar_trim(&ctx, face, surface, topology).unwrap().unwrap();
        assert_trim(&trim, 2);
        drop(ctx.reserve_scoped(peak, "test both raw circle rosters expire").unwrap());
        ctx.finish_session().unwrap();
    });
}

#[test]
fn circular_holes_refuse_intermediate_roster_one_below_overlap() {
    with_fixture(&[(0., 10.), (6., 2.), (-6., 2.)], false, |face, surface, topology| {
        let arena = DecodeArena::new();
        let bytes = raw_circle_bytes();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(2 * bytes - 1, final_hole_bytes(), 7)).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| planar_trim(&ctx, face, surface, topology));
        let Err(CodecError::ResourceLimit(limit)) = result else { panic!("intermediate roster refusal"); };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(limit.operation, "collect SLDPRT circular trim holes");
        assert_eq!((limit.used, limit.additional, limit.limit), (bytes, bytes, 2 * bytes - 1));
        assert_eq!(allocations, 1);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    });
}

#[test]
fn circular_holes_refuse_final_retained_slots_before_output_allocation() {
    with_fixture(&[(0., 10.), (6., 2.), (-6., 2.)], false, |face, surface, topology| {
        let arena = DecodeArena::new();
        let need = final_hole_bytes();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(2 * raw_circle_bytes(), need - 1, 7)).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| planar_trim(&ctx, face, surface, topology));
        let Err(CodecError::ResourceLimit(limit)) = result else { panic!("final hole slots refusal"); };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, "collect SLDPRT circular planar holes");
        assert_eq!((limit.used, limit.additional, limit.limit), (0, need, need - 1));
        assert_eq!(allocations, 2);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    });
}

#[test]
fn invalid_last_circle_releases_the_actual_prefix() {
    with_fixture(&[(0., 10.), (6., 2.), (-6., 2.)], true, |face, surface, topology| {
        let arena = DecodeArena::new();
        let bytes = raw_circle_bytes();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(bytes, 0, 2)).unwrap();
        assert!(planar_trim(&ctx, face, surface, topology).unwrap().is_none());
        drop(ctx.reserve_scoped(bytes, "test rejected circle prefix expiry").unwrap());
        ctx.finish_session().unwrap();
    });
}

#[test]
fn overlapping_circular_holes_release_both_raw_rosters() {
    with_fixture(&[(0., 10.), (0., 2.), (1., 2.)], false, |face, surface, topology| {
        let arena = DecodeArena::new();
        let peak = 2 * raw_circle_bytes();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(peak, 0, 5)).unwrap();
        assert!(planar_trim(&ctx, face, surface, topology).unwrap().is_none());
        drop(ctx.reserve_scoped(peak, "test overlapping circle roster expiry").unwrap());
        ctx.finish_session().unwrap();
    });
}

#[test]
fn circular_trim_preserves_original_refusal_before_allocation() {
    with_fixture(&[(0., 10.)], false, |face, surface, topology| {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(raw_circle_bytes(), 0, 1)).unwrap();
        let Err(CodecError::ResourceLimit(first)) = ctx.charge_collection_items(2, "test original circle refusal")
        else { panic!("original refusal"); };
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| planar_trim(&ctx, face, surface, topology));
        assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == first));
        assert_eq!(allocations, 0);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == first));
    });
}
