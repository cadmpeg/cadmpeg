// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

use super::{marker_reading_binds, EntityIndex};

fn sketch(id: &str, loops: &[&[&SketchEntity]]) -> Sketch {
    Sketch {
        id: SketchId::mint(format!("synthetic:test:id#{id}")).unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::try_from(
            loops
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
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        native_ref: None,
    }
}

fn entity(id: &str, geometry: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#{id}")).unwrap(),
        SketchId::mint("synthetic:test:id#owner").unwrap(),
        SketchGeometry::try_from(geometry).unwrap(),
    )
}

fn circle(id: &str, center: [f64; 2], radius: f64) -> SketchEntity {
    entity(
        id,
        SketchGeometryDefinition::Circle {
            center: Point2::new(center[0], center[1]),
            radius: Length::new(radius).unwrap(),
        },
    )
}

fn half(id: &str, radius: f64, start: f64) -> SketchEntity {
    entity(
        id,
        SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(radius).unwrap(),
            start_angle: Angle::new(start).unwrap(),
            end_angle: Angle::new((start + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU))
                .unwrap(),
        },
    )
}

fn square(id: &str, side: f64) -> [SketchEntity; 4] {
    let corners = [[0.0, 0.0], [side, 0.0], [side, side], [0.0, side]];
    std::array::from_fn(|index| {
        let [start, end] = [corners[index], corners[(index + 1) % 4]];
        entity(
            &format!("{id}-{index}"),
            SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            },
        )
    })
}

/// A marker reading of another loop binds in place of a stream reading of
/// one circle; a marker reading that repeats the circle, as two half arcs
/// placed elsewhere, does not.
#[test]
fn a_marker_reading_binds_in_place_of_a_lone_circle_it_does_not_repeat() {
    let whole = circle("whole", [5.0, 5.0], 10.0);
    let upper = half("upper", 10.0, 0.0);
    let lower = half("lower", 10.0, std::f64::consts::PI);
    let sides = square("side", 3.0);
    let entities = EntityIndex::new(sides.iter().chain([&whole, &upper, &lower]));
    let stream = sketch("stream", &[&[&whole]]);
    let split = sketch("split", &[&[&upper, &lower]]);
    let boxed = sketch("box", &[&sides.iter().collect::<Vec<_>>()]);
    assert!(!marker_reading_binds(&split, &stream, &entities));
    assert!(marker_reading_binds(&boxed, &stream, &entities));
}

/// A stream reading of anything but one circle keeps its binding, whatever
/// the marker reading holds.
#[test]
fn a_stream_reading_that_is_not_one_circle_keeps_its_binding() {
    let outer = circle("outer", [0.0, 0.0], 9.0);
    let inner = circle("inner", [0.0, 0.0], 4.0);
    let sides = square("side", 3.0);
    let entities = EntityIndex::new(sides.iter().chain([&outer, &inner]));
    let ring = sketch("ring", &[&[&outer], &[&inner]]);
    let boxed = sketch("box", &[&sides.iter().collect::<Vec<_>>()]);
    let stream_box = sketch("stream-box", &[&sides.iter().collect::<Vec<_>>()]);
    assert!(!marker_reading_binds(&ring, &stream_box, &entities));
    assert!(!marker_reading_binds(&boxed, &ring, &entities));
}
