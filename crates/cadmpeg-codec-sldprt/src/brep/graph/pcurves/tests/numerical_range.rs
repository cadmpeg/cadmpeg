// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::annotations::AnnotationBuilder;
use crate::brep::graph::Brep;
use cadmpeg_ir::geometry::analytic::CircleCurve;
use cadmpeg_ir::topology::Coedge;
use cadmpeg_ir::ids::CoedgeId;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::topology::EdgeCarrier;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::topology::Face;
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::topology::FaceLoops;
use cadmpeg_ir::topology::Loop;
use cadmpeg_ir::topology::LoopBoundary;
use cadmpeg_ir::ids::LoopId;
use cadmpeg_ir::topology::LoopRing;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::ids::ShellId;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::analytic::SphereSurface;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::ids::VertexId;
use crate::brep::graph::pcurves::derive_pcurves;

fn sphere_fixture(center: Point3, axis: Vector3, reference: Vector3) -> Brep {
    let mut out = Brep::default();
    let surface = SurfaceId::mint("test:audit:surface#1").unwrap();
    let curve = CurveId::mint("test:audit:curve#1").unwrap();
    let face = FaceId::mint("test:audit:face#1").unwrap();
    let owner_loop = LoopId::mint("test:audit:loop#1").unwrap();
    let coedge = CoedgeId::mint("test:audit:coedge#1").unwrap();
    let edge = EdgeId::mint("test:audit:edge#1").unwrap();
    let vertex = VertexId::mint("test:audit:vertex#1").unwrap();
    out.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(
                Point3::new(0., 0., 0.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
                1.,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    out.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(center, axis, reference, 1.).unwrap(),
        )),
        source_object: None,
    });
    out.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:audit:shell#1").unwrap(),
        surface,
        sense: Sense::Forward,
        loops: FaceLoops::Unspecified {
            loops: vec![owner_loop.clone()],
        },
        name: None,
        color: None,
        tolerance: None,
    });
    out.loops.push(Loop {
        id: owner_loop.clone(),
        face,
        boundary: LoopBoundary::Ring(LoopRing::single(coedge.clone())),
    });
    out.coedges.push(Coedge {
        id: coedge.clone(),
        owner_loop,
        edge: edge.clone(),
        radial_next: coedge,
        sense: Sense::Forward,
        pcurves: vec![],
        use_curve: None,
    });
    out.edges.push(Edge {
        id: edge,
        carrier: EdgeCarrier::new(Some(curve), Some([0., std::f64::consts::TAU])).unwrap(),
        start: vertex.clone(),
        end: vertex,
        tolerance: None,
    });
    out
}
fn derive_sphere(out: &mut Brep) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    derive_pcurves(
        &ctx,
        out,
        &mut AnnotationBuilder::new(),
        &StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            cadmpeg_ir::StreamName::try_from("audit".to_owned()).unwrap(),
            "fixture stream handle",
        )
        .unwrap(),
    )
    .expect("spherical pcurve derivation");
}
fn error_at(out: &Brep, t: f64) -> f64 {
    let uv = cadmpeg_ir::eval::decode::pcurve_uv(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &out.pcurves[0].geometry,
        t,
    )
    .unwrap();
    let hit = cadmpeg_ir::eval::decode::surface_point(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &out.surfaces[0].geometry,
        uv.u,
        uv.v,
    )
    .unwrap();
    let wanted = cadmpeg_ir::eval::decode::curve_point(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &out.curves[0].geometry,
        t,
    )
    .unwrap();
    hit.distance(wanted.get())
}
#[test]
fn numerical_0922b_sphere_circle_frames() {
    for (axis, reference) in [
        (Vector3::new(0., 0., 1.), Vector3::new(1., 0., 0.)),
        (Vector3::new(0., 0., 1.), Vector3::new(0., 1., 0.)),
        (Vector3::new(0., 0., -1.), Vector3::new(1., 0., 0.)),
        (Vector3::new(1., 0., 0.), Vector3::new(0., 0., 1.)),
    ] {
        let mut out = sphere_fixture(Point3::new(0., 0., 0.), axis, reference);
        derive_sphere(&mut out);
        assert_eq!(out.pcurves.len(), 1);
        let residual = error_at(&out, 0.3);
        println!(
            "SW circleaxis{axis:?},reference{reference:?},pcurve{:?}:residual{residual}",
            out.pcurves[0].geometry
        );
        assert!(residual < 32.0 * f64::EPSILON);
    }
}
#[test]
fn numerical_0922b_sphere_rejects_offset_circle() {
    let mut out = sphere_fixture(
        Point3::new(10., 0., 0.),
        Vector3::new(0., 0., 1.),
        Vector3::new(1., 0., 0.),
    );
    derive_sphere(&mut out);
    assert!(out.pcurves.is_empty());
}
#[test]
fn numerical_0922b_large_sphere_equator() {
    let mut out = sphere_fixture(
        Point3::new(0., 0., 0.),
        Vector3::new(0., 0., 1.),
        Vector3::new(1., 0., 0.),
    );
    out.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
            1e200,
        )
        .unwrap(),
    ));
    out.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
            1e200,
        )
        .unwrap(),
    ));
    derive_sphere(&mut out);
    println!("SW r1e200 equator pcurves{}", out.pcurves.len());
    assert_eq!(out.pcurves.len(), 1);
    assert!(error_at(&out, 0.3) / 1e200 < 32.0 * f64::EPSILON);
}
