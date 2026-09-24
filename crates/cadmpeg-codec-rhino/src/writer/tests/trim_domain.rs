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
                domain: [0., 1.],
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
