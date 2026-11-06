// SPDX-License-Identifier: Apache-2.0

use super::*;

const PROFILE_PROGRAM_TOLERANCE: f64 = 1.0e-7;

fn divided_circle(two_dividers: bool) -> (Sketch, Vec<SketchEntity>) {
    let id = SketchId::mint("synthetic:test:id#curve-program").unwrap();
    let mut entities = vec![SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#circle").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(5.0).unwrap(),
        })
        .unwrap(),
    )];
    for (name, x) in if two_dividers {
        vec![("left", -2.0), ("right", 2.0)]
    } else {
        vec![("left", 0.0)]
    } {
        entities.push(SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(x, -6.0),
                end: Point2::new(x, 6.0),
            })
            .unwrap(),
        ));
    }
    (
        Sketch {
            id,
            name: None,
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: None,
        },
        entities,
    )
}

#[test]
fn typed_profile_curve_program_resolves_one_trimmed_atomic_region() {
    let (sketch, entities) = divided_circle(true);
    let program = [
        entities[0].id().clone(),
        entities[1].id().clone(),
        entities[0].id().clone(),
        entities[2].id().clone(),
    ];
    let region = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_region_matching_curve_program(
            &sketch,
            &entities,
            &program,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .expect("one strip region");
    let SketchProfileRegion::Trimmed {
        outer_boundary: boundary,
        hole_boundaries: holes,
    } = region
    else {
        panic!("trimmed region");
    };
    assert!(holes.is_empty());
    assert_eq!(boundary.len(), 4);
    assert_eq!(
        boundary
            .iter()
            .filter(|use_| use_.entity == *entities[0].id())
            .count(),
        2
    );
    let mut reversed = program.to_vec();
    reversed.reverse();
    let reversed_region = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_region_matching_curve_program(
            &sketch,
            &entities,
            &reversed,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap();
    assert_eq!(
        reversed_region,
        Some(SketchProfileRegion::Trimmed {
            outer_boundary: boundary,
            hole_boundaries: holes
        })
    );
}

#[test]
fn typed_profile_curve_program_refuses_two_half_disks_with_the_same_program() {
    let (sketch, entities) = divided_circle(false);
    let program = [entities[0].id().clone(), entities[1].id().clone()];
    let region = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_region_matching_curve_program(
            &sketch,
            &entities,
            &program,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap();
    assert_eq!(region, None);
}

fn line_entity(sketch: &SketchId, name: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        })
        .unwrap(),
    )
}

/// Four counterclockwise lines named `{name}-0` to `{name}-3` around the
/// rectangle from `low` to `high`.
fn rectangle(
    sketch: &SketchId,
    name: &str,
    low: (f64, f64),
    high: (f64, f64),
) -> Vec<SketchEntity> {
    let corners = [low, (high.0, low.1), high, (low.0, high.1)];
    (0..4)
        .map(|index| {
            line_entity(
                sketch,
                &format!("{name}-{index}"),
                corners[index],
                corners[(index + 1) % 4],
            )
        })
        .collect()
}

fn sketch_with_loops(id: SketchId, loops: &[&[SketchEntity]]) -> Sketch {
    let profiles = loops
        .iter()
        .map(|entities| {
            entities
                .iter()
                .map(|entity| SketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: false,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    Sketch {
        id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(profiles).unwrap(),
        native_ref: None,
    }
}

/// The member ring that names every curve of `entities` in one direction.
fn ring(entities: &[SketchEntity], reversed: bool) -> Vec<(&SketchEntity, bool)> {
    entities.iter().map(|entity| (entity, reversed)).collect()
}

fn member_regions(
    sketch: &Sketch,
    entities: &[SketchEntity],
    rings: &[Vec<(&SketchEntity, bool)>],
) -> Option<Vec<SketchProfileRegion>> {
    let rings = rings
        .iter()
        .map(|ring| {
            ring.iter()
                .map(|(entity, reversed)| SketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: *reversed,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_regions_matching_member_rings(
            sketch,
            entities,
            &rings,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
}

/// The exterior and hole rings of the one region that `rings` select.
fn single_region(
    sketch: &Sketch,
    entities: &[SketchEntity],
    rings: &[Vec<(&SketchEntity, bool)>],
) -> (
    Vec<cadmpeg_ir::features::SketchProfileBoundaryUse>,
    Vec<Vec<cadmpeg_ir::features::SketchProfileBoundaryUse>>,
) {
    let regions = member_regions(sketch, entities, rings).expect("one region");
    let [SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }] = regions.as_slice()
    else {
        panic!("one trimmed region: {regions:?}");
    };
    (
        outer_boundary.iter().cloned().collect(),
        hole_boundaries
            .iter()
            .map(|ring| ring.iter().cloned().collect())
            .collect(),
    )
}

fn ring_entities(ring: &[cadmpeg_ir::features::SketchProfileBoundaryUse]) -> Vec<SketchEntityId> {
    let mut entities = ring
        .iter()
        .map(|use_| use_.entity.clone())
        .collect::<Vec<_>>();
    entities.sort_unstable();
    entities
}

fn sorted_ids(entities: &[&SketchEntity]) -> Vec<SketchEntityId> {
    let mut ids = entities
        .iter()
        .map(|entity| entity.id().clone())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

#[test]
fn counted_member_directions_select_one_of_two_half_disks_with_the_same_curves() {
    let (sketch, entities) = divided_circle(false);
    let [circle, line] = [&entities[0], &entities[1]];
    let (left, _) = single_region(&sketch, &entities, &[vec![(line, false), (circle, false)]]);
    assert!(left
        .iter()
        .any(|use_| use_.entity == *line.id() && !use_.reversed));
    let (right, _) = single_region(&sketch, &entities, &[vec![(line, true), (circle, false)]]);
    assert!(right
        .iter()
        .any(|use_| use_.entity == *line.id() && use_.reversed));
    assert_eq!(
        member_regions(&sketch, &entities, &[vec![(line, false), (circle, true)]]),
        None
    );
}

#[test]
fn counted_members_select_a_t_junction_region_with_a_curve_outside_the_profile_loops() {
    // A 40 x 30 rectangle with one profile loop, and a diagonal from its
    // lower-left corner that ends on the top side at (20, 30).
    let id = SketchId::mint("synthetic:test:id#t-junction").unwrap();
    let mut entities = rectangle(&id, "side", (0.0, 0.0), (40.0, 30.0));
    let sketch = sketch_with_loops(id.clone(), &[&entities]);
    entities.push(line_entity(&id, "diagonal", (0.0, 0.0), (20.0, 30.0)));
    let [bottom, right, top, left, diagonal] = [0, 1, 2, 3, 4].map(|index| &entities[index]);

    let (outer, holes) = single_region(
        &sketch,
        &entities,
        &[vec![
            (right, false),
            (bottom, false),
            (top, false),
            (diagonal, true),
        ]],
    );
    assert!(holes.is_empty());
    assert_eq!(
        ring_entities(&outer),
        sorted_ids(&[bottom, right, top, diagonal])
    );
    let top_use = outer.iter().find(|use_| use_.entity == *top.id()).unwrap();
    let [start, end] = top_use.parameter_range.endpoints();
    assert!(
        start.min(end).abs() < 1e-9 && (start.max(end) - 0.5).abs() < 1e-9,
        "{start} {end}"
    );

    let (triangle, holes) = single_region(
        &sketch,
        &entities,
        &[vec![(left, false), (top, false), (diagonal, false)]],
    );
    assert!(holes.is_empty());
    assert_eq!(triangle.len(), 3);

    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[vec![
                (right, false),
                (bottom, false),
                (top, false),
                (diagonal, false),
            ]],
        ),
        None
    );
}

#[test]
fn a_counted_hole_ring_names_the_island_of_the_selected_face() {
    // An 80 x 60 rectangle, a 20 x 20 island, and a bridge line from the
    // outer corner (0, 0) to the island corner (30, 20).
    let id = SketchId::mint("synthetic:test:id#bridge").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (80.0, 60.0));
    let island = rectangle(&id, "island", (30.0, 20.0), (50.0, 40.0));
    let sketch = sketch_with_loops(id.clone(), &[&island]);
    let mut entities = outer.clone();
    entities.extend(island.iter().cloned());
    entities.push(line_entity(&id, "bridge", (0.0, 0.0), (30.0, 20.0)));

    let (exterior, holes) = single_region(
        &sketch,
        &entities,
        &[ring(&outer, false), ring(&island, true)],
    );
    assert_eq!(
        ring_entities(&exterior),
        sorted_ids(&outer.iter().collect::<Vec<_>>())
    );
    assert!(exterior.iter().all(|use_| !use_.reversed));
    let [hole] = holes.as_slice() else {
        panic!("one hole ring: {holes:?}");
    };
    assert_eq!(
        ring_entities(hole),
        sorted_ids(&island.iter().collect::<Vec<_>>())
    );
    assert!(hole.iter().all(|use_| use_.reversed), "{hole:?}");

    let (interior, holes) = single_region(&sketch, &entities, &[ring(&island, false)]);
    assert!(holes.is_empty());
    assert!(interior.iter().all(|use_| !use_.reversed));

    assert_eq!(
        member_regions(&sketch, &entities, &[ring(&outer, false)]),
        None
    );
    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[ring(&outer, false), ring(&island, false)]
        ),
        None
    );
    assert_eq!(
        member_regions(&sketch, &entities, &[ring(&island, true)]),
        None
    );

    let program = outer
        .iter()
        .map(|entity| entity.id().clone())
        .collect::<Vec<_>>();
    let ordered = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_region_matching_curve_program(
            &sketch,
            &entities,
            &program,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap();
    assert_eq!(ordered, None);
}

#[test]
fn a_counted_hole_ring_names_only_an_island_that_no_smaller_face_holds() {
    let id = SketchId::mint("synthetic:test:id#nested-islands").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (100.0, 100.0));
    let middle = rectangle(&id, "middle", (20.0, 20.0), (80.0, 80.0));
    let inner = rectangle(&id, "inner", (40.0, 40.0), (60.0, 60.0));
    let sketch = sketch_with_loops(id, &[&outer, &middle, &inner]);
    let entities = [outer.clone(), middle.clone(), inner.clone()].concat();

    for (exterior, island) in [(&outer, &middle), (&middle, &inner)] {
        let (_, holes) = single_region(
            &sketch,
            &entities,
            &[ring(exterior, false), ring(island, true)],
        );
        let [hole] = holes.as_slice() else {
            panic!("one hole ring: {holes:?}");
        };
        assert_eq!(
            ring_entities(hole),
            sorted_ids(&island.iter().collect::<Vec<_>>())
        );
    }
    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[ring(&outer, false), ring(&inner, true)]
        ),
        None
    );
    let (_, holes) = single_region(&sketch, &entities, &[ring(&inner, false)]);
    assert!(holes.is_empty());
    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[ring(&inner, false), ring(&inner, false)]
        ),
        None
    );

    let regions = member_regions(
        &sketch,
        &entities,
        &[
            ring(&inner, false),
            ring(&outer, false),
            ring(&middle, true),
        ],
    )
    .expect("two regions");
    let [SketchProfileRegion::Trimmed {
        outer_boundary: innermost,
        hole_boundaries: innermost_holes,
    }, SketchProfileRegion::Trimmed {
        outer_boundary: exterior,
        hole_boundaries,
    }] = regions.as_slice()
    else {
        panic!("two trimmed regions: {regions:?}");
    };
    assert_eq!(
        ring_entities(innermost),
        sorted_ids(&inner.iter().collect::<Vec<_>>())
    );
    assert!(innermost_holes.is_empty());
    assert_eq!(
        ring_entities(exterior),
        sorted_ids(&outer.iter().collect::<Vec<_>>())
    );
    assert_eq!(hole_boundaries.len(), 1);
}

#[test]
fn counted_duplicate_members_select_the_region_around_a_circle_island() {
    // A 40 x 30 rectangle drawn twice and a circle inside it. The exterior
    // ring names both copies of each side.
    let id = SketchId::mint("synthetic:test:id#covered").unwrap();
    let first = rectangle(&id, "first", (0.0, 0.0), (40.0, 30.0));
    let second = rectangle(&id, "second", (0.0, 0.0), (40.0, 30.0));
    let circle = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#circle").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(20.0, 15.0),
            radius: Length::new(4.0).unwrap(),
        })
        .unwrap(),
    );
    let sketch = sketch_with_loops(id, &[&first, std::slice::from_ref(&circle)]);
    let mut entities = first.clone();
    entities.extend(second.iter().cloned());
    entities.push(circle.clone());
    let sides = [ring(&first, false), ring(&second, false)].concat();

    let (exterior, holes) =
        single_region(&sketch, &entities, &[sides.clone(), vec![(&circle, true)]]);
    assert_eq!(exterior.len(), 4);
    let [hole] = holes.as_slice() else {
        panic!("one hole ring: {holes:?}");
    };
    assert!(hole.iter().all(|use_| use_.entity == *circle.id()));

    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[ring(&first, false), vec![(&circle, true)]]
        ),
        None
    );
    let renamed = [
        ring(&first, false),
        ring(&second[..3], false),
        vec![(&circle, false)],
    ]
    .concat();
    assert_eq!(
        member_regions(&sketch, &entities, &[renamed, vec![(&circle, true)]]),
        None
    );
}

#[test]
fn a_coincident_edge_matches_when_one_of_its_curves_has_its_direction() {
    // A 10 x 5 rectangle drawn against its counterclockwise walk, whose
    // bottom side also lies on an earlier, longer line drawn the other way.
    // The longer line supplies the bottom edge.
    let id = SketchId::mint("synthetic:test:id#coincident-bottom").unwrap();
    let entities = vec![
        line_entity(&id, "top", (0.0, 5.0), (10.0, 5.0)),
        line_entity(&id, "right", (10.0, 5.0), (10.0, 0.0)),
        line_entity(&id, "left", (0.0, 0.0), (0.0, 5.0)),
        line_entity(&id, "long", (-5.0, 0.0), (15.0, 0.0)),
        line_entity(&id, "bottom", (10.0, 0.0), (0.0, 0.0)),
    ];
    let [top, right, left, long, bottom] = [0, 1, 2, 3, 4].map(|index| &entities[index]);
    let sketch = sketch_with_loops(
        id,
        &[&[top.clone(), right.clone(), left.clone(), bottom.clone()]],
    );
    let ring = |bottom_reversed: bool, long_reversed: bool| {
        vec![
            (top, true),
            (right, true),
            (left, true),
            (bottom, bottom_reversed),
            (long, long_reversed),
        ]
    };

    for (bottom_reversed, long_reversed) in [(true, true), (true, false), (false, false)] {
        let (exterior, holes) =
            single_region(&sketch, &entities, &[ring(bottom_reversed, long_reversed)]);
        assert!(holes.is_empty());
        assert_eq!(exterior.len(), 4);
    }
    assert_eq!(
        member_regions(&sketch, &entities, &[ring(false, true)]),
        None
    );
}

#[test]
fn a_full_ellipse_leaves_counted_members_without_a_region() {
    let id = SketchId::mint("synthetic:test:id#ellipse-island").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (40.0, 30.0));
    let sketch = sketch_with_loops(id.clone(), &[&outer]);
    let mut entities = outer.clone();
    entities.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#ellipse").unwrap(),
        id,
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(20.0, 15.0),
            major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(6.0).unwrap(),
                minor_radius: Length::new(3.0).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    ));
    assert_eq!(
        member_regions(&sketch, &entities, &[ring(&outer, false)]),
        None
    );
}

#[test]
fn counted_hole_rings_follow_list_order_and_name_each_island_once() {
    let id = SketchId::mint("synthetic:test:id#two-islands").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (100.0, 50.0));
    let first = rectangle(&id, "first", (10.0, 10.0), (30.0, 30.0));
    let second = rectangle(&id, "second", (60.0, 10.0), (80.0, 30.0));
    let sketch = sketch_with_loops(id, &[&outer, &first, &second]);
    let entities = [outer.clone(), first.clone(), second.clone()].concat();
    let first_ids = sorted_ids(&first.iter().collect::<Vec<_>>());
    let second_ids = sorted_ids(&second.iter().collect::<Vec<_>>());

    for (rings, expected) in [
        (
            [ring(&outer, false), ring(&first, true), ring(&second, true)],
            [&first_ids, &second_ids],
        ),
        (
            [ring(&outer, false), ring(&second, true), ring(&first, true)],
            [&second_ids, &first_ids],
        ),
    ] {
        let (_, holes) = single_region(&sketch, &entities, &rings);
        assert_eq!(
            holes
                .iter()
                .map(|hole| ring_entities(hole))
                .collect::<Vec<_>>(),
            expected.map(Clone::clone)
        );
    }
    for rings in [
        vec![ring(&outer, false), ring(&first, true), ring(&first, true)],
        vec![ring(&outer, false), ring(&outer, false), ring(&first, true)],
        vec![ring(&outer, false), ring(&first, true)],
    ] {
        assert_eq!(member_regions(&sketch, &entities, &rings), None);
    }
}

#[test]
fn a_counted_ring_that_two_walks_match_selects_nothing() {
    // A triangle drawn twice in opposite directions inside a rectangle. With
    // every member forward, the triangle face and the outer walk of the
    // island each traverse one curve of every edge forward.
    let id = SketchId::mint("synthetic:test:id#doubled-triangle").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (40.0, 30.0));
    let corners = [(10.0, 10.0), (30.0, 10.0), (20.0, 20.0)];
    let forward = (0..3)
        .map(|index| {
            line_entity(
                &id,
                &format!("forward-{index}"),
                corners[index],
                corners[(index + 1) % 3],
            )
        })
        .collect::<Vec<_>>();
    let backward = (0..3)
        .map(|index| {
            line_entity(
                &id,
                &format!("backward-{index}"),
                corners[(index + 1) % 3],
                corners[index],
            )
        })
        .collect::<Vec<_>>();
    let sketch = sketch_with_loops(id, &[&outer, &forward]);
    let entities = [outer.clone(), forward.clone(), backward.clone()].concat();
    let both = |forward_reversed: bool, backward_reversed: bool| {
        [
            ring(&forward, forward_reversed),
            ring(&backward, backward_reversed),
        ]
        .concat()
    };
    assert_eq!(
        member_regions(&sketch, &entities, &[both(false, false)]),
        None
    );
    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[ring(&outer, false), both(false, false)]
        ),
        None
    );

    let (_, holes) = single_region(&sketch, &entities, &[both(false, true)]);
    assert!(holes.is_empty());
    let (_, holes) = single_region(
        &sketch,
        &entities,
        &[ring(&outer, false), both(true, false)],
    );
    assert_eq!(holes.len(), 1);
}

#[test]
fn each_counted_member_states_the_direction_of_one_fragment() {
    // A line that ends on the bottom side at (5, 0) cuts it, so the walk of
    // the rectangle runs along two forward fragments of the bottom side.
    let id = SketchId::mint("synthetic:test:id#cut-bottom").unwrap();
    let mut entities = rectangle(&id, "side", (0.0, 0.0), (10.0, 5.0));
    let sketch = sketch_with_loops(id.clone(), &[&entities]);
    entities.push(line_entity(&id, "spur", (5.0, 0.0), (5.0, -3.0)));
    let [bottom, right, top, left] = [0, 1, 2, 3].map(|index| &entities[index]);
    let (exterior, _) = single_region(
        &sketch,
        &entities,
        &[vec![
            (bottom, false),
            (bottom, false),
            (right, false),
            (top, false),
            (left, false),
        ]],
    );
    assert_eq!(exterior.len(), 5);
    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[vec![
                (bottom, false),
                (bottom, true),
                (right, false),
                (top, false),
                (left, false),
            ]],
        ),
        None
    );
}

#[test]
fn coincident_fragments_take_distinct_members_in_any_member_order() {
    // The bottom line runs under two shorter lines drawn the other way, so
    // each of its two fragments can take the bottom member or the member of
    // its own shorter line.
    let id = SketchId::mint("synthetic:test:id#two-shared-fragments").unwrap();
    let entities = vec![
        line_entity(&id, "bottom", (0.0, 0.0), (20.0, 0.0)),
        line_entity(&id, "right", (20.0, 0.0), (20.0, 5.0)),
        line_entity(&id, "top", (20.0, 5.0), (0.0, 5.0)),
        line_entity(&id, "left", (0.0, 5.0), (0.0, 0.0)),
        line_entity(&id, "first-half", (10.0, 0.0), (0.0, 0.0)),
        line_entity(&id, "second-half", (20.0, 0.0), (10.0, 0.0)),
    ];
    let [bottom, right, top, left, first_half, second_half] =
        [0, 1, 2, 3, 4, 5].map(|index| &entities[index]);
    let sketch = sketch_with_loops(id, &[&entities[..4]]);
    let sides = [(right, false), (top, false), (left, false)];
    for (first, second) in [
        ((first_half, true), (second_half, false)),
        ((first_half, false), (second_half, true)),
    ] {
        for members in [
            [(bottom, false), (bottom, true), first, second],
            [first, second, (bottom, true), (bottom, false)],
        ] {
            let ring = [members.as_slice(), sides.as_slice()].concat();
            let (exterior, holes) = single_region(&sketch, &entities, &[ring]);
            assert!(holes.is_empty());
            assert_eq!(exterior.len(), 5);
        }
    }
    let ring = [
        [
            (bottom, true),
            (bottom, true),
            (first_half, false),
            (second_half, false),
        ]
        .as_slice(),
        sides.as_slice(),
    ]
    .concat();
    assert_eq!(member_regions(&sketch, &entities, &[ring]), None);
    let ring = [
        [
            (bottom, false),
            (bottom, true),
            (first_half, false),
            (second_half, false),
        ]
        .as_slice(),
        sides.as_slice(),
    ]
    .concat();
    assert_eq!(member_regions(&sketch, &entities, &[ring]), None);
}

#[test]
fn a_curve_program_resolves_in_the_arrangement_of_the_profile_loops() {
    // The diagonal ends on the top side and lies in no profile loop, so the
    // profile-loop arrangement has one face: the whole rectangle.
    let id = SketchId::mint("synthetic:test:id#t-junction-program").unwrap();
    let mut entities = rectangle(&id, "side", (0.0, 0.0), (40.0, 30.0));
    let sketch = sketch_with_loops(id.clone(), &[&entities]);
    entities.push(line_entity(&id, "diagonal", (0.0, 0.0), (20.0, 30.0)));
    let program = |indices: &[usize]| {
        let program = indices
            .iter()
            .map(|index| entities[*index].id().clone())
            .collect::<Vec<_>>();
        crate::test_support::with_decode_context(|ctx| {
            super::super::arrangement_region_matching_curve_program(
                &sketch,
                &entities,
                &program,
                PROFILE_PROGRAM_TOLERANCE,
                &ProfileGeometryWork::new(),
                ctx,
            )
        })
        .unwrap()
    };
    assert_eq!(program(&[1, 2, 4, 0]), None);
    let Some(SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }) = program(&[0, 1, 2, 3])
    else {
        panic!("the whole rectangle");
    };
    assert!(hole_boundaries.is_empty());
    assert_eq!(outer_boundary.len(), 5);
}

#[test]
fn a_single_curve_fragment_keeps_the_member_that_only_it_can_take() {
    // The bottom line has a single-curve fragment from (0, 0) to (5, 0) and a
    // fragment that a shorter line drawn the other way also covers.
    let id = SketchId::mint("synthetic:test:id#forced-fragment").unwrap();
    let entities = vec![
        line_entity(&id, "bottom", (0.0, 0.0), (10.0, 0.0)),
        line_entity(&id, "right", (10.0, 0.0), (10.0, 5.0)),
        line_entity(&id, "top", (10.0, 5.0), (0.0, 5.0)),
        line_entity(&id, "left", (0.0, 5.0), (0.0, 0.0)),
        line_entity(&id, "half", (10.0, 0.0), (5.0, 0.0)),
    ];
    let [bottom, right, top, left, half] = [0, 1, 2, 3, 4].map(|index| &entities[index]);
    let sketch = sketch_with_loops(id, &[&entities[..4]]);
    let ring = |half_reversed: bool| {
        vec![
            (bottom, false),
            (bottom, true),
            (half, half_reversed),
            (right, false),
            (top, false),
            (left, false),
        ]
    };
    let (exterior, holes) = single_region(&sketch, &entities, &[ring(true)]);
    assert!(holes.is_empty());
    assert_eq!(exterior.len(), 5);
    assert_eq!(member_regions(&sketch, &entities, &[ring(false)]), None);
}
