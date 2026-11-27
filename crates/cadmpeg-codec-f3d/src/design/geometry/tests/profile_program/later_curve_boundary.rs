// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::collections::HashSet;

fn selected(
    sketch: &Sketch,
    entities: &[SketchEntity],
    rings: &[&[usize]],
    later: &[usize],
) -> Option<Vec<SketchProfileRegion>> {
    let ids = |indices: &[usize]| -> HashSet<SketchEntityId> {
        indices
            .iter()
            .map(|index| entities[*index].id().clone())
            .collect()
    };
    let rings = rings.iter().map(|ring| ids(ring)).collect::<Vec<_>>();
    crate::test_support::with_decode_context(|ctx| {
        super::super::super::arrangement_regions_holding_named_curves(
            sketch,
            entities,
            &rings,
            &ids(later),
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
}

/// A square whose right side was shortened and closed by a curve the Sketch
/// gained afterwards, beside a second square that the selection leaves.
fn closed_by_later_curve() -> (Sketch, Vec<SketchEntity>) {
    let (sketch, _) = divided_circle(false);
    let line = |name: &str, start: (f64, f64), end: (f64, f64)| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        )
    };
    let entities = vec![
        line("bottom", (0.0, 0.0), (2.0, 0.0)),
        line("right", (2.0, 0.0), (2.0, 1.0)),
        line("top", (2.0, 2.0), (0.0, 2.0)),
        line("left", (0.0, 2.0), (0.0, 0.0)),
        line("later", (2.0, 1.0), (2.0, 2.0)),
        line("other-bottom", (5.0, 0.0), (7.0, 0.0)),
        line("other-right", (7.0, 0.0), (7.0, 2.0)),
        line("other-top", (7.0, 2.0), (5.0, 2.0)),
        line("other-left", (5.0, 2.0), (5.0, 0.0)),
    ];
    (sketch, entities)
}

#[test]
fn a_face_bounded_by_every_named_curve_and_a_later_curve_is_selected() {
    let (sketch, entities) = closed_by_later_curve();
    let regions = selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[4]).expect("one face");
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
    assert_eq!(boundary.len(), 5);
    assert!(boundary.contains(entities[4].id()));
    assert!(!boundary.contains(entities[5].id()));
}

#[test]
fn a_boundary_curve_that_is_not_later_leaves_the_selection_native() {
    let (sketch, entities) = closed_by_later_curve();
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[]).is_none());
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3]], &[5]).is_none());
}

#[test]
fn a_named_curve_off_the_face_or_two_rings_on_one_face_define_no_selection() {
    let (sketch, entities) = closed_by_later_curve();
    assert!(selected(&sketch, &entities, &[&[0, 1, 2, 3, 5]], &[4]).is_none());
    assert!(selected(&sketch, &entities, &[&[0, 1, 2], &[3, 0]], &[4]).is_none());
}
