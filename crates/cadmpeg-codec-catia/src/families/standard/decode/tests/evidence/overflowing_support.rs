use super::{
    native_support_circle_param_range, standard_native_support_endpoint_pair, PcurveGeometry,
    Point, Point2, Point3, PointId, SolvedSurfaceGeometry, StandardEdgeSupport, SurfaceGeometry,
    Vector3,
};

/// A unit-radius cone about +Z whose cross-section radius overflows at
/// v = 1e308, with the pcurve that lifts onto its unit circle and the pcurve
/// that lifts to points without finite coordinates.
fn overflowing_cone_support(parameter_range: [f64; 2]) -> StandardEdgeSupport {
    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
            1.0,
            1.5,
        )
        .expect("valid ConeSurface fixture"),
    ));
    let line = |origin_v: f64| {
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, origin_v),
                Point2::new(1.0, 0.0),
            )
            .expect("valid LinePcurve fixture"),
        )
    };
    StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(cone.clone()),
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(cone),
        ],
        pcurves: [line(0.0), line(1.0e308)],
        parameter_range,
    }
}

#[test]
fn a_native_circle_range_reads_from_the_finite_support_when_its_partner_overflows() {
    let native = overflowing_cone_support([0.0, 1.5 * std::f64::consts::PI]);
    assert_eq!(
        native_support_circle_param_range(
            &native,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, -1.0, 0.0)
        )
        .expect("evaluator allocation succeeds"),
        Some([0.0, 1.5 * std::f64::consts::PI])
    );
}

#[test]
fn a_native_endpoint_pair_reads_from_the_finite_support_when_its_partner_overflows() {
    let native = overflowing_cone_support([1.0, 4.0]);
    let points = [1.0_f64, 4.0]
        .into_iter()
        .enumerate()
        .map(|(index, angle)| {
            Point::new(
                PointId::mint(format!("catia:test:point#overflow-{index}"))
                    .expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(angle.cos(), angle.sin(), 0.0))
                    .expect("a finite position is a point"),
                None,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        standard_native_support_endpoint_pair(&native, &points, &[0, 1], None)
            .expect("evaluator allocation succeeds"),
        Some([0, 1])
    );
}

#[test]
fn a_native_endpoint_pair_reads_from_the_finite_support_when_its_placed_partner_overflows() {
    let mut native = overflowing_cone_support([1.0, 4.0]);
    native.carriers = native.carriers.map(|carrier| match carrier {
        crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
            SurfaceGeometry::Solved(cone),
        ) => crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
                cadmpeg_ir::geometry::PlacedSurface::try_new(
                    Box::new(cone),
                    cadmpeg_ir::transform::Transform::identity(),
                )
                .expect("valid PlacedSurface fixture"),
            )),
        ),
        carrier => carrier,
    });
    let points = [1.0_f64, 4.0]
        .into_iter()
        .enumerate()
        .map(|(index, angle)| {
            Point::new(
                PointId::mint(format!("catia:test:point#placed-overflow-{index}"))
                    .expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(angle.cos(), angle.sin(), 0.0))
                    .expect("a finite position is a point"),
                None,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        standard_native_support_endpoint_pair(&native, &points, &[0, 1], None)
            .expect("evaluator allocation succeeds"),
        Some([0, 1])
    );
}
