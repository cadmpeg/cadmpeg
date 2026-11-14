// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::topology::EdgeCarrier;

#[test]
fn inverse_boundary_cache_reports_distance_and_exact_isocurve_has_no_loss() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let body = crate::test_support::parasolid::triangle_body();
    let source_stream = cadmpeg_ir::stream_name!("approximation");
    let source_stream = StreamHandle::new(source_stream);
    for cached in [false, true] {
        let mut out = decode_body(
            &ctx,
            &body,
            &cadmpeg_ir::stream_name!("approximation"),
            false,
        )
        .unwrap();
        out.coedges.truncate(1);
        out.coedges[0].pcurves.clear();
        out.pcurves.clear();
        out.losses.clear();
        let edge_id = out.coedges[0].edge.clone();
        let edge = out
            .edges
            .iter_mut()
            .find(|edge| edge.id == edge_id)
            .unwrap();
        let curve_id = id_curve(70);
        edge.carrier = EdgeCarrier::new(Some(curve_id.clone()), Some([0.0, 1.0])).unwrap();
        let poles = if cached {
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
        } else {
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)]
        };
        let curve =
            NurbsCurve::from_lanes(1, vec![0.0, 0.0, 1.0, 1.0], poles, None, false).unwrap();
        out.curves.push(Curve {
            id: curve_id,
            source_object: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
        });
        let surface = NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                    vec![Point3::new(0.5, 0.0, 1.0), Point3::new(0.5, 1.0, 1.0)],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                ],
                None,
            ),
            false,
        )
        .unwrap();
        let surface_id = out.faces[0].surface.clone();
        out.surfaces
            .iter_mut()
            .find(|surface| surface.id == surface_id)
            .unwrap()
            .geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface));
        derive_nurbs_isoparametric_pcurves(
            &ctx,
            &mut out,
            &mut AnnotationBuilder::default(),
            &source_stream,
        )
        .unwrap();
        out.losses.extend(out.approximations.take_notes());
        assert_eq!(out.pcurves.len(), 1);
        assert_eq!(out.losses.len(), usize::from(cached));
        if cached {
            assert_eq!(
                out.losses[0].code,
                crate::loss::SldprtLossCode::GeometryCarrierApproximated.kind()
            );
            let distance = out.pcurves[0].fit_tolerance().unwrap().get();
            assert!(distance > 0.4 && distance < 0.6);
            assert_eq!(out.losses[0].message, format!(
                "1 inverse NURBS boundary cache(s) use approximate carriers; largest sampled maximum distance {distance} mm."
            ));
        }
    }
}
