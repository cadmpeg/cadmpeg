// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::design::expr::{Expression, ExpressionDialect, ExpressionToken, Neutral};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition};
use cadmpeg_ir::design::units::{Quantity, ValueKind};
use cadmpeg_ir::features::parameters::ParameterValue as LegacyValue;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

use crate::test_support::test_archive::assert_valid_document;
use crate::FcstdCodec;

const CONSTRAINT_BOUND: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/named_constraint_1.1.4_N10a.FCStd"
));
const SHEET_BOUND: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/named_constraint_1.1.4_P12a.FCStd"
));
const CORE_OPERATIONS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/core_operations.FCStd"
));

fn decode(bytes: &[u8]) -> CadIr {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_valid_document(decoded.ir());
    decoded.ir().clone()
}

fn parameter<'a>(ir: &'a CadIr, id: &str) -> &'a DesignParameter {
    ir.design
        .parameters
        .iter()
        .find(|parameter| parameter.id.as_str() == id)
        .unwrap_or_else(|| panic!("{id}"))
}

/// The canonical text and reference ids of a neutral expression.
fn neutral(parameter: &DesignParameter) -> (String, Vec<String>) {
    let wire = serde_json::to_value(&parameter.definition).unwrap();
    assert_eq!(wire["kind"], "expression", "{wire}");
    assert_eq!(wire["value"]["kind"], "neutral", "{wire}");
    let text = wire["value"]["value"]["text"].as_str().unwrap().to_owned();
    let refs = wire["value"]["value"]["refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reference| reference["target"]["value"].as_str().unwrap().to_owned())
        .collect();
    (text, refs)
}

#[test]
fn a_property_bound_to_a_named_constraint_is_a_neutral_expression() {
    let ir = decode(CONSTRAINT_BOUND);
    let width = "fcstd:design:parameter#Sketch:constraint:10";
    assert_eq!(
        neutral(parameter(&ir, "fcstd:design:parameter#Pad:Length")),
        ("$0 / 2".to_owned(), vec![width.to_owned()])
    );
    let width = parameter(&ir, width);
    assert_eq!(neutral(width), ("40[mm]".to_owned(), Vec::new()));
    assert_eq!(
        width
            .name
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("width")
    );
    assert_eq!(width.kind, ValueKind::Quantity(Quantity::LENGTH));
}

#[test]
fn a_reference_outside_every_body_stays_source_text() {
    let ir = decode(SHEET_BOUND);
    let ParameterDefinition::Expression(Expression::Source { dialect, tokens }) =
        &parameter(&ir, "fcstd:design:parameter#Pad:Length").definition
    else {
        panic!();
    };
    assert_eq!(*dialect, ExpressionDialect::FreeCad);
    assert_eq!(
        tokens.as_slice(),
        [ExpressionToken::Text("Parameters.plate_w / 2".to_owned())]
    );
    assert!(ir
        .design
        .parameters
        .iter()
        .all(|parameter| !parameter.id.as_str().contains("Parameters:cell")));
    assert!(ir.design.spreadsheets.is_empty());
}

#[test]
fn a_stored_angle_is_a_literal_in_degrees() {
    let ir = decode(CORE_OPERATIONS);
    let angle = parameter(&ir, "fcstd:design:parameter#Draft:Angle");
    assert_eq!(neutral(angle), ("5[deg]".to_owned(), Vec::new()));
    assert_eq!(angle.kind, ValueKind::Quantity(Quantity::ANGLE));
}

fn literal_text(text: &str, value: &LegacyValue, carrier: Option<&str>) -> Option<String> {
    let node = super::literal(text, Some(value), carrier)?;
    let parameter = Expression::Neutral(Neutral(node));
    let wire = serde_json::to_value(&parameter).unwrap();
    Some(wire["value"]["text"].as_str().unwrap().to_owned())
}

#[test]
fn a_literal_takes_its_carriers_unit_not_the_one_its_number_agrees_in() {
    let half_turn = LegacyValue::Angle(Angle::new(std::f64::consts::PI).unwrap());
    assert_eq!(
        literal_text(
            "3.141592653589793",
            &half_turn,
            Some(super::CONSTRAINT_LIST)
        )
        .as_deref(),
        Some("3.14159265358979[rad]")
    );
    let degrees = LegacyValue::Angle(Angle::new(180_f64.to_radians()).unwrap());
    assert_eq!(
        literal_text("180", &degrees, Some("App::PropertyAngle")).as_deref(),
        Some("180[deg]")
    );
    assert_eq!(literal_text("180", &degrees, None), None);
}

#[test]
fn a_stored_value_with_no_text_is_a_literal_in_its_carriers_unit() {
    let text = |value: &LegacyValue, carrier: Option<&str>| {
        let node = super::stored_literal(value, carrier)?;
        let wire = serde_json::to_value(Expression::Neutral(Neutral(node))).unwrap();
        Some(wire["value"]["text"].as_str().unwrap().to_owned())
    };
    let degrees = LegacyValue::Angle(Angle::new(12.5_f64.to_radians()).unwrap());
    assert_eq!(
        text(&degrees, Some("App::PropertyAngle")).as_deref(),
        Some("12.5[deg]")
    );
    assert_eq!(text(&degrees, None), None);
    let length = LegacyValue::Length(Length::new(-3.0).unwrap());
    assert_eq!(text(&length, None).as_deref(), Some("-3[mm]"));
}

#[test]
fn a_literal_is_the_shortest_decimal_at_fifteen_significant_digits() {
    let length = LegacyValue::Length(Length::new(5.751_842_036_105_048).unwrap());
    assert_eq!(
        literal_text("5.751842036105048", &length, Some("App::PropertyLength")).as_deref(),
        Some("5.75184203610505[mm]")
    );
    let tenth = LegacyValue::Length(Length::new(0.300_000_000_000_000_04).unwrap());
    assert_eq!(
        literal_text("0.30000000000000004", &tenth, None).as_deref(),
        Some("0.3[mm]")
    );
    assert_eq!(super::decimal(0.1 + 0.2, |value| value), "0.3");
    assert_eq!(super::decimal(12.5, f64::to_radians), "12.5");
}
