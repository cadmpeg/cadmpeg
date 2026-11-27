// SPDX-License-Identifier: Apache-2.0
//! The joint connectors, kinds, and alignment that body mates and joints
//! share, and the pair element.

use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;

use super::{
    angle, axis_construction, direction_construction, frame_construction, length, literal, ordered,
    parameter, plane_construction, point_construction, refuses, text, to_value, Report,
};
use crate::design::construction::FrameAxis;
use crate::design::ops::shared::{
    Alignment, AxisLimits, Bounds, Coupling, DriveAxis, DriveMode, JointConnector, JointDrive,
    JointKind, JointMotion, JointOffset, PairElement, PathAxisControl, PathPosition,
    PathRollControl, SlotPosition, TangentContact, WidthMode,
};
use crate::scalar::{
    Angle, FiniteReal, Fraction, Length, NonNegativeLength, NonNegativeReal, PositiveReal,
};

fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}

fn non_negative(value: f64) -> NonNegativeLength {
    NonNegativeLength::new(value).unwrap()
}

pub(super) fn connectors() -> Vec<JointConnector> {
    vec![
        JointConnector::Frame(frame_construction()),
        JointConnector::Plane(plane_construction()),
        JointConnector::Axis(axis_construction()),
        JointConnector::Point(point_construction()),
        JointConnector::Entity(ordered(60)),
    ]
}

fn alignments() -> Vec<Alignment> {
    vec![
        Alignment::Aligned,
        Alignment::Opposed,
        Alignment::Nearest,
        Alignment::Native(text("catia:orientation")),
    ]
}

fn motions() -> Vec<JointMotion> {
    vec![
        JointMotion::Rigid,
        JointMotion::Revolute { axis: FrameAxis::Z },
        JointMotion::Slider { axis: FrameAxis::X },
        JointMotion::Cylindrical { axis: FrameAxis::Y },
        JointMotion::Planar { axis: FrameAxis::Z },
        JointMotion::Ball,
        JointMotion::PinSlot {
            axis: FrameAxis::Z,
            slide: FrameAxis::X,
        },
        JointMotion::Native(text("f3d:freedom:3")),
    ]
}

fn offsets() -> Vec<JointOffset> {
    vec![
        JointOffset {
            x: Some(literal(length(1.0))),
            y: Some(parameter(61)),
            z: Some(literal(length(-2.0))),
            angle: Some(literal(angle(0.5))),
        },
        JointOffset {
            x: None,
            y: None,
            z: None,
            angle: None,
        },
    ]
}

fn length_bounds() -> Vec<Bounds<Length>> {
    vec![
        Bounds {
            min: Some(literal(length(-1.0))),
            max: Some(literal(length(4.0))),
        },
        Bounds {
            min: None,
            max: Some(parameter(62)),
        },
    ]
}

fn angle_bounds() -> Vec<Bounds<Angle>> {
    vec![
        Bounds {
            min: Some(literal(angle(0.0))),
            max: None,
        },
        Bounds {
            min: Some(literal(angle(-1.0))),
            max: Some(literal(angle(1.0))),
        },
    ]
}

fn axis_limits() -> Vec<AxisLimits> {
    vec![
        AxisLimits::Rotation {
            axis: FrameAxis::Z,
            limits: angle_bounds().pop(),
            rest: Some(literal(angle(0.25))),
        },
        AxisLimits::Rotation {
            axis: FrameAxis::X,
            limits: None,
            rest: Some(parameter(63)),
        },
        AxisLimits::Translation {
            axis: FrameAxis::Y,
            limits: length_bounds().pop(),
            rest: None,
        },
    ]
}

fn couplings() -> Vec<Coupling> {
    vec![
        Coupling::Gear {
            ratio: Some(literal(real(2.0))),
            radii: [Some(literal(length(10.0))), None],
        },
        Coupling::Belt {
            ratio: None,
            radii: [Some(literal(length(5.0))), Some(literal(length(15.0)))],
        },
        Coupling::RackPinion {
            ratio: Some(parameter(64)),
            pitch_radius: Some(literal(length(8.0))),
        },
        Coupling::RackPinion {
            ratio: None,
            pitch_radius: None,
        },
        Coupling::Screw {
            pitch: Some(literal(length(1.5))),
        },
        Coupling::Screw { pitch: None },
        Coupling::Native(text("f3d:motion_link")),
    ]
}

fn drive_axes() -> Vec<DriveAxis> {
    vec![
        DriveAxis::Rotation { axis: FrameAxis::Z },
        DriveAxis::Translation { axis: FrameAxis::X },
    ]
}

fn drives() -> Vec<JointDrive> {
    vec![
        JointDrive {
            axis: DriveAxis::Rotation { axis: FrameAxis::Z },
            mode: DriveMode::Force,
            target_position: Some(real(1.0)),
            target_velocity: Some(real(-2.0)),
            stiffness: Some(NonNegativeReal::new(100.0).unwrap()),
            damping: Some(NonNegativeReal::new(0.0).unwrap()),
            max_force: Some(PositiveReal::new(50.0).unwrap()),
        },
        JointDrive {
            axis: DriveAxis::Translation { axis: FrameAxis::Y },
            mode: DriveMode::Acceleration,
            target_position: None,
            target_velocity: None,
            stiffness: None,
            damping: None,
            max_force: None,
        },
    ]
}

fn width_modes() -> Vec<WidthMode> {
    vec![
        WidthMode::Centered,
        WidthMode::Free,
        WidthMode::Gap {
            distance: literal(non_negative(2.0)),
            from: Some(PairElement::Second),
        },
        WidthMode::Gap {
            distance: parameter(65),
            from: None,
        },
        WidthMode::Native(text("sldprt:percent")),
    ]
}

fn slot_positions() -> Vec<SlotPosition> {
    vec![
        SlotPosition::Free,
        SlotPosition::Centered,
        SlotPosition::Native(text("sldprt:slot_position")),
    ]
}

fn path_positions() -> Vec<PathPosition> {
    vec![
        PathPosition::Free,
        PathPosition::Distance {
            distance: literal(non_negative(12.0)),
            from_end: Some(literal(true)),
        },
        PathPosition::Distance {
            distance: literal(non_negative(0.0)),
            from_end: None,
        },
        PathPosition::Percent {
            fraction: literal(Fraction::new(0.25).unwrap()),
            from_end: Some(parameter(66)),
        },
    ]
}

fn pitch_yaws() -> Vec<PathAxisControl> {
    vec![
        PathAxisControl::Free,
        PathAxisControl::FollowPath {
            axis: FrameAxis::X,
            flip: Some(literal(false)),
        },
        PathAxisControl::FollowPath {
            axis: FrameAxis::Y,
            flip: None,
        },
    ]
}

fn rolls() -> Vec<PathRollControl> {
    vec![
        PathRollControl::Free,
        PathRollControl::UpVector {
            vector: Box::new(direction_construction()),
            axis: FrameAxis::Z,
            flip: Some(literal(true)),
        },
        PathRollControl::UpVector {
            vector: Box::new(direction_construction()),
            axis: FrameAxis::Y,
            flip: None,
        },
    ]
}

fn kinds() -> Vec<JointKind> {
    let [first, second, ..] = <[_; 5]>::try_from(connectors()).unwrap();
    vec![
        JointKind::Motion {
            motion: JointMotion::Revolute { axis: FrameAxis::Z },
            axes: Some(Box::new(frame_construction())),
            offset: offsets().remove(0),
            limits: axis_limits(),
            drives: drives(),
        },
        JointKind::Motion {
            motion: JointMotion::Rigid,
            axes: None,
            offset: offsets().remove(1),
            limits: Vec::new(),
            drives: Vec::new(),
        },
        JointKind::Coincident {
            offset: Some(literal(length(1.0))),
        },
        JointKind::Coincident { offset: None },
        JointKind::Concentric,
        JointKind::Insert {
            offset: Some(parameter(67)),
        },
        JointKind::Insert { offset: None },
        JointKind::Distance {
            distance: Some(literal(length(3.0))),
            limits: length_bounds().pop(),
        },
        JointKind::Distance {
            distance: None,
            limits: None,
        },
        JointKind::Angle {
            angle: Some(literal(angle(0.5))),
            limits: angle_bounds().pop(),
        },
        JointKind::Angle {
            angle: None,
            limits: None,
        },
        JointKind::Parallel,
        JointKind::Perpendicular,
        JointKind::Tangent {
            contact: Some(TangentContact::Inside),
        },
        JointKind::Tangent { contact: None },
        JointKind::PointOnAxis {
            distance: Some(literal(length(2.0))),
        },
        JointKind::PointOnAxis { distance: None },
        JointKind::Symmetric {
            plane: plane_construction(),
        },
        JointKind::Width {
            pair: Box::new([first, second]),
            mode: width_modes().pop(),
        },
        JointKind::Width {
            pair: Box::new([
                JointConnector::Entity(ordered(68)),
                JointConnector::Entity(ordered(69)),
            ]),
            mode: None,
        },
        JointKind::Slot {
            position: Some(SlotPosition::Centered),
            lock_rotation: Some(literal(true)),
        },
        JointKind::Slot {
            position: None,
            lock_rotation: None,
        },
        JointKind::Path {
            position: path_positions().pop(),
            pitch_yaw: pitch_yaws().pop(),
            roll: rolls().pop(),
        },
        JointKind::Path {
            position: None,
            pitch_yaw: None,
            roll: None,
        },
        JointKind::Coupling {
            coupling: couplings().remove(0),
            joints: Some(ordered(70)),
        },
        JointKind::Coupling {
            coupling: Coupling::Screw { pitch: None },
            joints: None,
        },
        JointKind::Native(text("creo:1450")),
    ]
}

#[test]
fn every_joint_type_round_trips() {
    let mut report = Report::default();
    report.check("JointConnector", connectors());
    report.check("Alignment", alignments());
    report.check("JointKind", kinds());
    report.check("JointMotion", motions());
    report.check("JointOffset", offsets());
    report.check("AxisLimits", axis_limits());
    report.check("Bounds<Length>", length_bounds());
    report.check("Bounds<Angle>", angle_bounds());
    report.check("Coupling", couplings());
    report.check("JointDrive", drives());
    report.check("DriveAxis", drive_axes());
    report.check("DriveMode", vec![DriveMode::Force, DriveMode::Acceleration]);
    report.check(
        "TangentContact",
        vec![TangentContact::Outside, TangentContact::Inside],
    );
    report.check("WidthMode", width_modes());
    report.check("SlotPosition", slot_positions());
    report.check("PathPosition", path_positions());
    report.check("PathAxisControl", pitch_yaws());
    report.check("PathRollControl", rolls());
    report.check("PairElement", vec![PairElement::First, PairElement::Second]);
    report.finish();
}

/// §2.5's tag forms: a unit-only enum writes a string, a unit arm of an
/// adjacently tagged enum is its tag alone, and a unit arm of an internally
/// tagged enum writes its tag and refuses another key.
#[test]
fn each_joint_enum_writes_its_tag_form() {
    assert_eq!(to_value(&PairElement::Second), json!("second"));
    assert_eq!(to_value(&DriveMode::Acceleration), json!("acceleration"));
    assert_eq!(
        to_value(&JointKind::Concentric),
        json!({"kind": "concentric"})
    );
    assert_eq!(to_value(&Alignment::Nearest), json!({"kind": "nearest"}));
    assert_eq!(
        to_value(&JointMotion::Revolute { axis: FrameAxis::Z }),
        json!({"kind": "revolute", "value": {"axis": "z"}})
    );
    assert_eq!(to_value(&PathPosition::Free), json!({"kind": "free"}));
    refuses::<PathPosition>(r#"{"kind": "free", "distance": 1}"#);
    refuses::<PathAxisControl>(r#"{"kind": "free", "axis": "x"}"#);
    refuses::<PathRollControl>(r#"{"kind": "free", "axis": "x"}"#);
    assert_eq!(
        to_value(&DriveAxis::Translation { axis: FrameAxis::X }),
        json!({"kind": "translation", "axis": "x"})
    );
    assert_eq!(
        to_value(&JointConnector::Entity(ordered(1)))["kind"],
        json!("entity")
    );
}

/// An unstated bound is omitted and refuses `null`; a coupling's radii are a
/// pair that writes `null` for an unstated radius.
#[test]
fn bounds_and_radii_state_only_what_the_source_states() {
    let empty = Bounds::<Length> {
        min: None,
        max: None,
    };
    assert_eq!(json::to_string(&empty).unwrap(), "{}");
    let message = refuses::<Bounds<Length>>(r#"{"min": null}"#);
    assert!(message.contains("min"), "{message}");
    let gear = Coupling::Gear {
        ratio: None,
        radii: [None, Some(literal(length(2.0)))],
    };
    assert_eq!(
        to_value(&gear),
        json!({"kind": "gear", "value": {"radii": [null, {"kind": "literal", "value": 2.0}]}})
    );
    refuses::<Coupling>(r#"{"kind": "gear", "value": {"radii": [null]}}"#);
}

/// A width joint's pair holds exactly two connectors.
#[test]
fn a_width_pair_holds_two_connectors() {
    let width = JointKind::Width {
        pair: Box::new([
            JointConnector::Entity(ordered(1)),
            JointConnector::Entity(ordered(2)),
        ]),
        mode: None,
    };
    let mut value = to_value(&width);
    assert_eq!(value["value"]["pair"].as_array().unwrap().len(), 2);
    value["value"]["pair"].as_array_mut().unwrap().pop();
    refuses::<JointKind>(&value.to_string());
}
