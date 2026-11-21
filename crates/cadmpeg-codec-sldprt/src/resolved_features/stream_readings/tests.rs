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

/// A circle read as two half arcs repeats the circle, wherever each reading
/// places it: a marker reading of the split circle and a hole binds in place
/// of a stream reading of the whole circle and the hole, and a marker reading
/// of the split circle alone does not.
#[test]
fn two_half_arcs_repeat_the_circle_they_split() {
    let whole = circle("whole", [5.0, 5.0], 10.0);
    let upper = half("upper", 10.0, 0.0);
    let lower = half("lower", 10.0, std::f64::consts::PI);
    let hole = circle("hole", [0.0, 0.0], 4.0);
    let stream_hole = circle("stream-hole", [5.0, 5.0], 4.0);
    let entities = EntityIndex::new([&whole, &upper, &lower, &hole, &stream_hole]);
    let stream = sketch("stream", &[&[&whole], &[&stream_hole]]);
    let marker = sketch("marker", &[&[&upper, &lower], &[&hole]]);
    assert!(marker_reading_binds(&marker, &stream, &entities));
    let split_only = sketch("split", &[&[&upper, &lower]]);
    assert!(!marker_reading_binds(&split_only, &stream, &entities));
}

/// A marker reading with fewer loops than a stream reading of other loops
/// leaves the stream reading bound; a marker reading with more loops binds.
#[test]
fn loop_count_decides_between_readings_of_different_loops() {
    let outer = circle("outer", [0.0, 0.0], 9.0);
    let inner = circle("inner", [0.0, 0.0], 4.0);
    let other = circle("other", [0.0, 0.0], 7.0);
    let spare = circle("spare", [0.0, 0.0], 2.0);
    let entities = EntityIndex::new([&outer, &inner, &other, &spare]);
    let ring = sketch("ring", &[&[&outer], &[&inner]]);
    let pair = sketch("pair", &[&[&other], &[&spare]]);
    let single = sketch("single", &[&[&other]]);
    assert!(marker_reading_binds(&ring, &single, &entities));
    assert!(!marker_reading_binds(&single, &ring, &entities));
    assert!(!marker_reading_binds(&ring, &pair, &entities));
}
