// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::{Loft, SectionForm};
use cadmpeg_ir::design::options::{BodyForm, Boolean, BooleanOp};
use cadmpeg_ir::design::selection::{PickTarget, Picks};

use crate::features::feature_id;
use crate::sketches::sketch_id;
use crate::test_support::{loft_document, Member2d, ProfileFrame, LOFT_SET};
use crate::SolidEdgeCodec;

/// A section on the plane z = `height` metres, normal +Z.
fn frame(height: f64) -> ProfileFrame {
    ProfileFrame {
        origin: [0.0, 0.0, height],
        normal: [0.0, 0.0, 1.0],
        x_axis: [1.0, 0.0, 0.0],
    }
}

/// A circle section of radius `radius` metres, centre (`x`, 0).
fn circle(x: f64, radius: f64) -> Vec<Member2d> {
    vec![Member2d::Circle([x, 0.0, radius])]
}

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

fn loft(result: &DecodeResult) -> &Loft {
    match operation(result) {
        FeatureOperation::Loft(loft) => loft,
        other => panic!("expected a loft, found {other:?}"),
    }
}

/// The sketch each section names, in loft order.
fn section_sketches(loft: &Loft) -> Vec<PickTarget> {
    loft.sections
        .iter()
        .map(|section| {
            let SectionForm::Profile(profile) = &section.form else {
                panic!("expected a profile section");
            };
            let Picks::Named(members) = &profile.picks else {
                panic!("expected a named selection");
            };
            let [pick] = members.as_slice() else {
                panic!("expected one pick");
            };
            pick.target.clone()
        })
        .collect()
}

#[test]
fn two_sections_are_a_loft_in_stored_order() {
    let result = decode(loft_document(&[
        (frame(0.0), circle(0.0, 0.008)),
        (frame(0.020), circle(0.0, 0.005)),
    ]));
    let loft = loft(&result);
    assert_eq!(
        section_sketches(loft),
        vec![
            PickTarget::Sketch(sketch_id(LOFT_SET + 1)),
            PickTarget::Sketch(sketch_id(LOFT_SET + 11)),
        ]
    );
    assert_eq!(loft.output.body, Some(BodyForm::Solid));
    assert!(matches!(
        loft.output.boolean,
        Some(Boolean::Apply {
            op: BooleanOp::Join,
            ..
        })
    ));
    assert!(loft.guides.is_empty() && loft.spine.is_none());
    let owner = feature_id(10);
    let design = &result.ir().design;
    assert_eq!(
        design
            .sketches
            .iter()
            .filter(|sketch| sketch.owner == owner)
            .count(),
        2
    );
    assert_eq!(design.sketch_entities.len(), 2);
    assert!(!codes(&result).contains(&"feature.extent-unread".to_owned()));
}

#[test]
fn three_sections_are_stated_in_stored_order() {
    let result = decode(loft_document(&[
        (frame(0.0), circle(0.0, 0.008)),
        (frame(0.020), circle(0.0, 0.005)),
        (frame(0.040), circle(0.0, 0.006)),
    ]));
    assert_eq!(
        section_sketches(loft(&result)),
        vec![
            PickTarget::Sketch(sketch_id(LOFT_SET + 1)),
            PickTarget::Sketch(sketch_id(LOFT_SET + 11)),
            PickTarget::Sketch(sketch_id(LOFT_SET + 21)),
        ]
    );
    assert_eq!(result.ir().design.sketches.len(), 3);
}

#[test]
fn a_moved_last_section_changes_its_sketch_entity_only() {
    let fixed = decode(loft_document(&[
        (frame(0.0), circle(0.0, 0.008)),
        (frame(0.020), circle(0.0, 0.005)),
        (frame(0.040), circle(0.0, 0.006)),
    ]));
    let moved = decode(loft_document(&[
        (frame(0.0), circle(0.0, 0.008)),
        (frame(0.020), circle(0.0, 0.005)),
        (frame(0.040), circle(0.006, 0.006)),
    ]));
    assert_eq!(
        section_sketches(loft(&fixed)),
        section_sketches(loft(&moved))
    );
    let before = &fixed.ir().design;
    let after = &moved.ir().design;
    let differing: Vec<_> = before
        .sketch_entities
        .iter()
        .zip(&after.sketch_entities)
        .filter(|(a, b)| format!("{:?}", a.definition) != format!("{:?}", b.definition))
        .map(|(a, _)| a.sketch.clone())
        .collect();
    assert_eq!(differing, vec![sketch_id(LOFT_SET + 21)]);
    assert_eq!(before.sketches, after.sketches);
}

#[test]
fn one_section_stays_unresolved() {
    let result = decode(loft_document(&[(frame(0.0), circle(0.0, 0.008))]));
    assert!(matches!(
        operation(&result),
        FeatureOperation::Native { native_options, .. }
            if native_options.keys().any(|key| key.as_str() == "solidedge:profile")
    ));
    assert!(codes(&result).contains(&"feature.extent-unread".to_owned()));
}
