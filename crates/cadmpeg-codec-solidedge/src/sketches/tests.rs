// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::f64::consts::FRAC_PI_2;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::construction::FrameValue;
use cadmpeg_ir::design::ops::sketch::{EntityDefinition, PlanarCurve, SketchPlane};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::units::FinitePoint2;

use crate::test_support::{profile_group, sketch_document, Member2d, ProfileFrame};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre coordinate with the hand-computed one.
const EPS_MM: f64 = 1e-9;
/// The profile group's set object.
const SET: u32 = 100;
/// The extrusion function, whose profile identifier stands 72 bytes after the
/// function name.
const EXTRUDE: &str = "JMProtrusionFunction";

const XY: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, 0.0, 1.0],
    x_axis: [1.0, 0.0, 0.0],
};

fn decode(bytes: Vec<u8>) -> DecodeResult {
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn with_members(frame: &ProfileFrame, members: &[Member2d]) -> DecodeResult {
    decode(sketch_document(
        EXTRUDE,
        72,
        SET + 1,
        vec![profile_group(SET, frame, 1, 200, members)],
    ))
}

fn codes(result: &DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect()
}

fn messages(result: &DecodeResult, code: &str) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == code)
        .map(|loss| loss.message.clone())
        .collect()
}

fn frame_of(result: &DecodeResult) -> FrameValue {
    let sketch = &result.ir().design.sketches[0];
    let SketchPlane::Placed(plane) = &sketch.plane else {
        panic!("a sketch of this decoder is placed");
    };
    plane.resolved.clone().unwrap()
}

fn curves(result: &DecodeResult) -> Vec<PlanarCurve> {
    result
        .ir()
        .design
        .sketch_entities
        .iter()
        .map(|entity| match &entity.definition {
            EntityDefinition::Free(curve) => curve.clone(),
            EntityDefinition::Defined { .. } => panic!("an entity of this decoder is free"),
        })
        .collect()
}

/// The model point (millimetres) of a sketch point on `frame`: origin + u·x +
/// v·y.
fn model(frame: &FrameValue, point: FinitePoint2) -> [f64; 3] {
    let at = point.get();
    let origin = frame.origin.get();
    let (x, y) = (Vector3::from(frame.x_axis), Vector3::from(frame.y_axis));
    [
        origin.x + at.u * x.x + at.v * y.x,
        origin.y + at.u * x.y + at.v * y.y,
        origin.z + at.u * x.z + at.v * y.z,
    ]
}

fn assert_near(found: [f64; 3], expected: [f64; 3]) {
    for (a, b) in found.into_iter().zip(expected) {
        assert!((a - b).abs() <= EPS_MM, "{found:?} != {expected:?}");
    }
}

fn line_ends(frame: &FrameValue, curve: &PlanarCurve) -> [[f64; 3]; 2] {
    let PlanarCurve::Line { start, end } = curve else {
        panic!("expected a line, found {curve:?}");
    };
    [model(frame, *start), model(frame, *end)]
}

#[test]
fn rectangle_of_four_lines_on_the_xy_frame() {
    // A 40 mm by 20 mm rectangle, stored in metres.
    let result = with_members(
        &XY,
        &[
            Member2d::Line([0.0, 0.0, 0.04, 0.0]),
            Member2d::Line([0.04, 0.0, 0.04, 0.02]),
            Member2d::Line([0.04, 0.02, 0.0, 0.02]),
            Member2d::Line([0.0, 0.02, 0.0, 0.0]),
        ],
    );
    let design = &result.ir().design;
    assert_eq!(design.sketches.len(), 1);
    assert_eq!(
        design.sketches[0].owner.as_str(),
        "solidedge:model:feature#10"
    );
    assert_eq!(design.sketches[0].id.as_str(), "solidedge:model:sketch#101");
    assert_eq!(design.sketch_entities.len(), 4);
    let frame = frame_of(&result);
    let found = curves(&result)
        .iter()
        .map(|curve| line_ends(&frame, curve))
        .collect::<Vec<_>>();
    let expected = [
        [[0.0, 0.0, 0.0], [40.0, 0.0, 0.0]],
        [[40.0, 0.0, 0.0], [40.0, 20.0, 0.0]],
        [[40.0, 20.0, 0.0], [0.0, 20.0, 0.0]],
        [[0.0, 20.0, 0.0], [0.0, 0.0, 0.0]],
    ];
    for (line, wanted) in found.into_iter().zip(expected) {
        assert_near(line[0], wanted[0]);
        assert_near(line[1], wanted[1]);
    }
    assert!(!codes(&result)
        .iter()
        .any(|code| code.starts_with("sketch.")));
}

#[test]
fn circle_maps_through_a_raised_frame() {
    let raised = ProfileFrame {
        origin: [0.0, 0.0, 0.01],
        ..XY
    };
    let result = with_members(&raised, &[Member2d::Circle([0.012, 0.007, 0.005])]);
    let frame = frame_of(&result);
    let origin = frame.origin.get();
    assert_near([origin.x, origin.y, origin.z], [0.0, 0.0, 10.0]);
    let [PlanarCurve::Circle { center, radius }] = curves(&result)[..] else {
        panic!("expected one circle");
    };
    assert_near(model(&frame, center), [12.0, 7.0, 10.0]);
    assert!((radius.get() - 5.0).abs() <= EPS_MM);
}

#[test]
fn arc_ends_map_to_model_space() {
    let result = with_members(&XY, &[Member2d::Arc([0.034, 0.024, 0.006, 0.0, FRAC_PI_2])]);
    let frame = frame_of(&result);
    let [PlanarCurve::Arc {
        center,
        radius,
        start,
        end,
    }] = curves(&result)[..]
    else {
        panic!("expected one arc");
    };
    assert!(start.get().abs() <= EPS_MM);
    assert!((end.get() - FRAC_PI_2).abs() <= EPS_MM);
    let r = radius.get();
    let at = |angle: f64| {
        FinitePoint2::new(Point2 {
            u: center.get().u + r * angle.cos(),
            v: center.get().v + r * angle.sin(),
        })
        .unwrap()
    };
    assert_near(model(&frame, at(start.get())), [40.0, 24.0, 0.0]);
    assert_near(model(&frame, at(end.get())), [34.0, 30.0, 0.0]);
}

#[test]
fn front_plane_frame_places_local_y_on_model_z() {
    let front = ProfileFrame {
        origin: [0.0, 0.0, 0.0],
        normal: [0.0, -1.0, 0.0],
        x_axis: [1.0, 0.0, 0.0],
    };
    // Local (5, 20) mm lies at model (5, 0, 20) mm.
    let result = with_members(&front, &[Member2d::Line([0.005, 0.02, 0.01, 0.03])]);
    let frame = frame_of(&result);
    let found = line_ends(&frame, &curves(&result)[0]);
    assert_near(found[0], [5.0, 0.0, 20.0]);
    assert_near(found[1], [10.0, 0.0, 30.0]);
}

#[test]
fn hole_function_stores_its_profile_68_bytes_after_the_name() {
    let result = decode(sketch_document(
        "JMHoleFunction",
        68,
        SET + 1,
        vec![profile_group(
            SET,
            &XY,
            1,
            200,
            &[Member2d::Circle([0.0, 0.0, 0.002])],
        )],
    ));
    assert_eq!(result.ir().design.sketches.len(), 1);
}

#[test]
fn profile_identifier_naming_a_plane_is_unread() {
    // Object 1 is a base reference plane.
    let result = decode(sketch_document(EXTRUDE, 72, 1, Vec::new()));
    assert!(result.ir().design.sketches.is_empty());
    assert!(result.ir().design.sketch_entities.is_empty());
    let found = messages(&result, "sketch.unread");
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("0x1 "), "{found:?}");
}

#[test]
fn pattern_profile_is_unread() {
    // Object 300 is a pattern profile.
    let result = decode(sketch_document(EXTRUDE, 72, 300, Vec::new()));
    assert!(result.ir().design.sketches.is_empty());
    let found = messages(&result, "sketch.unread");
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("0x12c"), "{found:?}");
}

#[test]
fn frame_that_is_not_orthonormal_is_unread() {
    let skew = ProfileFrame {
        x_axis: [1.0, 0.0, 0.5],
        ..XY
    };
    let result = with_members(&skew, &[Member2d::Line([0.0, 0.0, 0.01, 0.0])]);
    assert!(result.ir().design.sketches.is_empty());
    let found = messages(&result, "sketch.unread");
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("0x65"), "{found:?}");
}

#[test]
fn constraint_member_is_counted_and_a_keypoint_is_not() {
    let result = with_members(
        &XY,
        &[
            Member2d::Line([0.0, 0.0, 0.01, 0.0]),
            Member2d::Keypoint,
            Member2d::Constraint,
            Member2d::Constraint,
        ],
    );
    assert_eq!(result.ir().design.sketch_entities.len(), 1);
    let found = messages(&result, "sketch.relations-unread");
    assert_eq!(found.len(), 1);
    assert!(found[0].starts_with("2 "), "{found:?}");
}

#[test]
fn member_that_names_no_object_is_skipped() {
    let result = with_members(
        &XY,
        &[Member2d::Line([0.0, 0.0, 0.01, 0.0]), Member2d::None],
    );
    assert_eq!(result.ir().design.sketch_entities.len(), 1);
    assert!(messages(&result, "sketch.unread").is_empty());
    assert!(messages(&result, "sketch.relations-unread").is_empty());
}

#[test]
fn a_member_listed_twice_states_one_curve() {
    let result = with_members(
        &XY,
        &[Member2d::Line([0.0, 0.0, 0.01, 0.0]), Member2d::Repeat(0)],
    );
    assert_eq!(result.ir().design.sketch_entities.len(), 1);
    assert_eq!(messages(&result, "sketch.unread").len(), 1);
}
