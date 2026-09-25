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

#[test]
fn pcurve_selection_keeps_interior_knots_and_seeds_in_a_wide_finite_domain() {
    let (ir, id) = plane();
    let index = ModelIndex::new_model_only(&ir);
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![-f64::MAX, -f64::MAX, 0.0, f64::MAX, f64::MAX],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(2.0, 0.0),
            ],
            None,
            false,
        )
        .unwrap(),
    };
    let mut fractions = Vec::new();
    pcurve_parameter_break_fractions(&pcurve, [-f64::MAX, f64::MAX], &mut fractions);
    assert_eq!(fractions, vec![0.5]);
    let seeds = pcurve_selection_seeds(&index, &id, &pcurve, &ir.model.surfaces[0].geometry);
    assert!(seeds
        .iter()
        .any(|seed| (seed / f64::MAX + 0.5).abs() < f64::EPSILON));
    assert!(seeds.iter().all(|seed| seed.is_finite()));
}

#[test]
fn numerical_0922b_pcurve_retains_finite_seed_when_step_overflows() {
    let (ir, id) = plane();
    let index = ModelIndex::new_model_only(&ir);
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0., 0., 1., 1.],
            vec![Point2::new(0., 0.), Point2::new(1e-200, 0.)],
            None,
            false,
        )
        .unwrap(),
    };
    assert_eq!(
        mapped_pcurve_closest(&index, &id, &pcurve, Point3::new(1e200, 0., 0.), 0.),
        Some((1e200, 0.))
    );
}

/// A plane whose origin x coordinate is `origin_x`.
fn plane_at(origin_x: f64) -> (cadmpeg_ir::CadIr, SurfaceId) {
    let (mut ir, id) = plane();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(origin_x, 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
        )
        .unwrap(),
    ));
    (ir, id)
}

#[test]
fn a_declared_pcurve_fit_with_an_overflowing_end_is_measured_at_its_finite_end() {
    // On the plane at x = MAX, the line end u = MAX has no finite point and
    // the start u = -MAX maps to the model origin.
    let (ir, id) = plane_at(f64::MAX);
    let index = ModelIndex::new_model_only(&ir);
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0., 0.),
            Point2::new(f64::MAX, 0.),
        )
        .unwrap(),
    );
    assert_eq!(
        pcurve_declared_endpoint_fit_directed(
            &index,
            &id,
            &pcurve,
            [-1., 1.],
            Point3::new(0., 0., 0.),
            Point3::new(7., 7., 7.),
        ),
        Some(0.)
    );
}

#[test]
fn the_mapped_pcurve_search_halves_a_step_whose_point_overflows() {
    // On the plane at x = 1.78e308 the parabola `(t^2, t)` overflows for t
    // above about 1.33e153. The Newton step from t = 1e152 toward the point
    // at t = 1.2e153 lands at about 7.25e153; halving it returns the search
    // to the finite range.
    let (ir, id) = plane_at(1.78e308);
    let index = ModelIndex::new_model_only(&ir);
    let parabola = PcurveGeometry::Parabola(
        cadmpeg_ir::geometry::pcurve::ParabolaPcurve::try_new(
            Point2::new(0., 0.),
            Point2::new(1., 0.),
            Point2::new(0., 1.),
            0.25,
        )
        .unwrap(),
    );
    let target_parameter = 1.2e153;
    let target = Point3::new(
        1.78e308 + target_parameter * target_parameter,
        target_parameter,
        0.,
    );
    let (error, parameter) = mapped_pcurve_closest(&index, &id, &parabola, target, 1e152).unwrap();
    assert!(
        (parameter / target_parameter - 1.).abs() < 1e-6,
        "{parameter}"
    );
    assert!(error < 1e300, "{error}");
}

#[test]
fn a_declared_pcurve_fit_with_an_overflowing_placed_end_misses_by_an_infinite_distance() {
    // The plane through the origin, placed by a transform that adds the
    // largest finite x coordinate: the line end u = MAX has no finite point
    // and the start u = -MAX maps to the model origin.
    let (mut ir, id) = plane();
    let SurfaceGeometry::Solved(plane) = ir.model.surfaces[0].geometry.clone() else {
        panic!("the plane fixture is solved");
    };
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(
            Box::new(plane),
            cadmpeg_ir::transform::Transform::affine([
                [1., 0., 0., f64::MAX],
                [0., 1., 0., 0.],
                [0., 0., 1., 0.],
            ])
            .unwrap(),
        )
        .unwrap(),
    ));
    let index = ModelIndex::new_model_only(&ir);
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0., 0.),
            Point2::new(f64::MAX, 0.),
        )
        .unwrap(),
    );
    assert_eq!(
        pcurve_declared_endpoint_fit_directed(
            &index,
            &id,
            &pcurve,
            [-1., 1.],
            Point3::new(0., 0., 0.),
            Point3::new(7., 7., 7.),
        ),
        Some(f64::INFINITY)
    );
}

#[test]
fn a_declared_pcurve_fit_with_an_overflowing_line_end_is_measured_at_its_finite_end() {
    // The line reaches u = MAX + MAX at its end, which the plane maps to no
    // finite point, and u = 0 at its start, which it maps to the origin.
    let (ir, id) = plane();
    let index = ModelIndex::new_model_only(&ir);
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(f64::MAX, 0.),
            Point2::new(f64::MAX, 0.),
        )
        .unwrap(),
    );
    assert_eq!(
        pcurve_declared_endpoint_fit_directed(
            &index,
            &id,
            &pcurve,
            [-1., 1.],
            Point3::new(0., 0., 0.),
            Point3::new(7., 7., 7.),
        ),
        Some(0.)
    );
}
