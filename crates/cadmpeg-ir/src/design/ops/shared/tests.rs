// SPDX-License-Identifier: Apache-2.0
//! Every one of the shared operation types, written, read back strictly, and
//! written again; the two texts are equal. Their stated wire forms, and
//! their schemas.

pub(in crate::design::ops) mod fixtures;
use std::collections::BTreeMap;
use std::path::Path;

use cadmpeg_core::json;

use super::*;
use crate::design::ops::sketch::{TextAlign, TextContent, TextHorizontalBox, TextVerticalBox};
use crate::design::tests::unit_variants::{assert_units_refuse_value, reads, Reader};
use crate::scalar::PositiveLength;
use fixtures::*;

pub(in crate::design::ops) fn text_content_samples() -> Vec<TextContent> {
    vec![
        TextContent {
            content: text("codec:native"),
            font: Some(text("codec:native")),
            height: Some(lit(length(-1.5))),
            width_factor: Some(lit(real(0.44))),
            angle: Some(bound(1)),
            horizontal: Some(text_align_samples().remove(0)),
            vertical: Some(text_align_samples().remove(0)),
            horizontal_box: Some(text_horizontal_box_samples().remove(0)),
            vertical_box: Some(text_vertical_box_samples().remove(0)),
            tracking: Some(lit(length(-1.5))),
            oblique: Some(bound(1)),
            filled: Some(lit(true)),
        },
        TextContent {
            content: text("codec:native"),
            font: None,
            height: None,
            width_factor: None,
            angle: None,
            horizontal: None,
            vertical: None,
            horizontal_box: None,
            vertical_box: None,
            tracking: None,
            oblique: None,
            filled: None,
        },
    ]
}

pub(in crate::design::ops) fn text_align_samples() -> Vec<TextAlign> {
    vec![
        TextAlign::Start,
        TextAlign::Center,
        TextAlign::End,
        TextAlign::Baseline,
        TextAlign::Native(text("codec:native")),
    ]
}

pub(in crate::design::ops) fn text_horizontal_box_samples() -> Vec<TextHorizontalBox> {
    vec![TextHorizontalBox::Ink, TextHorizontalBox::Pen]
}

pub(in crate::design::ops) fn text_vertical_box_samples() -> Vec<TextVerticalBox> {
    vec![TextVerticalBox::Ink, TextVerticalBox::CapHeight]
}

pub(in crate::design::ops) fn tool_disposition_samples() -> Vec<ToolDisposition> {
    vec![
        ToolDisposition::Kept,
        ToolDisposition::Consumed,
        ToolDisposition::Absorbed,
    ]
}

pub(in crate::design::ops) fn pair_element_samples() -> Vec<PairElement> {
    vec![PairElement::First, PairElement::Second]
}

pub(in crate::design::ops) fn sheet_cut_normal_samples() -> Vec<SheetCutNormal> {
    vec![
        SheetCutNormal::Defining,
        SheetCutNormal::Opposite,
        SheetCutNormal::Both,
    ]
}

pub(in crate::design::ops) fn joint_samples() -> Vec<Joint> {
    vec![
        Joint {
            connectors: [
                joint_connector_samples().remove(0),
                joint_connector_samples().remove(0),
            ],
            kind: joint_kind_samples().remove(0),
            alignment: Some(alignment_samples().remove(0)),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Joint {
            connectors: [
                joint_connector_samples().remove(0),
                joint_connector_samples().remove(0),
            ],
            kind: joint_kind_samples().remove(0),
            alignment: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

pub(in crate::design::ops) fn joint_connector_samples() -> Vec<JointConnector> {
    vec![
        JointConnector::Frame(frame()),
        JointConnector::Plane(plane()),
        JointConnector::Axis(axis()),
        JointConnector::Point(point_construction()),
        JointConnector::Entity(selection(2)),
    ]
}

pub(in crate::design::ops) fn alignment_samples() -> Vec<Alignment> {
    vec![
        Alignment::Aligned,
        Alignment::Opposed,
        Alignment::Nearest,
        Alignment::Native(text("codec:native")),
    ]
}

pub(in crate::design::ops) fn joint_kind_samples() -> Vec<JointKind> {
    vec![
        JointKind::Motion {
            motion: joint_motion_samples().remove(0),
            axes: Some(Box::new(frame())),
            offset: joint_offset_samples().remove(0),
            limits: vec![axis_limits_samples().remove(0)],
            drives: vec![joint_drive_samples().remove(0)],
        },
        JointKind::Motion {
            motion: joint_motion_samples().remove(0),
            axes: None,
            offset: joint_offset_samples().remove(0),
            limits: Vec::new(),
            drives: Vec::new(),
        },
        JointKind::Coincident {
            offset: Some(lit(length(-1.5))),
        },
        JointKind::Coincident { offset: None },
        JointKind::Concentric,
        JointKind::Insert {
            offset: Some(lit(length(-1.5))),
        },
        JointKind::Insert { offset: None },
        JointKind::Distance {
            distance: Some(lit(length(-1.5))),
            limits: Some(bounds_length_samples().remove(0)),
        },
        JointKind::Distance {
            distance: None,
            limits: None,
        },
        JointKind::Angle {
            angle: Some(bound(1)),
            limits: Some(bounds_angle_samples().remove(0)),
        },
        JointKind::Angle {
            angle: None,
            limits: None,
        },
        JointKind::Parallel,
        JointKind::Perpendicular,
        JointKind::Tangent {
            contact: Some(tangent_contact_samples().remove(0)),
        },
        JointKind::Tangent { contact: None },
        JointKind::PointOnAxis {
            distance: Some(lit(length(-1.5))),
        },
        JointKind::PointOnAxis { distance: None },
        JointKind::Symmetric { plane: plane() },
        JointKind::Width {
            pair: Box::new([
                joint_connector_samples().remove(0),
                joint_connector_samples().remove(0),
            ]),
            mode: Some(width_mode_samples().remove(0)),
        },
        JointKind::Width {
            pair: Box::new([
                joint_connector_samples().remove(0),
                joint_connector_samples().remove(0),
            ]),
            mode: None,
        },
        JointKind::Slot {
            position: Some(slot_position_samples().remove(0)),
            lock_rotation: Some(lit(true)),
        },
        JointKind::Slot {
            position: None,
            lock_rotation: None,
        },
        JointKind::Path {
            position: Some(path_position_samples().remove(0)),
            pitch_yaw: Some(path_axis_control_samples().remove(0)),
            roll: Some(path_roll_control_samples().remove(0)),
        },
        JointKind::Path {
            position: None,
            pitch_yaw: None,
            roll: None,
        },
        JointKind::Coupling {
            coupling: coupling_samples().remove(0),
            joints: Some(selection(2)),
        },
        JointKind::Coupling {
            coupling: coupling_samples().remove(0),
            joints: None,
        },
        JointKind::Native(text("codec:native")),
    ]
}

pub(in crate::design::ops) fn joint_motion_samples() -> Vec<JointMotion> {
    vec![
        JointMotion::Rigid,
        JointMotion::Revolute { axis: FrameAxis::Z },
        JointMotion::Slider { axis: FrameAxis::Z },
        JointMotion::Cylindrical { axis: FrameAxis::Z },
        JointMotion::Planar { axis: FrameAxis::Z },
        JointMotion::Ball,
        JointMotion::PinSlot {
            axis: FrameAxis::Z,
            slide: FrameAxis::Z,
        },
        JointMotion::Native(text("codec:native")),
    ]
}

pub(in crate::design::ops) fn joint_offset_samples() -> Vec<JointOffset> {
    vec![
        JointOffset {
            x: Some(lit(length(-1.5))),
            y: Some(lit(length(-1.5))),
            z: Some(lit(length(-1.5))),
            angle: Some(bound(1)),
        },
        JointOffset {
            x: None,
            y: None,
            z: None,
            angle: None,
        },
    ]
}

pub(in crate::design::ops) fn axis_limits_samples() -> Vec<AxisLimits> {
    vec![
        AxisLimits::Rotation {
            axis: FrameAxis::Z,
            limits: Some(bounds_angle_samples().remove(0)),
            rest: Some(bound(1)),
        },
        AxisLimits::Rotation {
            axis: FrameAxis::Z,
            limits: None,
            rest: None,
        },
        AxisLimits::Translation {
            axis: FrameAxis::Z,
            limits: Some(bounds_length_samples().remove(0)),
            rest: Some(lit(length(-1.5))),
        },
        AxisLimits::Translation {
            axis: FrameAxis::Z,
            limits: None,
            rest: None,
        },
    ]
}

pub(in crate::design::ops) fn coupling_samples() -> Vec<Coupling> {
    vec![
        Coupling::Gear {
            ratio: Some(lit(real(0.44))),
            radii: [Some(lit(length(-1.5))), Some(lit(length(-1.5)))],
        },
        Coupling::Gear {
            ratio: None,
            radii: [None, None],
        },
        Coupling::Belt {
            ratio: Some(lit(real(0.44))),
            radii: [Some(lit(length(-1.5))), Some(lit(length(-1.5)))],
        },
        Coupling::Belt {
            ratio: None,
            radii: [None, None],
        },
        Coupling::RackPinion {
            ratio: Some(lit(real(0.44))),
            pitch_radius: Some(lit(length(-1.5))),
        },
        Coupling::RackPinion {
            ratio: None,
            pitch_radius: None,
        },
        Coupling::Screw {
            pitch: Some(lit(length(-1.5))),
        },
        Coupling::Screw { pitch: None },
        Coupling::Native(text("codec:native")),
    ]
}

pub(in crate::design::ops) fn joint_drive_samples() -> Vec<JointDrive> {
    vec![
        JointDrive {
            axis: drive_axis_samples().remove(0),
            mode: drive_mode_samples().remove(0),
            target_position: Some(real(0.4)),
            target_velocity: Some(real(0.4)),
            stiffness: Some(nonnegative_real(0.5)),
            damping: Some(nonnegative_real(0.5)),
            max_force: Some(positive_real(2.0)),
        },
        JointDrive {
            axis: drive_axis_samples().remove(0),
            mode: drive_mode_samples().remove(0),
            target_position: None,
            target_velocity: None,
            stiffness: None,
            damping: None,
            max_force: None,
        },
    ]
}

pub(in crate::design::ops) fn drive_axis_samples() -> Vec<DriveAxis> {
    vec![
        DriveAxis::Rotation { axis: FrameAxis::Z },
        DriveAxis::Translation { axis: FrameAxis::Z },
    ]
}

pub(in crate::design::ops) fn drive_mode_samples() -> Vec<DriveMode> {
    vec![DriveMode::Force, DriveMode::Acceleration]
}

pub(in crate::design::ops) fn tangent_contact_samples() -> Vec<TangentContact> {
    vec![TangentContact::Outside, TangentContact::Inside]
}

pub(in crate::design::ops) fn width_mode_samples() -> Vec<WidthMode> {
    vec![
        WidthMode::Centered,
        WidthMode::Free,
        WidthMode::Gap {
            distance: lit(nonnegative(0.5)),
            from: Some(pair_element_samples().remove(0)),
        },
        WidthMode::Gap {
            distance: lit(nonnegative(0.5)),
            from: None,
        },
        WidthMode::Native(text("codec:native")),
    ]
}

pub(in crate::design::ops) fn slot_position_samples() -> Vec<SlotPosition> {
    vec![
        SlotPosition::Free,
        SlotPosition::Centered,
        SlotPosition::Native(text("codec:native")),
    ]
}

pub(in crate::design::ops) fn path_position_samples() -> Vec<PathPosition> {
    vec![
        PathPosition::Free,
        PathPosition::Distance {
            distance: lit(nonnegative(0.5)),
            from_end: Some(lit(true)),
        },
        PathPosition::Distance {
            distance: lit(nonnegative(0.5)),
            from_end: None,
        },
        PathPosition::Percent {
            fraction: lit(fraction(0.25)),
            from_end: Some(lit(true)),
        },
        PathPosition::Percent {
            fraction: lit(fraction(0.25)),
            from_end: None,
        },
    ]
}

pub(in crate::design::ops) fn path_axis_control_samples() -> Vec<PathAxisControl> {
    vec![
        PathAxisControl::Free,
        PathAxisControl::FollowPath {
            axis: FrameAxis::Z,
            flip: Some(lit(true)),
        },
        PathAxisControl::FollowPath {
            axis: FrameAxis::Z,
            flip: None,
        },
    ]
}

pub(in crate::design::ops) fn path_roll_control_samples() -> Vec<PathRollControl> {
    vec![
        PathRollControl::Free,
        PathRollControl::UpVector {
            vector: Box::new(direction()),
            axis: FrameAxis::Z,
            flip: Some(lit(true)),
        },
        PathRollControl::UpVector {
            vector: Box::new(direction()),
            axis: FrameAxis::Z,
            flip: None,
        },
    ]
}

pub(in crate::design::ops) fn radial_dimension_positive_length_samples(
) -> Vec<RadialDimension<PositiveLength>> {
    vec![
        RadialDimension::Radius(lit(positive(2.0))),
        RadialDimension::Diameter(lit(positive(2.0))),
    ]
}

pub(in crate::design::ops) fn law_length_samples() -> Vec<Law<Length>> {
    vec![
        Law::Constant(lit(length(-1.5))),
        Law::Linear {
            start: lit(length(-1.5)),
            end: lit(length(-1.5)),
        },
        Law::Curve(selection(2)),
        Law::Expression(expression()),
        Law::Native(text("codec:native")),
    ]
}

pub(in crate::design::ops) fn bounds_length_samples() -> Vec<Bounds<Length>> {
    vec![
        Bounds::<Length> {
            min: Some(lit(length(-1.5))),
            max: Some(lit(length(-1.5))),
        },
        Bounds::<Length> {
            min: None,
            max: None,
        },
    ]
}

pub(in crate::design::ops) fn bounds_angle_samples() -> Vec<Bounds<Angle>> {
    vec![
        Bounds::<Angle> {
            min: Some(bound(1)),
            max: Some(bound(1)),
        },
        Bounds::<Angle> {
            min: None,
            max: None,
        },
    ]
}

pub(in crate::design::ops) fn law_fraction_samples() -> Vec<Law<Fraction>> {
    vec![
        Law::Constant(lit(fraction(0.25))),
        Law::Linear {
            start: lit(fraction(0.25)),
            end: lit(fraction(0.25)),
        },
        Law::Curve(selection(2)),
        Law::Expression(expression()),
        Law::Native(text("codec:native")),
    ]
}

pub(in crate::design::ops) fn law_angle_samples() -> Vec<Law<Angle>> {
    vec![
        Law::Constant(bound(1)),
        Law::Linear {
            start: bound(1),
            end: bound(1),
        },
        Law::Curve(selection(2)),
        Law::Expression(expression()),
        Law::Native(text("codec:native")),
    ]
}

pub(in crate::design::ops) fn span_length_samples() -> Vec<Span<Length>> {
    vec![
        Span::Between {
            start: Box::new(limit_length_samples().remove(0)),
            end: Box::new(limit_length_samples().remove(0)),
        },
        Span::Symmetric {
            amount: lit(length(-1.5)),
            measure: Some(SymmetricMeasure::PerSide),
        },
        Span::Symmetric {
            amount: lit(length(-1.5)),
            measure: None,
        },
    ]
}

pub(in crate::design::ops) fn limit_length_samples() -> Vec<Limit<Length>> {
    vec![
        Limit::AtOrigin,
        Limit::Amount(lit(length(-1.5))),
        Limit::ThroughAll,
        Limit::Next,
        Limit::Last,
        Limit::ToGeometry {
            target: selection(1),
            offset: Some(LimitOffset::Translated(lit(length(0.5)))),
            extend: Some(lit(true)),
            far: Some(lit(true)),
        },
        Limit::ToGeometry {
            target: selection(1),
            offset: None,
            extend: None,
            far: None,
        },
        Limit::ToPlane(plane()),
        Limit::ToPoint(point_construction()),
        Limit::Native(text("codec:native")),
    ]
}

#[test]
fn every_shared_type_round_trips() {
    let mut report = Report::default();
    report.check("TextContent", text_content_samples());
    report.check("TextAlign", text_align_samples());
    report.check("TextHorizontalBox", text_horizontal_box_samples());
    report.check("TextVerticalBox", text_vertical_box_samples());
    report.check("ToolDisposition", tool_disposition_samples());
    report.check("PairElement", pair_element_samples());
    report.check("SheetCutNormal", sheet_cut_normal_samples());
    report.check("Joint", joint_samples());
    report.check("JointConnector", joint_connector_samples());
    report.check("Alignment", alignment_samples());
    report.check("JointKind", joint_kind_samples());
    report.check("JointMotion", joint_motion_samples());
    report.check("JointOffset", joint_offset_samples());
    report.check("AxisLimits", axis_limits_samples());
    report.check("Coupling", coupling_samples());
    report.check("JointDrive", joint_drive_samples());
    report.check("DriveAxis", drive_axis_samples());
    report.check("DriveMode", drive_mode_samples());
    report.check("TangentContact", tangent_contact_samples());
    report.check("WidthMode", width_mode_samples());
    report.check("SlotPosition", slot_position_samples());
    report.check("PathPosition", path_position_samples());
    report.check("PathAxisControl", path_axis_control_samples());
    report.check("PathRollControl", path_roll_control_samples());
    report.check(
        "RadialDimension<PositiveLength>",
        radial_dimension_positive_length_samples(),
    );
    report.check("Law<Length>", law_length_samples());
    report.check("Bounds<Length>", bounds_length_samples());
    report.check("Bounds<Angle>", bounds_angle_samples());
    report.check("Law<Fraction>", law_fraction_samples());
    report.check("Law<Angle>", law_angle_samples());
    report.check("Span<Length>", span_length_samples());
    report.check("Limit<Length>", limit_length_samples());
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

/// A joint's connectors are a fixed pair, and three refuse.
#[test]
fn joint_connectors_are_a_fixed_pair() {
    let written = to_value(&joint_samples().remove(0));
    assert_eq!(written["connectors"].as_array().unwrap().len(), 2);
    let mut three = written.clone();
    let first = three["connectors"][0].clone();
    three["connectors"].as_array_mut().unwrap().push(first);
    refuses::<Joint>(&three.to_string());
}

/// Each shared enum writes its stated tag form, an option field is omitted
/// for `None`, and `null` refuses.
#[test]
fn each_shared_enum_has_its_tag_form() {
    assert_eq!(to_value(&PairElement::Second), json!("second"));
    assert_eq!(to_value(&SheetCutNormal::Both), json!("both"));
    assert_eq!(to_value(&ToolDisposition::Absorbed), json!("absorbed"));
    assert_eq!(to_value(&Alignment::Opposed), json!({"kind": "opposed"}));
    assert_eq!(
        to_value(&JointKind::Concentric),
        json!({"kind": "concentric"})
    );
    assert_eq!(
        to_value(&JointKind::Coincident { offset: None }),
        json!({"kind": "coincident", "value": {}})
    );
    assert_eq!(
        to_value(&DriveAxis::Rotation { axis: FrameAxis::Z }),
        json!({"kind": "rotation", "axis": "z"})
    );
    assert_eq!(
        to_value(&WidthMode::Gap {
            distance: lit(nonnegative(1.0)),
            from: Some(PairElement::First),
        }),
        json!({
            "kind": "gap",
            "value": {"distance": {"kind": "literal", "value": 1.0}, "from": "first"}
        })
    );
    assert_eq!(
        to_value(&Bounds::<Length> {
            min: None,
            max: None
        }),
        json!({})
    );
    refuses::<Bounds<Length>>(r#"{"min": null}"#);
    refuses::<JointKind>(r#"{"kind": "coincident", "value": {"offset": null}}"#);
    refuses::<TextContent>(r#"{"content": "A", "font": null}"#);
    refuses::<TextContent>(r#"{"content": "A", "size": 1.0}"#);
}

/// §2.5: each value has one spelling, so a unit variant of an adjacently
/// tagged enum is its `kind` alone and refuses a stated `value`.
#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let readers: [(&'static str, Reader); 10] = [
        ("Alignment", reads::<Alignment>),
        ("InstanceOrientation", reads::<InstanceOrientation>),
        ("Interpolation", reads::<Interpolation>),
        ("JointKind", reads::<JointKind>),
        ("JointMotion", reads::<JointMotion>),
        ("Limit", reads::<Limit<Length>>),
        ("OffsetJoin", reads::<OffsetJoin>),
        ("RunSides", reads::<RunSides<Length>>),
        ("SlotPosition", reads::<SlotPosition>),
        ("WidthMode", reads::<WidthMode>),
    ];
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/design/ops/shared.rs");
    assert_units_refuse_value(&[file], &BTreeMap::from(readers));
}

/// An internally tagged unit variant is its `kind` alone; a key beside it
/// refuses.
#[test]
fn internal_unit_variants_are_their_kind_alone() {
    for extra in [r#""value": null"#, r#""value": {}"#, r#""x": 1"#] {
        refuses::<PathPosition>(&format!(r#"{{"kind": "free", {extra}}}"#));
        refuses::<PathAxisControl>(&format!(r#"{{"kind": "free", {extra}}}"#));
        refuses::<PathRollControl>(&format!(r#"{{"kind": "free", {extra}}}"#));
    }
}
