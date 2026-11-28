// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::f64::consts::{FRAC_PI_2, TAU};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::swept::{Extrude, ExtrudeDirection, Revolve};
use cadmpeg_ir::design::options::{Boolean, BooleanOp, SymmetricMeasure};

use crate::test_support::{extent_document, Member2d, ProfileFrame, Slot, Slots};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre or radian value with the hand-computed
/// one.
const EPS_VALUE: f64 = 1e-9;

const EXTRUDE: &str = "JMProtrusionFunction";
const CUTOUT: &str = "JMCutoutFunction";
const REVOLVE: &str = "JMRevolvedProtrusionFunction";

/// A profile on the XY plane with normal +Z.
const XY: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, 0.0, 1.0],
    x_axis: [1.0, 0.0, 0.0],
};

/// A profile on the XZ plane with normal -Y; its local y axis is +Z.
const XZ: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, -1.0, 0.0],
    x_axis: [1.0, 0.0, 0.0],
};

fn decode(bytes: Vec<u8>) -> DecodeResult {
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn codes(result: &DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect()
}

/// A square of side 10 mm on the profile plane.
fn square() -> Vec<Member2d> {
    vec![
        Member2d::Line([0.0, 0.0, 0.01, 0.0]),
        Member2d::Line([0.01, 0.0, 0.01, 0.01]),
        Member2d::Line([0.01, 0.01, 0.0, 0.01]),
        Member2d::Line([0.0, 0.01, 0.0, 0.0]),
    ]
}

/// A vertical segment 10 mm right of the local origin, 20 mm tall.
fn bar() -> Vec<Member2d> {
    vec![Member2d::Line([0.01, 0.0, 0.01, 0.02])]
}

fn extrusion(function: &str, kind: f64, side: f64, depth: Slot) -> DecodeResult {
    decode(extent_document(
        function,
        &XY,
        &square(),
        Slots {
            kind: Slot::Double(kind),
            side: Slot::Double(side),
            third: depth,
            angle: Slot::Empty,
        },
    ))
}

fn extrude_of(result: &DecodeResult) -> &Extrude {
    match &result.ir().design.features[0].definition.operation {
        FeatureOperation::Extrude(extrude) => extrude,
        other => panic!("expected an extrude, found {other:?}"),
    }
}

fn mm(limit: &Limit<cadmpeg_ir::scalar::Length>) -> f64 {
    match limit {
        Limit::Amount(Dimension::Literal(length)) => length.get(),
        other => panic!("expected a literal amount, found {other:?}"),
    }
}

fn reversed(extrude: &Extrude) -> bool {
    match &extrude.direction {
        Some(ExtrudeDirection::ProfileNormal {
            reversed: Some(Dimension::Literal(reversed)),
        }) => *reversed,
        other => panic!("expected a profile-normal direction, found {other:?}"),
    }
}

fn one_sided(extrude: &Extrude) -> &Limit<cadmpeg_ir::scalar::Length> {
    match extrude.span.as_ref().unwrap() {
        Span::Between { start, end } => {
            assert_eq!(**start, Limit::AtOrigin);
            end
        }
        other @ Span::Symmetric { .. } => panic!("expected a one-sided span, found {other:?}"),
    }
}

fn is_op(extrude: &Extrude, op: BooleanOp) -> bool {
    extrude.output.boolean == Some(Boolean::Apply { op, targets: None })
}

#[test]
fn finite_extrusion_runs_along_the_normal() {
    let result = extrusion(EXTRUDE, 13.0, 2.0, Slot::Dim(0.01));
    let extrude = extrude_of(&result);
    assert!(!reversed(extrude));
    assert!((mm(one_sided(extrude)) - 10.0).abs() <= EPS_VALUE);
    assert!(is_op(extrude, BooleanOp::Join));
    let found = codes(&result);
    assert!(!found.contains(&"feature.operands-pending".to_owned()));
    assert!(!found.contains(&"feature.extent-unread".to_owned()));
}

#[test]
fn depth_may_be_a_stored_number() {
    let result = extrusion(EXTRUDE, 13.0, 2.0, Slot::Double(0.02));
    assert!((mm(one_sided(extrude_of(&result))) - 20.0).abs() <= EPS_VALUE);
}

#[test]
fn side_one_reverses_the_extrusion() {
    let result = extrusion(EXTRUDE, 13.0, 1.0, Slot::Dim(0.01));
    let extrude = extrude_of(&result);
    assert!(reversed(extrude));
    assert!((mm(one_sided(extrude)) - 10.0).abs() <= EPS_VALUE);
}

#[test]
fn side_three_states_the_total_depth_symmetrically() {
    let result = extrusion(EXTRUDE, 13.0, 3.0, Slot::Dim(0.01));
    let extrude = extrude_of(&result);
    match extrude.span.as_ref().unwrap() {
        Span::Symmetric {
            amount: Dimension::Literal(amount),
            measure,
        } => {
            assert!((amount.get() - 10.0).abs() <= EPS_VALUE);
            assert_eq!(*measure, Some(SymmetricMeasure::Total));
        }
        other => panic!("expected a symmetric span, found {other:?}"),
    }
}

#[test]
fn through_all_cutout_cuts_through_all() {
    let result = extrusion(CUTOUT, 16.0, 2.0, Slot::Empty);
    let extrude = extrude_of(&result);
    assert_eq!(*one_sided(extrude), Limit::ThroughAll);
    assert!(is_op(extrude, BooleanOp::Cut));
}

#[test]
fn finite_cutout_against_the_normal() {
    let result = extrusion(CUTOUT, 13.0, 1.0, Slot::Dim(0.004));
    let extrude = extrude_of(&result);
    assert!(reversed(extrude));
    assert!((mm(one_sided(extrude)) - 4.0).abs() <= EPS_VALUE);
    assert!(is_op(extrude, BooleanOp::Cut));
}

fn revolution(side: f64, angle: Slot, axis_point: [f64; 3]) -> DecodeResult {
    revolution_of(&bar(), side, angle, axis_point)
}

fn revolution_of(
    members: &[Member2d],
    side: f64,
    angle: Slot,
    axis_point: [f64; 3],
) -> DecodeResult {
    decode(extent_document(
        REVOLVE,
        &XZ,
        members,
        Slots {
            kind: Slot::Double(13.0),
            side: Slot::Double(side),
            third: Slot::Axis {
                point: axis_point,
                direction: [0.0, 0.0, 1.0],
            },
            angle,
        },
    ))
}

fn revolve_of(result: &DecodeResult) -> &Revolve {
    match &result.ir().design.features[0].definition.operation {
        FeatureOperation::Revolve(revolve) => revolve,
        other => panic!("expected a revolve, found {other:?}"),
    }
}

fn sweep(revolve: &Revolve) -> f64 {
    match revolve.span.as_ref().unwrap() {
        Span::Between { start, end } => {
            assert_eq!(**start, Limit::AtOrigin);
            match &**end {
                Limit::Amount(Dimension::Literal(angle)) => angle.get(),
                other => panic!("expected a literal angle, found {other:?}"),
            }
        }
        other @ Span::Symmetric { .. } => panic!("expected a span between limits, found {other:?}"),
    }
}

#[test]
fn full_turn_about_z_through_the_origin() {
    let result = revolution(2.0, Slot::Dim(TAU), [0.0; 3]);
    let revolve = revolve_of(&result);
    // The bar lies at +x of the axis. A positive turn about +Z moves it
    // towards +y, and the profile normal is -y, so the sweep is negative.
    assert!((sweep(revolve) + TAU).abs() <= EPS_VALUE);
    let axis = revolve.axis.as_ref().unwrap().resolved.as_ref().unwrap();
    assert!(axis.origin.get().x.abs() <= EPS_VALUE);
    assert!((cadmpeg_ir::math::Vector3::from(axis.direction).z - 1.0).abs() <= EPS_VALUE);
    assert!(!codes(&result).contains(&"feature.operands-pending".to_owned()));
}

#[test]
fn quarter_turn_sides_have_opposite_signs() {
    let towards = revolution(2.0, Slot::Dim(FRAC_PI_2), [0.0; 3]);
    let away = revolution(1.0, Slot::Double(FRAC_PI_2), [0.0; 3]);
    assert!((sweep(revolve_of(&towards)) + FRAC_PI_2).abs() <= EPS_VALUE);
    assert!((sweep(revolve_of(&away)) - FRAC_PI_2).abs() <= EPS_VALUE);
}

#[test]
fn axis_point_is_stated_in_millimetres_and_sets_the_sense() {
    // The axis stands at x = 20 mm, so the bar at x = 10 mm lies at -x of it
    // and the sense of a turn towards the normal reverses.
    let result = revolution(2.0, Slot::Dim(FRAC_PI_2), [0.02, 0.0, 0.0]);
    let revolve = revolve_of(&result);
    let axis = revolve.axis.as_ref().unwrap().resolved.as_ref().unwrap();
    assert!((axis.origin.get().x - 20.0).abs() <= EPS_VALUE);
    assert!((sweep(revolve) - FRAC_PI_2).abs() <= EPS_VALUE);
}

#[test]
fn symmetric_revolve_states_the_total_angle() {
    let result = revolution(3.0, Slot::Dim(FRAC_PI_2), [0.0; 3]);
    match revolve_of(&result).span.as_ref().unwrap() {
        Span::Symmetric {
            amount: Dimension::Literal(amount),
            measure,
        } => {
            assert!((amount.get() - FRAC_PI_2).abs() <= EPS_VALUE);
            assert_eq!(*measure, Some(SymmetricMeasure::Total));
        }
        other => panic!("expected a symmetric span, found {other:?}"),
    }
}

#[test]
fn profile_on_the_axis_stays_unresolved() {
    // The bar lies on the axis through x = 10 mm.
    let result = revolution(2.0, Slot::Dim(FRAC_PI_2), [0.01, 0.0, 0.0]);
    assert!(matches!(
        &result.ir().design.features[0].definition.operation,
        FeatureOperation::Native { native_options, .. }
            if native_options.keys().any(|key| key.as_str() == "solidedge:profile")
    ));
    assert!(codes(&result).contains(&"feature.extent-unread".to_owned()));
}

#[test]
fn unknown_extent_kind_stays_unresolved_with_a_loss() {
    let result = extrusion(EXTRUDE, 99.0, 2.0, Slot::Dim(0.01));
    assert!(matches!(
        &result.ir().design.features[0].definition.operation,
        FeatureOperation::Native { native_options, .. }
            if native_options.keys().any(|key| key.as_str() == "solidedge:profile")
    ));
    let notes: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "feature.extent-unread")
        .collect();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].message.contains("0xa"), "{}", notes[0].message);
    assert!(codes(&result).contains(&"feature.operands-pending".to_owned()));
}

#[test]
fn unresolved_depth_slot_stays_unresolved() {
    let result = extrusion(EXTRUDE, 13.0, 2.0, Slot::Empty);
    assert!(matches!(
        &result.ir().design.features[0].definition.operation,
        FeatureOperation::Native { native_options, .. }
            if native_options.keys().any(|key| key.as_str() == "solidedge:profile")
    ));
    assert!(codes(&result).contains(&"feature.extent-unread".to_owned()));
}

#[test]
fn arc_crossing_the_axis_between_its_ends_stays_unresolved() {
    // The arc is centred 10 mm right of the axis with radius 15 mm and runs
    // from 30 to 330 degrees. Both ends lie right of the axis; the arc
    // passes 5 mm left of it at 180 degrees.
    let arc = [Member2d::Arc([
        0.01,
        0.0,
        0.015,
        30_f64.to_radians(),
        330_f64.to_radians(),
    ])];
    let result = revolution_of(&arc, 2.0, Slot::Dim(FRAC_PI_2), [0.0; 3]);
    assert!(!matches!(
        &result.ir().design.features[0].definition.operation,
        FeatureOperation::Revolve(_)
    ));
    assert!(codes(&result).contains(&"feature.extent-unread".to_owned()));
}

#[test]
fn arc_clear_of_the_axis_keeps_its_sense() {
    // The same arc 30 mm right of the axis stays right of it.
    let arc = [Member2d::Arc([
        0.03,
        0.0,
        0.015,
        30_f64.to_radians(),
        330_f64.to_radians(),
    ])];
    let result = revolution_of(&arc, 2.0, Slot::Dim(FRAC_PI_2), [0.0; 3]);
    assert!((sweep(revolve_of(&result)) + FRAC_PI_2).abs() <= EPS_VALUE);
}

#[test]
fn circle_crossing_the_axis_stays_unresolved() {
    // A 10 mm circle centred 9 mm right of the axis passes 1 mm left of it.
    let circle = [Member2d::Circle([0.009, 0.0, 0.01])];
    let result = revolution_of(&circle, 2.0, Slot::Dim(FRAC_PI_2), [0.0; 3]);
    assert!(codes(&result).contains(&"feature.extent-unread".to_owned()));
}
