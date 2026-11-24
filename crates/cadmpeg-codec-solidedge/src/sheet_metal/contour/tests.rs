// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};

use crate::test_support::{kind_document, Member2d, Operand, ProfileFrame, Slot};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre value with the hand-computed one.
const EPS_VALUE: f64 = 1e-9;

const XZ: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, -1.0, 0.0],
    x_axis: [1.0, 0.0, 0.0],
};

fn decode_operand(operand: &Operand<'_>, slots: &[(usize, Slot)]) -> DecodeResult {
    SolidEdgeCodec
        .decode(
            &mut Cursor::new(kind_document(
                "JMBaseContourFlangeFunction",
                operand,
                slots,
                &[],
            )),
            &DecodeOptions::default(),
        )
        .unwrap()
}

fn decode(members: &[Member2d], slots: &[(usize, Slot)]) -> DecodeResult {
    decode_operand(&Operand::Profile(&XZ, members), slots)
}

fn open_profile() -> Vec<Member2d> {
    vec![
        Member2d::Line([0.0, 0.0, 0.05, 0.0]),
        Member2d::Line([0.05, 0.0, 0.05, 0.03]),
    ]
}

fn codes(result: &DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .filter(|code| code.starts_with("sheet-metal."))
        .collect()
}

fn full_slots() -> Vec<(usize, Slot)> {
    vec![
        (152, Slot::Double(13.0)),
        (208, Slot::Dim(0.04)),
        (236, Slot::Double(0.001)),
    ]
}

fn options(result: &DecodeResult) -> &NativeOptions {
    let FeatureOperation::Native { native_options, .. } =
        &result.ir().design.features[0].definition.operation
    else {
        panic!("expected a native operation");
    };
    native_options
}

fn length(options: &NativeOptions, name: &str) -> Option<f64> {
    let key = format!("solidedge:{name}");
    let (_, option) = options.iter().find(|(k, _)| k.as_str() == key)?;
    let NativeOption::Length(Dimension::Literal(length)) = option else {
        panic!("expected a literal length");
    };
    Some(length.get())
}

#[test]
fn contour_flange_states_its_profile_radius_and_extent_distance_natively() {
    let result = decode(&open_profile(), &full_slots());
    let options = options(&result);
    assert!(options
        .iter()
        .any(|(key, option)| key.as_str() == "solidedge:profile"
            && matches!(option, NativeOption::OrderedSelection(_))));
    assert!((length(options, "inside_bend_radius").unwrap() - 1.0).abs() <= EPS_VALUE);
    assert!((length(options, "extent_distance").unwrap() - 40.0).abs() <= EPS_VALUE);
    assert_eq!(codes(&result), ["sheet-metal.contour-flange-unread"]);
}

#[test]
fn a_profile_that_is_not_a_sketch_leaves_the_feature_unresolved() {
    let result = decode_operand(&Operand::None, &full_slots());
    assert!(matches!(
        result.ir().design.features[0].definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::SheetMetal
        }
    ));
    assert_eq!(codes(&result), ["sheet-metal.contour-flange-unread"]);
}

#[test]
fn an_unfinite_extent_kind_states_no_extent_distance() {
    let result = decode(
        &open_profile(),
        &[
            (152, Slot::Double(16.0)),
            (208, Slot::Dim(0.04)),
            (236, Slot::Double(0.001)),
        ],
    );
    assert!(length(options(&result), "extent_distance").is_none());
    assert_eq!(codes(&result).len(), 2);
}

#[test]
fn a_missing_radius_is_unread_and_the_extent_is_still_stated() {
    let result = decode(
        &open_profile(),
        &[(152, Slot::Double(13.0)), (208, Slot::Dim(0.04))],
    );
    let options = options(&result);
    assert!(length(options, "inside_bend_radius").is_none());
    assert!(length(options, "extent_distance").is_some());
    assert_eq!(codes(&result).len(), 2);
}
