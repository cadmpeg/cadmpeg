// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::features::SketchProfileRegion;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

const LOOP_UNION_TOLERANCE: f64 = 1.0e-7;

fn divided_rectangle() -> (Sketch, Vec<SketchEntity>, Vec<SketchEntityUse>) {
    let id = SketchId::mint("synthetic:test:id#loop-union").unwrap();
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(4.0, 0.0),
        Point2::new(4.0, 3.0),
        Point2::new(0.0, 3.0),
    ];
    let mut entities = (0..4)
        .map(|i| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#side-{i}")).unwrap(),
                id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: points[i],
                    end: points[(i + 1) % 4],
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let members = entities
        .iter()
        .map(|entity| SketchEntityUse {
            entity: entity.id().clone(),
            reversed: false,
        })
        .collect();
    entities.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#divider").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(2.0, 0.0),
            end: Point2::new(2.0, 3.0),
        })
        .unwrap(),
    ));
    (
        Sketch {
            id,
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved {},
            profiles: SketchProfiles::default(),
            native_ref: None,
        },
        entities,
        members,
    )
}

fn selected(
    sketch: &Sketch,
    entities: &[SketchEntity],
    members: &[SketchEntityUse],
) -> Option<Vec<SketchProfileRegion>> {
    crate::test_support::with_decode_context(|ctx| {
        resolve(
            sketch,
            entities,
            &members
                .iter()
                .map(|use_| (use_.clone(), [1, 1]))
                .collect::<Vec<_>>(),
            LOOP_UNION_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
}

#[test]
fn complete_exterior_selects_both_sides_of_an_unselected_divider() {
    let (sketch, entities, members) = divided_rectangle();
    let regions = selected(&sketch, &entities, &members).unwrap();
    assert_eq!(regions.len(), 2);
    let mut directions = Vec::new();
    let mut bottom_intervals = Vec::new();
    for region in regions {
        let SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            panic!("trimmed subregion");
        };
        assert!(hole_boundaries.is_empty());
        assert_eq!(outer_boundary.len(), 4);
        for use_ in &outer_boundary {
            if use_.entity == *entities[4].id() {
                directions.push(use_.reversed);
                assert_eq!(use_.parameter_range.endpoints(), [0.0, 1.0]);
            }
            if use_.entity == *entities[0].id() {
                bottom_intervals.push(use_.parameter_range.endpoints());
            }
        }
    }
    directions.sort_unstable();
    assert_eq!(directions, [false, true]);
    bottom_intervals.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(bottom_intervals, [[0.0, 0.5], [0.5, 1.0]]);
}

#[test]
fn an_omitted_closing_member_preserves_the_native_selection() {
    let (sketch, entities, members) = divided_rectangle();
    assert!(selected(&sketch, &entities, &members[..3]).is_none());
}

#[test]
fn repeated_member_identity_requires_the_encoded_fragment_multiplicity() {
    let (sketch, entities, mut members) = divided_rectangle();
    members.push(members[0].clone());
    assert!(selected(&sketch, &entities, &members).is_none());
}

#[test]
fn a_fragment_count_that_disagrees_with_the_member_graph_stays_native() {
    let (sketch, entities, members) = divided_rectangle();
    let mut program = members
        .into_iter()
        .map(|use_| (use_, [1, 1]))
        .collect::<Vec<_>>();
    program[0].1 = [2, 2];
    let result = crate::test_support::with_decode_context(|ctx| {
        resolve(
            &sketch,
            &entities,
            &program,
            LOOP_UNION_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap();
    assert!(result.is_none());
}

#[test]
fn coincident_fragment_ordinals_preserve_the_whole_triangle_across_a_divider() {
    let (sketch, _, _) = divided_rectangle();
    let line = |name: &str, start, end| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let entities = vec![
        line("vertical", Point2::new(4.0, 0.0), Point2::new(4.0, 2.0)),
        line("diagonal", Point2::new(1.0, 0.0), Point2::new(4.0, 2.0)),
        line("base", Point2::new(4.0, 0.0), Point2::new(1.0, 0.0)),
        line("short", Point2::new(4.0, 0.0), Point2::new(3.0, 0.0)),
        line("long", Point2::new(0.0, 0.0), Point2::new(3.0, 0.0)),
        line("divider", Point2::new(3.0, 0.0), Point2::new(3.0, 2.0)),
    ];
    let program = [
        (0, false, [1, 1]),
        (1, true, [1, 1]),
        (2, true, [1, 2]),
        (2, true, [2, 2]),
        (3, true, [1, 1]),
        (4, true, [2, 2]),
    ]
    .map(|(index, reversed, incidence)| {
        (
            SketchEntityUse {
                entity: entities[index].id().clone(),
                reversed,
            },
            incidence,
        )
    });
    let regions = crate::test_support::with_decode_context(|ctx| {
        resolve(
            &sketch,
            &entities,
            &program,
            LOOP_UNION_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .unwrap();
    assert_eq!(regions.len(), 2);
    let mut intervals = Vec::new();
    for region in regions {
        let SketchProfileRegion::Trimmed { outer_boundary, .. } = region else {
            panic!("trimmed triangle subdivision");
        };
        intervals.extend(
            outer_boundary
                .iter()
                .filter(|use_| use_.entity == *entities[1].id())
                .map(|use_| use_.parameter_range.endpoints()),
        );
    }
    intervals.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(intervals.len(), 2);
    for (actual, expected) in intervals.iter().zip([[0.0, 2.0 / 3.0], [2.0 / 3.0, 1.0]]) {
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < LOOP_UNION_TOLERANCE);
        }
    }
}
