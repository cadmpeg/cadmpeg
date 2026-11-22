// SPDX-License-Identifier: Apache-2.0

use super::*;

/// The selection of counted `rings`, each member with its direction and
/// `[k, n]` piece words.
fn counted_selection(
    sketch: &Sketch,
    entities: &[SketchEntity],
    rings: &[Vec<(&SketchEntity, bool, [u32; 2])>],
) -> Option<super::super::super::MemberRingRegions> {
    let rings = rings
        .iter()
        .map(|ring| {
            ring.iter()
                .map(|(entity, reversed, incidence)| {
                    (
                        SketchEntityUse {
                            entity: entity.id().clone(),
                            reversed: *reversed,
                        },
                        *incidence,
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    crate::test_support::with_decode_context(|ctx| {
        super::super::super::arrangement_regions_matching_counted_rings(
            sketch,
            entities,
            &rings,
            &std::collections::HashSet::new(),
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
}

/// A 6 by 2 strip split by a divider at `x = 2`, with a leg below the strip
/// whose sides meet the strip's bottom line at `x = 4` and `x = 5`.
/// Entities: bottom, right, top, left, divider, leg sides and leg bottom.
fn strip_with_leg() -> (Sketch, Vec<SketchEntity>) {
    let id = SketchId::mint("synthetic:test:id#counted-strip").unwrap();
    let entities = vec![
        line_entity(&id, "bottom", (0.0, 0.0), (6.0, 0.0)),
        line_entity(&id, "right", (6.0, 0.0), (6.0, 2.0)),
        line_entity(&id, "top", (6.0, 2.0), (0.0, 2.0)),
        line_entity(&id, "left", (0.0, 2.0), (0.0, 0.0)),
        line_entity(&id, "divider", (2.0, 2.0), (2.0, 0.0)),
        line_entity(&id, "leg-left", (4.0, 0.0), (4.0, -2.0)),
        line_entity(&id, "leg-bottom", (4.0, -2.0), (5.0, -2.0)),
        line_entity(&id, "leg-right", (5.0, -2.0), (5.0, 0.0)),
    ];
    (sketch_with_loops(id, &[]), entities)
}

/// The right strip face: its bottom line is piece 2 of 2 and its top line
/// piece 1 of 2, cut by the divider.
fn right_strip_ring(
    entities: &[SketchEntity],
    bottom: [u32; 2],
) -> Vec<(&SketchEntity, bool, [u32; 2])> {
    vec![
        (&entities[4], false, [1, 1]),
        (&entities[0], false, bottom),
        (&entities[2], false, [1, 2]),
        (&entities[1], false, [1, 1]),
    ]
}

#[test]
fn counted_ring_spans_fragments_that_an_outside_curve_splits() {
    let (sketch, entities) = strip_with_leg();
    let selected =
        counted_selection(&sketch, &entities, &[right_strip_ring(&entities, [2, 2])]).unwrap();
    let [SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }] = selected.regions.as_slice()
    else {
        panic!("one trimmed region: {:?}", selected.regions);
    };
    assert!(hole_boundaries.is_empty());
    let bottom_fragments = outer_boundary
        .iter()
        .filter(|use_| use_.entity == *entities[0].id())
        .count();
    assert_eq!(bottom_fragments, 3);
    let uncounted = right_strip_ring(&entities, [2, 2])
        .into_iter()
        .map(|(entity, reversed, _)| (entity, reversed))
        .collect::<Vec<_>>();
    assert!(member_regions(&sketch, &entities, &[uncounted]).is_none());
}

#[test]
fn counted_ring_rejects_pieces_of_the_whole_arrangement() {
    let (sketch, entities) = strip_with_leg();
    for bottom in [[4, 4], [2, 3], [1, 2]] {
        assert!(
            counted_selection(&sketch, &entities, &[right_strip_ring(&entities, bottom)]).is_none(),
            "{bottom:?}"
        );
    }
}

#[test]
fn counted_ring_orders_reversed_line_pieces_along_the_line() {
    let id = SketchId::mint("synthetic:test:id#counted-reversed").unwrap();
    let entities = vec![
        line_entity(&id, "bottom", (6.0, 0.0), (0.0, 0.0)),
        line_entity(&id, "left", (0.0, 0.0), (0.0, 2.0)),
        line_entity(&id, "top", (0.0, 2.0), (6.0, 2.0)),
        line_entity(&id, "right", (6.0, 2.0), (6.0, 0.0)),
        line_entity(&id, "divider", (2.0, 2.0), (2.0, 0.0)),
        line_entity(&id, "second", (4.0, 2.0), (4.0, 0.0)),
        line_entity(&id, "tee", (1.0, 0.0), (1.0, -2.0)),
    ];
    let sketch = sketch_with_loops(id, &[]);
    let ring = |bottom| {
        vec![
            (&entities[0], true, bottom),
            (&entities[4], true, [1, 1]),
            (&entities[2], true, [1, 2]),
            (&entities[1], true, [1, 1]),
        ]
    };
    assert!(counted_selection(&sketch, &entities, &[ring([2, 2])]).is_some());
    assert!(counted_selection(&sketch, &entities, &[ring([1, 2])]).is_none());
}

#[test]
fn counted_ring_numbers_circle_pieces_from_angle_zero() {
    let id = SketchId::mint("synthetic:test:id#counted-circle").unwrap();
    let chord = |name: &str, u: f64| {
        let v = (25.0 - u * u).sqrt();
        line_entity(&id, name, (u, v), (u, -v))
    };
    let entities = vec![
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#counted-circle-curve").unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(5.0).unwrap(),
            })
            .unwrap(),
        ),
        chord("left-chord", -2.0),
        chord("right-chord", 3.0),
        line_entity(&id, "antenna", (0.0, 5.0), (0.0, 8.0)),
    ];
    let sketch = sketch_with_loops(id, &[]);
    let band = |top, bottom| {
        vec![
            (&entities[1], false, [1, 1]),
            (&entities[0], false, top),
            (&entities[0], false, bottom),
            (&entities[2], true, [1, 1]),
        ]
    };
    assert!(counted_selection(&sketch, &entities, &[band([2, 4], [4, 4])]).is_some());
    assert!(counted_selection(&sketch, &entities, &[band([1, 4], [3, 4])]).is_none());
}

#[test]
fn counted_ring_counts_a_line_beyond_the_corner_as_a_piece() {
    let id = SketchId::mint("synthetic:test:id#counted-extension").unwrap();
    let entities = vec![
        line_entity(&id, "bottom", (8.0, 0.0), (0.0, 0.0)),
        line_entity(&id, "right", (6.0, 0.0), (6.0, 2.0)),
        line_entity(&id, "top", (6.0, 2.0), (0.0, 2.0)),
        line_entity(&id, "left", (0.0, 2.0), (0.0, 0.0)),
        line_entity(&id, "tee", (3.0, 0.0), (3.0, -2.0)),
    ];
    let sketch = sketch_with_loops(id, &[]);
    let ring = |bottom| {
        vec![
            (&entities[0], true, bottom),
            (&entities[1], false, [1, 1]),
            (&entities[2], false, [1, 1]),
            (&entities[3], false, [1, 1]),
        ]
    };
    assert!(counted_selection(&sketch, &entities, &[ring([2, 2])]).is_some());
    assert!(counted_selection(&sketch, &entities, &[ring([1, 2])]).is_none());
    assert!(counted_selection(&sketch, &entities, &[ring([1, 1])]).is_none());
}
