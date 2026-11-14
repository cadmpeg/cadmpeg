// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn coincident_members_have_one_identity_match_with_conflicting_directions() {
    let (sketch, _) = divided_circle(false);
    let line = |name: &str, start, end| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let entities = vec![
        line("bottom", Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
        line("right", Point2::new(4.0, 0.0), Point2::new(4.0, 3.0)),
        line("top", Point2::new(4.0, 3.0), Point2::new(0.0, 3.0)),
        line("left", Point2::new(0.0, 3.0), Point2::new(0.0, 0.0)),
        line("overlap", Point2::new(2.0, 0.0), Point2::new(0.0, 0.0)),
    ];
    let selected = member_selection(
        &sketch,
        &entities,
        &[vec![
            (&entities[0], false),
            (&entities[0], false),
            (&entities[1], false),
            (&entities[2], false),
            (&entities[3], false),
            (&entities[4], false),
        ]],
    )
    .unwrap();
    assert!(selected.coincident);
    assert!(selected.identity_unique);
    assert_eq!(selected.regions.len(), 1);
}

#[test]
fn two_half_discs_share_identities_and_need_the_traversal_direction() {
    let (sketch, entities) = divided_circle(false);
    let selected = member_selection(
        &sketch,
        &entities,
        &[vec![(&entities[0], false), (&entities[1], false)]],
    )
    .unwrap();
    assert!(!selected.identity_unique);
    let [SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }] = selected.regions.as_slice()
    else {
        panic!("trimmed half disc");
    };
    assert!(hole_boundaries.is_empty());
    let divider = outer_boundary
        .iter()
        .find(|u| u.entity == *entities[1].id())
        .unwrap();
    assert!(
        (divider.parameter_range.endpoints()[0] - 1.0 / 12.0).abs() < PROFILE_PROGRAM_TOLERANCE
    );
    assert!(
        (divider.parameter_range.endpoints()[1] - 11.0 / 12.0).abs() < PROFILE_PROGRAM_TOLERANCE
    );
    let arc = outer_boundary
        .iter()
        .find(|u| u.entity == *entities[0].id())
        .unwrap();
    assert!(
        (arc.parameter_range.endpoints()[1]
            - arc.parameter_range.endpoints()[0]
            - std::f64::consts::PI)
            .abs()
            < PROFILE_PROGRAM_TOLERANCE
    );
}

#[test]
fn a_member_ring_that_omits_a_closing_curve_stays_unresolved() {
    let (sketch, _) = divided_circle(false);
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(4.0, 0.0),
        Point2::new(4.0, 3.0),
        Point2::new(0.0, 3.0),
    ];
    let entities = (0..4)
        .map(|i| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#side-{i}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: points[i],
                    end: points[(i + 1) % 4],
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert!(member_selection(
        &sketch,
        &entities,
        &[vec![
            (&entities[0], false),
            (&entities[1], false),
            (&entities[2], false),
        ]],
    )
    .is_none());
}

#[test]
fn an_unselected_divider_keeps_a_whole_boundary_unresolved() {
    let (sketch, _) = divided_circle(false);
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
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: points[i],
                    end: points[(i + 1) % 4],
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    entities.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#divider").unwrap(),
        sketch.id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(2.0, 0.0),
            end: Point2::new(2.0, 3.0),
        })
        .unwrap(),
    ));
    let members = entities[..4].iter().map(|entity| (entity, false)).collect();
    assert!(member_selection(&sketch, &entities, &[members]).is_none());
}
