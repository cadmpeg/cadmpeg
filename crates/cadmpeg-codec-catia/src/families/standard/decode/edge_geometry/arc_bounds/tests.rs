// SPDX-License-Identifier: Apache-2.0
//! Distinct circular and elliptic arcs selected by source face bounds.

use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeLength};
use cadmpeg_ir::units::UnitVector3;

use super::{bounded_circle_branch, bounded_ellipse_branch, BranchEvidence};
use crate::families::standard::records::StandardFaceBounds;

fn bounds(center: [f64; 3], half_extents: [f64; 3]) -> StandardFaceBounds {
    StandardFaceBounds {
        aabb_center: center.map(|value| FiniteReal::new(value).expect("finite centre")),
        aabb_half_extents: half_extents
            .map(|value| NonNegativeLength::new(value).expect("positive extent")),
        sphere_center: [FiniteReal::ZERO; 3],
        sphere_radius: NonNegativeLength::new(2.0).expect("positive radius"),
    }
}

#[test]
fn source_box_selects_the_short_or_long_arc() {
    let center = Point3::new(0.0, 0.0, 0.0);
    let axis = UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).expect("unit axis");
    let short = bounded_circle_branch(
        center,
        1.0,
        axis,
        [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        &[Some(bounds([0.5, 0.5, 0.0], [0.5, 0.5, 0.1])), None],
    )
    .expect("one bounded quarter arc");
    assert!((short.2[1] - short.2[0] - std::f64::consts::FRAC_PI_2).abs() < 1e-12);

    let c = std::f64::consts::FRAC_1_SQRT_2;
    let long = bounded_circle_branch(
        center,
        1.0,
        axis,
        [Point3::new(c, -c, 0.0), Point3::new(c, c, 0.0)],
        &[Some(bounds([-0.15, 0.0, 0.0], [0.95, 1.1, 0.1])), None],
    )
    .expect("one bounded long arc");
    assert!((long.2[1] - long.2[0] - 1.5 * std::f64::consts::PI).abs() < 1e-12);
}

#[test]
fn boxes_that_admit_both_or_neither_arc_keep_the_branch_unresolved() {
    let center = Point3::new(0.0, 0.0, 0.0);
    let axis = UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).expect("unit axis");
    let endpoints = [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)];
    for boxes in [
        [None, None],
        [Some(bounds([0.0; 3], [1.1, 1.1, 0.1])), None],
        [Some(bounds([0.0; 3], [0.1; 3])), None],
    ] {
        assert!(bounded_circle_branch(center, 1.0, axis, endpoints, &boxes).is_none());
    }
    assert!(bounded_circle_branch(
        center,
        1.0,
        axis,
        [endpoints[0], Point3::new(0.0, 1.0, 0.5)],
        &[Some(bounds([0.5, 0.5, 0.0], [0.5, 0.5, 1.0])), None],
    )
    .is_none());
}

#[test]
fn large_source_coordinates_keep_sub_ulp_arc_choices_ambiguous() {
    let center = Point3::new(100_000_000.0, 0.0, 0.0);
    let axis = UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).expect("unit axis");
    let endpoints = [
        Point3::new(100_000_001.0, 0.0, 0.0),
        Point3::new(100_000_000.0, 1.0, 0.0),
    ];
    assert!(bounded_circle_branch(
        center,
        1.0,
        axis,
        endpoints,
        &[
            Some(bounds([100_000_000.0, 0.0, 0.0], [0.0, 1.1, 0.1])),
            None
        ],
    )
    .is_none());
}

fn ellipse_geometry(axis: [f64; 3]) -> cadmpeg_ir::geometry::analytic::EllipseCurve {
    cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(axis[0], axis[1], axis[2]),
        Vector3::new(1.0, 0.0, 0.0),
        2.0,
        1.0,
    )
    .expect("ellipse")
}

#[test]
fn source_box_selects_the_ellipse_half_and_its_direction() {
    let endpoints = [Point3::new(2.0, 0.0, 0.0), Point3::new(-2.0, 0.0, 0.0)];
    let upper_half = [Some(bounds([0.0, 0.5, 0.0], [2.0, 0.5, 0.1])), None];
    let (reversed, range, evidence) =
        bounded_ellipse_branch(&ellipse_geometry([0.0, 0.0, 1.0]), endpoints, &upper_half)
            .expect("one bounded half");
    assert!(!reversed);
    assert_eq!(evidence, BranchEvidence::Contained);
    assert!((range[1] - range[0] - std::f64::consts::PI).abs() < 1e-12);

    let (reversed, _, _) =
        bounded_ellipse_branch(&ellipse_geometry([0.0, 0.0, -1.0]), endpoints, &upper_half)
            .expect("one bounded half");
    assert!(reversed);
}

#[test]
fn an_undershooting_box_selects_the_arc_with_the_smaller_excess() {
    let endpoints = [Point3::new(2.0, 0.0, 0.0), Point3::new(-2.0, 0.0, 0.0)];
    let geometry = ellipse_geometry([0.0, 0.0, 1.0]);
    let short_of_the_apex = [Some(bounds([0.0, 0.497, 0.0], [2.0, 0.497, 0.1])), None];
    let (reversed, _, evidence) = bounded_ellipse_branch(&geometry, endpoints, &short_of_the_apex)
        .expect("least-excess half");
    assert!(!reversed);
    let BranchEvidence::LeastExcess(excess) = evidence else {
        panic!("neither half is contained");
    };
    assert!(excess > 0.0 && excess < 0.006);

    let straddling = [Some(bounds([0.0, 0.0, 0.0], [2.0, 0.9, 0.1])), None];
    assert!(bounded_ellipse_branch(&geometry, endpoints, &straddling).is_none());
}

#[test]
fn ellipse_endpoint_admission_uses_model_space_distance_at_every_scale() {
    for scale in [1.0, 1000.0] {
        let ellipse = cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0 * scale,
            scale,
        )
        .expect("ellipse");
        let face_bounds = [
            Some(bounds(
                [0.0, 0.5 * scale, 0.0],
                [2.0 * scale + 1.0, 0.5 * scale, 1.0],
            )),
            None,
        ];
        for (gap, accepted) in [(0.001, true), (0.1, false)] {
            let endpoints = [
                Point3::new(2.0 * scale + gap, 0.0, 0.0),
                Point3::new(-2.0 * scale, 0.0, 0.0),
            ];
            assert_eq!(
                bounded_ellipse_branch(&ellipse, endpoints, &face_bounds).is_some(),
                accepted,
                "scale {scale}, endpoint gap {gap} mm",
            );
        }
    }
}
