// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::options::NativeOption;

use crate::test_support::{kind_document, Operand, Slot};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre value with the hand-computed one.
const EPS_VALUE: f64 = 1e-9;

fn decode(slots: &[(usize, Slot)]) -> DecodeResult {
    SolidEdgeCodec
        .decode(
            &mut Cursor::new(kind_document(
                "JMCloseCornerAssocFunction",
                &Operand::None,
                slots,
                &[],
            )),
            &DecodeOptions::default(),
        )
        .unwrap()
}

#[test]
fn a_close_corner_stays_native_and_keeps_its_stored_fields() {
    let result = decode(&[
        (152, Slot::Double(107.0)),
        (180, Slot::Double(0.004)),
        (244, Slot::Dim(0.0005)),
    ]);
    let FeatureOperation::Native {
        kind,
        native_options,
        ..
    } = &result.ir().design.features[0].definition.operation
    else {
        panic!("expected a native operation");
    };
    assert_eq!(kind.as_str(), "solidedge:JMCloseCornerAssocFunction");
    let get = |name: &str| {
        native_options
            .iter()
            .find(|(key, _)| key.as_str() == format!("solidedge:{name}").as_str())
            .map(|(_, value)| value)
    };
    assert!(matches!(
        get("corner_relief_code"),
        Some(NativeOption::Integer(Dimension::Literal(107)))
    ));
    let Some(NativeOption::Length(Dimension::Literal(size))) = get("corner_relief_size") else {
        panic!("expected the relief size");
    };
    assert!((size.get() - 4.0).abs() <= EPS_VALUE);
    let Some(NativeOption::Length(Dimension::Literal(gap))) = get("corner_gap") else {
        panic!("expected the corner gap");
    };
    assert!((gap.get() - 0.5).abs() <= EPS_VALUE);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "feature.kind-unknown"));
}

#[test]
fn unreadable_slots_are_left_out() {
    let result = decode(&[]);
    let FeatureOperation::Native { native_options, .. } =
        &result.ir().design.features[0].definition.operation
    else {
        panic!("expected a native operation");
    };
    assert!(native_options.is_empty());
}

#[test]
fn the_closure_code_is_a_native_option() {
    for closure in [103_i32, 104] {
        let result = decode(&[
            (124, Slot::Double(f64::from(closure))),
            (152, Slot::Double(232.0)),
        ]);
        let FeatureOperation::Native { native_options, .. } =
            &result.ir().design.features[0].definition.operation
        else {
            panic!("expected a native operation");
        };
        let get = |name: &str| {
            native_options
                .iter()
                .find(|(key, _)| key.as_str() == format!("solidedge:{name}").as_str())
                .map(|(_, value)| value)
        };
        let expected = i64::from(closure);
        assert!(matches!(
            get("corner_closure_code"),
            Some(NativeOption::Integer(Dimension::Literal(code))) if *code == expected
        ));
        assert!(matches!(
            get("corner_relief_code"),
            Some(NativeOption::Integer(Dimension::Literal(232)))
        ));
    }
}
