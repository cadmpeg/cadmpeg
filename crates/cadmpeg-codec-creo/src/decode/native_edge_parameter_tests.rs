use super::*;

fn circle() -> CurveGeometry {
    CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    }
}

fn ellipse() -> CurveGeometry {
    CurveGeometry::Ellipse {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 4.0,
        minor_radius: 2.0,
    }
}

fn evaluated(geometry: &CurveGeometry, parameter: f64) -> [f64; 3] {
    let point = cadmpeg_ir::eval::curve_point(geometry, parameter).expect("conic point");
    [point.x, point.y, point.z]
}

#[test]
fn preserves_scaled_line_parameterization_and_orders_the_interval() {
    let line = CurveGeometry::Line {
        origin: Point3::new(1.0, 2.0, 3.0),
        direction: Vector3::new(2.0, 0.0, 0.0),
    };
    assert_eq!(
        exact_line_edge_parameter_range(&line, [[7.0, 2.0, 3.0], [-3.0, 2.0, 3.0]]),
        Some([-2.0, 3.0])
    );
}

#[test]
fn withholds_parameters_for_points_off_the_line() {
    let line = CurveGeometry::Line {
        origin: Point3::new(1.0, 2.0, 3.0),
        direction: Vector3::new(2.0, 0.0, 0.0),
    };
    assert_eq!(
        exact_line_edge_parameter_range(&line, [[7.0, 2.0, 3.0], [-3.0, 2.1, 3.0]]),
        None
    );
}

#[test]
fn full_nonperiodic_nurbs_recovers_its_intrinsic_domain() {
    let nurbs = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 2,
        knots: vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
        control_points: vec![
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(4.0, 7.0, 3.0),
            Point3::new(9.0, 8.0, 3.0),
        ],
        weights: Some(vec![1.0, 0.5, 1.0]),
        periodic: false,
    });
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(&nurbs, [[9.0, 8.0, 3.0], [1.0, 2.0, 3.0]],),
        Some([2.0, 5.0])
    );
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(&nurbs, [[9.0, 8.0, 3.0], [1.0, 2.1, 3.0]],),
        None
    );
}

#[test]
fn degree_one_nurbs_recovers_unique_bounded_parameters() {
    let nurbs = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![2.0, 2.0, 5.0, 9.0, 9.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(3.0, 4.0, 0.0),
        ],
        weights: Some(vec![2.0, 1.0, 3.0]),
        periodic: false,
    });
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(&nurbs, [[3.0, 3.0, 0.0], [1.0, 0.0, 0.0]],),
        Some([3.5, 7.0])
    );
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(&nurbs, [[3.0, 3.0, 0.0], [1.0, 0.1, 0.0]],),
        None
    );

    let translated = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(100_000_000.0, 0.0, 0.0),
            Point3::new(100_000_004.0, 0.0, 0.0),
        ],
        weights: None,
        periodic: false,
    });
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(
            &translated,
            [[100_000_001.0, 0.01, 0.0], [100_000_003.0, 0.0, 0.0]],
        ),
        None
    );

    let self_intersecting = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        weights: None,
        periodic: false,
    });
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(
            &self_intersecting,
            [[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        ),
        None
    );

    let constant_span = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        weights: None,
        periodic: false,
    });
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(&constant_span, [[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],),
        None
    );
}

#[test]
fn periodic_nurbs_does_not_imply_a_full_edge_trim() {
    let nurbs = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        weights: None,
        periodic: true,
    });
    assert_eq!(
        nonperiodic_nurbs_edge_parameter_range(&nurbs, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],),
        None
    );

    let closed = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![2.0, 2.0, 4.0, 7.0, 9.0, 9.0],
        control_points: vec![
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(-1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        weights: None,
        periodic: true,
    });
    assert_eq!(
        full_periodic_nurbs_edge_parameter_range(&closed, [1.0, 0.0, 0.0]),
        Some([2.0, 9.0])
    );
    assert_eq!(
        full_periodic_nurbs_edge_parameter_range(&closed, [0.0, 1.0, 0.0]),
        None
    );
}

#[test]
fn pcurve_midpoint_selects_minor_major_and_full_circle_intervals() {
    let circle = circle();
    let points = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let root_two = std::f64::consts::SQRT_2;
    assert_eq!(
        periodic_conic_edge_parameter_range(&circle, points, [root_two, root_two, 0.0]),
        Some([0.0, std::f64::consts::FRAC_PI_2])
    );
    assert_eq!(
        periodic_conic_edge_parameter_range(&circle, points, [-root_two, -root_two, 0.0]),
        Some([std::f64::consts::FRAC_PI_2, std::f64::consts::TAU])
    );
    assert_eq!(
        periodic_conic_edge_parameter_range(&circle, [points[0], points[0]], [-2.0, 0.0, 0.0],),
        Some([0.0, std::f64::consts::TAU])
    );
    assert_eq!(
        periodic_conic_edge_parameter_range(&circle, [points[0], points[0]], points[0]),
        None
    );
}

#[test]
fn conic_parameters_preserve_ellipse_axis_scales() {
    let ellipse = ellipse();
    let points = [[4.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let root_two = std::f64::consts::SQRT_2;
    assert!(curve_contains_points(&ellipse, points));
    assert!(!curve_contains_points(
        &ellipse,
        [points[0], [0.0, 4.0, 0.0]],
    ));
    assert_eq!(
        periodic_conic_edge_parameter_range(&ellipse, points, [2.0 * root_two, root_two, 0.0],),
        Some([0.0, std::f64::consts::FRAC_PI_2])
    );
    assert_eq!(
        periodic_conic_edge_parameter_range(&ellipse, points, [-2.0 * root_two, -root_two, 0.0],),
        Some([std::f64::consts::FRAC_PI_2, std::f64::consts::TAU])
    );
    assert_eq!(
        periodic_conic_edge_parameter_range(&ellipse, [points[0], points[0]], [-4.0, 0.0, 0.0],),
        Some([0.0, std::f64::consts::TAU])
    );
}

#[test]
fn closed_periodic_conic_uses_one_full_period_from_its_seam() {
    let circle = circle();
    let range = full_periodic_conic_edge_parameter_range(&circle, [0.0, 2.0, 0.0])
        .expect("full circle range");
    assert!((range[0] - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!((range[1] - (std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU)).abs() < 1e-12);

    let ellipse = CurveGeometry::Ellipse {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 4.0,
        minor_radius: 2.0,
    };
    assert_eq!(
        full_periodic_conic_edge_parameter_range(&ellipse, [4.0, 0.0, 0.0]),
        Some([0.0, std::f64::consts::TAU])
    );
    assert_eq!(
        full_periodic_conic_edge_parameter_range(&ellipse, [4.0, 0.1, 0.0]),
        None
    );
}

#[test]
fn nonperiodic_conics_recover_their_native_parameters() {
    let parabola = CurveGeometry::Parabola {
        vertex: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        focal_distance: 2.0,
    };
    let parabola_points = [evaluated(&parabola, 3.0), evaluated(&parabola, -2.0)];
    assert_eq!(
        nonperiodic_conic_edge_parameter_range(&parabola, parabola_points),
        Some([-2.0, 3.0])
    );
    assert_eq!(
        nonperiodic_conic_parameter(&parabola, [1.0, 6.0, 3.0]),
        None
    );

    let hyperbola = CurveGeometry::Hyperbola {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 3.0,
        minor_radius: 2.0,
    };
    let hyperbola_points = [evaluated(&hyperbola, 2.0), evaluated(&hyperbola, -1.0)];
    let range = nonperiodic_conic_edge_parameter_range(&hyperbola, hyperbola_points)
        .expect("hyperbola range");
    assert!((range[0] + 1.0).abs() <= 1e-12);
    assert!((range[1] - 2.0).abs() <= 1e-12);
    assert_eq!(
        nonperiodic_conic_parameter(&hyperbola, [-2.0, 2.0, 3.0]),
        None
    );
}

#[test]
fn solved_endpoints_select_one_hyperbola_branch() {
    let hyperbola = CurveGeometry::Hyperbola {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 3.0,
        minor_radius: 2.0,
    };
    let branches = analytic_curve_branches(&hyperbola, "hyperbola");
    let points = [
        evaluated(&branches[1].0, -1.0),
        evaluated(&branches[1].0, 2.0),
    ];
    let selected = select_unique_curve_candidate(branches, points).expect("one branch");
    let CurveGeometry::Hyperbola {
        major_direction, ..
    } = selected.0
    else {
        panic!("hyperbola branch");
    };
    assert_eq!(major_direction, Vector3::new(-1.0, 0.0, 0.0));
}

#[test]
fn surface_pcurve_midpoint_retains_periodic_path() {
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let midpoint = native_pcurve_midpoint(
        &cylinder,
        [[0.0, 0.0], [-3.0 * std::f64::consts::FRAC_PI_2, 0.0]],
        [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
    )
    .expect("periodic midpoint");
    assert!((midpoint[0] + std::f64::consts::SQRT_2).abs() <= 1e-12);
    assert!((midpoint[1] + std::f64::consts::SQRT_2).abs() <= 1e-12);
}

#[test]
fn adjacent_face_pcurves_must_select_the_same_circle_arc() {
    let surface_geometry = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let surfaces = [10, 11]
        .map(|face| Surface {
            id: SurfaceId(format!("creo:visibgeom:surface#{face}")),
            geometry: surface_geometry.clone(),
            source_object: None,
        })
        .to_vec();
    let points = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let mut candidates = NativePcurveCandidates::new();
    candidates.insert(
        (7, 10),
        vec![([[0.0, 0.0], [std::f64::consts::FRAC_PI_2, 0.0]], 10)],
    );
    candidates.insert(
        (7, 11),
        vec![([[0.0, 0.0], [std::f64::consts::FRAC_PI_2, 0.0]], 20)],
    );
    assert_eq!(
        pcurve_backed_periodic_conic_parameter_range(
            &circle(),
            7,
            [10, 11],
            &candidates,
            &surfaces,
            points,
        ),
        Some([0.0, std::f64::consts::FRAC_PI_2])
    );

    candidates.insert(
        (7, 11),
        vec![([[0.0, 0.0], [-3.0 * std::f64::consts::FRAC_PI_2, 0.0]], 20)],
    );
    assert_eq!(
        pcurve_backed_periodic_conic_parameter_range(
            &circle(),
            7,
            [10, 11],
            &candidates,
            &surfaces,
            points,
        ),
        None
    );
}
