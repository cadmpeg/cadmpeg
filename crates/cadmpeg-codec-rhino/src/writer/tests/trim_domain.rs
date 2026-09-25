// SPDX-License-Identifier: Apache-2.0

use super::super::{validate_nurbs_trim, WritableEdge, WritableEdgeCurve, WritablePcurve};
use cadmpeg_ir::{
    geometry::{
        nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes},
        pcurve::{Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs},
    },
    ids::{CurveId, EdgeId, PcurveId, VertexId},
    math::{Point2, Point3},
    topology::{Edge, EdgeCarrier, Sense},
};
const EPS_FIT: f64 = 1e-6;
#[test]
fn numerical_audit_trim_domain_check_ignores_surface_knot_units() {
    let source = Edge {
        id: EdgeId::mint("test:audit:edge#1").unwrap(),
        carrier: EdgeCarrier::new(
            Some(CurveId::mint("test:audit:curve#1").unwrap()),
            Some([0., 1.]),
        )
        .unwrap(),
        start: VertexId::mint("test:audit:vertex#1").unwrap(),
        end: VertexId::mint("test:audit:vertex#2").unwrap(),
        tolerance: None,
    };
    for d in [1., 1e-16] {
        let surface = NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0., 0., d, d], false),
            NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0., 0., 0.), Point3::new(0., 1., 0.)],
                    vec![Point3::new(1., 0., 0.), Point3::new(1., 1., 0.)],
                ],
                None,
            ),
            false,
        )
        .unwrap();
        for x in [0.5, 2.] {
            let curve = NurbsCurve::from_lanes(
                1,
                vec![0., 0., 1., 1.],
                vec![Point3::new(x, 0., 0.), Point3::new(x, 1., 0.)],
                None,
                false,
            )
            .unwrap();
            let uv = vec![Point2::new(x * d, 0.), Point2::new(x * d, 1.)];
            let p = Pcurve {
                id: PcurveId::mint("test:audit:pcurve#1").unwrap(),
                geometry: PcurveGeometry::Nurbs {
                    nurbs: PcurveNurbs::from_lanes(
                        1,
                        vec![0., 0., 1., 1.],
                        uv.clone(),
                        None,
                        false,
                    )
                    .unwrap(),
                },
                metadata: PcurveMetadata::general(
                    None,
                    Some(
                        cadmpeg_ir::units::FiniteVector::new([0., 1.])
                            .expect("finite fixture range"),
                    ),
                    Some(
                        cadmpeg_ir::geometry::FitTolerance::try_new(EPS_FIT)
                            .expect("finite non-negative fixture tolerance"),
                    ),
                ),
            };
            let edge = WritableEdge {
                source: &source,
                start: 0,
                end: 1,
                domain: [
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
                    cadmpeg_ir::scalar::FiniteReal::ONE,
                ],
                curve_id: "audit-line",
                curve: WritableEdgeCurve::Nurbs(&curve),
                uses: vec![],
            };
            let explicit = WritablePcurve {
                source: &p,
                payload: ([0; 16], vec![]),
                domain_extent_points: uv,
            };
            assert_eq!(
                validate_nurbs_trim(&surface, EPS_FIT, &edge, Sense::Forward, &explicit).is_ok(),
                x == 0.5
            );
        }
    }
}

/// The refusal of a trim over the unit NURBS square whose pcurve is `geometry`.
fn trim_error(geometry: PcurveGeometry) -> String {
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0., 0., 1., 1.],
        vec![Point3::new(0.5, 0., 0.), Point3::new(0.5, 1., 0.)],
        None,
        false,
    )
    .unwrap();
    trim_refusal(geometry, &curve, [0., 1.])
}

/// The refusal of a trim over the unit NURBS square whose pcurve is
/// `geometry` and whose edge runs along `curve` over `domain`.
fn trim_refusal(geometry: PcurveGeometry, curve: &NurbsCurve, domain: [f64; 2]) -> String {
    let surface = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
        NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0., 0., 0.), Point3::new(0., 1., 0.)],
                vec![Point3::new(1., 0., 0.), Point3::new(1., 1., 0.)],
            ],
            None,
        ),
        false,
    )
    .unwrap();
    trim_refusal_on(&surface, geometry, curve, domain)
}

/// The refusal of a trim over `surface` whose pcurve is `geometry` and whose
/// edge runs along `curve` over `domain`.
fn trim_refusal_on(
    surface: &NurbsSurface,
    geometry: PcurveGeometry,
    curve: &NurbsCurve,
    domain: [f64; 2],
) -> String {
    let source = Edge {
        id: EdgeId::mint("test:overflow:edge#1").unwrap(),
        carrier: EdgeCarrier::new(
            Some(CurveId::mint("test:overflow:curve#1").unwrap()),
            Some([0., 1.]),
        )
        .unwrap(),
        start: VertexId::mint("test:overflow:vertex#1").unwrap(),
        end: VertexId::mint("test:overflow:vertex#2").unwrap(),
        tolerance: None,
    };
    let p = Pcurve {
        id: PcurveId::mint("test:overflow:pcurve#1").unwrap(),
        geometry,
        metadata: PcurveMetadata::default(),
    };
    let edge = WritableEdge {
        source: &source,
        start: 0,
        end: 1,
        domain: domain.map(|end| cadmpeg_ir::scalar::FiniteReal::new(end).unwrap()),
        curve_id: "overflow-line",
        curve: WritableEdgeCurve::Nurbs(curve),
        uses: vec![],
    };
    let explicit = WritablePcurve {
        source: &p,
        payload: ([0; 16], vec![]),
        domain_extent_points: vec![Point2::new(0.5, 0.), Point2::new(0.5, 1.)],
    };
    validate_nurbs_trim(surface, EPS_FIT, &edge, Sense::Forward, &explicit)
        .expect_err("the pcurve has no point in the surface domain")
        .to_string()
}

fn vertical_line_at_max() -> PcurveGeometry {
    PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(f64::MAX, 0.),
            Point2::new(0., 1.),
        )
        .unwrap(),
    )
}

#[test]
fn a_trim_pcurve_whose_offset_point_overflows_leaves_the_surface_domain() {
    // The offset of the vertical line at the largest finite u by the largest
    // finite distance reaches u = +inf, which no surface domain contains.
    let error = trim_error(PcurveGeometry::Offset(
        cadmpeg_ir::geometry::pcurve::OffsetPcurve::try_new(
            -f64::MAX,
            Box::new(vertical_line_at_max()),
        )
        .unwrap(),
    ));
    assert!(
        error.contains("pcurve test:overflow:pcurve#1 leaves its NURBS surface parameter domain"),
        "{error}"
    );
}

#[test]
fn a_trim_pcurve_whose_placed_point_overflows_leaves_the_surface_domain() {
    // The vertical line at the largest finite u, placed by a transform that
    // doubles u, reaches u = +inf, which no surface domain contains.
    let error = trim_error(PcurveGeometry::Transformed(
        cadmpeg_ir::geometry::pcurve::PlacedPcurve::try_new(
            Box::new(vertical_line_at_max()),
            cadmpeg_ir::transform::Transform2::affine([[2., 0., 0.], [0., 1., 0.]]).unwrap(),
        )
        .unwrap(),
    ));
    assert!(
        error.contains("pcurve test:overflow:pcurve#1 leaves its NURBS surface parameter domain"),
        "{error}"
    );
}

#[test]
fn a_trim_edge_curve_whose_point_overflows_misses_its_pcurve_by_the_distance_it_reached() {
    // At t = 1/2 the homogeneous weight of (1, -1 + 2^-40) is 2^-41, so the
    // projected second pole row reaches y = -inf; the pcurve maps the same
    // parameter to (1/2, 1/2, 0), infinitely far from the edge point.
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0., 0., 1., 1.],
        vec![Point3::new(0.5, 0., 0.), Point3::new(0.5, 1.0e300, 0.)],
        Some(vec![1., -1. + 2f64.powi(-40)]),
        false,
    )
    .unwrap();
    let error = trim_refusal(
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.5, 0.),
                Point2::new(0., 1.),
            )
            .unwrap(),
        ),
        &curve,
        [0.5, 0.75],
    );
    assert!(
        error.contains(
            "pcurve test:overflow:pcurve#1 misses directed edge curve overflow-line by inf"
        ),
        "{error}"
    );
}

#[test]
fn a_trim_surface_whose_point_overflows_misses_the_edge_by_the_distance_it_reached() {
    // The u weights 1 and -1 + 2^-40 blend to 2^-41 at u = 1/2, so the
    // surface point at u = 1/2 reaches x = -inf from the poles at x = 1e300;
    // the edge line runs along u = 1/2, infinitely far away.
    let weight = -1. + 2f64.powi(-40);
    let surface = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
        NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0., 0., 0.), Point3::new(0., 1., 0.)],
                vec![Point3::new(1.0e300, 0., 0.), Point3::new(1.0e300, 1., 0.)],
            ],
            Some(vec![vec![1., 1.], vec![weight, weight]]),
        ),
        false,
    )
    .unwrap();
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0., 0., 1., 1.],
        vec![Point3::new(0.5, 0., 0.), Point3::new(0.5, 1., 0.)],
        None,
        false,
    )
    .unwrap();
    let error = trim_refusal_on(
        &surface,
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.5, 0.),
                Point2::new(0., 1.),
            )
            .unwrap(),
        ),
        &curve,
        [0., 1.],
    );
    assert!(
        error.contains(
            "pcurve test:overflow:pcurve#1 misses directed edge curve overflow-line by inf"
        ),
        "{error}"
    );
}
