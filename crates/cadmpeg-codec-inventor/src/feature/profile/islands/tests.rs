// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchPlacement,
    SketchProfiles,
};

use super::*;

fn sketch_id() -> cadmpeg_ir::sketches::SketchId {
    cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#islands").expect("sketch id")
}

fn entity(ordinal: u32, definition: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("test:model:sketch-entity#{ordinal}")).expect("entity id"),
        sketch_id(),
        SketchGeometry::try_from(definition).expect("geometry"),
    )
}

fn line(ordinal: u32, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    entity(
        ordinal,
        SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        },
    )
}

fn circle(ordinal: u32, center: (f64, f64), radius: f64) -> SketchEntity {
    entity(
        ordinal,
        SketchGeometryDefinition::Circle {
            center: Point2::new(center.0, center.1),
            radius: Length::new(radius).expect("radius"),
        },
    )
}

/// The four sides of an axis-aligned rectangle, numbered from `first`.
fn rectangle(first: u32, low: (f64, f64), high: (f64, f64)) -> Vec<SketchEntity> {
    let corners = [low, (high.0, low.1), high, (low.0, high.1)];
    (0..4_u32)
        .map(|side| {
            let side_index = usize::try_from(side).expect("side index");
            line(
                first + side,
                corners[side_index],
                corners[(side_index + 1) % 4],
            )
        })
        .collect()
}

fn use_of(entity: &SketchEntity) -> SketchEntityUse {
    SketchEntityUse {
        entity: entity.id().clone(),
        reversed: false,
    }
}

fn sketch(loops: Vec<Vec<SketchEntityUse>>) -> Sketch {
    Sketch {
        id: sketch_id(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("placement"),
        profiles: SketchProfiles::try_from(loops).expect("profiles"),
        native_ref: None,
    }
}

/// A 40 mm square (loop 0) followed by one loop per circle.
fn square_and_circles(circles: &[((f64, f64), f64)]) -> (Sketch, Vec<SketchEntity>) {
    let mut entities = rectangle(1, (0.0, 0.0), (40.0, 40.0));
    for (ordinal, (center, radius)) in (10..).zip(circles) {
        entities.push(circle(ordinal, *center, *radius));
    }
    let mut loops = vec![entities[..4].iter().map(use_of).collect::<Vec<_>>()];
    loops.extend(entities[4..].iter().map(|circle| vec![use_of(circle)]));
    (sketch(loops), entities)
}

fn faces(
    circles: &[((f64, f64), f64)],
    extra: Vec<SketchEntity>,
    selected: &[usize],
    nested: &[(usize, Vec<usize>)],
) -> Option<Vec<SketchProfileRegion>> {
    let (sketch, mut entities) = square_and_circles(circles);
    entities.extend(extra);
    let index = ProfileIndex::new(&[], &[], &[], &[], &entities);
    index.material_faces(&sketch, sketch.profiles.as_slice(), selected, nested, &[])
}

fn region(outer: u32, holes: &[u32]) -> SketchProfileRegion {
    SketchProfileRegion::loops(outer, holes.to_vec()).expect("region")
}

#[test]
fn unselected_loops_in_the_material_become_faces_after_the_exterior() {
    let circles = [
        ((10.0, 10.0), 2.0),
        ((30.0, 30.0), 2.0),
        ((20.0, 20.0), 1.0),
    ];
    // The square selects its material with the first circle as a hole.
    assert_eq!(
        faces(&circles, Vec::new(), &[0, 1], &[(0, vec![1])]),
        Some(vec![region(0, &[1, 2, 3]), region(2, &[]), region(3, &[])])
    );
}

#[test]
fn an_unselected_loop_inside_an_unselected_loop_is_a_hole_of_that_face() {
    let circles = [((20.0, 20.0), 8.0), ((20.0, 20.0), 3.0)];
    assert_eq!(
        faces(&circles, Vec::new(), &[0], &[(0, Vec::new())]),
        Some(vec![region(0, &[1]), region(1, &[2]), region(2, &[])])
    );
}

#[test]
fn a_selected_hole_inside_an_unselected_loop_is_a_hole_of_that_face() {
    let circles = [((20.0, 20.0), 8.0), ((20.0, 20.0), 3.0)];
    assert_eq!(
        faces(&circles, Vec::new(), &[0, 2], &[(0, vec![1])]),
        Some(vec![region(0, &[1]), region(1, &[2])])
    );
}

#[test]
fn loops_inside_a_selected_hole_are_not_material() {
    let circles = [((20.0, 20.0), 8.0), ((20.0, 20.0), 3.0)];
    assert_eq!(faces(&circles, Vec::new(), &[0, 1], &[(0, vec![1])]), None);
}

#[test]
fn a_loop_outside_the_material_is_no_face() {
    let circles = [((60.0, 20.0), 3.0)];
    assert_eq!(faces(&circles, Vec::new(), &[0], &[(0, Vec::new())]), None);
}

#[test]
fn a_selected_island_in_a_hole_keeps_its_own_unselected_loops() {
    // Hole radius 12, island radius 8 inside it, one unselected loop in the
    // island and one in the material around the hole.
    let circles = [
        ((20.0, 20.0), 12.0),
        ((20.0, 20.0), 8.0),
        ((20.0, 20.0), 3.0),
        ((3.0, 3.0), 1.0),
    ];
    assert_eq!(
        faces(
            &circles,
            Vec::new(),
            &[0, 1, 2],
            &[(0, vec![1]), (2, Vec::new())]
        ),
        Some(vec![
            region(0, &[1, 4]),
            region(4, &[]),
            region(2, &[3]),
            region(3, &[])
        ])
    );
}

#[test]
fn a_curve_outside_the_loops_leaves_the_selected_rings_to_state_the_material() {
    let circles = [((10.0, 10.0), 2.0)];
    let open = vec![line(30, (5.0, 30.0), (35.0, 30.0))];
    assert_eq!(
        faces(&circles, Vec::new(), &[0], &[(0, Vec::new())]),
        Some(vec![region(0, &[1]), region(1, &[])])
    );
    assert_eq!(
        faces(&circles, open.clone(), &[0], &[(0, Vec::new())]),
        None
    );
    // Without an unselected loop in the material the open curve changes nothing.
    assert_eq!(faces(&circles, open, &[0, 1], &[(0, vec![1])]), None);
}

#[test]
fn loops_that_meet_leave_the_selected_rings_to_state_the_material() {
    let apart = [((10.0, 20.0), 6.0), ((30.0, 20.0), 6.0)];
    assert_eq!(
        faces(&apart, Vec::new(), &[0], &[(0, Vec::new())]),
        Some(vec![region(0, &[1, 2]), region(1, &[]), region(2, &[])])
    );
    let overlapping = [((15.0, 20.0), 6.0), ((22.0, 20.0), 6.0)];
    assert_eq!(
        faces(&overlapping, Vec::new(), &[0], &[(0, Vec::new())]),
        None
    );
    let touching = [((20.0, 6.0), 6.0)];
    assert_eq!(faces(&touching, Vec::new(), &[0], &[(0, Vec::new())]), None);
    let sharing = [((20.0, 20.0), 6.0), ((20.0, 20.0), 6.0)];
    assert_eq!(faces(&sharing, Vec::new(), &[0], &[(0, Vec::new())]), None);
}

#[test]
fn a_table_without_loop_geometry_or_over_the_cap_leaves_the_selected_rings_to_state_the_material() {
    let (mut sketch, mut entities) = square_and_circles(&[((10.0, 10.0), 2.0)]);
    let ellipse = entity(
        50,
        SketchGeometryDefinition::Ellipse {
            center: Point2::new(30.0, 30.0),
            major_angle: Angle::new(0.0).expect("angle"),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(2.0).expect("radius"),
                minor_radius: Length::new(1.0).expect("radius"),
            },
            bounds: None,
        },
    );
    let mut loops = sketch.profiles.iter().cloned().collect::<Vec<_>>();
    loops.push(vec![use_of(&ellipse)]);
    entities.push(ellipse);
    sketch.profiles = SketchProfiles::try_from(loops).expect("profiles");
    let index = ProfileIndex::new(&[], &[], &[], &[], &entities);
    assert_eq!(
        index.material_faces(
            &sketch,
            sketch.profiles.as_slice(),
            &[0],
            &[(0, Vec::new())],
            &[]
        ),
        None
    );
    let many = (100..)
        .zip(0..=SPLIT_LOOP_LIMIT)
        .map(|(ordinal, _)| circle(ordinal, (f64::from(ordinal) * 0.2 - 19.0, 1.0), 0.05))
        .collect::<Vec<_>>();
    let (mut sketch, mut entities) = square_and_circles(&[]);
    let mut loops = sketch.profiles.iter().cloned().collect::<Vec<_>>();
    loops.extend(many.iter().map(|circle| vec![use_of(circle)]));
    entities.extend(many);
    sketch.profiles = SketchProfiles::try_from(loops).expect("profiles");
    let index = ProfileIndex::new(&[], &[], &[], &[], &entities);
    assert_eq!(
        index.material_faces(
            &sketch,
            sketch.profiles.as_slice(),
            &[0],
            &[(0, Vec::new())],
            &[]
        ),
        None
    );
}

fn segment(start: (f64, f64), end: (f64, f64)) -> LoopCurve {
    LoopCurve::Line {
        start: Point2::new(start.0, start.1),
        end: Point2::new(end.0, end.1),
    }
}

fn round(center: (f64, f64), radius: f64) -> LoopCurve {
    LoopCurve::Circle {
        center: Point2::new(center.0, center.1),
        radius,
    }
}

#[test]
fn lines_meet_where_they_cross_touch_or_overlap() {
    assert!(curves_meet(
        segment((0.0, 0.0), (4.0, 4.0)),
        segment((0.0, 4.0), (4.0, 0.0))
    ));
    assert!(curves_meet(
        segment((0.0, 0.0), (4.0, 0.0)),
        segment((4.0, 0.0), (4.0, 3.0))
    ));
    assert!(curves_meet(
        segment((0.0, 0.0), (4.0, 0.0)),
        segment((2.0, 0.0), (9.0, 0.0))
    ));
    assert!(!curves_meet(
        segment((0.0, 0.0), (4.0, 0.0)),
        segment((5.0, 0.0), (9.0, 0.0))
    ));
    assert!(!curves_meet(
        segment((0.0, 0.0), (4.0, 0.0)),
        segment((0.0, 1.0), (4.0, 1.0))
    ));
    assert!(!curves_meet(
        segment((0.0, 0.0), (4.0, 0.0)),
        segment((6.0, -1.0), (6.0, 1.0))
    ));
}

#[test]
fn a_line_meets_a_circle_where_it_crosses_or_touches_it() {
    let circle = round((0.0, 0.0), 2.0);
    assert!(curves_meet(segment((-5.0, 0.0), (5.0, 0.0)), circle));
    assert!(curves_meet(circle, segment((2.0, -5.0), (2.0, 5.0))));
    assert!(!curves_meet(segment((-5.0, 3.0), (5.0, 3.0)), circle));
    assert!(!curves_meet(segment((-1.0, 0.0), (1.0, 0.0)), circle));
    // The line stops short of the circle.
    assert!(!curves_meet(segment((3.0, 0.0), (6.0, 0.0)), circle));
}

#[test]
fn circles_meet_where_they_cross_touch_or_coincide_and_not_when_nested() {
    assert!(curves_meet(round((0.0, 0.0), 2.0), round((3.0, 0.0), 2.0)));
    assert!(curves_meet(round((0.0, 0.0), 2.0), round((4.0, 0.0), 2.0)));
    assert!(curves_meet(round((0.0, 0.0), 2.0), round((0.0, 0.0), 2.0)));
    assert!(!curves_meet(round((0.0, 0.0), 2.0), round((0.0, 0.0), 1.0)));
    assert!(!curves_meet(round((0.0, 0.0), 2.0), round((9.0, 0.0), 2.0)));
    assert!(!curves_meet(round((0.0, 0.0), 5.0), round((1.0, 0.0), 1.0)));
}

#[test]
fn an_arc_meets_a_line_only_within_its_sweep() {
    let arc = LoopCurve::Arc {
        center: Point2::new(0.0, 0.0),
        radius: 2.0,
        start_angle: 0.0,
        sweep: std::f64::consts::FRAC_PI_2,
    };
    assert!(curves_meet(arc, segment((0.0, -3.0), (0.0, 3.0))));
    assert!(!curves_meet(arc, segment((-3.0, 0.0), (-1.0, 0.0))));
    assert!(!curves_meet(arc, segment((0.0, -3.0), (0.0, -1.0))));
}

#[test]
fn loose_lines_block_the_split_at_the_boundary_and_inside_the_outer_closure() {
    let circles = [((10.0, 10.0), 2.0)];
    for (start, end, blocks) in [
        ((50.0, 10.0), (60.0, 10.0), false),
        ((-10.0, 20.0), (50.0, 20.0), true),
        ((5.0, 30.0), (35.0, 30.0), true),
        ((40.0, 15.0), (50.0, 15.0), true),
    ] {
        assert_eq!(
            faces(
                &circles,
                vec![line(50, start, end)],
                &[0],
                &[(0, Vec::new())]
            )
            .is_none(),
            blocks,
        );
    }
}

#[test]
fn loose_round_curves_use_their_locus_for_the_split() {
    let circles = [((10.0, 10.0), 2.0)];
    for (center, radius, blocks) in [
        ((60.0, 20.0), 2.0, false),
        ((20.0, 20.0), 2.0, true),
        ((40.0, 20.0), 2.0, true),
        ((20.0, 20.0), 50.0, false),
    ] {
        assert_eq!(
            faces(
                &circles,
                vec![circle(50, center, radius)],
                &[0],
                &[(0, Vec::new())]
            )
            .is_none(),
            blocks,
        );
    }
    for (center, radius, start, end, blocks) in [
        ((41.0, 20.0), 2.0, 0.0, std::f64::consts::FRAC_PI_2, false),
        (
            (41.0, 20.0),
            2.0,
            std::f64::consts::PI,
            std::f64::consts::TAU,
            true,
        ),
        ((20.0, 20.0), 50.0, 0.0, std::f64::consts::FRAC_PI_2, false),
        ((20.0, 20.0), 2.0, 0.0, std::f64::consts::FRAC_PI_2, true),
    ] {
        let arc = entity(
            50,
            SketchGeometryDefinition::Arc {
                center: Point2::new(center.0, center.1),
                radius: Length::new(radius).expect("radius"),
                start_angle: Angle::new(start).expect("angle"),
                end_angle: Angle::new(end).expect("angle"),
            },
        );
        assert_eq!(
            faces(&circles, vec![arc], &[0], &[(0, Vec::new())]).is_none(),
            blocks
        );
    }
}

fn raw(ordinal: u32, owner: u32, kind: &serde_json::Value) -> crate::sketch::PmDcSketchEntity {
    raw_with_flags(ordinal, owner, 0, kind)
}

fn raw_with_flags(
    ordinal: u32,
    owner: u32,
    flags: u32,
    kind: &serde_json::Value,
) -> crate::sketch::PmDcSketchEntity {
    let mut entity: crate::sketch::PmDcSketchEntity = serde_json::from_value(serde_json::json!({
        "id": format!("inventor:pmdc:sketch-entity#segment-{ordinal}"),
        "type_id": "3bdf52ced011d0d20008ccbc0663dc09",
        "segment_token": "segment", "record_ordinal": ordinal,
        "save_version_major": 22,
        "header": {"header_value": 0, "header_id": 0, "flags": 0, "source_index": 0,
            "next": {"index": 0, "qualified": false},
            "context": {"index": 0, "qualified": false}},
        "entity_flags": flags, "sketch": {"index": owner, "qualified": true}, "kind": kind,
    }))
    .expect("raw entity");
    entity.identity.type_id =
        crate::record_identity::RecordTypeId::from_bytes(entity.kind.record_type_id());
    entity
}

fn native(raw: &crate::sketch::PmDcSketchEntity) -> SketchEntity {
    SketchEntity::new(
        crate::sketch::entity_id(raw).expect("id"),
        sketch_id(),
        SketchGeometry::native(cadmpeg_core::nonblank_literal!("test")),
    )
    .with_native_ref(Some(raw.id()))
}

fn non_curve_kinds() -> [serde_json::Value; 3] {
    [
        serde_json::json!({"form": "point", "position": [0.0, 0.0],
            "endpoint_of": {"marker": 2, "metadata": null, "references": []},
            "center_of": {"marker": 2, "metadata": null, "references": []},
            "state": null, "associations": null}),
        serde_json::json!({"form": "polygon", "reference": {"index": 0, "qualified": false}, "value": 0}),
        serde_json::json!({"form": "block_member", "instances": {"marker": 2, "metadata": null, "references": []},
            "member": {"index": 0, "qualified": false}}),
    ]
}

#[test]
fn helper_records_are_classified_by_their_raw_kind() {
    for kind in non_curve_kinds() {
        let helper = raw(50, 1, &kind);
        let (sketch, mut entities) = square_and_circles(&[((10.0, 10.0), 2.0)]);
        entities.push(native(&helper));
        let raw_entities = [helper];
        let index = ProfileIndex::new(&[], &[], &[], &raw_entities, &entities);
        assert_eq!(
            index.material_faces(
                &sketch,
                sketch.profiles.as_slice(),
                &[0],
                &[(0, Vec::new())],
                &[]
            ),
            Some(vec![region(0, &[1]), region(1, &[])])
        );
        let unknown = ProfileIndex::new(&[], &[], &[], &[], &entities);
        assert_eq!(
            unknown.material_faces(
                &sketch,
                sketch.profiles.as_slice(),
                &[0],
                &[(0, Vec::new())],
                &[]
            ),
            None
        );
    }
}

fn block(ordinal: u32, owner: u32, definition: u32) -> crate::sketch::PmDcSketchEntity {
    raw(
        ordinal,
        owner,
        &serde_json::json!({
            "form": "block_instance", "definition": {"index": definition, "qualified": true},
            "group": {"index": 0, "qualified": false}, "masks": [273, 202],
            "placement": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "value": 1,
        }),
    )
}

#[test]
fn a_block_requires_every_placed_definition_curve() {
    let instance = block(50, 1, 101);
    let definition_curve = |ordinal| {
        raw(
            ordinal,
            101,
            &serde_json::json!({
                "form": "circle", "points": {"marker": 2, "metadata": null, "references": []},
                "auxiliary": [], "center": {"index": 0, "qualified": false}, "radius": 0.2, "state": 1,
            }),
        )
    };
    let definitions = [definition_curve(101), definition_curve(102)];
    for (definition_count, placed_count, native_geometry) in [
        (0, 0, false),
        (2, 0, false),
        (2, 1, false),
        (2, 2, false),
        (2, 2, true),
    ] {
        let (sketch, mut entities) = square_and_circles(&[((10.0, 10.0), 2.0)]);
        entities.push(native(&instance));
        for definition in &definitions[..placed_count] {
            entities.push(SketchEntity::new(
                crate::sketch::block_curve_id(&instance, definition).expect("id"),
                sketch_id(),
                if native_geometry {
                    SketchGeometry::native(cadmpeg_core::nonblank_literal!("test"))
                } else {
                    circle(60, (60.0, 20.0), 2.0).geometry
                },
            ));
        }
        let mut raw_entities = vec![instance.clone()];
        raw_entities.extend(definitions[..definition_count].iter().cloned());
        let index = ProfileIndex::new(&[], &[], &[], &raw_entities, &entities);
        let missing = index
            .incomplete_sketches
            .get(&("segment", 0))
            .map_or(&[][..], Vec::as_slice);
        assert_eq!(
            index
                .material_faces(
                    &sketch,
                    sketch.profiles.as_slice(),
                    &[0],
                    &[(0, Vec::new())],
                    missing
                )
                .is_some(),
            definition_count == 2 && placed_count == 2 && !native_geometry
        );
    }
}

#[test]
fn block_completion_requires_every_geometry_producer_in_the_definition() {
    let instance = block(50, 1, 101);
    let curve = raw(
        101,
        101,
        &serde_json::json!({
            "form": "circle", "points": {"marker": 2, "metadata": null, "references": []},
            "auxiliary": [], "center": {"index": 0, "qualified": false}, "radius": 0.2, "state": 1,
        }),
    );
    let mut spline = crate::test_support::sketch_curves::open_cubic_entities()
        .into_iter()
        .find(|entity| {
            matches!(
                entity.kind,
                crate::sketch::PmDcSketchEntityKind::Spline { .. }
            )
        })
        .expect("spline");
    spline.identity.segment_token = cadmpeg_ir::identity_key!("segment");
    spline.identity.record_ordinal = 102;
    spline.edit(|payload| payload.sketch = crate::pmdc::PmDcReference::from_packed(0x8000_0065));
    let mut nested_curve = curve.clone();
    nested_curve.identity.record_ordinal = 201;
    nested_curve
        .edit(|payload| payload.sketch = crate::pmdc::PmDcReference::from_packed(0x8000_00c9));
    for (additional, complete) in [
        (Vec::new(), true),
        (vec![block(102, 101, 201), nested_curve], false),
        (vec![spline], false),
    ]
    .into_iter()
    .chain(
        non_curve_kinds()
            .into_iter()
            .map(|kind| (vec![raw(102, 101, &kind)], true)),
    ) {
        let (sketch, mut entities) = square_and_circles(&[((10.0, 10.0), 2.0)]);
        entities.push(native(&instance));
        entities.push(SketchEntity::new(
            crate::sketch::block_curve_id(&instance, &curve).expect("id"),
            sketch_id(),
            circle(60, (60.0, 20.0), 2.0).geometry,
        ));
        let mut raw_entities = vec![instance.clone(), curve.clone()];
        raw_entities.extend(additional);
        let index = ProfileIndex::new(&[], &[], &[], &raw_entities, &entities);
        let missing = index
            .incomplete_sketches
            .get(&("segment", 0))
            .map_or(&[][..], Vec::as_slice);
        assert_eq!(
            index
                .material_faces(
                    &sketch,
                    sketch.profiles.as_slice(),
                    &[0],
                    &[(0, Vec::new())],
                    missing,
                )
                .is_some(),
            complete
        );
    }
}

#[test]
fn block_definition_nullness_qualifies_the_construction_curve_join() {
    for (definition, complete) in [(0, false), (101, true)] {
        let instance = block(50, 1, definition);
        let construction = raw_with_flags(
            101,
            definition,
            0x8_0000,
            &serde_json::json!({
                "form": "line", "points": {"marker": 2, "metadata": null, "references": []},
                "auxiliary": [], "origin": [0.0, 0.0], "direction": [1.0, 0.0],
            }),
        );
        let (sketch, mut entities) = square_and_circles(&[((10.0, 10.0), 2.0)]);
        entities.push(native(&instance));
        let raw_entities = [instance, construction];
        let index = ProfileIndex::new(&[], &[], &[], &raw_entities, &entities);
        let missing = index
            .incomplete_sketches
            .get(&("segment", 0))
            .map_or(&[][..], Vec::as_slice);
        assert_eq!(
            index
                .material_faces(
                    &sketch,
                    sketch.profiles.as_slice(),
                    &[0],
                    &[(0, Vec::new())],
                    missing,
                )
                .is_some(),
            complete
        );
    }
}

#[test]
fn missing_curves_share_the_unused_curve_intersection_gate() {
    let (sketch, entities) = square_and_circles(&[((10.0, 10.0), 2.0)]);
    let index = ProfileIndex::new(&[], &[], &[], &[], &entities);
    for (curve, blocks) in [
        (Some(segment((50.0, 10.0), (60.0, 10.0))), false),
        (Some(segment((-10.0, 20.0), (50.0, 20.0))), true),
        (Some(segment((5.0, 30.0), (35.0, 30.0))), true),
        (None, true),
    ] {
        assert_eq!(
            index
                .material_faces(
                    &sketch,
                    sketch.profiles.as_slice(),
                    &[0],
                    &[(0, Vec::new())],
                    &[curve]
                )
                .is_none(),
            blocks
        );
    }
}

#[test]
fn loose_ellipses_stay_conservative() {
    let ellipse = entity(
        50,
        SketchGeometryDefinition::Ellipse {
            center: Point2::new(60.0, 60.0),
            major_angle: Angle::new(0.0).expect("angle"),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(2.0).expect("radius"),
                minor_radius: Length::new(1.0).expect("radius"),
            },
            bounds: None,
        },
    );
    assert_eq!(
        faces(
            &[((10.0, 10.0), 2.0)],
            vec![ellipse],
            &[0],
            &[(0, Vec::new())]
        ),
        None
    );
}

#[test]
fn missing_raw_lines_are_located_before_the_split_gate() {
    let references = |indices: &[u32]| {
        serde_json::json!({
            "marker": 2,
            "metadata": if indices.is_empty() { serde_json::Value::Null } else {
                serde_json::json!({"values": [2, 0], "width": "u32"})
            },
            "references": indices.iter().map(|index| serde_json::json!({"index": index, "qualified": true})).collect::<Vec<_>>(),
        })
    };
    for (start, end, blocks) in [
        ([5.0, 1.0], [6.0, 1.0], false),
        ([-1.0, 2.0], [5.0, 2.0], true),
        ([0.5, 3.0], [3.5, 3.0], true),
    ] {
        let point = |ordinal, position| {
            raw(
                ordinal,
                1,
                &serde_json::json!({
                    "form": "point", "position": position,
                    "endpoint_of": references(&[]), "center_of": references(&[]),
                    "state": null, "associations": null,
                }),
            )
        };
        let mut raw_entities = [
            point(50, start),
            point(51, end),
            raw(
                52,
                1,
                &serde_json::json!({
                    "form": "line", "points": references(&[51, 52]), "auxiliary": [],
                    "origin": start, "direction": [1.0, 0.0],
                }),
            ),
        ];
        let (sketch, entities) = square_and_circles(&[((10.0, 10.0), 2.0)]);
        let index = ProfileIndex::new(&[], &[], &[], &raw_entities, &entities);
        let missing = index
            .incomplete_sketches
            .get(&("segment", 0))
            .expect("missing line");
        assert!(matches!(missing.as_slice(), [Some(LoopCurve::Line { .. })]));
        assert_eq!(
            index
                .material_faces(
                    &sketch,
                    sketch.profiles.as_slice(),
                    &[0],
                    &[(0, Vec::new())],
                    missing
                )
                .is_none(),
            blocks
        );
        let mut construction = serde_json::to_value(&raw_entities[2]).expect("line");
        construction["entity_flags"] = serde_json::json!(0x80000);
        raw_entities[2] = serde_json::from_value(construction).expect("construction line");
        let index = ProfileIndex::new(&[], &[], &[], &raw_entities, &entities);
        assert!(!index.incomplete_sketches.contains_key(&("segment", 0)));
        assert!(index
            .material_faces(
                &sketch,
                sketch.profiles.as_slice(),
                &[0],
                &[(0, Vec::new())],
                &[]
            )
            .is_some());
    }
}
