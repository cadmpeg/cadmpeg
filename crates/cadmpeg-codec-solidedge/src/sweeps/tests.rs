// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::{Sweep, SweepSection};
use cadmpeg_ir::design::options::{BodyForm, Boolean, BooleanOp};
use cadmpeg_ir::design::selection::{PickTarget, Picks};

use crate::features::feature_id;
use crate::sketches::sketch_id;
use crate::test_support::{
    sweep_document, Member2d, ProfileFrame, SWEEP_PATH_SET, SWEEP_SECTION_SET,
};
use crate::SolidEdgeCodec;

/// The path profile: the XY plane with normal +Z.
const PATH_FRAME: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, 0.0, 1.0],
    x_axis: [1.0, 0.0, 0.0],
};

/// The section profile: the YZ plane with normal +X, at the path start.
const SECTION_FRAME: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [1.0, 0.0, 0.0],
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

fn operation(result: &DecodeResult) -> &FeatureOperation {
    &result.ir().design.features[0].definition.operation
}

/// A straight path of 50 mm along x.
fn path() -> Vec<Member2d> {
    vec![Member2d::Line([0.0, 0.0, 0.050, 0.0])]
}

/// A square section of 10 mm.
fn square() -> Vec<Member2d> {
    vec![
        Member2d::Line([0.0, 0.0, 0.010, 0.0]),
        Member2d::Line([0.010, 0.0, 0.010, 0.010]),
        Member2d::Line([0.010, 0.010, 0.0, 0.010]),
        Member2d::Line([0.0, 0.010, 0.0, 0.0]),
    ]
}

fn picked_sketch(selection: &cadmpeg_ir::design::selection::OrderedSelection) -> PickTarget {
    let Picks::Named(members) = &selection.picks else {
        panic!("expected a named selection");
    };
    let [pick] = members.as_slice() else {
        panic!("expected one pick");
    };
    pick.target.clone()
}

#[test]
fn a_square_section_along_a_straight_path_is_a_sweep() {
    let result = decode(sweep_document(
        Some((&PATH_FRAME, &path())),
        Some((&SECTION_FRAME, &square())),
    ));
    let FeatureOperation::Sweep(sweep) = operation(&result) else {
        panic!("expected a sweep, found {:?}", operation(&result));
    };
    let Sweep {
        section,
        path,
        output,
        ..
    } = sweep.as_ref();
    let SweepSection::Profile(section) = section else {
        panic!("expected a profile section");
    };
    assert_eq!(
        picked_sketch(section),
        PickTarget::Sketch(sketch_id(SWEEP_SECTION_SET + 1))
    );
    assert_eq!(
        picked_sketch(path),
        PickTarget::Sketch(sketch_id(SWEEP_PATH_SET + 1))
    );
    assert_eq!(output.body, Some(BodyForm::Solid));
    assert!(matches!(
        output.boolean,
        Some(Boolean::Apply {
            op: BooleanOp::Join,
            ..
        })
    ));
    let design = &result.ir().design;
    let owner = feature_id(10);
    assert_eq!(
        design
            .sketches
            .iter()
            .filter(|sketch| sketch.owner == owner)
            .count(),
        2
    );
    // The path line and the four section lines.
    assert_eq!(design.sketch_entities.len(), 5);
    assert!(!codes(&result).contains(&"feature.extent-unread".to_owned()));
}

#[test]
fn a_sweep_without_a_section_stays_unresolved() {
    let result = decode(sweep_document(Some((&PATH_FRAME, &path())), None));
    assert!(matches!(
        operation(&result),
        FeatureOperation::Native { native_options, .. }
            if native_options.keys().any(|key| key.as_str() == "solidedge:profile")
    ));
    assert!(codes(&result).contains(&"feature.extent-unread".to_owned()));
}
