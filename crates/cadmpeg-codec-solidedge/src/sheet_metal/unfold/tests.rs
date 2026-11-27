// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sheet_metal::SheetMetalKind;

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::options::NativeOption;

use super::bends::bend_faces;
use crate::test_support::{bends_proxy, kind_document, BendSpec, Operand};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre value with the hand-computed one.
const EPS_VALUE: f64 = 1e-9;
/// Bytes after the function name of the bends proxy of an unfold (§10.6).
const UNFOLD_BENDS: usize = 96;

fn decode(function: &str) -> DecodeResult {
    SolidEdgeCodec
        .decode(
            &mut Cursor::new(kind_document(function, &Operand::None, &[], &[])),
            &DecodeOptions::default(),
        )
        .unwrap()
}

fn decode_bends(function: &str, proxy: &[u8], after: usize) -> DecodeResult {
    SolidEdgeCodec
        .decode(
            &mut Cursor::new(kind_document(
                function,
                &Operand::Proxy(proxy, after),
                &[],
                &[],
            )),
            &DecodeOptions::default(),
        )
        .unwrap()
}

fn option_mm(result: &DecodeResult, name: &str) -> Option<f64> {
    let FeatureOperation::SheetMetal(sheet) = &result.ir().design.features[0].definition.operation
    else {
        panic!("expected a sheet-metal operation");
    };
    sheet.native_options.iter().find_map(|(key, value)| {
        let NativeOption::Length(Dimension::Literal(length)) = value else {
            return None;
        };
        (key.as_str() == format!("solidedge:{name}")).then(|| length.get())
    })
}

fn kind(result: &DecodeResult) -> &SheetMetalKind {
    let FeatureOperation::SheetMetal(sheet) = &result.ir().design.features[0].definition.operation
    else {
        panic!("expected a sheet-metal operation");
    };
    &sheet.kind
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

#[test]
fn an_unfold_is_stated_without_operands() {
    let result = decode("JMUnfoldFunction");
    assert!(matches!(
        kind(&result),
        SheetMetalKind::Unfold {
            fixed: None,
            bends: None
        }
    ));
    assert_eq!(codes(&result), ["sheet-metal.bend-operand-unread"]);
}

#[test]
fn a_refold_is_stated_without_operands() {
    let result = decode("JMRefoldFunction");
    assert!(matches!(
        kind(&result),
        SheetMetalKind::Refold { bends: None }
    ));
    assert_eq!(codes(&result), ["sheet-metal.bend-operand-unread"]);
}

#[test]
fn the_bends_proxy_of_an_unfold_states_the_bend_width_and_radii() {
    let proxy = bends_proxy(&BendSpec {
        radii: [0.003, 0.001],
        widths: [0.04, 0.04],
    });
    let result = decode_bends("JMUnfoldFunction", &proxy, UNFOLD_BENDS);
    assert!((option_mm(&result, "bend_width").unwrap() - 40.0).abs() <= EPS_VALUE);
    assert!((option_mm(&result, "bend_inside_radius").unwrap() - 1.0).abs() <= EPS_VALUE);
    assert!((option_mm(&result, "bend_outside_radius").unwrap() - 3.0).abs() <= EPS_VALUE);
}

#[test]
fn the_radii_of_the_two_groups_may_come_in_either_order() {
    let proxy = bends_proxy(&BendSpec {
        radii: [0.001, 0.003],
        widths: [0.03, 0.03],
    });
    let faces = bend_faces(&proxy_with_frame(&proxy)).unwrap();
    assert!((faces.inside_radius - 0.001).abs() <= EPS_VALUE);
    assert!((faces.outside_radius - 0.003).abs() <= EPS_VALUE);
    assert!((faces.width - 0.03).abs() <= EPS_VALUE);
}

#[test]
fn a_bends_proxy_with_different_group_widths_states_no_bend() {
    let proxy = bends_proxy(&BendSpec {
        radii: [0.003, 0.001],
        widths: [0.03, 0.04],
    });
    assert!(bend_faces(&proxy_with_frame(&proxy)).is_none());
}

#[test]
fn a_bends_proxy_with_equal_group_radii_states_no_bend() {
    let proxy = bends_proxy(&BendSpec {
        radii: [0.002, 0.002],
        widths: [0.03, 0.03],
    });
    assert!(bend_faces(&proxy_with_frame(&proxy)).is_none());
}

#[test]
fn a_bends_proxy_of_another_size_states_no_bend() {
    let mut proxy = proxy_with_frame(&bends_proxy(&BendSpec::default()));
    proxy.push(0);
    assert!(bend_faces(&proxy).is_none());
}

#[test]
fn a_bends_proxy_with_a_straight_edge_in_place_of_an_arc_states_no_bend() {
    let mut proxy = proxy_with_frame(&bends_proxy(&BendSpec::default()));
    // The kind byte of the first record of the first group.
    proxy[247] = 3;
    assert!(bend_faces(&proxy).is_none());
}

#[test]
fn an_unfold_with_an_unreadable_bends_proxy_adds_no_bend_options() {
    let result = decode_bends("JMUnfoldFunction", &[0_u8; 100], UNFOLD_BENDS);
    assert!(option_mm(&result, "bend_width").is_none());
    assert_eq!(codes(&result), ["sheet-metal.bend-operand-unread"]);
}

#[test]
fn a_refold_adds_no_bend_options() {
    let proxy = bends_proxy(&BendSpec::default());
    let result = decode_bends("JMRefoldFunction", &proxy, 68);
    assert!(option_mm(&result, "bend_width").is_none());
}

/// The object bytes of a proxy body: the 8-byte frame in front of it.
fn proxy_with_frame(body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 8];
    bytes.extend_from_slice(body);
    bytes
}
