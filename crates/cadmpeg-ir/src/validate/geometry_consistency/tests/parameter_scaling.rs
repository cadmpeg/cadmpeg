// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crate::geometry::pcurve::PcurveNurbs;
use crate::{ids::SurfaceId, index::ModelIndex, math::Point2};
#[test]
fn numerical_audit_mapped_pcurve_search_ignores_knot_units() {
    const EPS_POINT: f64 = 1e-6;
    let mut ir = crate::CadIr::empty();
    let id = SurfaceId::mint("test:audit:surface#1").unwrap();
    ir.model.surfaces.push(crate::geometry::Surface {
        id: id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            crate::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0., 0., 0.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    let index = ModelIndex::new_model_only(&ir);
    let context = SurfacePcurveContext {
        index: &index,
        surface_id: &id,
        geometry: &ir.model.surfaces[0].geometry,
    };
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
        let t = mapped_pcurve_parameter_near_point(
            &context,
            &p,
            Point3::new(0.3, 0., 0.),
            0.,
            EPS_POINT,
        )
        .unwrap();
        assert!((pcurve_uv(&p, t).unwrap().u - 0.3).abs() <= EPS_POINT);
    }
}
