// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

use super::{marker_reading, EntityIndex, MarkerReading, UnboundReason};

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

/// `sketch` placed at `origin` with `normal` and `u_axis`.
fn placed(mut sketch: Sketch, origin: [f64; 3], normal: [f64; 3], u_axis: [f64; 3]) -> Sketch {
    sketch.placement = placement(origin, normal, u_axis);
    sketch
}

fn placement(origin: [f64; 3], normal: [f64; 3], u_axis: [f64; 3]) -> SketchPlacement {
    SketchPlacement::try_resolved(
        Point3::from(origin),
        Vector3::from(normal),
        Vector3::from(u_axis),
    )
    .unwrap()
}

/// `marker_reading` for a marker reading whose entities are all planar and
/// that no revolve takes.
fn reading(marker: &Sketch, stream: &Sketch, entities: &EntityIndex<'_>) -> MarkerReading {
    marker_reading(marker, stream, entities, true, false)
}

/// A marker reading of another loop binds in place of a stream reading of
/// one circle in its plane; a marker reading that repeats the circle, as two
/// half arcs placed elsewhere, does not, nor does one with an entity that
/// is not planar.
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
    assert_eq!(reading(&split, &stream, &entities), MarkerReading::Stays);
    assert_eq!(
        reading(&boxed, &stream, &entities),
        MarkerReading::Binds(stream.placement)
    );
    assert_eq!(
        marker_reading(&boxed, &stream, &entities, false, true),
        MarkerReading::Stays
    );
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
    assert_eq!(reading(&ring, &stream_box, &entities), MarkerReading::Stays);
    assert_eq!(reading(&boxed, &ring, &entities), MarkerReading::Stays);
}

/// A marker reading whose normal is parallel to the stream circle's binds on
/// the stream sketch's plane: the stream sketch's normal, u axis and offset,
/// through its own origin projected onto that plane. Where the normals
/// oppose, it takes the stream sketch's.
#[test]
fn a_parallel_marker_reading_binds_on_the_stream_plane() {
    let whole = circle("whole", [0.0, 0.0], 10.0);
    let sides = square("side", 3.0);
    let entities = EntityIndex::new(sides.iter().chain([&whole]));
    let boxed = placed(
        sketch("box", &[&sides.iter().collect::<Vec<_>>()]),
        [1.0, 2.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    let above = placed(
        sketch("above", &[&[&whole]]),
        [30.0, -4.0, 20.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    assert_eq!(
        reading(&boxed, &above, &entities),
        MarkerReading::Binds(placement(
            [1.0, 2.0, 20.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0]
        ))
    );
    let below = placed(
        sketch("below", &[&[&whole]]),
        [0.0, 0.0, -18.5],
        [0.0, 0.0, -1.0],
        [-1.0, 0.0, 0.0],
    );
    assert_eq!(
        reading(&boxed, &below, &entities),
        MarkerReading::Binds(placement(
            [1.0, 2.0, -18.5],
            [0.0, 0.0, -1.0],
            [-1.0, 0.0, 0.0]
        ))
    );
    assert_eq!(
        marker_reading(&boxed, &below, &entities, true, true),
        reading(&boxed, &below, &entities),
        "a revolve's profile takes the stream plane too where the normals are parallel"
    );
}

/// A marker reading whose normal is not parallel to the stream circle's, or
/// whose placement is unresolved, leaves the stream circle bound, unless a
/// revolve takes it as its profile; then it binds with its own placement.
#[test]
fn a_marker_reading_on_another_plane_leaves_the_stream_bound_unless_a_revolve_takes_it() {
    let whole = circle("whole", [0.0, 0.0], 10.0);
    let sides = square("side", 3.0);
    let entities = EntityIndex::new(sides.iter().chain([&whole]));
    let boxed = sketch("box", &[&sides.iter().collect::<Vec<_>>()]);
    let side = placed(
        sketch("side-circle", &[&[&whole]]),
        [0.0, 5.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
    );
    assert_eq!(
        reading(&boxed, &side, &entities),
        MarkerReading::Unbound(UnboundReason::OtherPlane)
    );
    assert_eq!(
        marker_reading(&boxed, &side, &entities, true, true),
        MarkerReading::Binds(boxed.placement)
    );
    let mut unplaced = boxed.clone();
    unplaced.placement = SketchPlacement::Unresolved {};
    assert_eq!(
        reading(&unplaced, &side, &entities),
        MarkerReading::Unbound(UnboundReason::PlacementUnresolved)
    );
    assert_eq!(
        marker_reading(&unplaced, &side, &entities, true, true),
        MarkerReading::Binds(SketchPlacement::Unresolved {})
    );
}

/// A marker reading that holds the stream circle's loop among other loops
/// leaves the stream circle bound, also in the stream circle's plane and
/// where a revolve takes it.
#[test]
fn a_marker_reading_that_holds_the_circle_among_other_loops_leaves_the_stream_bound() {
    let hole = circle("hole", [8.0, 8.0], 2.0);
    let repeat = circle("repeat", [1.0, 1.0], 2.0);
    let sides = square("side", 20.0);
    let entities = EntityIndex::new(sides.iter().chain([&hole, &repeat]));
    let stream = sketch("stream", &[&[&repeat]]);
    let plate = sketch("plate", &[&sides.iter().collect::<Vec<_>>(), &[&hole]]);
    assert_eq!(
        reading(&plate, &stream, &entities),
        MarkerReading::Unbound(UnboundReason::HoldsCircle)
    );
    assert_eq!(
        marker_reading(&plate, &stream, &entities, true, true),
        MarkerReading::Unbound(UnboundReason::HoldsCircle)
    );
}
