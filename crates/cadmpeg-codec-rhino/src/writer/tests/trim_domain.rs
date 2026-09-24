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
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0., 0., 1., 1.],
        vec![Point3::new(0.5, 0., 0.), Point3::new(0.5, 1., 0.)],
        None,
        false,
    )
    .unwrap();
    let p = Pcurve {
        id: PcurveId::mint("test:overflow:pcurve#1").unwrap(),
        geometry,
        metadata: PcurveMetadata::default(),
    };
    let edge = WritableEdge {
        source: &source,
        start: 0,
        end: 1,
        domain: [
            cadmpeg_ir::scalar::FiniteReal::ZERO,
            cadmpeg_ir::scalar::FiniteReal::ONE,
        ],
        curve_id: "overflow-line",
        curve: WritableEdgeCurve::Nurbs(&curve),
        uses: vec![],
    };
    let explicit = WritablePcurve {
        source: &p,
        payload: ([0; 16], vec![]),
        domain_extent_points: vec![Point2::new(0.5, 0.), Point2::new(0.5, 1.)],
    };
    validate_nurbs_trim(&surface, EPS_FIT, &edge, Sense::Forward, &explicit)
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
