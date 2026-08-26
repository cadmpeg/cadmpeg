// SPDX-License-Identifier: Apache-2.0
//! Tests: carrier solver.

use crate::decode::analytic::{
    intersect_plane_with_two_quadrics, intersect_two_planes_with_torus, point_on_carrier,
    solve_carriers, CarrierEquation, ConeEquation, CylinderEquation, PlaneEquation, SphereEquation,
    TorusEquation,
};
use crate::decode::surfaces::{
    apex_plane_cone_generator_candidates, axis_normal_plane_torus_circle_candidates,
    carrier_intersection_curve, coaxial_cone_cylinder_circle_candidates,
    coaxial_cone_sphere_circle_candidates, coaxial_cylinder_sphere_circle_candidates,
    coaxial_cylinder_torus_circle_candidates, coaxial_sphere_torus_circle_candidates,
    coaxial_tori_circle_candidates, fc14_held_coordinate, parallel_cylinder_generator_candidates,
    parallel_plane_cylinder_generator_candidates, select_fc14_axis_coordinate_candidate,
    select_unique_curve_candidate,
};
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::math::Point3;

#[test]
fn carrier_solver_accepts_unique_plane_plane_quadric_vertices() {
    let cylinder = CarrierEquation::Cylinder(CylinderEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
    });
    let cap = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 3.0],
        normal: [0.0, 0.0, 1.0],
    });
    let tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [2.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[cylinder, cap, tangent]),
        Some([2.0, 0.0, 3.0])
    );
    let x_axis_cylinder = CarrierEquation::Cylinder(CylinderEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [1.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 1.0,
    });
    let y_axis_cylinder = CarrierEquation::Cylinder(CylinderEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 1.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 1.0,
    });
    let tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 1.0],
        normal: [0.0, 0.0, 1.0],
    });
    assert_eq!(
        solve_carriers(&[x_axis_cylinder, y_axis_cylinder, tangent_plane]),
        Some([0.0, 0.0, 1.0])
    );
    let cone = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 1.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    });
    let offset_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 1.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    });
    let generator_parallel_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [1.0, 0.0, -1.0],
    });
    assert_eq!(
        solve_carriers(&[cone, offset_plane, generator_parallel_plane]),
        Some([0.0, 1.0, 0.0])
    );
    let secant_plane = PlaneEquation {
        origin: [1.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    };
    let mut secant_points =
        intersect_plane_with_two_quadrics(secant_plane, x_axis_cylinder, y_axis_cylinder);
    secant_points.sort_by(|left, right| left[1].total_cmp(&right[1]));
    assert_eq!(secant_points, vec![[1.0, -1.0, 0.0], [1.0, 1.0, 0.0]]);
    assert_eq!(
        solve_carriers(&[
            x_axis_cylinder,
            y_axis_cylinder,
            CarrierEquation::Plane(secant_plane),
        ]),
        None
    );

    let secant = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(solve_carriers(&[cylinder, cap, secant]), None);

    assert!(matches!(
        carrier_intersection_curve(cap, cylinder),
        Some((CurveGeometry::Circle { center, radius, .. }, "plane_cylinder_circle"))
            if center.z == 3.0 && radius == 2.0
    ));
    let oblique = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 1.0, 1.0],
    });
    assert!(matches!(
        carrier_intersection_curve(oblique, cylinder),
        Some((CurveGeometry::Ellipse { major_radius, minor_radius, .. }, "plane_cylinder_ellipse"))
            if (major_radius - 2.0 * 2.0_f64.sqrt()).abs() < 1.0e-12
                && minor_radius == 2.0
    ));
    assert!(matches!(
        carrier_intersection_curve(tangent, cylinder),
        Some((CurveGeometry::Line { origin, direction }, "plane_cylinder_tangent_line"))
            if origin.x == 2.0 && direction.z == 1.0
    ));
    assert!(carrier_intersection_curve(secant, cylinder).is_none());
    let generators = parallel_plane_cylinder_generator_candidates(secant, cylinder);
    assert_eq!(generators.len(), 2);
    assert!(matches!(
        select_unique_curve_candidate(
            parallel_plane_cylinder_generator_candidates(secant, cylinder),
            [[0.0, 2.0, -1.0], [0.0, 2.0, 4.0]],
        ),
        Some((CurveGeometry::Line { origin, direction }, "plane_cylinder_secant_generator"))
            if (origin.y - 2.0).abs() < 1.0e-12 && direction.z == 1.0
    ));
    assert!(select_unique_curve_candidate(
        parallel_plane_cylinder_generator_candidates(secant, cylinder),
        [[0.0, 0.0, -1.0], [0.0, 0.0, 4.0]],
    )
    .is_none());

    let parallel_cylinder = |origin: [f64; 3], radius| {
        CarrierEquation::Cylinder(CylinderEquation {
            origin,
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius,
        })
    };
    assert!(matches!(
        carrier_intersection_curve(
            parallel_cylinder([0.0, 0.0, 0.0], 2.0),
            parallel_cylinder([5.0, 0.0, 0.0], 3.0),
        ),
        Some((CurveGeometry::Line { origin, direction }, "parallel_cylinder_tangent_line"))
            if origin.x == 2.0 && direction.z == 1.0
    ));
    assert_eq!(
        solve_carriers(&[
            cap,
            parallel_cylinder([0.0, 0.0, 0.0], 2.0),
            parallel_cylinder([5.0, 0.0, 0.0], 3.0),
        ]),
        Some([2.0, 0.0, 3.0])
    );
    assert!(matches!(
        carrier_intersection_curve(
            parallel_cylinder([0.0, 0.0, 0.0], 5.0),
            parallel_cylinder([3.0, 0.0, 0.0], 2.0),
        ),
        Some((CurveGeometry::Line { origin, .. }, "parallel_cylinder_tangent_line"))
            if origin.x == 5.0
    ));
    assert!(carrier_intersection_curve(
        parallel_cylinder([0.0, 0.0, 0.0], 3.0),
        parallel_cylinder([4.0, 0.0, 0.0], 3.0),
    )
    .is_none());
    let secant_cylinders = [
        parallel_cylinder([0.0, 0.0, 0.0], 3.0),
        parallel_cylinder([4.0, 0.0, 1.0], 3.0),
    ];
    assert_eq!(
        parallel_cylinder_generator_candidates(secant_cylinders[0], secant_cylinders[1]).len(),
        2
    );
    let height = 5.0_f64.sqrt();
    assert!(matches!(
        select_unique_curve_candidate(
            parallel_cylinder_generator_candidates(
                secant_cylinders[0],
                secant_cylinders[1]
            ),
            [[2.0, height, -2.0], [2.0, height, 4.0]],
        ),
        Some((CurveGeometry::Line { origin, direction }, "parallel_cylinder_secant_generator"))
            if (origin.x - 2.0).abs() < 1.0e-12
                && (origin.y - height).abs() < 1.0e-12
                && direction.z == 1.0
    ));
    assert!(select_unique_curve_candidate(
        parallel_cylinder_generator_candidates(secant_cylinders[0], secant_cylinders[1]),
        [[2.0, 0.0, -2.0], [2.0, 0.0, 4.0]],
    )
    .is_none());

    let sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
    });
    let equator = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    });
    assert!(matches!(
        carrier_intersection_curve(equator, sphere),
        Some((CurveGeometry::Circle { center, radius, .. }, "plane_sphere_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && radius == 2.0
    ));
    assert_eq!(solve_carriers(&[equator, secant, sphere]), None);
    assert_eq!(
        solve_carriers(&[equator, tangent, sphere]),
        Some([2.0, 0.0, 0.0])
    );
    let second_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [4.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 3.0,
    });
    let first_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 3.0,
    });
    assert!(matches!(
        carrier_intersection_curve(first_sphere, second_sphere),
        Some((CurveGeometry::Circle { center, radius, .. }, "sphere_intersection_circle"))
            if center.x == 2.0 && (radius - 5.0_f64.sqrt()).abs() < 1.0e-12
    ));
    let sphere_circle_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 5.0_f64.sqrt(), 0.0],
        normal: [0.0, 1.0, 0.0],
    });
    let sphere_circle_point = solve_carriers(&[first_sphere, second_sphere, sphere_circle_tangent])
        .expect("unique sphere-circle tangent point");
    assert!((sphere_circle_point[0] - 2.0).abs() < 1.0e-12);
    assert!((sphere_circle_point[1] - 5.0_f64.sqrt()).abs() < 1.0e-12);
    assert!(sphere_circle_point[2].abs() < 1.0e-12);
    let external_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [5.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 3.0,
    });
    assert_eq!(
        solve_carriers(&[sphere, external_tangent_sphere, equator]),
        Some([2.0, 0.0, 0.0])
    );
    let noncoaxial_cylinder = CarrierEquation::Cylinder(CylinderEquation {
        origin: [1.0, 3.0_f64.sqrt(), 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
    });
    assert_eq!(
        solve_carriers(&[sphere, tangent, noncoaxial_cylinder]),
        Some([2.0, 0.0, 0.0])
    );
    let enclosing_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 5.0,
    });
    let internally_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [3.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 2.0,
    });
    assert_eq!(
        solve_carriers(&[enclosing_sphere, internally_tangent_sphere, equator]),
        Some([5.0, 0.0, 0.0])
    );
    assert!(matches!(
        carrier_intersection_curve(cylinder, sphere),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_sphere_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && radius == 2.0
    ));
    assert_eq!(
        solve_carriers(&[cylinder, sphere, tangent]),
        Some([2.0, 0.0, 0.0])
    );
    assert!(carrier_intersection_curve(parallel_cylinder([0.0, 0.0, 0.0], 1.0), sphere,).is_none());
    let coaxial_secant = parallel_cylinder([0.0, 0.0, 0.0], 1.0);
    let sphere_offset = 3.0_f64.sqrt();
    assert_eq!(
        coaxial_cylinder_sphere_circle_candidates(coaxial_secant, sphere).len(),
        2
    );
    assert!(matches!(
        select_unique_curve_candidate(
            coaxial_cylinder_sphere_circle_candidates(coaxial_secant, sphere),
            [[1.0, 0.0, sphere_offset], [-1.0, 0.0, sphere_offset]],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_sphere_secant_circle"))
            if (center.z - sphere_offset).abs() < 1.0e-12 && radius == 1.0
    ));
    assert!(select_unique_curve_candidate(
        coaxial_cylinder_sphere_circle_candidates(coaxial_secant, sphere),
        [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]],
    )
    .is_none());
    let upper_circle_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [
            f64::midpoint(1.0, sphere_offset),
            0.0,
            f64::midpoint(1.0, sphere_offset),
        ],
        normal: [1.0, 0.0, 1.0],
    });
    let solved = solve_carriers(&[coaxial_secant, sphere, upper_circle_tangent])
        .expect("unique upper-circle tangent");
    assert!((solved[0] - 1.0).abs() < 1.0e-12);
    assert!(solved[1].abs() < 1.0e-12);
    assert!((solved[2] - sphere_offset).abs() < 1.0e-12);
    assert_eq!(
        solve_carriers(&[
            coaxial_secant,
            sphere,
            CarrierEquation::Plane(PlaneEquation {
                origin: [1.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            }),
        ]),
        None
    );

    let cone = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    });
    assert!(matches!(
        carrier_intersection_curve(cap, cone),
        Some((CurveGeometry::Circle { center, radius, .. }, "plane_cone_circle"))
            if center == Point3::new(0.0, 0.0, 3.0) && (radius - 5.0).abs() < 1.0e-12
    ));
    let elliptical_cone = CarrierEquation::Cone(ConeEquation {
        origin: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
        ratio: 0.5,
        half_angle: std::f64::consts::FRAC_PI_4,
    });
    assert!(matches!(
        carrier_intersection_curve(cap, elliptical_cone),
        Some((
            CurveGeometry::Ellipse {
                center,
                major_radius,
                minor_radius,
                ..
            },
            "plane_cone_parallel_ellipse"
        )) if center == Point3::new(0.0, 0.0, 3.0)
            && (major_radius - 5.0).abs() < 1.0e-12
            && (minor_radius - 2.5).abs() < 1.0e-12
    ));
    let elliptical_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [5.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[elliptical_cone, cap, elliptical_tangent]),
        Some([5.0, 0.0, 3.0])
    );
    let elliptical_secant = CarrierEquation::Plane(PlaneEquation {
        origin: [3.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[elliptical_cone, cap, elliptical_secant]),
        None
    );
    let inverse_sqrt_two = 1.0 / 2.0_f64.sqrt();
    let cone_tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, -2.0],
        normal: [inverse_sqrt_two, 0.0, inverse_sqrt_two],
    });
    assert!(matches!(
        carrier_intersection_curve(cone_tangent_plane, cone),
        Some((CurveGeometry::Line { origin, direction }, "plane_cone_tangent_line"))
            if origin.x.abs() < 1.0e-12
                && origin.y.abs() < 1.0e-12
                && (origin.z + 2.0).abs() < 1.0e-12
                && (direction.x + inverse_sqrt_two).abs() < 1.0e-12
                && (direction.z - inverse_sqrt_two).abs() < 1.0e-12
    ));
    let (elliptical_tangent_geometry, elliptical_tangent_tag) =
        carrier_intersection_curve(cone_tangent_plane, elliptical_cone)
            .expect("elliptical cone tangent generator");
    assert_eq!(elliptical_tangent_tag, "plane_cone_tangent_line");
    for parameter in [-1.0, 0.0, 1.0] {
        let point = cadmpeg_ir::eval::curve_point(&elliptical_tangent_geometry, parameter)
            .expect("elliptical tangent point");
        let point = [point.x, point.y, point.z];
        assert!(point_on_carrier(point, cone_tangent_plane));
        assert!(point_on_carrier(point, elliptical_cone));
    }
    let cone_ellipse_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [-0.2, 0.0, 1.0],
    });
    assert!(matches!(
        carrier_intersection_curve(cone_ellipse_plane, cone),
        Some((
            CurveGeometry::Ellipse {
                major_radius,
                minor_radius,
                ..
            },
            "plane_cone_ellipse"
        )) if major_radius > minor_radius && minor_radius > 0.0
    ));
    let cone_parabola_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [inverse_sqrt_two, 0.0, inverse_sqrt_two],
    });
    assert!(matches!(
        carrier_intersection_curve(cone_parabola_plane, cone),
        Some((
            CurveGeometry::Parabola { focal_distance, .. },
            "plane_cone_parabola"
        )) if focal_distance > 0.0
    ));
    let cone_hyperbola_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [1.0, 0.0, 0.2],
    });
    assert!(matches!(
        carrier_intersection_curve(cone_hyperbola_plane, cone),
        Some((
            CurveGeometry::Hyperbola {
                major_radius,
                minor_radius,
                ..
            },
            "plane_cone_hyperbola"
        )) if major_radius > 0.0 && minor_radius > 0.0
    ));
    let rotated_ellipse_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [-0.2, -0.3, 1.0],
    });
    for (plane, expected_tag) in [
        (rotated_ellipse_plane, "plane_cone_ellipse"),
        (cone_parabola_plane, "plane_cone_parabola"),
        (cone_hyperbola_plane, "plane_cone_hyperbola"),
    ] {
        let (geometry, tag) =
            carrier_intersection_curve(plane, elliptical_cone).expect("elliptical cone conic");
        assert_eq!(tag, expected_tag);
        for parameter in [-1.0, 0.0, 1.0] {
            let point = cadmpeg_ir::eval::curve_point(&geometry, parameter).expect("conic point");
            let point = [point.x, point.y, point.z];
            assert!(point_on_carrier(point, plane));
            assert!(point_on_carrier(point, elliptical_cone));
        }
    }
    let cone_degenerate_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, -2.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert!(carrier_intersection_curve(cone_degenerate_plane, cone).is_none());
    let cone_generators = apex_plane_cone_generator_candidates(cone_degenerate_plane, cone);
    assert_eq!(cone_generators.len(), 2);
    assert!(matches!(
        select_unique_curve_candidate(
            cone_generators,
            [[0.0, 1.0, -1.0], [0.0, 2.0, 0.0]],
        ),
        Some((CurveGeometry::Line { origin, .. }, "plane_cone_secant_generator"))
            if (origin.z + 2.0).abs() < 1.0e-12
    ));
    let elliptical_generators =
        apex_plane_cone_generator_candidates(cone_degenerate_plane, elliptical_cone);
    assert_eq!(elliptical_generators.len(), 2);
    let (elliptical_generator, tag) =
        select_unique_curve_candidate(elliptical_generators, [[0.0, 1.0, 0.0], [0.0, 2.0, 2.0]])
            .expect("selected elliptical cone generator");
    assert_eq!(tag, "plane_cone_secant_generator");
    for parameter in [-1.0, 0.0, 1.0] {
        let point = cadmpeg_ir::eval::curve_point(&elliptical_generator, parameter)
            .expect("elliptical generator point");
        let point = [point.x, point.y, point.z];
        assert!(point_on_carrier(point, cone_degenerate_plane));
        assert!(point_on_carrier(point, elliptical_cone));
    }
    assert_eq!(solve_carriers(&[cone, cap, tangent]), None);
    let cone_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [5.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[cone, cap, cone_tangent]),
        Some([5.0, 0.0, 3.0])
    );
    let cone_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0_f64.sqrt(),
    });
    assert!(matches!(
        carrier_intersection_curve(cone_tangent_sphere, cone),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_sphere_tangent_circle"))
            if (center.z + 1.0).abs() < 1.0e-12 && (radius - 1.0).abs() < 1.0e-12
    ));
    let cone_sphere_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [1.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    let cone_sphere_vertex =
        solve_carriers(&[cone_tangent_sphere, cone, cone_sphere_plane]).expect("unique vertex");
    assert!((cone_sphere_vertex[0] - 1.0).abs() < 1.0e-12);
    assert!(cone_sphere_vertex[1].abs() < 1.0e-12);
    assert!((cone_sphere_vertex[2] + 1.0).abs() < 1.0e-12);
    assert!(carrier_intersection_curve(sphere, cone).is_none());
    let cone_secant_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 5.0,
    });
    let cone_sphere_candidates = coaxial_cone_sphere_circle_candidates(cone, cone_secant_sphere);
    assert_eq!(cone_sphere_candidates.len(), 2);
    let upper_parameter = (-4.0 + 184.0_f64.sqrt()) / 4.0;
    let upper_radius = 2.0 + upper_parameter;
    assert!(matches!(
        select_unique_curve_candidate(
            cone_sphere_candidates,
            [
                [upper_radius, 0.0, upper_parameter],
                [0.0, upper_radius, upper_parameter],
            ],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_sphere_secant_circle"))
            if (center.z - upper_parameter).abs() < 1.0e-12
                && (radius - upper_radius).abs() < 1.0e-12
    ));

    let coaxial_cone_cylinder = parallel_cylinder([0.0, 0.0, 0.0], 3.0);
    assert!(carrier_intersection_curve(cone, coaxial_cone_cylinder).is_none());
    let cone_cylinder_candidates =
        coaxial_cone_cylinder_circle_candidates(cone, coaxial_cone_cylinder);
    assert_eq!(cone_cylinder_candidates.len(), 2);
    assert!(matches!(
        select_unique_curve_candidate(
            cone_cylinder_candidates,
            [[3.0, 0.0, 1.0], [0.0, 3.0, 1.0]],
        ),
        Some((
            CurveGeometry::Circle { center, radius, .. },
            "coaxial_cone_cylinder_secant_circle"
        )) if (center.z - 1.0).abs() < 1.0e-12 && radius == 3.0
    ));
    let held_token = crate::curve::FcCurveCoordinateToken {
        value_mm: 1.0,
        raw: vec![0x2d, 0, 0, 0, 0, 0, 0, 0],
        offset: 3,
        length: 8,
    };
    let held_coordinates = crate::curve::FcCurveCoordinates {
        curve_id: 77,
        subtype: 0x14,
        body: Vec::new(),
        values_mm: vec![1.0; 4],
        tokens: (0..4)
            .map(|index| crate::curve::FcCurveCoordinateToken {
                offset: held_token.offset + index * held_token.length,
                ..held_token.clone()
            })
            .collect(),
        opaque_spans: Vec::new(),
        offset: 0,
    };
    assert_eq!(
        fc14_held_coordinate(std::slice::from_ref(&held_coordinates), 77),
        Some(1.0)
    );
    assert!(matches!(
        select_fc14_axis_coordinate_candidate(
            coaxial_cone_cylinder_circle_candidates(cone, coaxial_cone_cylinder),
            1.0,
        ),
        Some((
            CurveGeometry::Circle { center, radius, .. },
            "coaxial_cone_cylinder_secant_circle"
        )) if (center.z - 1.0).abs() < 1.0e-12 && radius == 3.0
    ));
    let mut mixed_coordinates = held_coordinates;
    mixed_coordinates.tokens[3].value_mm = -1.0;
    mixed_coordinates.tokens[3].raw[1] = 1;
    assert_eq!(fc14_held_coordinate(&[mixed_coordinates], 77), None);
    assert_eq!(
        select_fc14_axis_coordinate_candidate(
            coaxial_cone_cylinder_circle_candidates(cone, coaxial_cone_cylinder),
            0.0,
        ),
        None
    );
    let cone_cylinder_tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [4.0, 0.0, 0.0],
        normal: [1.0, 0.0, 1.0],
    });
    let cone_cylinder_vertex =
        solve_carriers(&[cone, coaxial_cone_cylinder, cone_cylinder_tangent_plane])
            .expect("unique cone-cylinder circle tangent");
    assert!((cone_cylinder_vertex[0] - 3.0).abs() < 1.0e-12);
    assert!(cone_cylinder_vertex[1].abs() < 1.0e-12);
    assert!((cone_cylinder_vertex[2] - 1.0).abs() < 1.0e-12);
    assert!(
        coaxial_cone_cylinder_circle_candidates(cone, parallel_cylinder([1.0, 0.0, 0.0], 3.0),)
            .is_empty()
    );

    let torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 5.0,
        minor_radius: 2.0,
    });
    let torus_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [0.0, 0.0, 1.0],
    });
    assert!(matches!(
        carrier_intersection_curve(torus_tangent, torus),
        Some((CurveGeometry::Circle { center, radius, .. }, "plane_torus_tangent_circle"))
            if center == Point3::new(0.0, 0.0, 2.0) && radius == 5.0
    ));
    assert!(carrier_intersection_curve(equator, torus).is_none());
    let plane_torus_candidates = axis_normal_plane_torus_circle_candidates(equator, torus);
    assert_eq!(plane_torus_candidates.len(), 2);
    assert!(matches!(
        select_unique_curve_candidate(
            plane_torus_candidates,
            [[7.0, 0.0, 0.0], [0.0, 7.0, 0.0]],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "plane_torus_secant_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && radius == 7.0
    ));
    let outer_tangent_cylinder = parallel_cylinder([0.0, 0.0, 0.0], 7.0);
    assert!(matches!(
        carrier_intersection_curve(outer_tangent_cylinder, torus),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_torus_tangent_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && radius == 7.0
    ));
    let secant_cylinder = parallel_cylinder([0.0, 0.0, 0.0], 6.0);
    let cylinder_torus_candidates =
        coaxial_cylinder_torus_circle_candidates(secant_cylinder, torus);
    assert_eq!(cylinder_torus_candidates.len(), 2);
    let section_height = 3.0_f64.sqrt();
    assert!(matches!(
        select_unique_curve_candidate(
            cylinder_torus_candidates,
            [
                [6.0, 0.0, section_height],
                [0.0, 6.0, section_height],
            ],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_torus_secant_circle"))
            if (center.z - section_height).abs() < 1.0e-12 && radius == 6.0
    ));
    let outer_circle_tangent = CarrierEquation::Plane(PlaneEquation {
        origin: [7.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[outer_tangent_cylinder, torus, outer_circle_tangent]),
        Some([7.0, 0.0, 0.0])
    );
    assert!(carrier_intersection_curve(parallel_cylinder([0.0, 0.0, 0.0], 6.0), torus).is_none());
    let torus_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 3.0,
    });
    assert!(matches!(
        carrier_intersection_curve(torus_tangent_sphere, torus),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_sphere_torus_tangent_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && (radius - 3.0).abs() < 1.0e-12
    ));
    let torus_secant_sphere = CarrierEquation::Sphere(SphereEquation {
        center: [0.0, 0.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 5.0,
    });
    let sphere_torus_candidates =
        coaxial_sphere_torus_circle_candidates(torus_secant_sphere, torus);
    assert_eq!(sphere_torus_candidates.len(), 2);
    let sphere_torus_height = 3.84_f64.sqrt();
    assert!(matches!(
        select_unique_curve_candidate(
            sphere_torus_candidates,
            [
                [4.6, 0.0, sphere_torus_height],
                [0.0, 4.6, sphere_torus_height],
            ],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_sphere_torus_secant_circle"))
            if (center.z - sphere_torus_height).abs() < 1.0e-12
                && (radius - 4.6).abs() < 1.0e-12
    ));
    let torus_sphere_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [3.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[torus_tangent_sphere, torus, torus_sphere_plane]),
        Some([3.0, 0.0, 0.0])
    );
    let outer_tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [7.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    let oblique_tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 1.0, -1.0],
    });
    assert_eq!(
        solve_carriers(&[torus, outer_tangent_plane, oblique_tangent_plane]),
        Some([7.0, 0.0, 0.0])
    );
    let axial_plane = PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    };
    let equatorial_plane = PlaneEquation {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    };
    let mut secant_points = intersect_two_planes_with_torus(
        axial_plane,
        equatorial_plane,
        match torus {
            CarrierEquation::Torus(torus) => torus,
            _ => unreachable!(),
        },
    );
    secant_points.sort_by(|left, right| left[0].total_cmp(&right[0]));
    assert_eq!(
        secant_points,
        vec![
            [-7.0, 0.0, 0.0],
            [-3.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [7.0, 0.0, 0.0]
        ]
    );
    assert!(carrier_intersection_curve(sphere, torus).is_none());
    let second_torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 9.0,
        minor_radius: 2.0,
    });
    assert!(matches!(
        carrier_intersection_curve(torus, second_torus),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_tori_tangent_circle"))
            if center == Point3::new(0.0, 0.0, 0.0) && (radius - 7.0).abs() < 1.0e-12
    ));
    let secant_torus = CarrierEquation::Torus(TorusEquation {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 6.0,
        minor_radius: 2.0,
    });
    let tori_candidates = coaxial_tori_circle_candidates(torus, secant_torus);
    assert_eq!(tori_candidates.len(), 2);
    let tori_height = 3.75_f64.sqrt();
    assert!(matches!(
        select_unique_curve_candidate(
            tori_candidates,
            [[5.5, 0.0, tori_height], [0.0, 5.5, tori_height]],
        ),
        Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_tori_secant_circle"))
            if (center.z - tori_height).abs() < 1.0e-12
                && (radius - 5.5).abs() < 1.0e-12
    ));
    let tori_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [7.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        solve_carriers(&[torus, second_torus, tori_plane]),
        Some([7.0, 0.0, 0.0])
    );
    assert!(point_on_carrier([5.0, 0.0, 2.0], torus));
    assert!(!point_on_carrier([5.0, 0.0, 0.0], torus));
    assert_eq!(
        solve_carriers(&[torus, torus_tangent, cone_tangent]),
        Some([5.0, 0.0, 2.0])
    );
}
