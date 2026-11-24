// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::{HelicalSection, HelicalSweep, HelixLaw, HelixShape};
use cadmpeg_ir::design::options::{Boolean, BooleanOp, Hand, NativeOption};
use cadmpeg_ir::design::selection::{PickTarget, Picks};

use crate::sketches::sketch_id;
use crate::test_support::{helix_document, HelixSpec, Member2d, ProfileFrame, EXTENT_SET};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre value with the hand-computed one.
const EPS_VALUE: f64 = 1e-9;

/// A profile on the XY plane with normal +Z.
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

fn codes(result: &DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect()
}

/// A circle of radius 1 mm centred 10 mm from the origin along x.
fn circle() -> Vec<Member2d> {
    vec![Member2d::Circle([0.010, 0.0, 0.001])]
}

fn helix(spec: &HelixSpec) -> DecodeResult {
    decode(helix_document(&XY, &circle(), spec))
}

fn sweep_of(result: &DecodeResult) -> &HelicalSweep {
    match &result.ir().design.features[0].definition.operation {
        FeatureOperation::HelicalSweep(sweep) => sweep,
        other => panic!("expected a helical sweep, found {other:?}"),
    }
}

fn axis_of(sweep: &HelicalSweep) -> [f64; 3] {
    let frame = sweep.helix.placement.resolved.as_ref().unwrap();
    let z = cadmpeg_ir::math::Vector3::from(frame.z_axis);
    [z.x, z.y, z.z]
}

fn assert_vector(found: [f64; 3], expected: [f64; 3]) {
    for (found, expected) in found.into_iter().zip(expected) {
        assert!(
            (found - expected).abs() <= EPS_VALUE,
            "{found} != {expected}"
        );
    }
}

#[test]
fn a_right_hand_helix_runs_along_the_axis() {
    let result = helix(&HelixSpec::default());
    let sweep = sweep_of(&result);
    assert_eq!(sweep.helix.hand, Some(Dimension::Literal(Hand::Right)));
    assert_vector(axis_of(sweep), [0.0, 1.0, 0.0]);
    let frame = sweep.helix.placement.resolved.as_ref().unwrap();
    let x = cadmpeg_ir::math::Vector3::from(frame.x_axis);
    assert_vector([x.x, x.y, x.z], [1.0, 0.0, 0.0]);
    let HelixShape::Helix {
        law: Some(HelixLaw::PitchHeight { pitch, height }),
        taper: None,
    } = &sweep.helix.shape
    else {
        panic!(
            "expected a pitch and height law, found {:?}",
            sweep.helix.shape
        );
    };
    let (Dimension::Literal(pitch), Dimension::Literal(height)) = (pitch, height) else {
        panic!("expected literal values");
    };
    assert!((pitch.get() - 5.0).abs() <= EPS_VALUE);
    assert!((height.get() - 20.0).abs() <= EPS_VALUE);
    assert_eq!(sweep.helix.start, None);
    assert!(matches!(
        sweep.output.boolean,
        Some(Boolean::Apply {
            op: BooleanOp::Join,
            ..
        })
    ));
    let HelicalSection::Profile(profile) = &sweep.section else {
        panic!("expected a profile section");
    };
    let Picks::Named(members) = &profile.picks else {
        panic!("expected a named selection");
    };
    assert_eq!(
        members.as_slice()[0].target,
        PickTarget::Sketch(sketch_id(EXTENT_SET + 1))
    );
    let found = codes(&result);
    assert!(!found.contains(&"feature.kind-unknown".to_owned()));
    assert!(!found.contains(&"feature.extent-unread".to_owned()));
}

#[test]
fn the_revolutions_and_unsettled_slots_stay_native() {
    let result = helix(&HelixSpec::default());
    let options = &sweep_of(&result).native_options;
    let real = |name: &str| match options
        .iter()
        .find(|(key, _)| key.as_str() == format!("solidedge:{name}").as_str())
    {
        Some((_, NativeOption::Real(Dimension::Literal(value)))) => value.get(),
        other => panic!("expected a real option {name}, found {other:?}"),
    };
    assert!((real("revolutions") - 4.0).abs() <= EPS_VALUE);
    assert!((real("slot_352")).abs() <= EPS_VALUE);
    assert!((real("slot_520") - 1.0).abs() <= EPS_VALUE);
}

#[test]
fn the_end_value_runs_against_the_axis() {
    let result = helix(&HelixSpec {
        from: 30.0,
        ..HelixSpec::default()
    });
    let sweep = sweep_of(&result);
    assert_vector(axis_of(sweep), [0.0, -1.0, 0.0]);
    assert_eq!(sweep.helix.hand, Some(Dimension::Literal(Hand::Right)));
}

#[test]
fn hand_value_one_is_left() {
    let result = helix(&HelixSpec {
        hand: 1.0,
        ..HelixSpec::default()
    });
    assert_eq!(
        sweep_of(&result).helix.hand,
        Some(Dimension::Literal(Hand::Left))
    );
}

#[test]
fn an_oblique_axis_gives_the_radial_direction_from_the_axis() {
    // The axis through the origin along (3, 4, 0) / 5; the circle centre
    // (8, -6, 0) mm lies 10 mm from it along (0.8, -0.6, 0).
    let result = decode(helix_document(
        &XY,
        &[Member2d::Circle([0.008, -0.006, 0.001])],
        &HelixSpec {
            axis_direction: [0.6, 0.8, 0.0],
            ..HelixSpec::default()
        },
    ));
    let sweep = sweep_of(&result);
    assert_vector(axis_of(sweep), [0.6, 0.8, 0.0]);
    let frame = sweep.helix.placement.resolved.as_ref().unwrap();
    let x = cadmpeg_ir::math::Vector3::from(frame.x_axis);
    assert_vector([x.x, x.y, x.z], [0.8, -0.6, 0.0]);
}

fn unread(result: &DecodeResult) {
    assert!(matches!(
        &result.ir().design.features[0].definition.operation,
        FeatureOperation::Native { native_options, .. }
            if native_options.keys().any(|key| key.as_str() == "solidedge:profile")
    ));
    assert!(codes(result).contains(&"feature.extent-unread".to_owned()));
}

#[test]
fn a_pitch_that_does_not_make_the_height_is_unread() {
    unread(&helix(&HelixSpec {
        pitch: 0.004,
        ..HelixSpec::default()
    }));
}

#[test]
fn an_extent_kind_other_than_finite_is_unread() {
    unread(&helix(&HelixSpec {
        kind: 16.0,
        ..HelixSpec::default()
    }));
}

#[test]
fn an_unknown_start_end_or_hand_value_is_unread() {
    unread(&helix(&HelixSpec {
        from: 31.0,
        ..HelixSpec::default()
    }));
    unread(&helix(&HelixSpec {
        hand: 3.0,
        ..HelixSpec::default()
    }));
}

#[test]
fn a_profile_centred_on_the_axis_is_unread() {
    unread(&decode(helix_document(
        &XY,
        &[Member2d::Circle([0.0, 0.0, 0.001])],
        &HelixSpec::default(),
    )));
}

#[test]
fn a_feature_without_a_profile_is_unread() {
    unread(&decode(helix_document(&XY, &[], &HelixSpec::default())));
}
