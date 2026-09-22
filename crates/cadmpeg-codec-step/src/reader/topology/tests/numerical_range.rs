// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
fn plane() -> (cadmpeg_ir::CadIr, SurfaceId) {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let id = SurfaceId::mint("test:audit:surface#1").unwrap();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0., 0., 0.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    (ir, id)
}
#[test]
fn numerical_0922b_pcurve_knot_units() {
    let (ir, id) = plane();
    let index = ModelIndex::new_model_only(&ir);
    for d in [1., 1e9] {
        let p = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                vec![0., 0., d, d],
                vec![Point2::new(0., 0.), Point2::new(1., 0.)],
                None,
                false,
            )
            .unwrap(),
        };
        let seeds = pcurve_selection_seeds(&index, &id, &p, &ir.model.surfaces[0].geometry);
        let r = pcurve_surface_closest(&index, &id, &p, Point3::new(0.3, 0., 0.), &seeds).unwrap();
        println!("STEP d{d:e}, result{r:?}, x={}", r.1 / d);
        assert!(r.0 < 1e-14);
        assert!((r.1 / d - 0.3).abs() < 1e-14);
    }
}
