// SPDX-License-Identifier: Apache-2.0

use super::*;

const SMALL_PARAMETER_DOMAIN: f64 = 1e-12;
const INVERSE_FIT_TOLERANCE: f64 = 1e-6;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};

fn bilinear(domain: [f64; 2], scale: f64) -> NurbsSurface {
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![domain[0], domain[0], domain[1], domain[1]], false),
        NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0., 0., 0.), Point3::new(0., scale, 0.)],
                vec![Point3::new(scale, 0., 0.), Point3::new(scale, scale, 0.)],
            ],
            None,
        ),
        false,
    )
    .unwrap()
}
#[test]
fn numerical_0922_wide_domain_keeps_distinct_roots() {
    let r = unique_inverse_parameter(
        vec![(-5e307, 0.), (5e307, 0.)],
        INVERSE_FIT_TOLERANCE,
        [-1e308, 1e308],
    );
    assert!(matches!(r, InverseResolution::Ambiguous));
}
#[test]
fn numerical_0922_small_domain_keeps_fit_samples() {
    let mut s = bilinear([0., 1.], 1.);
    s.edit_control_points(|p| {
        p.z = p.x * p.y;
        Ok(())
    })
    .unwrap();
    for d in [1., SMALL_PARAMETER_DOMAIN] {
        let c = NurbsCurve::from_lanes(
            1,
            vec![0., 0., d, d],
            vec![Point3::new(0., 0., 0.), Point3::new(1., 1., 1.)],
            None,
            false,
        )
        .unwrap();
        let samples = nurbs_curve_sample_parameters(&c, [0., d]).unwrap();
        let (uv, error) = nurbs_degree_one_cache_lanes(&s, &c, [0., d]).unwrap();
        let observed = Point3::new(0.5, 0.5, 0.5).distance(
            cadmpeg_ir::eval::nurbs_surface_point(&s, 0.5, 0.5)
                .unwrap()
                .get(),
        );
        println!("SW d{d:e} samples{samples:?} uv{uv:?} reported_error={error} actual_midpoint_error={observed}");
        assert_eq!(error, observed);
        assert_eq!(samples.len(), 9);
        assert_eq!(samples.first(), Some(&0.0));
        assert_eq!(samples.last(), Some(&d));
    }
}

use cadmpeg_ir::eval::{curve_point, pcurve_uv};
use cadmpeg_ir::geometry::analytic::{CircleCurve, SphereSurface};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::{EdgeCarrier, FaceLoops, LoopBoundary, LoopRing};
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
    derive_spherical_pcurves(
        out,
        &mut AnnotationBuilder::new(),
        &StreamHandle::new(cadmpeg_ir::StreamName::try_from("audit".to_owned()).unwrap()),
    );
}
fn error_at(out: &Brep, t: f64) -> f64 {
    let uv = pcurve_uv(&out.pcurves[0].geometry, t).unwrap();
    let hit = surface_point(&out.surfaces[0].geometry, uv.u, uv.v).unwrap();
    let wanted = curve_point(&out.curves[0].geometry, t).unwrap();
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
#[test]
fn numerical_0922b_wide_curve_inverse() {
    for d in [[0., 1.], [-1e308, 1e308], [1e308, 1.1e308]] {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![d[0], d[0], d[1], d[1]],
            vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
            None,
            false,
        )
        .unwrap();
        let r = nurbs_parameter_at_point(&curve, Point3::new(0.3, 0., 0.));
        let result = match r {
            InverseResolution::Unique(p) => Some(p),
            InverseResolution::NoMatch => None,
            InverseResolution::Ambiguous => panic!("ambiguous"),
        };
        println!("SW chart{d:?}: inverse{result:?}");
        let hit = nurbs_curve_point(
            curve.degree(),
            curve.knots(),
            &curve.control_points(),
            None,
            result.unwrap(),
        )
        .unwrap();
        assert!((hit.x - 0.3).abs() < INVERSE_FIT_TOLERANCE);
    }
}
