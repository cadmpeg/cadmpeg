// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::f64::consts::{FRAC_PI_3, FRAC_PI_6, PI, TAU};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::body::{Pattern, PatternInstances, PatternSeeds};
use cadmpeg_ir::design::ops::shared::{
    CircularLayout, LinearLayout, MirrorLayout, PatternLayout, PatternRun, Spacing,
};
use cadmpeg_ir::design::selection::{PickTarget, Picks};

use crate::features::feature_id;
use crate::test_support::{
    mirror_document, pattern_document, Member2d, MirrorSpec, PatternSpec, ProfileFrame,
};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre or radian value with the hand-computed
/// one.
const EPS_VALUE: f64 = 1e-9;

/// A profile on the XY plane with normal +Z.
const XY: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, 0.0, 1.0],
    x_axis: [1.0, 0.0, 0.0],
};

/// A profile on the XY plane whose local x axis is +Y, so that its local y
/// axis is -X.
const TURNED: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, 0.0, 1.0],
    x_axis: [0.0, 1.0, 0.0],
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

/// The operation of the group, which is object 11.
fn operation(result: &DecodeResult) -> &FeatureOperation {
    let id = feature_id(11);
    &result
        .ir()
        .design
        .features
        .iter()
        .find(|feature| feature.id == id)
        .unwrap()
        .definition
        .operation
}

fn pattern(result: &DecodeResult) -> &Pattern {
    match operation(result) {
        FeatureOperation::Pattern(pattern) => pattern,
        other => panic!("expected a pattern, found {other:?}"),
    }
}

fn unresolved(result: &DecodeResult) -> bool {
    operation(result)
        == &FeatureOperation::Unresolved {
            family: OperationFamily::Pattern,
        }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS_VALUE
}

fn rectangular(
    frame: &ProfileFrame,
    counts: [u32; 2],
    steps: [f64; 2],
    lines: [[f64; 4]; 2],
) -> DecodeResult {
    decode(pattern_document(&PatternSpec {
        frame,
        seed: 10,
        kind: 1,
        counts,
        steps,
        geometry: &[Member2d::Line(lines[0]), Member2d::Line(lines[1])],
        guid: true,
    }))
}

fn circular(counts: u32, step: f64, centre: [f64; 2]) -> DecodeResult {
    decode(pattern_document(&PatternSpec {
        frame: &XY,
        seed: 10,
        kind: 2,
        counts: [counts, 0],
        steps: [step, 0.0],
        geometry: &[Member2d::Arc([centre[0], centre[1], 0.01, 0.0, TAU])],
        guid: true,
    }))
}

/// The count and the pitch of a length run.
fn length_run(run: &PatternRun<cadmpeg_ir::scalar::Length>) -> (u32, f64) {
    match &run.spacing {
        Spacing::Pitch {
            count: Dimension::Literal(count),
            pitch: Dimension::Literal(pitch),
        } => (*count, pitch.get()),
        other => panic!("expected a literal pitch, found {other:?}"),
    }
}

fn direction(
    construction: &cadmpeg_ir::design::construction::Construction<
        cadmpeg_ir::design::construction::kinds::Direction,
    >,
) -> [f64; 3] {
    let value = construction.resolved.as_ref().unwrap().as_raw();
    [value.x, value.y, value.z]
}

fn near3(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| near(*a, b))
}

fn assert_seed(pattern: &Pattern) {
    let PatternSeeds::Features { features, .. } = &pattern.seeds else {
        panic!("expected feature seeds");
    };
    let Picks::Named(members) = &features.picks else {
        panic!("expected named picks");
    };
    let picks = members.as_slice();
    assert_eq!(picks.len(), 1);
    assert_eq!(picks[0].target, PickTarget::FeatureResult(feature_id(10)));
    assert_eq!(pattern.instances, PatternInstances::Implicit(Vec::new()));
}

#[test]
fn rectangular_three_by_two_along_x_and_y() {
    let result = rectangular(
        &XY,
        [3, 2],
        [0.010, 0.008],
        [[0.005, 0.005, 0.035, 0.005], [0.005, 0.005, 0.005, 0.021]],
    );
    let pattern = pattern(&result);
    assert_seed(pattern);
    let PatternLayout::Linear(LinearLayout {
        first,
        second: Some(second),
        seed_only,
    }) = &pattern.layout
    else {
        panic!("expected two linear runs, found {:?}", pattern.layout);
    };
    assert_eq!(seed_only, &Some(Dimension::Literal(false)));
    assert!(near3(direction(&first.direction), [1.0, 0.0, 0.0]));
    assert!(near3(direction(&second.direction), [0.0, 1.0, 0.0]));
    let (count, pitch) = length_run(&first.run);
    assert_eq!(count, 3);
    assert!(near(pitch, 10.0));
    let (count, pitch) = length_run(&second.run);
    assert_eq!(count, 2);
    assert!(near(pitch, 8.0));
    assert!(!codes(&result).contains(&"feature.pattern-unread".to_owned()));
}

#[test]
fn rectangular_four_by_two_through_a_turned_frame() {
    let result = rectangular(
        &TURNED,
        [4, 2],
        [0.012, 0.006],
        [[0.0, 0.0, 0.036, 0.0], [0.0, 0.0, 0.0, 0.006]],
    );
    let pattern = pattern(&result);
    let PatternLayout::Linear(LinearLayout {
        first,
        second: Some(second),
        ..
    }) = &pattern.layout
    else {
        panic!("expected two linear runs");
    };
    // Local x is +Y; local y is A x B = Z x Y = -X.
    assert!(near3(direction(&first.direction), [0.0, 1.0, 0.0]));
    assert!(near3(direction(&second.direction), [-1.0, 0.0, 0.0]));
    let (count, pitch) = length_run(&first.run);
    assert_eq!(count, 4);
    assert!(near(pitch, 12.0));
    let (count, pitch) = length_run(&second.run);
    assert_eq!(count, 2);
    assert!(near(pitch, 6.0));
}

#[test]
fn circular_full_turn_has_no_sense_note() {
    let result = circular(6, FRAC_PI_3, [0.020, 0.015]);
    let pattern = pattern(&result);
    assert_seed(pattern);
    let PatternLayout::Circular(CircularLayout { axis, angular, .. }) = &pattern.layout else {
        panic!("expected a circular layout, found {:?}", pattern.layout);
    };
    let axis = axis.resolved.as_ref().unwrap();
    let origin = axis.origin.get();
    assert!(near(origin.x, 20.0) && near(origin.y, 15.0) && near(origin.z, 0.0));
    let direction = axis.direction.as_raw();
    assert!(near3(
        [direction.x, direction.y, direction.z],
        [0.0, 0.0, 1.0]
    ));
    match &angular.spacing {
        Spacing::Pitch {
            count: Dimension::Literal(count),
            pitch: Dimension::Literal(step),
        } => {
            assert_eq!(*count, 6);
            assert!(near(step.get(), FRAC_PI_3));
        }
        other => panic!("expected a literal pitch, found {other:?}"),
    }
    assert!(!codes(&result).contains(&"feature.pattern-sense-unread".to_owned()));
}

#[test]
fn circular_partial_turn_has_no_sense_note() {
    let result = circular(3, FRAC_PI_6, [0.020, 0.015]);
    let PatternLayout::Circular(CircularLayout { axis, angular, .. }) = &pattern(&result).layout
    else {
        panic!("expected a circular layout");
    };
    // Instances turn counter-clockwise about the profile normal, which is a
    // positive step about the axis direction +Z.
    let direction = axis.resolved.as_ref().unwrap().direction.as_raw();
    assert!(near3(
        [direction.x, direction.y, direction.z],
        [0.0, 0.0, 1.0]
    ));
    match &angular.spacing {
        Spacing::Pitch {
            count: Dimension::Literal(count),
            pitch: Dimension::Literal(step),
        } => {
            assert_eq!(*count, 3);
            assert!(near(step.get(), FRAC_PI_6));
        }
        other => panic!("expected a literal pitch, found {other:?}"),
    }
    assert!(!codes(&result).contains(&"feature.pattern-sense-unread".to_owned()));
}

fn plane_of(result: &DecodeResult) -> ([f64; 3], [f64; 3]) {
    let PatternLayout::Mirror(MirrorLayout { plane, .. }) = &pattern(result).layout else {
        panic!("expected a mirror layout");
    };
    let plane = plane.resolved.as_ref().unwrap();
    let origin = plane.origin.get();
    let normal = plane.normal.as_raw();
    // The in-plane axis is a unit vector perpendicular to the normal.
    let x = plane.x_axis.as_raw();
    assert!(near(x.dot(*normal), 0.0));
    (
        [origin.x, origin.y, origin.z],
        [normal.x, normal.y, normal.z],
    )
}

#[test]
fn mirror_about_x_20() {
    let result = decode(mirror_document(&MirrorSpec {
        count: 1,
        seeds: &[10],
        plane: Some(([0.020, 0.0, 0.0], [1.0, 0.0, 0.0])),
    }));
    assert_seed(pattern(&result));
    let (origin, normal) = plane_of(&result);
    assert!(near3(origin, [20.0, 0.0, 0.0]));
    assert!(near3(normal, [1.0, 0.0, 0.0]));
}

#[test]
fn mirror_about_y_15_with_negative_normal() {
    let result = decode(mirror_document(&MirrorSpec {
        count: 1,
        seeds: &[10],
        plane: Some(([0.0, 0.015, 0.0], [0.0, -1.0, 0.0])),
    }));
    let (origin, normal) = plane_of(&result);
    assert!(near3(origin, [0.0, 15.0, 0.0]));
    assert!(near3(normal, [0.0, -1.0, 0.0]));
}

#[test]
fn mirror_of_two_seeds_reads_the_plane_at_333() {
    let result = decode(mirror_document(&MirrorSpec {
        count: 2,
        seeds: &[10, 13],
        plane: Some(([0.0, 0.0, 0.025], [0.0, 0.0, 1.0])),
    }));
    let PatternSeeds::Features { features, .. } = &pattern(&result).seeds else {
        panic!("expected feature seeds");
    };
    let Picks::Named(members) = &features.picks else {
        panic!("expected named picks");
    };
    let targets: Vec<_> = members.as_slice().iter().map(|pick| &pick.target).collect();
    assert_eq!(
        targets,
        [
            &PickTarget::FeatureResult(feature_id(10)),
            &PickTarget::FeatureResult(feature_id(13))
        ]
    );
    let (origin, normal) = plane_of(&result);
    assert!(near3(origin, [0.0, 0.0, 25.0]));
    assert!(near3(normal, [0.0, 0.0, 1.0]));
}

#[test]
fn mirror_without_a_seed_is_unread() {
    let result = decode(mirror_document(&MirrorSpec {
        count: 0,
        seeds: &[],
        plane: Some(([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])),
    }));
    assert!(unresolved(&result));
    assert!(codes(&result).contains(&"feature.pattern-unread".to_owned()));
}

#[test]
fn mirror_with_an_unlisted_seed_is_unread() {
    let result = decode(mirror_document(&MirrorSpec {
        count: 1,
        seeds: &[999],
        plane: Some(([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])),
    }));
    assert!(unresolved(&result));
    assert!(codes(&result).contains(&"feature.pattern-unread".to_owned()));
}

#[test]
fn pattern_without_the_parameter_guid_is_unread() {
    let result = decode(pattern_document(&PatternSpec {
        frame: &XY,
        seed: 10,
        kind: 2,
        counts: [6, 0],
        steps: [PI / 3.0, 0.0],
        geometry: &[Member2d::Arc([0.0, 0.0, 0.01, 0.0, TAU])],
        guid: false,
    }));
    assert!(unresolved(&result));
    assert!(codes(&result).contains(&"feature.pattern-unread".to_owned()));
}
