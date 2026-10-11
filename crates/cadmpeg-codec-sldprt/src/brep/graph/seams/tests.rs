// SPDX-License-Identifier: Apache-2.0
use super::{synthesize_cylinder_seams, synthesize_sphere_seams};
use crate::brep::graph::Brep;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, PlaneSurface, SphereSurface};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense, Vertex,
};

fn stream() -> StreamHandle {
    StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::stream_name!("test"),
        "fixture stream",
    )
    .unwrap()
}

fn cylinder(height: f64) -> Brep {
    let surface = SurfaceId::mint("test:model:surface#cylinder").unwrap();
    let face = FaceId::mint("test:model:face#cylinder").unwrap();
    let mut out = Brep::default();
    out.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::new(0., 0., 0.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
                1.,
            )
            .unwrap(),
        )),
    });
    let mut loops = Vec::new();
    for (key, z) in [("a", 0.0), ("b", height)] {
        let curve = CurveId::mint(format!("test:model:curve#{key}")).unwrap();
        let edge = EdgeId::mint(format!("test:model:edge#{key}")).unwrap();
        let vertex = VertexId::mint(format!("test:model:vertex#{key}")).unwrap();
        let point = PointId::mint(format!("test:model:point#{key}")).unwrap();
        let coedge = CoedgeId::mint(format!("test:model:coedge#{key}")).unwrap();
        let lp = LoopId::mint(format!("test:model:loop#{key}")).unwrap();
        out.curves.push(Curve {
            id: curve.clone(),
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(0., 0., z),
                    Vector3::new(0., 0., 1.),
                    Vector3::new(1., 0., 0.),
                    1.,
                )
                .unwrap(),
            )),
        });
        out.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::unbounded(Some(curve)),
            start: vertex.clone(),
            end: vertex.clone(),
            tolerance: None,
        });
        out.vertices.push(Vertex {
            id: vertex,
            point: point.clone(),
            tolerance: None,
        });
        out.points.push(Point::new(
            point,
            Point3::new(0., 0., z).try_into().unwrap(),
            None,
        ));
        out.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: lp.clone(),
            edge,
            radial_next: coedge.clone(),
            sense: Sense::Forward,
            use_curve: None,
            pcurves: Vec::new(),
        });
        out.loops.push(Loop {
            id: lp.clone(),
            face: face.clone(),
            boundary: LoopBoundary::Ring(LoopRing::single(coedge)),
        });
        loops.push(lp);
    }
    out.faces.push(Face {
        id: face,
        shell: ShellId::mint("test:model:shell#cylinder").unwrap(),
        surface,
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(loops),
        name: None,
        color: None,
        tolerance: None,
    });
    out
}

fn sphere() -> Brep {
    let mut out = cylinder(0.0);
    out.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
            1.,
        )
        .unwrap(),
    ));
    let coedge = CoedgeId::mint("test:model:coedge#c").unwrap();
    let mut third = out.coedges[0].clone();
    third.id = coedge.clone();
    third.radial_next = coedge;
    out.coedges.push(third);
    out.loops[0].boundary = LoopBoundary::Ring(
        LoopRing::new(
            &cadmpeg_test_support::service_decode_context(),
            out.coedges.iter().map(|coedge| coedge.id.clone()).collect(),
            Vec::new(),
        )
        .unwrap()
        .unwrap(),
    );
    for coedge in &mut out.coedges {
        coedge.owner_loop = out.loops[0].id.clone();
    }
    out.loops.truncate(1);
    out.faces[0].loops = FaceLoops::unspecified(vec![out.loops[0].id.clone()]);
    out
}

const SCRATCH_BYTES: u64 = 64 * 1024;

#[test]
fn rejected_cylinder_candidate_releases_identity_children_and_keeps_point_updates() {
    let mut out = cylinder(0.0);
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    synthesize_cylinder_seams(&ctx, &mut out, &mut AnnotationBuilder::new(), &stream()).unwrap();
    assert_eq!(out.edges.len(), 2);
    assert_eq!(out.loops.len(), 2);
    assert_eq!(out.faces[0].loops.len(), 2);
    assert_eq!(out.points[0].position().get(), Point3::new(-1., 0., 0.));
    assert_eq!(out.points[1].position().get(), Point3::new(-1., 0., 0.));
    drop(
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test released cylinder candidate",
        )
        .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn analytic_faces_do_not_allocate_geometry_specific_seam_maps() {
    let mut out = cylinder(1.0);
    out.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
        )
        .unwrap(),
    ));
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let stream = stream();
    let mut annotations = AnnotationBuilder::new();
    synthesize_cylinder_seams(&ctx, &mut out, &mut annotations, &stream).unwrap();
    synthesize_sphere_seams(&ctx, &mut out, &mut annotations, &stream).unwrap();
    assert_eq!(out.coedges.len(), 2);
    assert_eq!(out.edges.len(), 2);
    assert_eq!(out.loops.len(), 2);
    ctx.finish_session().unwrap();
}

#[test]
fn seam_candidate_storage_expires_after_publication() {
    for spherical in [false, true] {
        let mut out = if spherical { sphere() } else { cylinder(1.0) };
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = SCRATCH_BYTES;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if spherical {
            synthesize_sphere_seams(&ctx, &mut out, &mut AnnotationBuilder::new(), &stream())
                .unwrap();
        } else {
            synthesize_cylinder_seams(&ctx, &mut out, &mut AnnotationBuilder::new(), &stream())
                .unwrap();
        }
        assert_eq!(out.faces[0].loops.len(), 1);
        assert_eq!(out.loops[0].coedges().len(), 4);
        assert_eq!(out.coedges.len(), 4);
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test expired seam candidate backing",
            )
            .unwrap(),
        );
        ctx.finish_session().unwrap();
    }
}

#[test]
fn rejected_sphere_candidate_releases_identity_children() {
    let mut out = sphere();
    out.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(
            Point3::new(0., 0., f64::MAX),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
            f64::MAX,
        )
        .unwrap(),
    ));
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    synthesize_sphere_seams(&ctx, &mut out, &mut AnnotationBuilder::new(), &stream()).unwrap();
    assert_eq!(out.curves.len(), 2);
    assert_eq!(out.coedges.len(), 3);
    assert_eq!(out.loops[0].coedges().len(), 3);
    assert_eq!(out.points[0].position().get(), Point3::new(0., 0., 0.));
    drop(
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test rejected sphere candidate",
        )
        .unwrap(),
    );
    ctx.finish_session().unwrap();
}

fn assert_seam_pass_skips_indexes(mut out: Brep, spherical: bool, operations: &[&str]) -> Brep {
    for operation in operations {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            None,
        );
        if spherical {
            synthesize_sphere_seams(&ctx, &mut out, &mut AnnotationBuilder::new(), &stream())
                .unwrap();
        } else {
            synthesize_cylinder_seams(&ctx, &mut out, &mut AnnotationBuilder::new(), &stream())
                .unwrap();
        }
        drop(probe);
        ctx.finish_session().unwrap();
    }
    out
}

#[test]
fn cylinder_nonsingleton_loops_skip_downstream_indexes() {
    let mut out = cylinder(1.);
    for (index, lp) in out.loops.iter_mut().enumerate() {
        let original = lp.coedges()[0].clone();
        let mut extra = out.coedges[index].clone();
        extra.id = CoedgeId::mint(format!("test:model:coedge#extra-{index}")).unwrap();
        extra.radial_next = extra.id.clone();
        let added = extra.id.clone();
        out.coedges.push(extra);
        lp.boundary = LoopBoundary::Ring(
            LoopRing::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![original, added],
                Vec::new(),
            )
            .unwrap()
            .unwrap(),
        );
    }
    let out = assert_seam_pass_skips_indexes(
        out,
        false,
        &[
            "index Parasolid seam coedges",
            "index Parasolid seam edges",
            "index Parasolid seam curves",
            "index Parasolid seam vertices",
            "index Parasolid seam points",
        ],
    );
    assert_eq!(out.edges.len(), 2);
    assert_eq!(out.loops.len(), 2);
    assert_eq!(out.loops[0].coedges().len(), 2);
}

#[test]
fn cylinder_missing_loop_skips_downstream_indexes() {
    let mut out = cylinder(1.);
    out.loops.clear();
    let out = assert_seam_pass_skips_indexes(
        out,
        false,
        &[
            "index Parasolid seam coedges",
            "index Parasolid seam edges",
            "index Parasolid seam curves",
            "index Parasolid seam vertices",
            "index Parasolid seam points",
        ],
    );
    assert_eq!(out.edges.len(), 2);
    assert!(out.loops.is_empty());
}

#[test]
fn sphere_unsupported_ring_skips_downstream_indexes() {
    let mut out = sphere();
    out.loops[0].boundary = LoopBoundary::Ring(LoopRing::single(out.coedges[0].id.clone()));
    let out = assert_seam_pass_skips_indexes(
        out,
        true,
        &[
            "index Parasolid sphere coedge edges",
            "index Parasolid sphere edges",
            "index Parasolid sphere curves",
            "index Parasolid sphere vertices",
            "index Parasolid sphere points",
            "index Parasolid sphere vertex points",
        ],
    );
    assert_eq!(out.curves.len(), 2);
    assert_eq!(out.loops[0].coedges().len(), 1);
}

#[test]
fn sphere_four_edge_repair_skips_pole_selector() {
    let mut out = sphere();
    let mut edge = out.edges[0].clone();
    edge.id = EdgeId::mint("test:model:edge#seam").unwrap();
    edge.carrier = EdgeCarrier::unbounded(None);
    let mut coedge = out.coedges[0].clone();
    coedge.id = CoedgeId::mint("test:model:coedge#seam").unwrap();
    coedge.radial_next = coedge.id.clone();
    coedge.edge = edge.id.clone();
    out.edges.push(edge);
    out.coedges.push(coedge);
    out.loops[0].boundary = LoopBoundary::Ring(
        LoopRing::new(
            &cadmpeg_test_support::service_decode_context(),
            out.coedges.iter().map(|coedge| coedge.id.clone()).collect(),
            Vec::new(),
        )
        .unwrap()
        .unwrap(),
    );
    let out = assert_seam_pass_skips_indexes(out, true, &["index Parasolid sphere vertex points"]);
    assert_eq!(out.coedges.len(), 4);
    assert_eq!(out.curves.len(), 3);
    assert!(out.edges[2].curve().is_some());
    assert!(
        matches!(out.curves[2].geometry.solved(), Some(SolvedCurveGeometry::Degenerate(curve))
        if curve.point().get() == Point3::new(0., 0., 1.))
    );
    assert_eq!(out.points[0].position().get(), Point3::new(0., 0., 1.));
}

#[test]
fn cylinder_rejected_first_ring_skips_second_ring_curve_index() {
    let mut out = cylinder(1.);
    out.edges[0].end = VertexId::mint("test:model:vertex#open-ring").unwrap();
    let original_points: Vec<_> = out.points.iter().map(|point| point.position()).collect();
    let out = assert_seam_pass_skips_indexes(
        out,
        false,
        &[
            "index Parasolid seam curves",
            "index Parasolid seam vertices",
            "index Parasolid seam points",
        ],
    );
    assert_eq!(out.curves.len(), 2);
    assert_eq!(out.edges.len(), 2);
    assert_eq!(out.loops.len(), 2);
    assert_eq!(out.points.iter().map(|point| point.position()).collect::<Vec<_>>(), original_points);
}
