// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::collections::HashSet;

/// The regions that rings of fixture entities select, where `later` and
/// `ignored` list fixture entity indices.
fn selected(
    sketch: &Sketch,
    entities: &[SketchEntity],
    rings: &[&[usize]],
    later: &[usize],
    ignored: &[usize],
) -> Option<Vec<SketchProfileRegion>> {
    let ids = |indices: &[usize]| -> HashSet<SketchEntityId> {
        indices
            .iter()
            .map(|index| entities[*index].id().clone())
            .collect()
    };
    let rings = rings.iter().map(|ring| ids(ring)).collect::<Vec<_>>();
    let unnamed = super::super::super::UnnamedBoundaryCurves {
        later: ids(later),
        ignored: ids(ignored),
    };
    crate::test_support::with_decode_context(|ctx| {
        super::super::super::arrangement_regions_holding_named_curves(
            sketch,
            entities,
            &rings,
            &unnamed,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
}

type LineSpec<'a> = (&'a str, (f64, f64), (f64, f64));

fn lines(sketch: &Sketch, specs: &[LineSpec<'_>]) -> Vec<SketchEntity> {
    specs
        .iter()
        .map(|(name, start, end)| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(start.0, start.1),
                    end: Point2::new(end.0, end.1),
                })
                .unwrap(),
            )
        })
        .collect()
}

/// A square whose right side stops at height one and is closed by two curves
/// the Sketch gained afterwards (entities 4 and 5), beside a second square
/// (entities 6 to 9) that no selection names.
fn closed_by_later_curves() -> (Sketch, Vec<SketchEntity>) {
    let (sketch, _) = divided_circle(false);
    let entities = lines(
        &sketch,
        &[
            ("bottom", (0.0, 0.0), (2.0, 0.0)),
            ("right", (2.0, 0.0), (2.0, 1.0)),
            ("top", (2.0, 2.0), (0.0, 2.0)),
            ("left", (0.0, 2.0), (0.0, 0.0)),
            ("later-a", (2.0, 1.0), (2.0, 1.5)),
            ("later-b", (2.0, 1.5), (2.0, 2.0)),
            ("other-bottom", (5.0, 0.0), (7.0, 0.0)),
            ("other-right", (7.0, 0.0), (7.0, 2.0)),
            ("other-top", (7.0, 2.0), (5.0, 2.0)),
            ("other-left", (5.0, 2.0), (5.0, 0.0)),
        ],
    );
    (sketch, entities)
}

#[test]
fn a_face_bounded_by_every_named_curve_and_later_curves_is_selected() {
    let (sketch, entities) = closed_by_later_curves();
    let regions = selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[4, 5], &[]).expect("one face");
    let [SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }] = regions.as_slice()
    else {
        panic!("one trimmed region: {regions:?}");
    };
    assert!(hole_boundaries.is_empty());
    let boundary = outer_boundary
        .iter()
        .map(|use_| use_.entity.clone())
        .collect::<HashSet<_>>();
    assert_eq!(boundary.len(), 6);
    assert!(boundary.contains(entities[4].id()));
    assert!(!boundary.contains(entities[6].id()));
}

#[test]
fn a_boundary_curve_that_is_not_later_leaves_the_selection_native() {
    let (sketch, entities) = closed_by_later_curves();
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[], &[]).is_none());
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[4], &[]).is_none());
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[6], &[]).is_none());
}

#[test]
fn an_ignored_curve_stays_on_the_boundary_but_is_never_later() {
    let (sketch, entities) = closed_by_later_curves();
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[4], &[5]).is_some());
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[], &[4, 5]).is_none());
}

#[test]
fn a_named_curve_off_the_face_defines_no_selection() {
    let (sketch, entities) = closed_by_later_curves();
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3, 6]], &[4, 5], &[]).is_none());
}

#[test]
fn an_empty_ring_or_no_ring_defines_no_selection() {
    let (sketch, entities) = closed_by_later_curves();
    assert!(selected(&sketch, &entities, &[], &[4, 5], &[]).is_none());
    assert!(selected(&sketch, &entities, &[&[]], &[4, 5], &[]).is_none());
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3], &[]], &[4, 5], &[]).is_none());
}

#[test]
fn two_rings_on_one_face_define_no_selection() {
    let (sketch, entities) = closed_by_later_curves();
    assert!(selected(
        &sketch,
        &entities,
        &[&[0, 1, 2, 3], &[0, 1, 2, 3]],
        &[4, 5],
        &[]
    )
    .is_none());
}

#[test]
fn several_qualifying_faces_define_no_selection() {
    let (sketch, _) = divided_circle(false);
    // Two squares share their bottom edge; every other curve is later.
    let entities = lines(
        &sketch,
        &[
            ("shared", (0.0, 0.0), (2.0, 0.0)),
            ("up-right", (2.0, 0.0), (2.0, 2.0)),
            ("up-top", (2.0, 2.0), (0.0, 2.0)),
            ("up-left", (0.0, 2.0), (0.0, 0.0)),
            ("down-right", (2.0, 0.0), (2.0, -2.0)),
            ("down-bottom", (2.0, -2.0), (0.0, -2.0)),
            ("down-left", (0.0, -2.0), (0.0, 0.0)),
        ],
    );
    assert!(selected(&sketch, &entities, &[&[0]], &[1, 2, 3, 4, 5, 6], &[]).is_none());
}

#[test]
fn a_face_with_an_island_defines_no_selection() {
    let (sketch, _) = divided_circle(false);
    let entities = lines(
        &sketch,
        &[
            ("bottom", (0.0, 0.0), (4.0, 0.0)),
            ("right", (4.0, 0.0), (4.0, 2.0)),
            ("top", (4.0, 4.0), (0.0, 4.0)),
            ("left", (0.0, 4.0), (0.0, 0.0)),
            ("later", (4.0, 2.0), (4.0, 4.0)),
            ("island-bottom", (1.0, 1.0), (2.0, 1.0)),
            ("island-right", (2.0, 1.0), (2.0, 2.0)),
            ("island-top", (2.0, 2.0), (1.0, 2.0)),
            ("island-left", (1.0, 2.0), (1.0, 1.0)),
        ],
    );
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[4], &[]).is_none());
}

/// A later chord cuts a corner off the named square. The ring names the
/// whole square, so the face beyond the chord is part of the selected region
/// and both faces are selected.
#[test]
fn a_later_curve_that_split_the_named_region_selects_both_faces() {
    let (sketch, _) = divided_circle(false);
    let entities = lines(
        &sketch,
        &[
            ("bottom", (0.0, 0.0), (2.0, 0.0)),
            ("right", (2.0, 0.0), (2.0, 2.0)),
            ("top", (2.0, 2.0), (0.0, 2.0)),
            ("left", (0.0, 2.0), (0.0, 0.0)),
            ("chord", (0.0, 1.5), (1.0, 2.0)),
        ],
    );
    let regions = selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[4], &[]).expect("two faces");
    assert_eq!(regions.len(), 2);
}

/// The face beyond the later chord is bounded by a curve that is neither of
/// the ring, later nor ignored, so it is not part of the named region.
#[test]
fn a_neighbour_bounded_by_an_unlisted_curve_defines_no_selection() {
    let (sketch, _) = divided_circle(false);
    let entities = lines(
        &sketch,
        &[
            ("bottom", (0.0, 0.0), (2.0, 0.0)),
            ("right", (2.0, 0.0), (2.0, 2.0)),
            ("top", (2.0, 2.0), (0.0, 2.0)),
            ("left", (0.0, 2.0), (0.0, 0.0)),
            ("chord", (0.0, 1.5), (1.0, 2.0)),
            ("unlisted", (0.5, 1.75), (0.5, 2.0)),
        ],
    );
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[4], &[]).is_none());
}
