// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::sheet_metal::SheetMetalKind;
use cadmpeg_ir::design::options::NativeOption;

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

#[test]
fn contour_flange_states_its_profile_radius_and_extent_option() {
    let result = decode(&open_profile(), &full_slots());
    let FeatureOperation::SheetMetal(sheet) = &result.ir().design.features[0].definition.operation
    else {
        panic!("expected a sheet-metal operation");
    };
    let SheetMetalKind::ContourFlange {
        edges,
        width,
        extent,
        thickness,
        bend,
        vertex_bends,
        ..
    } = &sheet.kind
    else {
        panic!("expected a contour flange");
    };
    assert!(edges.is_none() && width.is_none() && extent.is_none() && thickness.is_none());
    assert!(vertex_bends.is_empty());
    let Some(Dimension::Literal(radius)) = &bend.radius else {
        panic!("expected a literal radius");
    };
    assert!((radius.get() - 1.0).abs() <= EPS_VALUE);
    let (_, option) = sheet
        .native_options
        .iter()
        .find(|(key, _)| key.as_str() == "solidedge:extent_distance")
        .unwrap();
    let NativeOption::Length(Dimension::Literal(distance)) = option else {
        panic!("expected a literal length");
    };
    assert!((distance.get() - 40.0).abs() <= EPS_VALUE);
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
fn an_unfinite_extent_kind_states_no_extent_option() {
    let result = decode(
        &open_profile(),
        &[
            (152, Slot::Double(16.0)),
            (208, Slot::Dim(0.04)),
            (236, Slot::Double(0.001)),
        ],
    );
    let FeatureOperation::SheetMetal(sheet) = &result.ir().design.features[0].definition.operation
    else {
        panic!("expected a sheet-metal operation");
    };
    assert!(sheet.native_options.is_empty());
    assert_eq!(codes(&result).len(), 2);
}

#[test]
fn a_missing_radius_is_unread_and_the_flange_is_still_stated() {
    let result = decode(
        &open_profile(),
        &[(152, Slot::Double(13.0)), (208, Slot::Dim(0.04))],
    );
    let FeatureOperation::SheetMetal(sheet) = &result.ir().design.features[0].definition.operation
    else {
        panic!("expected a sheet-metal operation");
    };
    let SheetMetalKind::ContourFlange { bend, .. } = &sheet.kind else {
        panic!("expected a contour flange");
    };
    assert!(bend.radius.is_none());
    assert_eq!(codes(&result).len(), 2);
}
