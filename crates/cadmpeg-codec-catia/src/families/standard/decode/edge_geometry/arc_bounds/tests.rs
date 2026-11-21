// SPDX-License-Identifier: Apache-2.0
//! Distinct circular arcs selected by source face bounds.

use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeLength};
use cadmpeg_ir::units::UnitVector3;

use super::bounded_circle_branch;
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

#[test]
fn source_box_selects_an_ellipse_branch_with_distinct_radii() {
    use cadmpeg_ir::geometry::analytic::EllipseCurve;
    let ellipse = EllipseCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        2.0,
        1.0,
    )
    .expect("ellipse");
    let endpoints = [Point3::new(2.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)];
    let (_, range) = super::bounded_ellipse_branch(
        ellipse,
        endpoints,
        &[Some(bounds([1.0, 0.5, 0.0], [1.0, 0.5, 0.1])), None],
    )
    .expect("quarter ellipse inside source bounds");
    assert!((range[1] - range[0] - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!(super::bounded_ellipse_branch(
        ellipse,
        endpoints,
        &[Some(bounds([0.0; 3], [2.1, 1.1, 0.1])), None],
    )
    .is_none());
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let (_, range) = super::bounded_ellipse_branch(
        ellipse,
        [Point3::new(2.0 * c, -c, 0.0), Point3::new(2.0 * c, c, 0.0)],
        &[Some(bounds([-0.3, 0.0, 0.0], [1.9, 1.1, 0.1])), None],
    )
    .expect("long ellipse arc inside source bounds");
    assert!((range[1] - range[0] - 1.5 * std::f64::consts::PI).abs() < 1e-12);
}

#[test]
fn cylinder_witness_selects_oblique_ellipse_arc_independently_of_height() {
    use cadmpeg_ir::geometry::analytic::{CylinderSurface, EllipseCurve};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let cylinder = CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        2.0,
    )
    .expect("cylinder");
    let ellipse = EllipseCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(
            0.0,
            -std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        Vector3::new(
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        2.0 * 2.0_f64.sqrt(),
        2.0,
    )
    .expect("oblique section ellipse");
    let endpoints = [Point3::new(0.0, 2.0, 2.0), Point3::new(0.0, -2.0, -2.0)];
    for x in [-2.0, 2.0] {
        let (curve, range, gap) = super::witnessed_cylinder_ellipse_branch(
            ellipse,
            &cylinder,
            endpoints,
            Point3::new(x, 0.0, 100.0),
            &[Some(bounds([0.0; 3], [2.0; 3])), None],
        )
        .expect("witnessed half ellipse");
        assert_eq!(gap, 0.0);
        let midpoint = cadmpeg_ir::eval::decode::curve_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(curve)),
            range[0].midpoint(range[1]),
        )
        .expect("arc midpoint")
        .get();
        assert!(midpoint.distance(Point3::new(x, 0.0, 0.0)) < 1e-12);
    }
}

#[test]
fn cylinder_witness_cannot_override_either_incident_box() {
    use cadmpeg_ir::geometry::analytic::{CylinderSurface, EllipseCurve};
    let cylinder = CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        2.0,
    )
    .expect("cylinder");
    let ellipse = EllipseCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        2.0,
        2.0,
    )
    .expect("circular section");
    let endpoints = [Point3::new(2.0, 0.0, 0.0), Point3::new(0.0, 2.0, 0.0)];
    let broad = Some(bounds([0.0; 3], [2.0, 2.0, 0.1]));
    let quarter = Some(bounds([1.0, 1.0, 0.0], [1.0, 1.0, 0.1]));
    let witness = Point3::new(1.0, -1.38e-7, 0.0);
    for boxes in [
        [quarter, None],
        [broad, quarter],
        [quarter, broad],
        [None, None],
    ] {
        assert!(
            super::witnessed_cylinder_ellipse_branch(
                ellipse, &cylinder, endpoints, witness, &boxes,
            )
            .is_none(),
            "a complementary arc contradicts an incident box"
        );
    }
    assert!(
        super::witnessed_cylinder_ellipse_branch(
            ellipse,
            &cylinder,
            endpoints,
            Point3::new(1.0, 1.0, 0.0),
            &[broad, broad],
        )
        .is_some(),
        "a witness can resolve two box-admitted branches"
    );
}

#[test]
fn cylinder_witness_reports_box_excess_and_rejects_opposite_face_evidence() {
    use cadmpeg_ir::geometry::analytic::{CylinderSurface, EllipseCurve};
    let cylinder = CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        2.0,
    )
    .expect("valid analytic carrier");
    let ellipse = EllipseCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        2.0,
        2.0,
    )
    .expect("valid analytic carrier");
    let endpoints = [Point3::new(2.0, 0.0, 0.0), Point3::new(0.0, 2.0, 0.0)];
    let undersized = Some(bounds([0.9983, 0.9983, 0.0], [0.9983, 0.9983, 0.1]));
    let witness = Point3::new(1.0, 1.0, 0.0);
    let (_, _, gap) = super::witnessed_cylinder_ellipse_branch(
        ellipse,
        &cylinder,
        endpoints,
        witness,
        &[undersized, None],
    )
    .expect("reported inference for an undersized box");
    assert!((gap - 0.0034).abs() < 1e-12);
    let opposite = Some(bounds([-1.0, -1.0, 0.0], [1.0, 1.0, 0.1]));
    assert!(
        super::witnessed_cylinder_ellipse_branch(
            ellipse,
            &cylinder,
            endpoints,
            witness,
            &[undersized, opposite],
        )
        .is_none(),
        "incident bounds prefer different branches"
    );
}
