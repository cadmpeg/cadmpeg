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

#[test]
fn counted_partial_curve_members_refuse_two_regions_with_the_same_multiset() {
    let (sketch, entities) = divided_circle(false);
    let members = [entities[1].id().clone(), entities[0].id().clone()];
    let region = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_region_matching_curve_members(
            &sketch,
            &entities,
            &members,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap();
    assert_eq!(region, None);
}

fn plain_sketch(id: SketchId) -> Sketch {
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
    }
}

fn test_line(sketch: &SketchId, name: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
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

fn x_braced_window() -> (Sketch, Vec<SketchEntity>) {
    let id = SketchId::mint("synthetic:test:id#x-window").unwrap();
    let entities = vec![
        test_line(&id, "bottom", (0.0, 0.0), (10.0, 0.0)),
        test_line(&id, "right", (10.0, 0.0), (10.0, 10.0)),
        test_line(&id, "top", (10.0, 10.0), (0.0, 10.0)),
        test_line(&id, "left", (0.0, 10.0), (0.0, 0.0)),
        test_line(&id, "rise", (0.0, 0.0), (10.0, 10.0)),
        test_line(&id, "fall", (10.0, 0.0), (0.0, 10.0)),
    ];
    (plain_sketch(id), entities)
}

fn curve_ids(entities: &[SketchEntity], names: &[&str]) -> Vec<SketchEntityId> {
    names
        .iter()
        .map(|name| {
            entities
                .iter()
                .find(|entity| entity.id().as_str() == format!("synthetic:test:id#{name}"))
                .unwrap()
                .id()
                .clone()
        })
        .collect()
}

fn crossing_split(
    sketch: &Sketch,
    entities: &[SketchEntity],
    program: &[SketchEntityId],
) -> Option<Vec<SketchProfileRegion>> {
    crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_regions_of_crossing_loop(
            sketch,
            entities,
            program,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
}

#[test]
fn self_crossing_loop_splits_into_the_four_triangles_of_an_x_braced_window() {
    let (sketch, entities) = x_braced_window();
    // One walk over the four triangles: each side once, each brace piece
    // once per adjoining triangle.
    let program = curve_ids(
        &entities,
        &[
            "bottom", "fall", "rise", "right", "rise", "fall", "top", "fall", "rise", "left",
            "rise", "fall",
        ],
    );
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
    assert_eq!(ordered, None, "no single face has the crossing loop");
    let regions = crossing_split(&sketch, &entities, &program).expect("four triangles");
    assert_eq!(regions.len(), 4);
    let atomic = crate::test_support::with_decode_context(|ctx| {
        super::super::atomic_sketch_regions(
            &sketch,
            &entities,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .unwrap();
    for region in &regions {
        let SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            panic!("trimmed region");
        };
        assert!(hole_boundaries.is_empty());
        assert_eq!(outer_boundary.len(), 3);
        let face = atomic
            .iter()
            .find(|face| face.boundary[..] == outer_boundary[..])
            .expect("the boundary of an atomic region");
        let area = cadmpeg_ir::math::planar::polygon_area_twice(&face.polyline)
            .unwrap()
            .get()
            .abs()
            / 2.0;
        assert!((area - 25.0).abs() < 1e-9, "{area}");
    }
    for index in 0..regions.len() {
        assert!(!regions[..index].contains(&regions[index]));
    }
}

#[test]
fn self_crossing_loop_selects_only_the_triangles_its_curves_count_out() {
    let (sketch, entities) = x_braced_window();
    let program = curve_ids(
        &entities,
        &["bottom", "fall", "rise", "top", "fall", "rise"],
    );
    let regions = crossing_split(&sketch, &entities, &program).expect("two opposite triangles");
    assert_eq!(regions.len(), 2);
}

#[test]
fn self_crossing_loop_skips_an_island_face_and_keeps_the_candidates_it_weighed() {
    // A 2 by 2 grid. The loop lists the curves of the lower-left and
    // upper-right quarters; the upper-left quarter uses the same curves but
    // encloses an island square, so it is no candidate. Without the island
    // the upper-left and lower-right quarters would tie with the selection.
    let id = SketchId::mint("synthetic:test:id#grid-island").unwrap();
    let entities = vec![
        test_line(&id, "bottom", (0.0, 0.0), (10.0, 0.0)),
        test_line(&id, "right", (10.0, 0.0), (10.0, 10.0)),
        test_line(&id, "top", (10.0, 10.0), (0.0, 10.0)),
        test_line(&id, "left", (0.0, 10.0), (0.0, 0.0)),
        test_line(&id, "mid", (0.0, 5.0), (10.0, 5.0)),
        test_line(&id, "split", (5.0, 0.0), (5.0, 10.0)),
        test_line(&id, "island-bottom", (1.0, 6.0), (2.0, 6.0)),
        test_line(&id, "island-right", (2.0, 6.0), (2.0, 7.0)),
        test_line(&id, "island-top", (2.0, 7.0), (1.0, 7.0)),
        test_line(&id, "island-left", (1.0, 7.0), (1.0, 6.0)),
    ];
    let sketch = plain_sketch(id);
    let program = curve_ids(
        &entities,
        &[
            "bottom", "split", "mid", "left", "top", "split", "mid", "right",
        ],
    );
    let regions = crossing_split(&sketch, &entities, &program).expect("two diagonal quarters");
    assert_eq!(regions.len(), 2);
    let names = |region: &SketchProfileRegion| {
        let SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            panic!("a trimmed region");
        };
        assert!(hole_boundaries.is_empty());
        let mut names = outer_boundary
            .iter()
            .map(|use_| {
                use_.entity
                    .as_str()
                    .trim_start_matches("synthetic:test:id#")
                    .to_owned()
            })
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    };
    let mut quarters = regions.iter().map(names).collect::<Vec<_>>();
    quarters.sort();
    assert_eq!(
        quarters,
        [
            vec!["bottom", "left", "mid", "split"],
            vec!["mid", "right", "split", "top"],
        ]
    );
}

#[test]
fn self_crossing_loop_refuses_counts_no_set_of_regions_reaches() {
    let (sketch, entities) = x_braced_window();
    let program = curve_ids(&entities, &["bottom", "fall", "rise", "rise", "fall"]);
    assert_eq!(crossing_split(&sketch, &entities, &program), None);
}

#[test]
fn self_crossing_loop_refuses_two_sets_with_the_same_counts() {
    // A disk of two semicircles cut by a horizontal and a vertical chord: the
    // upper-right and upper-left quarters use the same curves.
    let id = SketchId::mint("synthetic:test:id#quartered").unwrap();
    let arc = |name: &str, start: f64, end: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(5.0).unwrap(),
                start_angle: Angle::new(start).unwrap(),
                end_angle: Angle::new(end).unwrap(),
            })
            .unwrap(),
        )
    };
    let entities = vec![
        arc("upper", 0.0, std::f64::consts::PI),
        arc("lower", std::f64::consts::PI, std::f64::consts::TAU),
        test_line(&id, "across", (-5.0, 0.0), (5.0, 0.0)),
        test_line(&id, "down", (0.0, -5.0), (0.0, 5.0)),
    ];
    let sketch = plain_sketch(id);
    let program = curve_ids(&entities, &["across", "upper", "down"]);
    assert_eq!(crossing_split(&sketch, &entities, &program), None);
}

#[test]
fn self_crossing_loop_refuses_a_carrier_that_is_not_a_line_or_arc() {
    let (sketch, mut entities) = divided_circle(true);
    let program = [
        entities[0].id().clone(),
        entities[1].id().clone(),
        entities[0].id().clone(),
        entities[2].id().clone(),
    ];
    assert_eq!(crossing_split(&sketch, &entities, &program), None);
    entities.truncate(1);
    assert_eq!(crossing_split(&sketch, &entities, &program), None);
}

#[test]
fn self_crossing_loop_over_the_candidate_limit_reports_the_cap() {
    // A rectangle cut by 16 verticals has 17 atomic regions, all in the loop.
    let id = SketchId::mint("synthetic:test:id#comb").unwrap();
    let mut entities = vec![
        test_line(&id, "bottom", (0.0, 0.0), (17.0, 0.0)),
        test_line(&id, "right", (17.0, 0.0), (17.0, 1.0)),
        test_line(&id, "top", (17.0, 1.0), (0.0, 1.0)),
        test_line(&id, "left", (0.0, 1.0), (0.0, 0.0)),
    ];
    for x in 1..=16 {
        entities.push(test_line(
            &id,
            &format!("cut{x}"),
            (f64::from(x), 0.0),
            (f64::from(x), 1.0),
        ));
    }
    let sketch = plain_sketch(id);
    let mut names = vec!["bottom".to_owned(), "right".to_owned(), "top".to_owned()];
    names.push("left".to_owned());
    names.extend((1..=16).map(|x| format!("cut{x}")));
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let program = curve_ids(&entities, &names);
    let work = ProfileGeometryWork::new();
    let split = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_regions_of_crossing_loop(
            &sketch,
            &entities,
            &program,
            PROFILE_PROGRAM_TOLERANCE,
            &work,
            ctx,
        )
    })
    .unwrap();
    assert_eq!(split, None);
    work.finish_selection();
    let losses = work.losses();
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeatureProfileCrossingLoopCapped.kind()
    );
    assert!(loss.message.contains("MAX_CROSSING_LOOP_CANDIDATES (16)"));
}
