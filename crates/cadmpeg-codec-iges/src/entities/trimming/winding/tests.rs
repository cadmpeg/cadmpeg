// SPDX-License-Identifier: Apache-2.0
//! Winding of a sampled Type 144 boundary: closure, degeneracy, and the
//! support's oriented normal.

use std::f64::consts::TAU;

use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface, TorusSurface};
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::test_support::with_service_context;

use super::{
    closed_parameter_area, pcurve_extremes, strip_windings, support_oriented_area,
    support_parameter_periods, support_turn, turn_of, winding_from_area, BoundaryWinding, Extremes,
    SignedArea, Turn, Turning, BOUNDARY_AREA_DEGENERATE_FRACTION, PARAMETER_RING_CLOSURE_FRACTION,
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

/// The turn at `position` of a boundary that is the outer boundary where
/// `outer`, around `axis`, between the exact coordinates `extremes` on the
/// other axis.
fn turning(
    position: usize,
    outer: bool,
    axis: usize,
    forward: bool,
    [lowest, highest]: [f64; 2],
) -> Turning {
    Turning {
        position,
        outer,
        turn: Turn { axis, forward },
        extremes: Extremes {
            lowest: [lowest; 2],
            highest: [highest; 2],
        },
    }
}

/// Bound on the distance between the two end rows of a NURBS support at
/// which [`support_turn`] reads it closed.
const SUPPORT_RESOLUTION: f64 = 1.0e-6;

/// The degree-1 ring of three points about `+z` swept along `+z` over one
/// unit, closed over its `u` knot domain `[0, 3]` and open in `v`. Over the
/// `u` range `[0, 2]` it is open.
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
    let whole = [Some([0.0, 3.0]), Some([0.0, 1.0])];
    with_service_context(b"", |ctx| {
        let turn = |support: &SolvedSurfaceGeometry, stated: [Option<[f64; 2]>; 2]| {
            support_turn(support, stated, SUPPORT_RESOLUTION, ctx).expect("the closure evaluates")
        };
        assert_eq!(turn(&cylinder, [None, None]), Some((0, TAU)));
        assert_eq!(turn(&torus, [None, None]), None);
        assert_eq!(turn(&bilinear_patch(false, false), whole), None);
        assert_eq!(turn(&closed_ring_patch(), whole), Some((0, 3.0)));
        assert_eq!(
            turn(&closed_ring_patch(), [Some([0.0, 2.0]), Some([0.0, 1.0])]),
            None
        );
        assert_eq!(turn(&closed_ring_patch(), [None, None]), None);
    });
}

#[test]
fn an_image_whose_ends_lie_one_turn_apart_turns_once() {
    let forward = turn_of(&turning_line(0, 2.0, 1.0), 0, TAU).expect("a turn");
    assert_eq!((forward.axis, forward.forward), (0, true));
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

/// The degree-1 parameter curve through `vertices` at the knots
/// `parameters`, over its whole range.
fn degree_one(vertices: &[[f64; 2]], parameters: &[f64]) -> (PcurveGeometry, [f64; 2]) {
    let (first, last) = (parameters[0], parameters[parameters.len() - 1]);
    let mut knots = vec![first];
    knots.extend_from_slice(parameters);
    knots.push(last);
    curve(1, knots, vertices, None, [first, last])
}

/// The parameter curve of `degree` with `knots`, `vertices`, and `weights`,
/// over `range`.
fn curve(
    degree: u32,
    knots: Vec<f64>,
    vertices: &[[f64; 2]],
    weights: Option<Vec<f64>>,
    range: [f64; 2],
) -> (PcurveGeometry, [f64; 2]) {
    let nurbs = PcurveNurbs::from_lanes(
        degree,
        knots,
        vertices
            .iter()
            .map(|&[u, v]| Point2::new(u, v))
            .collect::<Vec<_>>(),
        weights,
        false,
    )
    .expect("a parameter curve");
    (PcurveGeometry::Nurbs { nurbs }, range)
}

/// The extremes on `axis` of the parameter curve `(geometry, range)`.
fn extremes_of((geometry, range): &(PcurveGeometry, [f64; 2]), axis: usize) -> Extremes {
    with_service_context(b"", |ctx| {
        pcurve_extremes(geometry, *range, axis, ctx)
            .expect("the extremes evaluate")
            .expect("the curve has extremes")
    })
}

/// The points at eight equal steps of each side of the polyline through
/// `corners`, closed at the first corner moved by `closing` along `u`.
fn sampled(corners: &[[f64; 2]], closing: f64) -> Vec<[f64; 2]> {
    let mut ends = corners.to_vec();
    ends.push([corners[0][0] + closing, corners[0][1]]);
    let mut points = vec![ends[0]];
    for pair in ends.windows(2) {
        for step in 1..=8 {
            let t = f64::from(step) / 8.0;
            points.push([
                pair[0][0] + t * (pair[1][0] - pair[0][0]),
                pair[0][1] + t * (pair[1][1] - pair[0][1]),
            ]);
        }
    }
    points
}

/// Of two disjoint rings that turn around a cylinder, the lower has the
/// lower lowest point and the lower highest point. The lower ring here runs
/// at `v = 0` with a tab to 4 over `u` in `[1, 2.4]` that carries twelve
/// fingers to 7. The upper ring runs back at 8 with a slot to 4 over `u` in
/// `[3.8, 5.2]` that carries twelve slots to 1. Each keeps the strip
/// between them on its left.
#[test]
fn two_disjoint_rings_are_ordered_by_their_lowest_and_highest_points() {
    let mut lower = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 4.0]];
    let mut upper = vec![[TAU, 8.0], [5.2, 8.0], [5.2, 4.0]];
    for finger in 0..12 {
        let (a, b) = (
            0.1f64.mul_add(f64::from(finger), 1.05),
            (-0.1f64).mul_add(f64::from(finger), 5.15),
        );
        lower.extend([[a, 4.0], [a, 7.0], [a + 0.05, 7.0], [a + 0.05, 4.0]]);
        upper.extend([[b, 4.0], [b, 1.0], [b - 0.05, 1.0], [b - 0.05, 4.0]]);
    }
    lower.extend([[2.4, 4.0], [2.4, 0.0]]);
    upper.extend([[3.8, 4.0], [3.8, 8.0]]);
    let turns = [(lower, TAU, true), (upper, -TAU, false)].map(|(corners, closing, outer)| {
        let mut vertices = corners.clone();
        vertices.push([corners[0][0] + closing, corners[0][1]]);
        let parameters = std::iter::successors(Some(0.0), |parameter| Some(parameter + 1.0))
            .take(vertices.len())
            .collect::<Vec<_>>();
        Turning {
            position: usize::from(!outer),
            outer,
            turn: turn_of(&sampled(&corners, closing), 0, TAU).expect("a turn"),
            extremes: extremes_of(&degree_one(&vertices, &parameters), 1),
        }
    });
    assert_eq!(
        turns.map(|turning| turning.extremes),
        [[0.0, 7.0], [1.0, 8.0]].map(|[lowest, highest]| Extremes {
            lowest: [lowest; 2],
            highest: [highest; 2],
        })
    );
    assert_eq!(
        strip_windings(&turns, false),
        [(0, BoundaryWinding::Kept), (1, BoundaryWinding::Kept)]
    );
}

#[test]
fn two_turning_boundaries_keep_the_strip_between_them_on_their_left() {
    let along_u = [
        turning(0, true, 0, true, [0.0, 0.0]),
        turning(1, false, 0, true, [1.0, 1.0]),
    ];
    assert_eq!(
        strip_windings(&along_u, false),
        [(0, BoundaryWinding::Kept), (1, BoundaryWinding::Reversed)]
    );
    assert_eq!(
        strip_windings(&along_u, true),
        [(0, BoundaryWinding::Reversed), (1, BoundaryWinding::Kept)]
    );
    let upper_first = [
        turning(0, true, 0, false, [1.0, 1.0]),
        turning(1, false, 0, true, [0.0, 0.0]),
    ];
    assert_eq!(
        strip_windings(&upper_first, false),
        [(1, BoundaryWinding::Kept), (0, BoundaryWinding::Kept)]
    );
    let along_v = [
        turning(0, false, 1, true, [0.0, 0.0]),
        turning(1, true, 1, true, [1.0, 1.0]),
    ];
    assert_eq!(
        strip_windings(&along_v, false),
        [(0, BoundaryWinding::Reversed), (1, BoundaryWinding::Kept)]
    );
}

#[test]
fn turning_boundaries_that_bound_no_one_strip_have_no_winding() {
    let lower = turning(0, true, 0, true, [0.0, 0.0]);
    assert!(strip_windings(&[lower], false).is_empty());
    assert!(strip_windings(
        &[
            lower,
            turning(1, false, 0, true, [1.0, 1.0]),
            turning(2, false, 0, true, [2.0, 2.0]),
        ],
        false
    )
    .is_empty());
    assert!(strip_windings(&[lower, turning(1, false, 0, false, [0.0, 0.0])], false).is_empty());
}

/// Two turning boundaries bound the face's strip only where the file states
/// one of them as the face's outer boundary.
#[test]
fn two_inner_turning_boundaries_have_no_winding() {
    let inner = [
        turning(0, false, 0, true, [0.0, 0.0]),
        turning(1, false, 0, true, [1.0, 1.0]),
    ];
    assert!(strip_windings(&inner, false).is_empty());
}

/// Two rings whose lowest and highest points do not both put one ring below
/// the other touch or cross, and bound no strip.
#[test]
fn rings_that_are_not_both_lower_and_higher_have_no_winding() {
    let crossing = [
        turning(0, true, 0, true, [0.0, 5.0]),
        turning(1, false, 0, true, [1.0, 4.0]),
    ];
    assert!(strip_windings(&crossing, false).is_empty());
    let touching = [
        turning(0, true, 0, true, [0.0, 4.0]),
        turning(1, false, 0, true, [0.0, 5.0]),
    ];
    assert!(strip_windings(&touching, false).is_empty());
}

/// A degree-1 span has its extremes at its control points, also where they
/// fall between the reader's samples. The lower ring runs at 5 with a notch
/// to 0 between two samples and a tab to 15 at a sample. The upper ring runs
/// at 6 with a dip to 1 at a sample and a tab to 20 between two samples. The
/// rings are ordered by `[0, 15]` below `[1, 20]`.
#[test]
fn ring_extremes_include_the_spans_between_samples() {
    let knot = |sample: f64| sample / 64.0;
    let lower = degree_one(
        &[
            [0.0, 5.0],
            [1.0, 5.0],
            [1.1, 0.0],
            [1.2, 5.0],
            [3.0, 5.0],
            [3.1, 15.0],
            [3.2, 5.0],
            [TAU, 5.0],
        ],
        &[
            0.0,
            knot(10.2),
            knot(10.5),
            knot(10.8),
            knot(29.0),
            knot(30.0),
            knot(31.0),
            1.0,
        ],
    );
    let upper = degree_one(
        &[
            [0.0, 6.0],
            [1.08, 6.0],
            [1.1, 1.0],
            [1.12, 6.0],
            [2.9, 6.0],
            [3.1, 20.0],
            [3.3, 6.0],
            [TAU, 6.0],
        ],
        &[
            0.0,
            knot(20.0),
            knot(21.0),
            knot(22.0),
            knot(40.2),
            knot(40.5),
            knot(40.8),
            1.0,
        ],
    );
    let turns = [(&lower, true), (&upper, false)].map(|(ring, outer)| Turning {
        position: usize::from(!outer),
        outer,
        turn: Turn {
            axis: 0,
            forward: true,
        },
        extremes: extremes_of(ring, 1),
    });
    assert_eq!(
        turns.map(|turning| turning.extremes),
        [[0.0, 15.0], [1.0, 20.0]].map(|[lowest, highest]| Extremes {
            lowest: [lowest; 2],
            highest: [highest; 2],
        })
    );
    assert_eq!(
        strip_windings(&turns, false),
        [(0, BoundaryWinding::Kept), (1, BoundaryWinding::Reversed)]
    );
}

/// Bound on the difference between a computed extreme of a circular arc and
/// its exact value.
const ARC_EXTREME_AGREEMENT: f64 = 1.0e-12;

/// A degree-2 span has its extremes at its ends or where its derivative is
/// zero. The unit circle arc from `-π/4` to `π/4`, a rational quadratic with
/// middle weight `cos(π/4)`, reaches `u = 1` at its middle, inside the hull
/// bound `√2`.
#[test]
fn a_circular_arc_has_its_exact_extremes() {
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let arc = curve(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[[half, -half], [std::f64::consts::SQRT_2, 0.0], [half, half]],
        Some(vec![1.0, half, 1.0]),
        [0.0, 1.0],
    );
    for (axis, [lowest, highest]) in [(0, [half, 1.0]), (1, [-half, half])] {
        let extremes = extremes_of(&arc, axis);
        for (bound, exact) in extremes
            .lowest
            .into_iter()
            .map(|bound| (bound, lowest))
            .chain(extremes.highest.into_iter().map(|bound| (bound, highest)))
        {
            assert!(
                (bound - exact).abs() <= ARC_EXTREME_AGREEMENT,
                "{extremes:?}"
            );
        }
    }
}

/// A span of degree three has each extreme between its control extreme and
/// the extreme of its evaluated points. The cubic `v = 12t(1 − t)` reaches 3
/// inside the hull bound 4. A ring at 3.5 lies above it, inside the hull
/// bound, and the two rings have no winding; a ring at 5 is ordered.
#[test]
fn a_ring_of_degree_three_is_ordered_only_beyond_its_control_hull() {
    let cubic = curve(
        3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        &[
            [0.0, 0.0],
            [TAU / 3.0, 4.0],
            [2.0 * TAU / 3.0, 4.0],
            [TAU, 0.0],
        ],
        None,
        [0.0, 1.0],
    );
    let lower = extremes_of(&cubic, 1);
    assert_eq!(
        lower,
        Extremes {
            lowest: [0.0, 0.0],
            highest: [3.0, 4.0],
        }
    );
    let rings = |level: f64| {
        [
            Turning {
                position: 0,
                outer: true,
                turn: Turn {
                    axis: 0,
                    forward: true,
                },
                extremes: lower,
            },
            turning(1, false, 0, false, [level, level]),
        ]
    };
    assert!(strip_windings(&rings(3.5), false).is_empty());
    assert_eq!(
        strip_windings(&rings(5.0), false),
        [(0, BoundaryWinding::Kept), (1, BoundaryWinding::Kept)]
    );
}

/// The degree-2 parameter curve with the knots `0, 0, 0, 1, 2, 3` and the
/// `v` poles `v`, over `[0, 1]`. The knots are unclamped at the end of the
/// domain: over `[0, 1]` the curve ends at the middle of the last two poles.
fn unclamped_quadratic(v: [f64; 3]) -> (PcurveGeometry, [f64; 2]) {
    curve(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0],
        &[[0.0, v[0]], [3.0, v[1]], [3.0, v[2]]],
        None,
        [0.0, 1.0],
    )
}

/// With the `v` poles `0.5, 0.9, 0.1` the unclamped quadratic runs
/// `v = 0.5 → 0.7 → 0.5`: its extremes on `v` are `0.5` and `0.7`.
#[test]
fn an_unclamped_quadratic_has_the_extremes_of_its_curve() {
    let extremes = extremes_of(&unclamped_quadratic([0.5, 0.9, 0.1]), 1);
    for (bounds, exact) in [(extremes.lowest, 0.5), (extremes.highest, 0.7)] {
        for bound in bounds {
            assert!(
                (bound - exact).abs() <= ARC_EXTREME_AGREEMENT,
                "{extremes:?}"
            );
        }
    }
}

/// Two disjoint rings of unclamped quadratics that both advance along `u`:
/// the upper (outer) ring with the `v` poles `0.5, 0.9, 0.1` runs in
/// `[0.5, 0.7]`, and the lower (inner) ring with the `v` poles `0.4, 0.1,
/// 0.7` runs in `[0.25, 0.4]`. The strip lies above the lower ring, which
/// keeps its source direction, and below the upper ring, which is reversed.
#[test]
fn two_disjoint_rings_of_unclamped_quadratics_bound_the_strip_between_them() {
    let turns = [([0.5, 0.9, 0.1], true), ([0.4, 0.1, 0.7], false)].map(|(v, outer)| Turning {
        position: usize::from(!outer),
        outer,
        turn: Turn {
            axis: 0,
            forward: true,
        },
        extremes: extremes_of(&unclamped_quadratic(v), 1),
    });
    assert_eq!(
        strip_windings(&turns, false),
        [(1, BoundaryWinding::Kept), (0, BoundaryWinding::Reversed)]
    );
}

/// Points sampled on each parameter curve to check its stated extremes.
const SHAPE_SAMPLES: u32 = 1024;

/// Bound on how far the lowest or highest of [`SHAPE_SAMPLES`] points of a
/// curve in the knot-shape table lies from its exact extreme.
const SHAPE_SAMPLE_AGREEMENT: f64 = 1.0e-5;

/// Bound on the rounding of a stated extreme against an evaluated point.
const SHAPE_ROUNDING: f64 = 1.0e-12;

/// For each row of a table of knot shapes, without and with weights, the
/// stated interval of the lowest `v` contains the lowest evaluated point of
/// the curve over its range, and the stated interval of the highest `v` the
/// highest. Both ends of each interval are checked, as `strip_windings`
/// reads both. Where the degree is two or less both ends are the exact
/// extreme.
///
/// Each row of the table is a parameter curve over its range. The rows are of
/// degree two: clamped; unclamped at the end, at the start, and at both
/// ends; with an interior knot above the degree; with both range ends on
/// knots, one of them interior; with both range ends inside one span. The
/// row of degree one has both range ends inside spans. The rows of degree
/// three are unclamped at both ends; with an interior knot above the
/// degree, where the curve is continuous; with one range end on an interior
/// knot and the other inside a span.
#[test]
fn the_extremes_of_each_knot_shape_with_and_without_weights_contain_the_curve() {
    /// A row: its name, degree, knots, `v` poles at `u` = 0, 1, 2, …,
    /// positive weights, and range.
    type KnotShape = (&'static str, u32, Vec<f64>, Vec<f64>, Vec<f64>, [f64; 2]);
    let shapes: Vec<KnotShape> = vec![
        (
            "clamped",
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.5, 0.9, 0.1],
            vec![1.0, 2.0, 0.5],
            [0.0, 1.0],
        ),
        (
            "unclamped end",
            2,
            vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0],
            vec![0.5, 0.9, 0.1],
            vec![1.0, 2.0, 0.5],
            [0.0, 1.0],
        ),
        (
            "unclamped start",
            2,
            vec![-2.0, -1.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.1, 0.9, 0.5],
            vec![0.5, 2.0, 1.0],
            [0.0, 1.0],
        ),
        (
            "unclamped both ends",
            2,
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
            vec![0.1, 0.9, 0.2],
            vec![0.5, 3.0, 1.0],
            [2.0, 3.0],
        ),
        (
            "interior knot above the degree",
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0],
            vec![0.5, 0.9, 0.1, 0.3, -0.4, 0.6],
            vec![1.0, 2.0, 0.5, 1.0, 3.0, 0.5],
            [0.0, 2.0],
        ),
        (
            "range end on a knot",
            2,
            vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0],
            vec![0.5, 0.9, 0.1, 0.8],
            vec![1.0, 0.5, 2.0, 1.0],
            [1.0, 2.0],
        ),
        (
            "range inside a span",
            2,
            vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0],
            vec![0.5, 0.9, 0.1],
            vec![1.0, 0.25, 2.0],
            [0.25, 0.75],
        ),
        (
            "degree one, range ends inside spans",
            1,
            vec![0.0, 0.0, 1.0, 2.0, 2.0],
            vec![0.5, 0.9, 0.1],
            vec![1.0, 2.0, 0.5],
            [0.5, 1.5],
        ),
        (
            "cubic, unclamped both ends",
            3,
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
            vec![0.2, 1.0, -0.6, 0.4],
            vec![0.5, 2.0, 1.0, 3.0],
            [3.0, 4.0],
        ),
        (
            "cubic, interior knot above the degree",
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0],
            vec![0.2, 1.0, -0.6, 0.4, 0.4, -0.2, 0.3, 0.7],
            vec![1.0, 2.0, 0.5, 1.0, 1.5, 3.0, 0.5, 1.0],
            [0.0, 2.0],
        ),
        (
            "cubic, range end on a knot",
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0, 4.0, 4.0],
            vec![0.2, 1.0, -0.6, 0.4, 0.9, -0.2, 0.3],
            vec![1.0, 0.5, 2.0, 1.0, 3.0, 0.5, 1.0],
            [1.0, 2.5],
        ),
    ];
    for (name, degree, knots, v, weights, range) in shapes {
        let vertices = v
            .iter()
            .enumerate()
            .map(|(index, v)| [f64::from(u32::try_from(index).expect("a pole index")), *v])
            .collect::<Vec<_>>();
        for (form, weights) in [("polynomial", None), ("rational", Some(weights))] {
            let shape = curve(degree, knots.clone(), &vertices, weights, range);
            let [low, high] = with_service_context(b"", |ctx| {
                (0..=SHAPE_SAMPLES).fold([f64::INFINITY, f64::NEG_INFINITY], |[low, high], step| {
                    let parameter = (range[1] - range[0])
                        .mul_add(f64::from(step) / f64::from(SHAPE_SAMPLES), range[0]);
                    let point = cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::pcurve_uv(ctx, &shape.0, parameter),
                    )
                    .expect("the curve evaluates")
                    .expect("the point is finite");
                    [low.min(point.v), high.max(point.v)]
                })
            });
            let extremes = extremes_of(&shape, 1);
            let context =
                format!("{name}, {form}: {extremes:?} against the sampled [{low}, {high}]");
            assert!(
                extremes.lowest[0] <= low + SHAPE_ROUNDING
                    && high - SHAPE_ROUNDING <= extremes.highest[1],
                "outer ends exclude the curve; {context}"
            );
            assert!(
                low - SHAPE_SAMPLE_AGREEMENT <= extremes.lowest[1]
                    && extremes.highest[0] <= high + SHAPE_SAMPLE_AGREEMENT,
                "inner ends exclude the curve; {context}"
            );
            if degree <= 2 {
                for (bound, sampled) in extremes
                    .lowest
                    .into_iter()
                    .map(|bound| (bound, low))
                    .chain(extremes.highest.into_iter().map(|bound| (bound, high)))
                {
                    assert!(
                        (bound - sampled).abs() <= SHAPE_SAMPLE_AGREEMENT,
                        "an end is not the exact extreme; {context}"
                    );
                }
            }
        }
    }
}
