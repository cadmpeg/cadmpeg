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
    index.material_faces(&sketch, sketch.profiles.as_slice(), selected, nested)
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
            &[(0, Vec::new())]
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
            &[(0, Vec::new())]
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
