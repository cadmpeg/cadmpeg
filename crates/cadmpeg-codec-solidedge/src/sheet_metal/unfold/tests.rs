// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sheet_metal::SheetMetalKind;

use crate::test_support::{kind_document, Operand};
use crate::SolidEdgeCodec;

fn decode(function: &str) -> DecodeResult {
    SolidEdgeCodec
        .decode(
            &mut Cursor::new(kind_document(function, &Operand::None, &[], &[])),
            &DecodeOptions::default(),
        )
        .unwrap()
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
