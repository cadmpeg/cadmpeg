// SPDX-License-Identifier: Apache-2.0
//! Winding of a sampled Type 144 boundary: closure, degeneracy, and the
//! support's oriented normal.

use std::f64::consts::TAU;

use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface, TorusSurface};
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::test_support::with_service_context;

use super::{
    closed_parameter_area, strip_windings, support_oriented_area, support_parameter_periods,
    support_turn, turn_of, winding_from_area, BoundaryWinding, SignedArea, Turn,
    BOUNDARY_AREA_DEGENERATE_FRACTION, PARAMETER_RING_CLOSURE_FRACTION,
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

/// The image of a parameter line at `level` on the axis other than `axis`
/// over `turns` periods of `axis`, in 64 equal steps.
fn turning_line(axis: usize, level: f64, turns: f64) -> Vec<[f64; 2]> {
    (0..=64)
        .map(|step| {
            let mut point = [level; 2];
            point[axis] = TAU * turns * f64::from(step) / 64.0;
            point
        })
        .collect()
}

fn turn(axis: usize, forward: bool, level: f64) -> Turn {
    Turn {
        axis,
        forward,
        level,
    }
}

/// Bound on the distance between the two end rows of a NURBS support at
/// which [`support_turn`] reads it closed.
const SUPPORT_RESOLUTION: f64 = 1.0e-6;

/// The degree-1 ring of three points about `+z` swept along `+z` over one
/// unit, closed over its `u` knot domain `[0, 3]` and open in `v`.
fn closed_ring_patch() -> SolvedSurfaceGeometry {
    let ring = [[1.0, 0.0], [-0.5, 1.0], [-0.5, -1.0], [1.0, 0.0]];
    SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                ring.iter()
                    .map(|[x, y]| vec![Point3::new(*x, *y, 0.0), Point3::new(*x, *y, 1.0)])
                    .collect(),
                None,
            ),
            false,
        )
        .expect("closed ring patch"),
    )
}

#[test]
fn a_support_turns_only_around_its_one_closed_axis() {
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let cylinder = SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(origin, axis, reference, 2.0).expect("cylinder"),
    );
    let torus = SolvedSurfaceGeometry::Torus(
        TorusSurface::try_new(origin, axis, reference, 5.0, 1.0).expect("torus"),
    );
    with_service_context(b"", |ctx| {
        let turn = |support: &SolvedSurfaceGeometry| {
            support_turn(support, SUPPORT_RESOLUTION, ctx).expect("the closure evaluates")
        };
        assert_eq!(turn(&cylinder), Some((0, TAU)));
        assert_eq!(turn(&torus), None);
        assert_eq!(turn(&bilinear_patch(true, false)), Some((0, 3.0)));
        assert_eq!(turn(&bilinear_patch(false, false)), None);
        assert_eq!(turn(&closed_ring_patch()), Some((0, 3.0)));
    });
}

#[test]
fn an_image_whose_ends_lie_one_turn_apart_turns_once() {
    let forward = turn_of(&turning_line(0, 2.0, 1.0), 0, TAU).expect("a turn");
    assert_eq!((forward.axis, forward.forward), (0, true));
    assert!((forward.level - 2.0).abs() <= f64::EPSILON);
    let backward = turn_of(&turning_line(0, 2.0, -1.0), 0, TAU).expect("a turn");
    assert_eq!((backward.axis, backward.forward), (0, false));
    for turns in [2.0, 0.5] {
        assert!(turn_of(&turning_line(0, 2.0, turns), 0, TAU).is_none());
    }
    let mut rising = turning_line(0, 2.0, 1.0);
    rising.last_mut().expect("an end")[1] += 1.0;
    assert!(turn_of(&rising, 0, TAU).is_none());
    let meridian = turn_of(&turning_line(1, 2.0, 1.0), 1, TAU).expect("a turn");
    assert_eq!((meridian.axis, meridian.forward), (1, true));
}

/// The points at eight equal steps of each side of the counterclockwise
/// rectangle over `u` in `[-0.2, 0.2]` and `v` in `[1, 2]`, stated in
/// `[0, 2π]` from `(2π, 2)` along its upper side and split at the seam.
fn seam_split_hole_from_its_upper_edge() -> Vec<[f64; 2]> {
    let side = |from: [f64; 2], to: [f64; 2]| {
        (0..=8).map(move |step| {
            let t = f64::from(step) / 8.0;
            [
                from[0] + t * (to[0] - from[0]),
                from[1] + t * (to[1] - from[1]),
            ]
        })
    };
    side([TAU, 2.0], [TAU - 0.2, 2.0])
        .chain(side([TAU - 0.2, 2.0], [TAU - 0.2, 1.0]))
        .chain(side([TAU - 0.2, 1.0], [TAU, 1.0]))
        .chain(side([0.0, 1.0], [0.2, 1.0]))
        .chain(side([0.2, 1.0], [0.2, 2.0]))
        .chain(side([0.2, 2.0], [0.0, 2.0]))
        .collect()
}

#[test]
fn a_boundary_split_at_the_seam_does_not_turn() {
    assert!(turn_of(&seam_split_hole_from_its_upper_edge(), 0, TAU).is_none());
}

/// Bound on the difference between a computed turn level and its expected
/// value.
const LEVEL_AGREEMENT: f64 = 1.0e-12;

/// The level of a ring at `v = 0` with a notch up to `v = 1` over
/// `u` in `[1, 1.1]` is its mean `v` along its length, however densely the
/// notch is sampled.
#[test]
fn the_level_of_a_turn_is_its_mean_along_its_length() {
    let mut ring = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
    ring.extend((1..=100).map(|step| [1.0 + 0.1 * f64::from(step) / 100.0, 1.0]));
    ring.push([1.1, 0.0]);
    ring.extend((1..=8).map(|step| [1.1 + (TAU - 1.1) * f64::from(step) / 8.0, 0.0]));
    let turn = turn_of(&ring, 0, TAU).expect("a turn");
    let expected = (0.1 * 1.0 + 2.0 * 0.5) / (TAU + 2.0);
    assert!(
        (turn.level - expected).abs() <= LEVEL_AGREEMENT,
        "{}",
        turn.level
    );
}

/// A strip between two boundaries that turn along `u` lies to the left of
/// the lower one as it advances and of the upper one as it returns; along
/// `v` the left of an advance lies toward smaller `u`.
#[test]
fn two_turning_boundaries_keep_the_strip_between_them_on_their_left() {
    let along_u = [(0, turn(0, true, 0.0)), (1, turn(0, true, 1.0))];
    assert_eq!(
        strip_windings(&along_u, false),
        [(0, BoundaryWinding::Kept), (1, BoundaryWinding::Reversed)]
    );
    assert_eq!(
        strip_windings(&along_u, true),
        [(0, BoundaryWinding::Reversed), (1, BoundaryWinding::Kept)]
    );
    let upper_first = [(0, turn(0, false, 1.0)), (1, turn(0, true, 0.0))];
    assert_eq!(
        strip_windings(&upper_first, false),
        [(1, BoundaryWinding::Kept), (0, BoundaryWinding::Kept)]
    );
    let along_v = [(0, turn(1, true, 0.0)), (1, turn(1, true, 1.0))];
    assert_eq!(
        strip_windings(&along_v, false),
        [(0, BoundaryWinding::Reversed), (1, BoundaryWinding::Kept)]
    );
}

#[test]
fn turning_boundaries_that_bound_no_one_strip_have_no_winding() {
    let lower = (0, turn(0, true, 0.0));
    assert!(strip_windings(&[lower], false).is_empty());
    assert!(strip_windings(
        &[lower, (1, turn(0, true, 1.0)), (2, turn(0, true, 2.0))],
        false
    )
    .is_empty());
    assert!(strip_windings(&[lower, (1, turn(0, false, 0.0))], false).is_empty());
}
