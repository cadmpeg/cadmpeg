// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::sketches::SketchEntityId;
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
