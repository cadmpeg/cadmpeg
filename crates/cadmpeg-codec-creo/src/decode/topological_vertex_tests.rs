use super::*;

fn line(origin: [f64; 3], direction: [f64; 3]) -> CurveGeometry {
    CurveGeometry::Line {
        origin: Point3::new(origin[0], origin[1], origin[2]),
        direction: Vector3::new(direction[0], direction[1], direction[2]),
    }
}

#[test]
fn incident_lines_define_one_validated_vertex() {
    let first = line([1.0, 2.0, 3.0], [2.0, 0.0, 0.0]);
    let second = line([1.0, -4.0, 3.0], [0.0, 3.0, 0.0]);
    let third = line([1.0, 2.0, -5.0], [0.0, 0.0, 4.0]);

    assert_eq!(
        incident_analytic_vertex_domain(&[&first, &second, &third]),
        [[1.0, 2.0, 3.0]]
    );
}

#[test]
fn incident_lines_reject_skew_parallel_and_disagreeing_candidates() {
    let x = line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let skew_y = line([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    let parallel = line([0.0, 1.0, 0.0], [2.0, 0.0, 0.0]);
    let crossing_y = line([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let displaced_z = line([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    assert_eq!(line_line_intersection(&x, &skew_y), None);
    assert_eq!(line_line_intersection(&x, &parallel), None);
    assert!(incident_analytic_vertex_domain(&[&x, &crossing_y, &displaced_z]).is_empty());
}

#[test]
fn line_conic_candidates_cover_periodic_and_nonperiodic_families() {
    let circle = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let secant = line([-3.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let tangent = line([-3.0, 2.0, 0.0], [1.0, 0.0, 0.0]);
    let skew = line([-3.0, 0.0, 1.0], [1.0, 0.0, 0.0]);

    assert_eq!(
        line_conic_intersections(&secant, &circle),
        [[2.0, 0.0, 0.0], [-2.0, 0.0, 0.0]]
    );
    assert_eq!(
        line_conic_intersections(&tangent, &circle),
        [[0.0, 2.0, 0.0]]
    );
    assert!(line_conic_intersections(&skew, &circle).is_empty());

    let ellipse = CurveGeometry::Ellipse {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 3.0,
        minor_radius: 2.0,
    };
    assert_eq!(
        line_conic_intersections(&secant, &ellipse),
        [[3.0, 0.0, 0.0], [-3.0, 0.0, 0.0]]
    );

    let parabola = CurveGeometry::Parabola {
        vertex: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        focal_distance: 1.0,
    };
    assert_eq!(
        line_conic_intersections(&line([1.0, -3.0, 0.0], [0.0, 1.0, 0.0]), &parabola),
        [[1.0, 2.0, 0.0], [1.0, -2.0, 0.0]]
    );
    assert_eq!(
        line_conic_intersections(&line([-3.0, 2.0, 0.0], [1.0, 0.0, 0.0]), &parabola),
        [[1.0, 2.0, 0.0]]
    );

    let hyperbola = CurveGeometry::Hyperbola {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 2.0,
        minor_radius: 1.0,
    };
    let hyperbola_points =
        line_conic_intersections(&line([4.0, -3.0, 0.0], [0.0, 1.0, 0.0]), &hyperbola);
    assert_eq!(hyperbola_points.len(), 2);
    assert!(hyperbola_points.iter().all(|point| {
        model_points_agree(*point, [4.0, 3.0_f64.sqrt(), 0.0])
            || model_points_agree(*point, [4.0, -3.0_f64.sqrt(), 0.0])
    }));
    assert_eq!(
        line_conic_intersections(&line([-3.0, 0.0, 0.0], [1.0, 0.0, 0.0]), &hyperbola),
        [[2.0, 0.0, 0.0]]
    );
}

#[test]
fn conic_pair_candidates_cover_coplanar_and_transverse_planes() {
    let circle = |center: [f64; 3], axis: [f64; 3], radius| {
        let reference = if axis[0].abs() > 0.5 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        CurveGeometry::Circle {
            center: Point3::new(center[0], center[1], center[2]),
            axis: Vector3::new(axis[0], axis[1], axis[2]),
            ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
            radius,
        }
    };
    let first = circle([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.0);
    let secant = circle([2.0, 0.0, 0.0], [0.0, 0.0, -1.0], 2.0);
    let tangent = circle([4.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.0);
    let transverse = circle([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 2.0);

    assert!(conic_conic_intersections(&first, &first).is_empty());
    assert!(
        conic_conic_intersections(&first, &circle([0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 2.0),)
            .is_empty()
    );
    let secant_points = conic_conic_intersections(&first, &secant);
    assert_eq!(secant_points.len(), 2);
    assert!(secant_points.iter().all(|point| {
        model_points_agree(*point, [1.0, 3.0_f64.sqrt(), 0.0])
            || model_points_agree(*point, [1.0, -3.0_f64.sqrt(), 0.0])
    }));
    assert_eq!(
        conic_conic_intersections(&first, &tangent),
        [[2.0, 0.0, 0.0]]
    );
    let transverse_points = conic_conic_intersections(&first, &transverse);
    assert_eq!(transverse_points.len(), 2);
    assert!(transverse_points.iter().all(|point| {
        model_points_agree(*point, [0.0, 2.0, 0.0]) || model_points_agree(*point, [0.0, -2.0, 0.0])
    }));

    let ellipse = CurveGeometry::Ellipse {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 3.0,
        minor_radius: 2.0,
    };
    let ellipse_points = conic_conic_intersections(&first, &ellipse);
    assert_eq!(ellipse_points.len(), 2);
    assert!(ellipse_points.iter().all(|point| {
        model_points_agree(*point, [0.0, 2.0, 0.0]) || model_points_agree(*point, [0.0, -2.0, 0.0])
    }));
    let diagonal_ellipse = CurveGeometry::Ellipse {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 1.0, 0.0),
        major_radius: 3.0,
        minor_radius: 2.0,
    };
    let larger_circle = circle([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.5);
    let diagonal_points = conic_conic_intersections(&larger_circle, &diagonal_ellipse);
    assert_eq!(diagonal_points.len(), 4);
    assert!(diagonal_points.iter().all(|point| {
        curve_contains_points(&larger_circle, [*point, *point])
            && curve_contains_points(&diagonal_ellipse, [*point, *point])
    }));

    let parabola = CurveGeometry::Parabola {
        vertex: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        focal_distance: 1.0,
    };
    let tangent_circle = circle([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0);
    let tangent_points = conic_conic_intersections(&parabola, &tangent_circle);
    assert_eq!(tangent_points.len(), 1);
    assert!(model_points_agree(tangent_points[0], [0.0, 0.0, 0.0]));
}
