// SPDX-License-Identifier: Apache-2.0
use super::{chain, charge, profile_cost, Outline, Point, Ring, Segment, WORK_LIMIT};

fn polygon(corners: &[Point]) -> Vec<Segment> {
    corners
        .iter()
        .copied()
        .zip(corners.iter().copied().cycle().skip(1))
        .take(corners.len())
        .collect()
}

fn ring(corners: &[Point]) -> Ring {
    let segments = polygon(corners);
    Ring::new(&segments, &chain(&segments).unwrap()).unwrap()
}

fn square(low: Point, high: Point) -> Ring {
    ring(&[low, [high[0], low[1]], high, [low[0], high[1]]])
}

fn outline(outer: Ring, holes: Vec<Ring>) -> Outline {
    Outline::new(
        std::iter::once((true, outer))
            .chain(holes.into_iter().map(|hole| (false, hole)))
            .collect(),
    )
    .unwrap()
}

#[test]
fn a_chain_reorders_and_reverses_segments_into_one_closed_boundary() {
    let mut segments = polygon(&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]]);
    segments.swap(0, 2);
    segments[1] = (segments[1].1, segments[1].0);
    let ring = Ring::new(&segments, &chain(&segments).unwrap()).unwrap();
    assert_eq!(ring.0.len(), 4);
    ring.admit().unwrap();
    assert!(ring.contains([2.0, 1.5]));
    assert!(!ring.contains([5.0, 1.5]));
}

#[test]
fn an_open_or_forked_chain_is_refused() {
    let mut open = polygon(&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]]);
    open.pop();
    assert!(chain(&open).is_err());
    let mut fork = polygon(&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]]);
    fork.push(([4.0, 0.0], [9.0, 9.0]));
    assert!(chain(&fork).is_err());
}

#[test]
fn a_bowtie_boundary_is_not_a_simple_polygon() {
    ring(&[[0.0, 0.0], [4.0, 4.0], [4.0, 0.0], [0.0, 4.0]])
        .admit()
        .unwrap_err();
    ring(&[[0.0, 0.0], [4.0, 0.0], [8.0, 0.0]])
        .admit()
        .unwrap_err();
}

#[test]
fn a_hole_must_lie_inside_the_outer_boundary_without_touching_it() {
    let outer = || square([0.0, 0.0], [10.0, 10.0]);
    for hole in [
        square([11.0, 1.0], [12.0, 2.0]),
        square([0.0, 1.0], [2.0, 2.0]),
        square([4.0, 4.0], [12.0, 6.0]),
    ] {
        Outline::new(vec![(true, outer()), (false, hole)]).unwrap_err();
    }
    Outline::new(vec![
        (true, outer()),
        (false, square([2.0, 2.0], [3.0, 3.0])),
        (false, square([2.5, 2.5], [2.75, 2.75])),
    ])
    .unwrap_err();
}

#[test]
fn a_convex_face_covers_a_profile_inside_or_on_its_rim() {
    let face = outline(square([0.0, 0.0], [20.0, 10.0]), vec![]);
    assert!(face.covers(&square([0.0, 0.0], [20.0, 10.0])).unwrap());
    assert!(face.covers(&square([1.0, 1.0], [5.0, 5.0])).unwrap());
    assert!(!face.covers(&square([1.0, 1.0], [21.0, 5.0])).unwrap());
}

#[test]
fn a_concave_face_covers_only_a_profile_in_one_arm() {
    let face = outline(
        ring(&[
            [-4.0, -4.0],
            [4.0, -4.0],
            [4.0, 0.0],
            [0.0, 0.0],
            [0.0, 4.0],
            [-4.0, 4.0],
        ]),
        vec![],
    );
    assert!(face.covers(&square([-3.0, -3.0], [-1.0, 3.0])).unwrap());
    assert!(!face.covers(&square([1.0, 1.0], [3.0, 3.0])).unwrap());
    assert!(!face.covers(&square([-1.0, -1.0], [2.0, 2.0])).unwrap());
    assert!(face.overlaps(&square([-1.0, -1.0], [2.0, 2.0])).unwrap());
    assert!(!face.overlaps(&square([1.0, 1.0], [3.0, 3.0])).unwrap());
}

#[test]
fn a_hole_in_the_footprint_defeats_coverage_wherever_it_lies() {
    let face = outline(
        square([0.0, 0.0], [20.0, 10.0]),
        vec![square([9.0, 4.0], [11.0, 6.0])],
    );
    // The hole sits inside the profile, on its boundary, or around it.
    assert!(!face.covers(&square([0.0, 0.0], [20.0, 10.0])).unwrap());
    assert!(!face.covers(&square([10.0, 5.0], [15.0, 8.0])).unwrap());
    assert!(!face.covers(&square([9.5, 4.5], [10.5, 5.5])).unwrap());
    assert!(face.covers(&square([12.0, 1.0], [15.0, 9.0])).unwrap());
    assert!(face.overlaps(&square([10.0, 5.0], [15.0, 8.0])).unwrap());
    assert!(!face.overlaps(&square([9.5, 4.5], [10.5, 5.5])).unwrap());
}

#[test]
fn a_distant_profile_does_not_overlap_the_footprint() {
    let face = outline(square([0.0, 0.0], [20.0, 10.0]), vec![]);
    assert!(!face.overlaps(&square([30.0, 0.0], [31.0, 1.0])).unwrap());
    assert!(face.overlaps(&square([-5.0, -5.0], [30.0, 30.0])).unwrap());
}

#[test]
fn profile_work_is_charged_to_the_shared_budget() {
    let mut work = WORK_LIMIT;
    charge(&mut work, profile_cost(700)).unwrap();
    assert_eq!(work, WORK_LIMIT - 2 * 700 * 700);
    // A second charge from the same budget runs out before a profile of this size would.
    charge(&mut work, profile_cost(700)).unwrap_err();
    let mut fresh = WORK_LIMIT;
    charge(&mut fresh, profile_cost(1_100)).unwrap_err();
    charge(&mut fresh, profile_cost(usize::MAX)).unwrap_err();
}
