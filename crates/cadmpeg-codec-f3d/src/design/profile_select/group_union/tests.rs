// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::design::geometry::ProfileGeometryWork;
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId};
use std::f64::consts::PI;

fn edge(id: &str, range: [f64; 2], reversed: bool) -> SketchProfileBoundaryUse {
    SketchProfileBoundaryUse {
        entity: SketchEntityId::mint(format!("synthetic:test:edge#{id}")).unwrap(),
        parameter_range: DirectedParameterRange::new(range).unwrap(),
        reversed,
    }
}

fn region(edges: Vec<SketchProfileBoundaryUse>) -> SketchProfileRegion {
    SketchProfileRegion::trimmed(edges, Vec::new()).unwrap()
}

/// A circle that a line divides at angles `PI` and `2 PI`: the whole disc,
/// then the half above the line and the half below it.
fn divided_disc() -> Vec<SketchProfileRegion> {
    vec![
        region(vec![
            edge("circle", [PI, 2.0 * PI], false),
            edge("circle", [2.0 * PI, 3.0 * PI], false),
        ]),
        region(vec![
            edge("line", [0.25, 0.75], false),
            edge("circle", [2.0 * PI, 3.0 * PI], false),
        ]),
        region(vec![
            edge("line", [0.25, 0.75], true),
            edge("circle", [PI, 2.0 * PI], false),
        ]),
    ]
}

#[test]
fn first_region_that_is_the_union_of_its_further_regions_adds_no_region() {
    let mut regions = divided_disc();
    let disc = regions[0].clone();
    let halves = regions[1..].to_vec();
    assert_eq!(remove_union_first_region(&mut regions, 0, 1), Some(disc));
    assert_eq!(regions, halves);
}

#[test]
fn earlier_groups_keep_their_regions() {
    let earlier = region(vec![edge("other", [0.0, 2.0 * PI], false)]);
    let mut regions = vec![earlier.clone()];
    regions.extend(divided_disc());
    let mut expected = vec![earlier];
    expected.extend(divided_disc().into_iter().skip(1));
    assert!(remove_union_first_region(&mut regions, 1, 2).is_some());
    assert_eq!(regions, expected);
}

#[test]
fn first_region_stays_when_the_further_regions_do_not_cover_it() {
    let mut one_half = divided_disc();
    one_half.pop();
    one_half.push(region(vec![
        edge("line", [0.25, 0.75], true),
        edge("other", [PI, 2.0 * PI], false),
    ]));
    let unchanged = one_half.clone();
    assert_eq!(remove_union_first_region(&mut one_half, 0, 1), None);
    assert_eq!(one_half, unchanged);
}

#[test]
fn first_region_stays_when_the_shared_interval_does_not_cancel() {
    let mut regions = divided_disc();
    regions[2] = region(vec![
        edge("line", [0.25, 0.75], false),
        edge("circle", [PI, 2.0 * PI], false),
    ]);
    let unchanged = regions.clone();
    assert_eq!(remove_union_first_region(&mut regions, 0, 1), None);
    assert_eq!(regions, unchanged);
}

#[test]
fn first_region_stays_without_further_regions_or_with_one() {
    let mut regions = divided_disc();
    regions.truncate(2);
    let unchanged = regions.clone();
    assert_eq!(remove_union_first_region(&mut regions, 0, 1), None);
    assert_eq!(regions, unchanged);
    let mut single = divided_disc();
    single.truncate(1);
    let unchanged = single.clone();
    assert_eq!(remove_union_first_region(&mut single, 0, 1), None);
    assert_eq!(single, unchanged);
}

#[test]
fn a_first_region_that_added_no_region_or_several_is_kept() {
    let mut regions = divided_disc();
    let unchanged = regions.clone();
    assert_eq!(remove_union_first_region(&mut regions, 0, 0), None);
    assert_eq!(regions, unchanged);
    assert_eq!(remove_union_first_region(&mut regions, 0, 2), None);
    assert_eq!(regions, unchanged);
}

/// The linear tolerance of these regions, in centimetres.
const EPS_SAME_POINT: f64 = 1.0e-7;

fn line(id: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:edge#{id}")).unwrap(),
        SketchId::mint("synthetic:test:id#union").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        })
        .unwrap(),
    )
}

/// A 4 by 4 square, the line `mid` across it at height 2, and the line
/// `over` that runs along the top side past both ends.
fn square_lines() -> Vec<SketchEntity> {
    vec![
        line("bottom", (0.0, 0.0), (4.0, 0.0)),
        line("right", (4.0, 0.0), (4.0, 4.0)),
        line("top", (4.0, 4.0), (0.0, 4.0)),
        line("left", (0.0, 4.0), (0.0, 0.0)),
        line("mid", (0.0, 2.0), (4.0, 2.0)),
        line("over", (-1.0, 4.0), (5.0, 4.0)),
    ]
}

/// The two halves of the square that `mid` divides.
fn square_halves() -> Vec<SketchProfileRegion> {
    vec![
        region(vec![
            edge("bottom", [0.0, 1.0], false),
            edge("right", [0.0, 0.5], false),
            edge("mid", [0.0, 1.0], true),
            edge("left", [0.5, 1.0], false),
        ]),
        region(vec![
            edge("mid", [0.0, 1.0], false),
            edge("right", [0.5, 1.0], false),
            edge("top", [0.0, 1.0], false),
            edge("left", [0.0, 0.5], false),
        ]),
    ]
}

/// The first region's entries: the curve, the piece ordinal, and the piece
/// count.
fn entries(entries: &[(&str, [u32; 2])]) -> Vec<(SketchEntityId, [u32; 2])> {
    entries
        .iter()
        .map(|(id, fields)| (edge_id(id), *fields))
        .collect()
}

fn edge_id(id: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("synthetic:test:edge#{id}")).unwrap()
}

const SIDES: [(&str, [u32; 2]); 4] = [
    ("bottom", [1, 1]),
    ("right", [1, 1]),
    ("top", [1, 1]),
    ("left", [1, 1]),
];

fn composes(further: &[SketchProfileRegion], first: &[(&str, [u32; 2])]) -> bool {
    let work = ProfileGeometryWork::new();
    crate::test_support::with_decode_context(|ctx| {
        further_regions_compose_first(
            further,
            &entries(first),
            &square_lines(),
            EPS_SAME_POINT,
            &work,
            ctx,
        )
    })
    .unwrap()
}

#[test]
fn further_regions_compose_a_first_region_that_names_their_union_boundary() {
    assert!(composes(&square_halves(), &SIDES));
}

#[test]
fn further_regions_compose_a_first_region_with_a_coincident_line() {
    let mut first = SIDES.to_vec();
    first.push(("over", [2, 3]));
    assert!(composes(&square_halves(), &first));
}

#[test]
fn further_regions_compose_no_first_region_with_a_line_piece_off_the_boundary() {
    let mut first = SIDES.to_vec();
    first.push(("over", [1, 3]));
    assert!(!composes(&square_halves(), &first));
    first.pop();
    first.push(("over", [3, 3]));
    assert!(!composes(&square_halves(), &first));
}

#[test]
fn further_regions_compose_no_first_region_with_a_wrong_piece_count() {
    let mut first = SIDES.to_vec();
    first.push(("over", [2, 2]));
    assert!(!composes(&square_halves(), &first));
    first.pop();
    first.push(("over", [2, 4]));
    assert!(!composes(&square_halves(), &first));
    first.pop();
    first.push(("over", [0, 3]));
    assert!(!composes(&square_halves(), &first));
}

#[test]
fn further_regions_compose_no_first_region_that_names_an_interior_curve() {
    // The side pieces below `mid` lie on the boundary; `mid` does not.
    let first = [
        ("bottom", [1, 1]),
        ("right", [1, 2]),
        ("top", [1, 1]),
        ("left", [1, 2]),
        ("mid", [1, 1]),
    ];
    assert!(!composes(&square_halves(), &first));
}

#[test]
fn further_regions_compose_no_first_region_that_misses_a_boundary_curve() {
    assert!(!composes(&square_halves(), &SIDES[..3]));
}

#[test]
fn one_further_region_composes_no_first_region() {
    let halves = square_halves();
    assert!(!composes(&halves[..1], &SIDES));
    let repeated = vec![halves[0].clone(), halves[0].clone()];
    assert!(!composes(&repeated, &SIDES));
}
