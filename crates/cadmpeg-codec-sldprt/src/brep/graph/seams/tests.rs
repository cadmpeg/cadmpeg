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
