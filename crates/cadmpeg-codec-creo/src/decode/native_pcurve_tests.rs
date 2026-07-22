use super::*;

#[test]
fn reconciles_pcurve_endpoints_across_evaluable_face_charts() {
    let mut ir = CadIr::empty(Units::default());
    for (id, normal, u_axis) in [
        (1, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        (2, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
    ] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: None,
        });
    }
    assert_eq!(
        mapped_pcurve_endpoints(
            &ir,
            [1, 2],
            [[[1.0, 2.0], [3.0, 4.0]], [[2.0, 1.0], [4.0, 3.0]]],
        ),
        Some([[1.0, 2.0, 0.0], [3.0, 4.0, 0.0]])
    );
    assert!(mapped_pcurve_endpoints(
        &ir,
        [1, 2],
        [[[1.0, 2.0], [3.0, 4.0]], [[2.0, 1.0], [5.0, 3.0]]],
    )
    .is_none());
}

#[test]
fn identifies_linear_pcurves_that_are_exact_model_lines() {
    let plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 0.5,
        half_angle: 0.25,
    };
    let sphere = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };

    assert!(linear_pcurve_is_straight(&plane, [[1.0, 2.0], [3.0, 4.0]]));
    assert!(linear_pcurve_is_straight(
        &cylinder,
        [[1.0, 2.0], [1.0, 4.0]]
    ));
    assert!(linear_pcurve_is_straight(&cone, [[1.0, 2.0], [1.0, 4.0]]));
    assert!(!linear_pcurve_is_straight(
        &cylinder,
        [[1.0, 2.0], [2.0, 4.0]]
    ));
    assert!(!linear_pcurve_is_straight(
        &sphere,
        [[1.0, 2.0], [1.0, 4.0]]
    ));
}

#[test]
fn propagates_unique_pcurve_endpoints_through_a_vertex_component() {
    let a = [1.0, 0.0, 0.0];
    let b = [2.0, 0.0, 0.0];
    let c = [3.0, 0.0, 0.0];
    let constraints = [([1, 2], [a, b]), ([2, 3], [c, b])];
    assert_eq!(
        solve_pcurve_vertex_domains(
            &constraints,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        ),
        BTreeMap::from([(1, a), (2, b), (3, c)])
    );
    assert!(solve_pcurve_vertex_domains(
        &constraints[..1],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .is_empty());
    assert!(solve_pcurve_vertex_domains(
        &constraints,
        &BTreeMap::from([(2, Some([9.0, 0.0, 0.0]))]),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .is_empty());

    let line = CurveGeometry::Line {
        origin: Point3::new(a[0], a[1], a[2]),
        direction: Vector3::new(0.0, 1.0, 0.0),
    };
    assert_eq!(
        solve_pcurve_vertex_domains(
            &constraints[..1],
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::from([(1, vec![&line])]),
        ),
        BTreeMap::from([(1, a), (2, b)])
    );

    let analytic_domains = BTreeMap::from([(1, vec![a, c])]);
    assert!(solve_pcurve_vertex_domains(
        &[],
        &BTreeMap::new(),
        &analytic_domains,
        &BTreeMap::new(),
    )
    .is_empty());
    assert_eq!(
        solve_pcurve_vertex_domains(
            &constraints[..1],
            &BTreeMap::new(),
            &analytic_domains,
            &BTreeMap::new(),
        ),
        BTreeMap::from([(1, a), (2, b)])
    );
}

#[test]
fn pcurve_direction_flags_assign_endpoint_order() {
    let points = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    assert_eq!(directed_pcurve_points([0x01, 0xf6], points), Some(points));
    assert_eq!(
        directed_pcurve_points([0xf6, 0x01], points),
        Some([points[1], points[0]])
    );
    assert_eq!(directed_pcurve_points([0x01, 0x01], points), None);
}

fn plane() -> SurfaceGeometry {
    SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 3.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    }
}

fn assert_pcurve_matches_curve(
    surface: &SurfaceGeometry,
    curve: &CurveGeometry,
    pcurve: &PcurveGeometry,
    parameters: &[f64],
) {
    for parameter in parameters {
        let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, *parameter).expect("pcurve point");
        let mapped = cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v).expect("surface point");
        let expected = cadmpeg_ir::eval::curve_point(curve, *parameter).expect("curve point");
        assert!((mapped.x - expected.x).abs() <= 1e-10);
        assert!((mapped.y - expected.y).abs() <= 1e-10);
        assert!((mapped.z - expected.z).abs() <= 1e-10);
    }
}

#[test]
fn orients_uv_endpoints_by_the_coedge_traversal() {
    let endpoints = [[2.0, 4.0], [5.0, 7.0]];
    assert_eq!(
        oriented_native_pcurve_endpoints(&plane(), endpoints, [[5.0, 7.0, 3.0], [2.0, 4.0, 3.0]],),
        Some([endpoints[1], endpoints[0]])
    );
}

#[test]
fn withholds_uv_endpoints_that_do_not_map_to_the_edge() {
    assert_eq!(
        oriented_native_pcurve_endpoints(
            &plane(),
            [[2.0, 4.0], [5.0, 7.0]],
            [[2.0, 4.0, 3.0], [9.0, 7.0, 3.0]],
        ),
        None
    );
}

#[test]
fn reconciles_agreeing_source_forms_and_rejects_competing_paths() {
    let traversal = [[2.0, 4.0, 3.0], [5.0, 7.0, 3.0]];
    let endpoints = [[2.0, 4.0], [5.0, 7.0]];
    assert_eq!(
        unique_oriented_native_pcurve(
            &plane(),
            &[(endpoints, 20), ([endpoints[1], endpoints[0]], 10)],
            traversal,
        ),
        Some((endpoints, 10))
    );
    assert_eq!(
        unique_oriented_native_pcurve(
            &plane(),
            &[(endpoints, 20), ([[2.0, 4.0], [5.0, 8.0]], 10)],
            traversal,
        ),
        Some((endpoints, 20))
    );

    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    assert_eq!(
        unique_oriented_native_pcurve(
            &cylinder,
            &[
                ([[0.0, 0.0], [std::f64::consts::FRAC_PI_2, 0.0]], 10),
                (
                    [
                        [std::f64::consts::TAU, 0.0],
                        [std::f64::consts::TAU + std::f64::consts::FRAC_PI_2, 0.0],
                    ],
                    20,
                ),
            ],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        ),
        None
    );
}

#[test]
fn projects_exact_planar_carriers_without_changing_parameters() {
    let circle = CurveGeometry::Circle {
        center: Point3::new(2.0, 4.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(0.0, 1.0, 0.0),
        radius: 2.0,
    };
    assert!(matches!(
        planar_curve_pcurve(&plane(), &circle),
        Some(PcurveGeometry::Circle { center, x_axis, y_axis, radius })
            if center == Point2::new(2.0, 4.0)
                && x_axis == Point2::new(0.0, 1.0)
                && y_axis == Point2::new(-1.0, 0.0)
                && radius == 2.0
    ));

    let nurbs = CurveGeometry::Nurbs(NurbsCurve {
        degree: 1,
        knots: vec![2.0, 2.0, 5.0, 5.0],
        control_points: vec![Point3::new(2.0, 4.0, 3.0), Point3::new(5.0, 7.0, 3.0)],
        weights: Some(vec![2.0, 1.0]),
        periodic: false,
    });
    assert!(matches!(
        planar_curve_pcurve(&plane(), &nurbs),
        Some(PcurveGeometry::Nurbs { degree: 1, knots, control_points, weights: Some(weights), periodic: false })
            if knots == [2.0, 2.0, 5.0, 5.0]
                && control_points == [Point2::new(2.0, 4.0), Point2::new(5.0, 7.0)]
                && weights == [2.0, 1.0]
    ));

    let off_plane = CurveGeometry::Line {
        origin: Point3::new(0.0, 0.0, 3.1),
        direction: Vector3::new(1.0, 0.0, 0.0),
    };
    assert!(planar_curve_pcurve(&plane(), &off_plane).is_none());

    let malformed_nurbs = CurveGeometry::Nurbs(NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0],
        control_points: vec![Point3::new(0.0, 0.0, 3.0), Point3::new(1.0, 0.0, 3.0)],
        weights: None,
        periodic: false,
    });
    assert!(planar_curve_pcurve(&plane(), &malformed_nurbs).is_none());
}

#[test]
fn projects_a_coaxial_cylinder_circle_with_its_native_angle() {
    let surface = SurfaceGeometry::Cylinder {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let circle = CurveGeometry::Circle {
        center: Point3::new(1.0, 2.0, 8.0),
        axis: Vector3::new(0.0, 0.0, -1.0),
        ref_direction: Vector3::new(0.0, 1.0, 0.0),
        radius: 2.0,
    };
    let pcurve = surface_of_revolution_parallel_pcurve(&surface, &circle).expect("cylinder pcurve");
    let PcurveGeometry::Line { origin, direction } = &pcurve else {
        panic!("cylinder-circle pcurve: {pcurve:#?}");
    };
    assert!((origin.u - std::f64::consts::FRAC_PI_2).abs() <= 1e-12);
    assert!((origin.v - 5.0).abs() <= 1e-12);
    assert_eq!(*direction, Point2::new(-1.0, 0.0));
    assert_pcurve_matches_curve(&surface, &circle, &pcurve, &[-2.0, 0.0, 1.25, 4.0]);

    let off_axis = CurveGeometry::Circle {
        center: Point3::new(1.1, 2.0, 8.0),
        axis: Vector3::new(0.0, 0.0, -1.0),
        ref_direction: Vector3::new(0.0, 1.0, 0.0),
        radius: 2.0,
    };
    assert!(surface_of_revolution_parallel_pcurve(&surface, &off_axis).is_none());

    let scaled_frame = SurfaceGeometry::Cylinder {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 2.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    assert!(surface_of_revolution_parallel_pcurve(&scaled_frame, &circle).is_none());
}

#[test]
fn projects_cone_parallel_conics_on_either_side_of_the_apex() {
    let surface = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    for (height, radius, expected_phase) in [(3.0, 5.0, 0.0), (-3.0, 1.0, std::f64::consts::PI)] {
        let circle = CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, height),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius,
        };
        let pcurve =
            surface_of_revolution_parallel_pcurve(&surface, &circle).expect("cone section pcurve");
        let PcurveGeometry::Line { origin, direction } = &pcurve else {
            panic!("cone-circle pcurve: {pcurve:#?}");
        };
        assert!((origin.u - expected_phase).sin().abs() <= 1e-12);
        assert!(((origin.u - expected_phase).cos() - 1.0).abs() <= 1e-12);
        assert!((origin.v - height).abs() <= 1e-12);
        assert_eq!(*direction, Point2::new(1.0, 0.0));
        assert_pcurve_matches_curve(&surface, &circle, &pcurve, &[-1.0, 0.0, 2.0]);
    }

    let elliptical = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 0.5,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    let circle = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };
    assert!(surface_of_revolution_parallel_pcurve(&elliptical, &circle).is_none());
    for (height, major_radius, minor_radius, expected_phase) in
        [(3.0, 5.0, 2.5, 0.0), (-3.0, 1.0, 0.5, std::f64::consts::PI)]
    {
        let ellipse = CurveGeometry::Ellipse {
            center: Point3::new(0.0, 0.0, height),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius,
            minor_radius,
        };
        let pcurve = surface_of_revolution_parallel_pcurve(&elliptical, &ellipse)
            .expect("elliptical cone parallel pcurve");
        let PcurveGeometry::Line { origin, direction } = &pcurve else {
            panic!("cone-ellipse pcurve: {pcurve:#?}");
        };
        assert!((origin.u - expected_phase).sin().abs() <= 1e-12);
        assert!(((origin.u - expected_phase).cos() - 1.0).abs() <= 1e-12);
        assert!((origin.v - height).abs() <= 1e-12);
        assert_eq!(*direction, Point2::new(1.0, 0.0));
        assert_pcurve_matches_curve(&elliptical, &ellipse, &pcurve, &[-1.0, 0.0, 2.0]);
    }
}

#[test]
fn projects_sphere_latitude_circles_to_the_canonical_polar_chart() {
    let surface = SurfaceGeometry::Sphere {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };
    for axial in [-3.0, 3.0] {
        let circle = CurveGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.0 + axial),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 4.0,
        };
        let pcurve = surface_of_revolution_parallel_pcurve(&surface, &circle)
            .expect("sphere latitude pcurve");
        let PcurveGeometry::Line { origin, direction } = &pcurve else {
            panic!("sphere-circle pcurve: {pcurve:#?}");
        };
        assert!((origin.u - std::f64::consts::FRAC_PI_2).abs() <= 1e-12);
        assert!((origin.v - axial.atan2(4.0)).abs() <= 1e-12);
        assert_eq!(*direction, Point2::new(1.0, 0.0));
        assert_pcurve_matches_curve(&surface, &circle, &pcurve, &[-1.0, 0.0, 2.0]);
    }

    let invalid_circle = CurveGeometry::Circle {
        center: Point3::new(1.0, 2.0, 6.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.1,
    };
    assert!(surface_of_revolution_parallel_pcurve(&surface, &invalid_circle).is_none());
}

#[test]
fn projects_torus_parallel_circles_with_signed_ring_branches() {
    for (major_radius, minor_radius, polar, circle_radius, expected_phase) in [
        (4.0, 1.0, std::f64::consts::FRAC_PI_2, 4.0, 0.0),
        (1.0, 2.0, std::f64::consts::PI, 1.0, std::f64::consts::PI),
    ] {
        let surface = SurfaceGeometry::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius,
            minor_radius,
        };
        let circle = CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, minor_radius * polar.sin()),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: circle_radius,
        };
        let pcurve = surface_of_revolution_parallel_pcurve(&surface, &circle)
            .expect("torus parallel pcurve");
        let PcurveGeometry::Line { origin, direction } = &pcurve else {
            panic!("torus-circle pcurve: {pcurve:#?}");
        };
        assert!((origin.u - expected_phase).sin().abs() <= 1e-12);
        assert!(((origin.u - expected_phase).cos() - 1.0).abs() <= 1e-12);
        assert!((origin.v - polar).sin().abs() <= 1e-12);
        assert!(((origin.v - polar).cos() - 1.0).abs() <= 1e-12);
        assert_eq!(*direction, Point2::new(1.0, 0.0));
        assert_pcurve_matches_curve(&surface, &circle, &pcurve, &[-1.0, 0.0, 2.0]);
    }
}

#[test]
fn projects_torus_meridian_circles_with_native_angle_phase() {
    let surface = SurfaceGeometry::Torus {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 4.0,
        minor_radius: 1.5,
    };
    let circle = CurveGeometry::Circle {
        center: Point3::new(1.0, 6.0, 3.0),
        axis: Vector3::new(1.0, 0.0, 0.0),
        ref_direction: Vector3::new(0.0, 0.0, 1.0),
        radius: 1.5,
    };
    let pcurve = meridian_circle_pcurve(&surface, &circle).expect("meridian pcurve");
    let PcurveGeometry::Line { origin, direction } = &pcurve else {
        panic!("torus-meridian pcurve: {pcurve:#?}");
    };
    assert!((origin.u - std::f64::consts::FRAC_PI_2).abs() <= 1e-12);
    assert!((origin.v - std::f64::consts::FRAC_PI_2).abs() <= 1e-12);
    assert_eq!(*direction, Point2::new(0.0, 1.0));
    assert_pcurve_matches_curve(&surface, &circle, &pcurve, &[-1.0, 0.0, 2.0]);

    let displaced = CurveGeometry::Circle {
        center: Point3::new(1.1, 6.0, 3.0),
        axis: Vector3::new(1.0, 0.0, 0.0),
        ref_direction: Vector3::new(0.0, 0.0, 1.0),
        radius: 1.5,
    };
    assert!(meridian_circle_pcurve(&surface, &displaced).is_none());
}

#[test]
fn projects_sphere_meridians_through_both_poles() {
    let surface = SurfaceGeometry::Sphere {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };
    let circle = CurveGeometry::Circle {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        ref_direction: Vector3::new(0.0, 0.0, 1.0),
        radius: 5.0,
    };
    let pcurve = meridian_circle_pcurve(&surface, &circle).expect("sphere meridian pcurve");
    let PcurveGeometry::Line { origin, direction } = &pcurve else {
        panic!("sphere-meridian pcurve: {pcurve:#?}");
    };
    assert!(origin.u.abs() <= 1e-12);
    assert!((origin.v - std::f64::consts::FRAC_PI_2).abs() <= 1e-12);
    assert_eq!(*direction, Point2::new(0.0, -1.0));
    assert_pcurve_matches_curve(
        &surface,
        &circle,
        &pcurve,
        &[
            -std::f64::consts::PI,
            -std::f64::consts::FRAC_PI_2,
            0.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
        ],
    );

    let small_circle = CurveGeometry::Circle {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        ref_direction: Vector3::new(0.0, 0.0, 1.0),
        radius: 4.0,
    };
    assert!(meridian_circle_pcurve(&surface, &small_circle).is_none());
}

#[test]
fn projects_cylinder_and_cone_generators_with_native_line_parameters() {
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let cylinder_line = CurveGeometry::Line {
        origin: Point3::new(1.0, 4.0, 8.0),
        direction: Vector3::new(0.0, 0.0, -2.0),
    };
    let pcurve =
        ruled_generator_line_pcurve(&cylinder, &cylinder_line).expect("cylinder generator pcurve");
    let PcurveGeometry::Line { origin, direction } = &pcurve else {
        panic!("cylinder-generator pcurve: {pcurve:#?}");
    };
    assert!((origin.u - std::f64::consts::FRAC_PI_2).abs() <= 1e-12);
    assert!((origin.v - 5.0).abs() <= 1e-12);
    assert_eq!(*direction, Point2::new(0.0, -2.0));
    assert_pcurve_matches_curve(&cylinder, &cylinder_line, &pcurve, &[-1.0, 0.0, 2.0]);
    let tiny_skew = CurveGeometry::Line {
        origin: Point3::new(1.0, 4.0, 8.0),
        direction: Vector3::new(1e-13, 0.0, 1e-13),
    };
    assert!(ruled_generator_line_pcurve(&cylinder, &tiny_skew).is_none());

    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    let cone_line = CurveGeometry::Line {
        origin: Point3::new(0.0, 5.0, 3.0),
        direction: Vector3::new(0.0, 2.0, 2.0),
    };
    let pcurve = ruled_generator_line_pcurve(&cone, &cone_line).expect("cone generator pcurve");
    let PcurveGeometry::Line { origin, direction } = &pcurve else {
        panic!("cone-generator pcurve: {pcurve:#?}");
    };
    assert!((origin.u - std::f64::consts::FRAC_PI_2).abs() <= 1e-12);
    assert!((origin.v - 3.0).abs() <= 1e-12);
    assert!(direction.u.abs() <= 1e-12);
    assert!((direction.v - 2.0).abs() <= 1e-12);
    assert_pcurve_matches_curve(&cone, &cone_line, &pcurve, &[-1.0, 0.0, 2.0]);

    let elliptical_cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 0.5,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    let root_half = std::f64::consts::FRAC_1_SQRT_2;
    let elliptical_generator = CurveGeometry::Line {
        origin: Point3::new(5.0 * root_half, 2.5 * root_half, 3.0),
        direction: Vector3::new(2.0 * root_half, root_half, 2.0),
    };
    let pcurve = ruled_generator_line_pcurve(&elliptical_cone, &elliptical_generator)
        .expect("elliptical cone generator pcurve");
    let PcurveGeometry::Line { origin, direction } = &pcurve else {
        panic!("elliptical-cone generator pcurve: {pcurve:#?}");
    };
    assert!((origin.u - std::f64::consts::FRAC_PI_4).abs() <= 1e-12);
    assert!((origin.v - 3.0).abs() <= 1e-12);
    assert!(direction.u.abs() <= 1e-12);
    assert!((direction.v - 2.0).abs() <= 1e-12);
    assert_pcurve_matches_curve(
        &elliptical_cone,
        &elliptical_generator,
        &pcurve,
        &[-3.0, 0.0, 2.0],
    );

    let skew = CurveGeometry::Line {
        origin: Point3::new(0.0, 5.0, 3.0),
        direction: Vector3::new(0.1, 2.0, 2.0),
    };
    assert!(ruled_generator_line_pcurve(&cone, &skew).is_none());
}
