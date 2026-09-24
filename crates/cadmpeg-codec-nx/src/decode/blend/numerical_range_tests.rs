// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;

#[test]
fn periodic_circle_inverse_refuses_nonfinite_seeds() {
    let circle = SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("unit circle is valid"),
    );
    for seed in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            closest_periodic_analytic_curve_parameter_with_budget(
                &circle,
                Point3::new(1.0, 0.0, 0.0),
                Some(seed),
                &GeometryWorkBudget::new(100),
            ),
            None
        );
    }
}

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

#[test]
fn numerical_0922b_unclamped_curve_inverse() {
    for knots in [vec![0., 0., 0., 1., 1., 1.], vec![-2., -1., 0., 1., 2., 3.]] {
        let curve = NurbsCurve::from_lanes(
            2,
            knots,
            vec![
                Point3::new(-1., 0., 0.),
                Point3::new(0., 1., 0.),
                Point3::new(1., 0., 0.),
            ],
            None,
            false,
        )
        .unwrap();
        let target = cadmpeg_ir::eval::nurbs_curve_point(
            curve.degree(),
            curve.knots(),
            &curve.pole_rows().raw_points(),
            None,
            0.,
        )
        .unwrap();
        let budget = GeometryWorkBudget::new(100_000);
        let p =
            closest_nurbs_curve_parameter_with_budget(&curve, target.get(), None, &budget).unwrap();
        let actual = cadmpeg_ir::eval::nurbs_curve_point(
            curve.degree(),
            curve.knots(),
            &curve.pole_rows().raw_points(),
            None,
            p,
        )
        .unwrap();
        println!(
            "NX knots{:?}, exact start{target:?}: inverse{p}, residual{}",
            curve.knots(),
            actual.distance(target.get())
        );
        assert!(actual.distance(target.get()) < 1e-14);
    }
}
#[test]
fn numerical_0922b_small_domain_inverse() {
    for d in [1., 1e-16] {
        let curve = NurbsCurve::from_lanes(
            2,
            vec![0., 0., 0., d, d, d],
            vec![
                Point3::new(0.1875, 0., 0.),
                Point3::new(-0.3125, 0.05, 0.),
                Point3::new(0.1875, 0.1, 0.),
            ],
            None,
            false,
        )
        .unwrap();
        let target = cadmpeg_ir::eval::nurbs_curve_point(
            curve.degree(),
            curve.knots(),
            &curve.pole_rows().raw_points(),
            None,
            0.75 * d,
        )
        .unwrap();
        let budget = GeometryWorkBudget::new(100_000);
        let p =
            closest_nurbs_curve_parameter_with_budget(&curve, target.get(), None, &budget).unwrap();
        let actual = cadmpeg_ir::eval::nurbs_curve_point(
            curve.degree(),
            curve.knots(),
            &curve.pole_rows().raw_points(),
            None,
            p,
        )
        .unwrap();
        println!(
            "NX d{d:e},target{target:?}:inverse{},residual{}",
            p / d,
            actual.distance(target.get())
        );
        assert!(actual.distance(target.get()) < 1e-14);
    }
}
#[test]
fn numerical_0922b_discontinuous_curve_inverse() {
    let curve = NurbsCurve::from_lanes(
        2,
        vec![0., 0., 0., 0.5, 0.5, 0.5, 1., 1., 1.],
        vec![0., 1., 2., 10., 11., 12.]
            .into_iter()
            .map(|x| Point3::new(x, 0., 0.))
            .collect(),
        None,
        false,
    )
    .unwrap();
    let target = Point3::new(11., 0., 0.);
    let p = closest_nurbs_curve_parameter_with_budget(
        &curve,
        target,
        None,
        &GeometryWorkBudget::new(100_000),
    )
    .unwrap();
    let actual = cadmpeg_ir::eval::nurbs_curve_point(
        curve.degree(),
        curve.knots(),
        &curve.pole_rows().raw_points(),
        None,
        p,
    )
    .unwrap();
    println!("NX discontinuous quadratic target11: parameter{p}, actual{actual:?}");
    assert!(actual.distance(target) < 1e-14);
}
#[test]
fn numerical_0922b_common_weight_inverse() {
    for w in [1., 1e200] {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0., 0., 1., 1.],
            vec![Point3::new(0., 0., 0.), Point3::new(1e200, 0., 0.)],
            Some(vec![w, w]),
            false,
        )
        .unwrap();
        let budget = GeometryWorkBudget::new(100_000);
        let p = closest_nurbs_curve_parameter_with_budget(
            &curve,
            Point3::new(0., 0., 0.),
            None,
            &budget,
        );
        println!("NX commonweight{w:e}: inverse{p:?}");
        assert_eq!(p, Some(0.));
    }
}

#[test]
fn numerical_audit_pcurve_newton_converges_on_small_chart() {
    for d in [1., 1e-16] {
        let p = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                2,
                vec![0., 0., 0., d, d, d],
                vec![
                    Point2::new(0., 0.),
                    Point2::new(0., 0.),
                    Point2::new(1., 0.),
                ],
                None,
                false,
            )
            .unwrap(),
        };
        let t = closest_pcurve_parameter_from_seed(&p, Point2::new(0.25, 0.), 0.9 * d).unwrap();
        assert!((pcurve_uv(&p, t).unwrap().u - 0.25).abs() < 64. * f64::EPSILON);
    }
}
#[test]
fn numerical_audit_inverse_and_grid_keep_wide_finite_chart() {
    for [a, b] in [[0., 1.], [-1e308, 1e308]] {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![a, a, b, b],
            vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
            None,
            false,
        )
        .unwrap();
        let t = closest_nurbs_curve_parameter_with_budget(
            &curve,
            Point3::new(0.3, 0., 0.),
            None,
            &GeometryWorkBudget::new(100_000),
        )
        .unwrap();
        assert!(
            (cadmpeg_ir::eval::nurbs_curve_point(
                1,
                curve.knots(),
                &curve.pole_rows().raw_points(),
                None,
                t
            )
            .unwrap()
            .x - 0.3)
                .abs()
                < 64. * f64::EPSILON
        );
        let p = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                vec![a, a, b, b],
                vec![Point2::new(0., 0.), Point2::new(1., 0.)],
                None,
                false,
            )
            .unwrap(),
        };
        let t = closest_pcurve_parameter_from_coarse_grid(&p, Point2::new(0.3, 0.)).unwrap();
        assert!((pcurve_uv(&p, t).unwrap().u - 0.3).abs() < 64. * f64::EPSILON);
    }
    assert_eq!(scalar_bezier_value(&[0., 1.], 0., [-1e308, 1e308]), 0.5);
    assert_eq!(
        homogeneous_residual_distance(&[[-0.5, 0., 1.], [0.5, 0., 1.]], 0., [-1e308, 1e308]),
        0.
    );
}

/// A model holding one plane whose origin is the largest finite x
/// coordinate: its points at u = MAX have no finite x.
fn overflowing_plane_model() -> (cadmpeg_ir::CadIr, SurfaceId, SurfaceGeometry) {
    let surface_id = SurfaceId::mint("test:nx:surface#overflow").expect("identity grammar");
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(f64::MAX, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: surface_id.clone(),
        geometry: geometry.clone(),
        source_object: None,
    });
    (ir, surface_id, geometry)
}

#[test]
fn a_decoded_surface_point_that_overflows_is_returned_without_a_fallback() {
    let (ir, surface_id, geometry) = overflowing_plane_model();
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    let budget = GeometryWorkBudget::new(1024);
    for point in [
        decoded_surface_point_inner_with_budget(&index, &surface_id, f64::MAX, 3.0, 0, &budget),
        decoded_surface_point_with_geometry_and_budget(
            &index,
            &surface_id,
            &geometry,
            f64::MAX,
            3.0,
            0,
            &budget,
        ),
    ] {
        assert!(
            point.is_some_and(|point| point.x.is_nan() && point.y == 3.0 && point.z == 0.0),
            "{point:?}"
        );
    }
}

#[test]
fn a_decoded_placed_surface_point_that_overflows_is_returned_without_a_fallback() {
    // The plane through the origin, placed by a transform that adds the
    // largest finite x coordinate: its point at u = MAX has no finite x.
    let surface_id = SurfaceId::mint("test:nx:surface#placed-overflow").expect("identity grammar");
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(
            Box::new(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            cadmpeg_ir::transform::Transform::affine([
                [1.0, 0.0, 0.0, f64::MAX],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
        )
        .expect("valid PlacedSurface fixture"),
    ));
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: surface_id.clone(),
        geometry: geometry.clone(),
        source_object: None,
    });
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    let budget = GeometryWorkBudget::new(1024);
    for point in [
        decoded_surface_point_inner_with_budget(&index, &surface_id, f64::MAX, 3.0, 0, &budget),
        decoded_surface_point_with_geometry_and_budget(
            &index,
            &surface_id,
            &geometry,
            f64::MAX,
            3.0,
            0,
            &budget,
        ),
    ] {
        assert_eq!(point, Some(Point3::new(f64::INFINITY, 3.0, 0.0)));
    }
}
