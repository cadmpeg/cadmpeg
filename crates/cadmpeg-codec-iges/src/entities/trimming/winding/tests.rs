// SPDX-License-Identifier: Apache-2.0
//! Winding of a sampled Type 144 boundary: closure, degeneracy, and the
//! support's oriented normal.

use std::f64::consts::TAU;

use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

use super::{
    closed_parameter_area, support_oriented_area, support_parameter_periods, winding_from_area,
    BoundaryWinding, SignedArea, BOUNDARY_AREA_DEGENERATE_FRACTION,
    PARAMETER_RING_CLOSURE_FRACTION,
};

/// The counterclockwise unit square, closed at `(0, 0)`.
const SQUARE: [[f64; 2]; 5] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]];

fn bilinear_patch(u_periodic: bool, normal_reversed: bool) -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 3.0, 3.0], u_periodic),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                    vec![Point3::new(3.0, 0.0, 0.0), Point3::new(3.0, 1.0, 0.0)],
                ],
                None,
            ),
            normal_reversed,
        )
        .expect("bilinear patch"),
    )
}

/// `SQUARE` with its last point moved by `gap` along `axis`.
fn square_with_gap(axis: usize, gap: f64) -> Vec<[f64; 2]> {
    let mut points = SQUARE.to_vec();
    points[4][axis] += gap;
    points
}

fn area(twice: f64, extent: f64) -> SignedArea {
    SignedArea { twice, extent }
}

#[test]
fn analytic_and_nurbs_supports_state_their_periodic_axes() {
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let origin = Point3::new(0.0, 0.0, 0.0);
    let plane = SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(origin, axis, reference).expect("plane"),
    );
    let cylinder = SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(origin, axis, reference, 2.0).expect("cylinder"),
    );
    assert_eq!(support_parameter_periods(&plane), [None, None]);
    assert_eq!(support_parameter_periods(&cylinder), [Some(TAU), None]);
    assert_eq!(
        support_parameter_periods(&bilinear_patch(true, false)),
        [Some(3.0), None]
    );
    assert_eq!(
        support_parameter_periods(&bilinear_patch(false, false)),
        [None, None]
    );
}

#[test]
fn an_ordinary_gap_closes_the_image_and_a_larger_one_does_not() {
    let below = 0.5 * PARAMETER_RING_CLOSURE_FRACTION;
    let above = 2.0 * PARAMETER_RING_CLOSURE_FRACTION;
    let extent = 2.0_f64.sqrt();
    for axis in 0..2 {
        let closed = closed_parameter_area(&square_with_gap(axis, below * extent), [None, None]);
        assert!(closed.is_some_and(|area| area.twice > 0.0));
        assert!(
            closed_parameter_area(&square_with_gap(axis, above * extent), [None, None]).is_none()
        );
    }
    let periods = [Some(TAU), None];
    assert!(closed_parameter_area(&square_with_gap(0, below * TAU), periods).is_some());
    assert!(closed_parameter_area(&square_with_gap(0, above * TAU), periods).is_none());
}

/// A boundary that winds once about a cylinder axis while it rises 1000 in
/// `v` and returns along the seam ends one period from its start. Against its extent the gap is small;
/// against the period it is not, so the image does not close.
#[test]
fn a_seam_crossing_image_does_not_close_on_a_periodic_axis() {
    let mut boundary: Vec<[f64; 2]> = (0..=64)
        .map(|step| {
            let fraction = f64::from(step) / 64.0;
            [TAU * fraction, 1000.0 * fraction]
        })
        .collect();
    boundary.extend([[TAU, 500.0], [TAU, 0.0]]);
    assert!(closed_parameter_area(&boundary, [None, None]).is_some());
    assert!(closed_parameter_area(&boundary, [Some(TAU), None]).is_none());
    assert_eq!(
        winding_from_area(closed_parameter_area(&boundary, [Some(TAU), None]), true),
        BoundaryWinding::Unresolved(
            "it has no closed parameter-space image and no planar model-space image"
        )
    );
}

#[test]
fn an_area_at_the_degenerate_fraction_has_no_winding() {
    let extent = 10.0;
    let threshold = BOUNDARY_AREA_DEGENERATE_FRACTION * extent * extent;
    for twice in [threshold, -threshold, 0.0] {
        assert!(matches!(
            winding_from_area(Some(area(twice, extent)), true),
            BoundaryWinding::Unresolved(_)
        ));
    }
    assert_eq!(
        winding_from_area(Some(area(2.0 * threshold, extent)), true),
        BoundaryWinding::Kept
    );
    assert_eq!(
        winding_from_area(Some(area(-2.0 * threshold, extent)), true),
        BoundaryWinding::Reversed
    );
}

#[test]
fn outer_boundaries_run_counterclockwise_and_inner_boundaries_clockwise() {
    assert_eq!(
        winding_from_area(Some(area(2.0, 1.0)), true),
        BoundaryWinding::Kept
    );
    assert_eq!(
        winding_from_area(Some(area(-2.0, 1.0)), true),
        BoundaryWinding::Reversed
    );
    assert_eq!(
        winding_from_area(Some(area(2.0, 1.0)), false),
        BoundaryWinding::Reversed
    );
    assert_eq!(
        winding_from_area(Some(area(-2.0, 1.0)), false),
        BoundaryWinding::Kept
    );
}

/// On a NURBS support whose oriented normal is opposite `Pu × Pv`, a
/// counterclockwise parameter-space image runs clockwise about the normal.
#[test]
fn a_reversed_nurbs_normal_inverts_the_parameter_area() {
    assert_eq!(
        support_oriented_area(2.0, &bilinear_patch(false, false)),
        2.0
    );
    assert_eq!(
        support_oriented_area(2.0, &bilinear_patch(false, true)),
        -2.0
    );
    let image = closed_parameter_area(&SQUARE, [None, None]).expect("the square closes");
    let reversed = support_oriented_area(image.twice, &bilinear_patch(false, true));
    assert_eq!(
        winding_from_area(Some(area(reversed, image.extent)), true),
        BoundaryWinding::Reversed
    );
}
