// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sheet_metal::{Hem, HemForm, SheetMetalKind};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::selection::Picks;

use crate::test_support::{kind_document, EdgeSpec, Operand, Slot};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre value with the hand-computed one.
const EPS_VALUE: f64 = 1e-9;

fn decode(operand: &Operand<'_>, slots: &[(usize, Slot)]) -> DecodeResult {
    SolidEdgeCodec
        .decode(
            &mut Cursor::new(kind_document("JMHemAssocFunction", operand, slots, &[])),
            &DecodeOptions::default(),
        )
        .unwrap()
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

fn hem(result: &DecodeResult) -> (&Hem, &NativeOptions) {
    let FeatureOperation::SheetMetal(sheet) = &result.ir().design.features[0].definition.operation
    else {
        panic!("expected a sheet-metal operation");
    };
    let SheetMetalKind::Hem(hem) = &sheet.kind else {
        panic!("expected a hem");
    };
    (hem, &sheet.native_options)
}

fn option<'a>(options: &'a NativeOptions, name: &str) -> Option<&'a NativeOption> {
    options
        .iter()
        .find(|(key, _)| key.as_str() == format!("solidedge:{name}").as_str())
        .map(|(_, value)| value)
}

fn native_name(hem: &Hem) -> &str {
    let HemForm::Native(name) = &hem.form else {
        panic!("expected a native form");
    };
    name.as_str()
}

#[test]
fn single_hem_states_its_edge_and_stored_lengths() {
    let result = decode(
        &Operand::Edge(EdgeSpec::default()),
        &[
            (96, Slot::Double(208.0)),
            (208, Slot::Double(0.0005)),
            (236, Slot::Double(0.0055)),
        ],
    );
    let (hem, options) = hem(&result);
    assert_eq!(native_name(hem), "solidedge:hem-single");
    let Picks::Named(picks) = &hem.edges.picks else {
        panic!("expected a stated edge");
    };
    assert_eq!(picks.as_slice().len(), 1);
    assert!(hem.side.is_none() && hem.position.is_none());
    let Some(NativeOption::Length(Dimension::Literal(radius))) =
        option(options, "hem_inner_radius")
    else {
        panic!("expected the inner radius");
    };
    assert!((radius.get() - 0.5).abs() <= EPS_VALUE);
    let Some(NativeOption::Length(Dimension::Literal(straight))) =
        option(options, "hem_straight_length")
    else {
        panic!("expected the straight length");
    };
    assert!((straight.get() - 5.5).abs() <= EPS_VALUE);
    assert!(matches!(
        option(options, "hem_form_code"),
        Some(NativeOption::Integer(Dimension::Literal(208)))
    ));
    assert_eq!(codes(&result), ["sheet-metal.hem-field-unread"]);
}

#[test]
fn another_form_code_states_only_its_code() {
    let result = decode(
        &Operand::Edge(EdgeSpec::default()),
        &[
            (96, Slot::Double(210.0)),
            (208, Slot::Double(0.003)),
            (236, Slot::Double(0.008)),
        ],
    );
    let (hem, options) = hem(&result);
    assert_eq!(native_name(hem), "solidedge:hem-form-210");
    assert_eq!(options.len(), 1);
    assert!(matches!(
        option(options, "hem_form_code"),
        Some(NativeOption::Integer(Dimension::Literal(210)))
    ));
}

#[test]
fn an_unreadable_edge_is_unread_and_the_hem_is_still_stated() {
    let result = decode(&Operand::BlankEdge, &[(96, Slot::Double(208.0))]);
    let (hem, _) = hem(&result);
    assert!(matches!(hem.edges.picks, Picks::Unread));
    // The edge, the radius, the straight length and the native form.
    assert_eq!(
        codes(&result)
            .iter()
            .filter(|code| *code == "sheet-metal.hem-field-unread")
            .count(),
        4
    );
}

#[test]
fn a_missing_form_slot_gives_a_form_without_a_code() {
    let result = decode(&Operand::Edge(EdgeSpec::default()), &[]);
    let (hem, options) = hem(&result);
    assert_eq!(native_name(hem), "solidedge:hem-form");
    assert!(options.is_empty());
    assert_eq!(
        codes(&result),
        [
            "sheet-metal.hem-field-unread",
            "sheet-metal.hem-field-unread"
        ]
    );
}

#[test]
fn a_non_positive_stored_length_is_unread() {
    let result = decode(
        &Operand::Edge(EdgeSpec::default()),
        &[
            (96, Slot::Double(208.0)),
            (208, Slot::Double(0.0)),
            (236, Slot::Double(0.004)),
        ],
    );
    let (_, options) = hem(&result);
    assert!(option(options, "hem_inner_radius").is_none());
    assert!(option(options, "hem_straight_length").is_some());
    assert_eq!(codes(&result).len(), 2);
}
