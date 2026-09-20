// SPDX-License-Identifier: Apache-2.0

#[test]
fn numerical_ranges_meridian_circles_retain_two_small_intersections() {
    for radius in [1e-7, 1.0, 1e100] {
        let roots = super::meridian_circle_intersections([0., 0.], radius, [radius, 0.], radius);
        assert_eq!(roots.len(), 2);
        for point in roots {
            assert!((point[0] / radius - 0.5).abs() < 32.0 * f64::EPSILON);
            assert!((point[1].abs() / radius - 3.0_f64.sqrt() * 0.5).abs() < 32.0 * f64::EPSILON);
        }
        assert!(
            super::meridian_circle_intersections([0., 0.], radius, [3. * radius, 0.], radius)
                .is_empty()
        );
    }
}

#[test]
fn intersection_candidate_multiplicity_is_invariant_under_length_scale() {
    use crate::decode::analytic::equations::{
        CarrierEquation, ConeEquation, CylinderEquation, PlaneEquation, SphereEquation,
        TorusEquation,
    };
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let axis = [0.0, 0.0, 1.0];
    let reference = [1.0, 0.0, 0.0];
    for scale in [1e-200, 1e-10, 1.0, 1e200] {
        let cylinder = |radius, x| {
            CarrierEquation::Cylinder(CylinderEquation {
                origin: [x, 0.0, 0.0],
                axis,
                ref_direction: reference,
                radius,
            })
        };
        let sphere = |radius| {
            CarrierEquation::Sphere(SphereEquation {
                center: [0.0; 3],
                ref_direction: reference,
                radius,
            })
        };
        let cone = |radius| {
            CarrierEquation::Cone(
                ConeEquation::new(
                    [0.0; 3],
                    axis,
                    reference,
                    radius,
                    1.0,
                    std::f64::consts::FRAC_PI_4,
                )
                .unwrap(),
            )
        };
        let torus = CarrierEquation::Torus(TorusEquation {
            center: [0.0; 3],
            axis,
            ref_direction: reference,
            major_radius: 3.0 * scale,
            minor_radius: scale,
        });
        let plane = |z, normal| {
            CarrierEquation::Plane(PlaneEquation {
                origin: [0.0, 0.0, z],
                normal,
            })
        };
        assert!(super::coaxial_cylinder_sphere_circle_candidates(
            cylinder(2.0 * scale, 0.0),
            sphere(scale)
        )
        .is_empty());
        let circles = super::coaxial_cylinder_sphere_circle_candidates(
            cylinder(scale, 0.0),
            sphere(2.0 * scale),
        );
        assert_eq!(circles.len(), 2);
        for (curve, _) in circles {
            let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = curve else {
                panic!("expected circle")
            };
            assert!((circle.center().z.abs() / scale - 3.0_f64.sqrt()).abs() < 64.0 * f64::EPSILON);
        }
        let circles =
            super::coaxial_cone_sphere_circle_candidates(cone(scale), sphere(2.0 * scale));
        assert_eq!(circles.len(), 2);
        for (curve, _) in circles {
            let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = curve else {
                panic!("expected circle")
            };
            assert!(
                (circle.radius().hypot(circle.center().z) / scale - 2.0).abs()
                    < 64.0 * f64::EPSILON
            );
        }
        let circles = super::coaxial_cone_torus_circle_candidates(cone(3.0 * scale), torus);
        assert_eq!(circles.len(), 2);
        for (curve, _) in circles {
            let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = curve else {
                panic!("expected circle")
            };
            assert!(
                ((circle.radius() / scale - 3.0).hypot(circle.center().z / scale) - 1.0).abs()
                    < 64.0 * f64::EPSILON
            );
        }
        assert!(
            super::coaxial_cylinder_torus_circle_candidates(cylinder(5.0 * scale, 0.0), torus)
                .is_empty()
        );
        assert!(
            super::axis_normal_plane_torus_circle_candidates(plane(2.0 * scale, axis), torus)
                .is_empty()
        );
        assert_eq!(
            super::coaxial_cylinder_torus_circle_candidates(cylinder(3.0 * scale, 0.0), torus)
                .len(),
            2
        );
        assert_eq!(
            super::axis_normal_plane_torus_circle_candidates(plane(0.0, axis), torus).len(),
            2
        );
        assert_eq!(
            super::parallel_cylinder_generator_candidates(
                cylinder(scale, 0.0),
                cylinder(scale, scale)
            )
            .len(),
            2
        );
        assert_eq!(
            super::parallel_plane_cylinder_generator_candidates(
                plane(0.0, reference),
                cylinder(scale, 0.0)
            )
            .len(),
            2
        );
    }
}
