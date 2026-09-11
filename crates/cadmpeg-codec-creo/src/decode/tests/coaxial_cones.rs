// SPDX-License-Identifier: Apache-2.0
//! Tests: coaxial cone orientation.

use crate::decode::analytic::equations::{CarrierEquation, ConeEquation, PlaneEquation};
use crate::decode::analytic::planes::{point_on_carrier, solve_carriers};
use crate::decode::surfaces::intersection_candidates::coaxial_cones_section_candidates;
use crate::decode::surfaces::select_unique_curve_candidate;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

const EPS_COAXIAL_CIRCLE: f64 = 1.0e-12;

const EPS_FILLET_CIRCLE: f64 = 1.0e-12;

const EPS_CONIC_INTERSECTION: f64 = 1.0e-12;

#[test]
fn coaxial_cone_components_respect_axis_orientation_and_coincidence() {
    let first = CarrierEquation::Cone(
        ConeEquation::new(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            2.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("valid test cone"),
    );
    let second = CarrierEquation::Cone(
        ConeEquation::new(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            4.0,
            1.0,
            0.5_f64.atan(),
        )
        .expect("valid test cone"),
    );
    let candidates = coaxial_cones_section_candidates(first, second);
    assert_eq!(candidates.len(), 2);
    assert!(
        matches!(select_unique_curve_candidate(candidates, [[6.0, 0.0, 4.0], [0.0, 6.0, 4.0]]), Some((CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)), "coaxial_cones_circle"))
                if {
                    let center = circle_curve.center();
        let radius = circle_curve.radius();
                    (center.z - 4.0).abs() < EPS_COAXIAL_CIRCLE && (radius - 6.0).abs() < EPS_COAXIAL_CIRCLE
                })
    );
    let tangent_plane = CarrierEquation::Plane(PlaneEquation {
        origin: [10.0, 0.0, 0.0],
        normal: [1.0, 0.0, 1.0],
    });
    let vertex = solve_carriers(&[first, second, tangent_plane])
        .expect("unique coaxial-cone circle tangent");
    assert!((vertex[0] - 6.0).abs() < 1.0e-12);
    assert!(vertex[1].abs() < 1.0e-12);
    assert!((vertex[2] - 4.0).abs() < 1.0e-12);

    let reversed = CarrierEquation::Cone(
        ConeEquation::new(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
            4.0,
            1.0,
            0.5_f64.atan(),
        )
        .expect("valid test cone"),
    );
    let reversed_candidates = coaxial_cones_section_candidates(first, reversed);
    assert_eq!(reversed_candidates.len(), 2);
    assert!(reversed_candidates
        .iter()
        .any(|(geometry, _)| matches!(geometry, CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))
                if {
                    let center = circle_curve.center();
                    let radius = circle_curve.radius();
                    (center.z - 4.0 / 3.0).abs() < EPS_FILLET_CIRCLE && (radius - 10.0 / 3.0).abs() < EPS_FILLET_CIRCLE
                })));
    assert!(coaxial_cones_section_candidates(first, first).is_empty());
    let shifted = CarrierEquation::Cone(
        ConeEquation::new(
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            4.0,
            1.0,
            0.5_f64.atan(),
        )
        .expect("valid test cone"),
    );
    assert!(coaxial_cones_section_candidates(first, shifted).is_empty());

    let CarrierEquation::Cone(mut elliptical_first_equation) = first else {
        unreachable!();
    };
    elliptical_first_equation = ConeEquation::new(
        elliptical_first_equation.origin(),
        elliptical_first_equation.axis(),
        elliptical_first_equation.ref_direction(),
        elliptical_first_equation.radius(),
        0.5,
        elliptical_first_equation.half_angle(),
    )
    .expect("valid test cone");
    let elliptical_first = CarrierEquation::Cone(elliptical_first_equation);
    let CarrierEquation::Cone(mut elliptical_second_equation) = second else {
        unreachable!();
    };
    elliptical_second_equation = ConeEquation::new(
        elliptical_second_equation.origin(),
        elliptical_second_equation.axis(),
        elliptical_second_equation.ref_direction(),
        elliptical_second_equation.radius(),
        0.5,
        elliptical_second_equation.half_angle(),
    )
    .expect("valid test cone");
    let elliptical_second = CarrierEquation::Cone(elliptical_second_equation);
    let candidates = coaxial_cones_section_candidates(elliptical_first, elliptical_second);
    assert_eq!(candidates.len(), 2);
    let selected = select_unique_curve_candidate(candidates, [[6.0, 0.0, 4.0], [0.0, 3.0, 4.0]])
        .expect("selected coaxial elliptical-cone section");
    assert!(
        matches!(&selected, (CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)), "coaxial_cones_ellipse")
                if {
                    let center = ellipse_curve.center();
        let major_radius = ellipse_curve.major_radius();
        let minor_radius = ellipse_curve.minor_radius();
                    (center.z - 4.0).abs() < EPS_CONIC_INTERSECTION
                        && (major_radius - 6.0).abs() < EPS_CONIC_INTERSECTION
                        && (minor_radius - 3.0).abs() < EPS_CONIC_INTERSECTION
                })
    );
    for parameter in [-1.0, 0.0, 1.0] {
        let point = cadmpeg_ir::eval::curve_point(&selected.0, parameter)
            .expect("coaxial cone ellipse point");
        let point = [point.x, point.y, point.z];
        assert!(point_on_carrier(point, elliptical_first));
        assert!(point_on_carrier(point, elliptical_second));
    }
    elliptical_second_equation = ConeEquation::new(
        elliptical_second_equation.origin(),
        elliptical_second_equation.axis(),
        [0.0, 1.0, 0.0],
        elliptical_second_equation.radius(),
        elliptical_second_equation.ratio(),
        elliptical_second_equation.half_angle(),
    )
    .expect("valid test cone");
    let incompatible_frame = CarrierEquation::Cone(elliptical_second_equation);
    assert!(coaxial_cones_section_candidates(elliptical_first, incompatible_frame).is_empty());

    elliptical_second_equation = ConeEquation::new(
        elliptical_second_equation.origin(),
        elliptical_second_equation.axis(),
        elliptical_second_equation.ref_direction(),
        elliptical_second_equation.radius(),
        2.0,
        0.25_f64.atan(),
    )
    .expect("valid test cone");
    let reciprocal_swapped = CarrierEquation::Cone(elliptical_second_equation);
    let candidates = coaxial_cones_section_candidates(elliptical_first, reciprocal_swapped);
    assert_eq!(candidates.len(), 2);
    let selected = select_unique_curve_candidate(candidates, [[14.0, 0.0, 12.0], [0.0, 7.0, 12.0]])
        .expect("selected reciprocal-frame cone section");
    assert!(
        matches!(&selected, (CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)), "coaxial_cones_ellipse")
                if {
                    let center = ellipse_curve.center();
        let major_radius = ellipse_curve.major_radius();
        let minor_radius = ellipse_curve.minor_radius();
                    (center.z - 12.0).abs() < EPS_CONIC_INTERSECTION
                        && (major_radius - 14.0).abs() < EPS_CONIC_INTERSECTION
                        && (minor_radius - 7.0).abs() < EPS_CONIC_INTERSECTION
                })
    );
    for parameter in [-1.0, 0.0, 1.0] {
        let point = cadmpeg_ir::eval::curve_point(&selected.0, parameter)
            .expect("reciprocal-frame section point");
        let point = [point.x, point.y, point.z];
        assert!(point_on_carrier(point, elliptical_first));
        assert!(point_on_carrier(point, reciprocal_swapped));
    }
}
