// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;

#[test]
fn numerical_0922_contact_inverse_ignores_knot_units() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let id = SurfaceId::mint("nx:test:surface#1").unwrap();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: id.clone(),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0., 0., 0.),
                    Vector3::new(0., 0., 1.),
                    Vector3::new(1., 0., 0.),
                )
                .unwrap(),
            ),
        ),
        source_object: None,
    });
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
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
        let t = closest_contact_pcurve_parameter_with_geometry_and_budget(
            &index,
            &id,
            &p,
            Point3::new(0.3, 0., 0.),
            None,
            &GeometryWorkBudget::new(100_000),
        )
        .unwrap();
        let hit = pcurve_uv(&p, t).unwrap();
        println!("NX plane contact domain{d:e}: parameter{t:e}, hit{hit:?}");
        assert!((hit.u - 0.3).abs() < 1e-14);
    }
}
#[test]
fn numerical_0922_large_ellipse_keeps_inverse() {
    for scale in [1., 1e200] {
        let g = SolvedCurveGeometry::Ellipse(
            cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                Point3::new(0., 0., 0.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
                scale,
                0.5 * scale,
            )
            .unwrap(),
        );
        let r = closest_periodic_analytic_curve_parameter_with_budget(
            &g,
            Point3::new(scale, 0., 0.),
            None,
            &GeometryWorkBudget::new(10000),
        );
        println!(
            "NX ellipse axes ({scale:e}, {}), exact major tip: {r:?}",
            scale * 0.5
        );
        assert_eq!(r, Some(0.));
    }
}
#[test]
fn numerical_0922_far_ellipse_query_keeps_inverse() {
    let g = SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
            2.,
            1.,
        )
        .unwrap(),
    );
    let r = closest_periodic_analytic_curve_parameter_with_budget(
        &g,
        Point3::new(2., 0., 1e200),
        None,
        &GeometryWorkBudget::new(10000),
    );
    println!("NX ordinary ellipse with query z1e200: {r:?}");
    assert_eq!(r, Some(0.));
}
