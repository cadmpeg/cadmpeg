// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use std::io::Cursor;

use cadmpeg_ir::design::expr::{Expression, Neutral};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterOwner};
use cadmpeg_ir::design::units::{Quantity, ValueKind};
use cadmpeg_ir::features::parameters::ParameterValue as LegacyValue;
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

use crate::test_support::test_archive::{archive, assert_valid_document};
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
    let wire = json::to_value(&parameter.definition).unwrap();
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
fn a_reference_to_a_sheet_alias_names_the_cell_parameter() {
    let ir = decode(SHEET_BOUND);
    let sheet = cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#Parameters").unwrap();
    let cell = ir
        .design
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner == Some(ParameterOwner::Feature(sheet.clone()))
                && parameter
                    .name
                    .as_ref()
                    .is_some_and(|name| name.as_str() == "plate_w")
        })
        .unwrap_or_else(|| panic!("{:?}", ir.design.parameters));
    assert!(
        cell.id.as_str().contains("Parameters:cell:"),
        "{}",
        cell.id.as_str()
    );
    assert_eq!(
        neutral(parameter(&ir, "fcstd:design:parameter#Pad:Length")),
        ("$0 / 2".to_owned(), vec![cell.id.as_str().to_owned()])
    );
}

#[test]
fn a_dimensionless_reference_bound_to_a_length_is_in_millimetres() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object name="Sketch" type="Sketcher::SketchObject"/><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="2">
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="1"><Cell address="A1" alias="w" content="12"/></Cells></Property></Properties></Object>
<Object name="Sketch"><Properties Count="3">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1"><Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="12" EndY="0"/></Geometry></GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="1">
<Constrain Name="width" Type="6" First="0" FirstPos="1" Second="0" SecondPos="2" Value="12"/>
</ConstraintList></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path=".Constraints.width" expression="Sheet.w"/></ExpressionEngine></Property>
</Properties></Object></ObjectData></Document>"#;
    let ir = decode(&archive(document));
    let distance = parameter(&ir, "fcstd:design:parameter#Sketch:constraint:1");
    assert_eq!(distance.kind, ValueKind::Quantity(Quantity::LENGTH));
    let (text, refs) = neutral(distance);
    assert_eq!(text, "$0 * 1[mm]");
    assert_eq!(refs.len(), 1);
}

#[test]
fn a_dimensionless_reference_bound_to_an_angle_is_in_degrees() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object name="Sketch" type="Sketcher::SketchObject"/><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="2">
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="1"><Cell address="A1" alias="tilt" content="30"/></Cells></Property></Properties></Object>
<Object name="Sketch"><Properties Count="3">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1"><Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="12" EndY="0"/></Geometry></GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="1">
<Constrain Name="tilt" Type="9" First="0" FirstPos="0" Value="0.5235987755982988"/>
</ConstraintList></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path=".Constraints.tilt" expression="Sheet.tilt"/></ExpressionEngine></Property>
</Properties></Object></ObjectData></Document>"#;
    let ir = decode(&archive(document));
    let angle = parameter(&ir, "fcstd:design:parameter#Sketch:constraint:1");
    assert_eq!(angle.kind, ValueKind::Quantity(Quantity::ANGLE));
    let (text, refs) = neutral(angle);
    assert_eq!(text, "$0 * 1[deg]");
    assert_eq!(refs.len(), 1);
}

#[test]
fn a_cell_of_unknown_quantity_is_an_assumed_real_that_a_lone_reference_keeps() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object name="Sketch" type="Sketcher::SketchObject"/><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="2">
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="1"><Cell address="A1" alias="w" content="=width0 * 2"/></Cells></Property></Properties></Object>
<Object name="Sketch"><Properties Count="3">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1"><Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="12" EndY="0"/></Geometry></GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="1">
<Constrain Name="width" Type="6" First="0" FirstPos="1" Second="0" SecondPos="2" Value="12"/>
</ConstraintList></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path=".Constraints.width" expression="Sheet.w"/></ExpressionEngine></Property>
</Properties></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = decoded.ir();
    assert_valid_document(ir);
    let cell = parameter(ir, "fcstd:design:parameter#Sheet:cell:A1");
    assert_eq!(cell.kind, ValueKind::Quantity(Quantity::REAL));
    let assumed = crate::loss::FreecadLossCode::ParameterQuantityAssumed.note("");
    assert_eq!(assumed.code.to_string(), "fcstd/parameter.quantity-assumed");
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == assumed.code && loss.message.contains("Sheet:cell:A1")),
        "{:?}",
        decoded.report().losses
    );
    // The constraint bound to the cell keeps its dependency as a reference.
    let distance = parameter(ir, "fcstd:design:parameter#Sketch:constraint:1");
    assert_eq!(
        neutral(distance),
        (
            "$0 * 1[mm]".to_owned(),
            vec!["fcstd:design:parameter#Sheet:cell:A1".to_owned()]
        )
    );
}

#[test]
fn a_cell_reads_its_content_as_freecad_does() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="1">
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="10">
<Cell address="A1" content="3 bolts"/>
<Cell address="A2" content="-abc"/>
<Cell address="A3" content="2*3"/>
<Cell address="A4" content="=2*3"/>
<Cell address="A5" content="-5 mm"/>
<Cell address="A6" content="1/2"/>
<Cell address="A7" content="2 in" displayUnit="in"/>
<Cell address="A8" content="3 dm" displayUnit="kg"/>
<Cell address="A9" content="5 mA"/>
<Cell address="A10" content="'note" displayUnit="mm"/>
</Cells></Property></Properties></Object>
</ObjectData></Document>"#;
    let ir = decode(&archive(document));
    let cell =
        |address: &str| parameter(&ir, &format!("fcstd:design:parameter#Sheet:cell:{address}"));
    let text = |address: &str, text: &str| {
        assert_eq!(cell(address).kind, ValueKind::Text, "{address}");
        assert_eq!(
            cell(address).value,
            Some(cadmpeg_ir::design::parameter::ParameterValue::Text(
                text.to_owned()
            )),
            "{address}"
        );
    };
    text("A1", "3 bolts");
    text("A2", "-abc");
    text("A3", "2*3");
    text("A5", "-5 mm");
    text("A10", "note");

    assert_eq!(cell("A4").kind, ValueKind::Quantity(Quantity::REAL));
    assert_eq!(neutral(cell("A4")), ("2 * 3".to_owned(), Vec::new()));

    assert_eq!(cell("A6").kind, ValueKind::Quantity(Quantity::REAL));
    assert_eq!(neutral(cell("A6")), ("1 / 2".to_owned(), Vec::new()));
    assert_eq!(
        cell("A6").value,
        FiniteReal::new(0.5).map(cadmpeg_ir::design::parameter::ParameterValue::Number)
    );

    // A display unit of the cell's quantity is the parameter's unit.
    assert_eq!(cell("A7").kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(neutral(cell("A7")), ("2[in]".to_owned(), Vec::new()));
    assert_eq!(
        cell("A7").unit,
        Some(cadmpeg_ir::design::units::Unit(vec![
            cadmpeg_ir::design::units::UnitFactor {
                symbol: cadmpeg_ir::design::units::UnitSymbol::In,
                power: std::num::NonZeroI8::new(1).unwrap(),
            }
        ]))
    );
    assert!(cell("A7").source_settings.is_empty());

    // A unit with no symbol keeps the source's text; a display unit of
    // another quantity, or of a text cell, stays a source setting.
    assert_eq!(cell("A8").kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(
        cell("A8").value,
        FiniteReal::new(300.0).map(cadmpeg_ir::design::parameter::ParameterValue::Number)
    );
    assert_eq!(
        cell("A8").definition,
        cadmpeg_ir::design::parameter::ParameterDefinition::Expression(Expression::Source {
            dialect: cadmpeg_ir::design::expr::ExpressionDialect::FreeCad,
            tokens: vec![cadmpeg_ir::design::expr::ExpressionToken::Text(
                "3 dm".to_owned()
            )],
        })
    );
    assert_eq!(cell("A8").unit, None);
    for address in ["A8", "A10"] {
        let settings = json::to_value(&cell(address).source_settings).unwrap();
        assert!(settings.get("fcstd:displayUnit").is_some(), "{settings}");
    }

    // A unit of a dimension the vocabulary does not type is a native kind.
    assert_eq!(
        cell("A9").kind,
        ValueKind::Native(cadmpeg_core::text::NonBlankString::new("mA".to_owned()).unwrap())
    );
    assert_eq!(
        cell("A9").value,
        FiniteReal::new(5.0).map(cadmpeg_ir::design::parameter::ParameterValue::Number)
    );
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
    let wire = json::to_value(&parameter).unwrap();
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
        let wire = json::to_value(Expression::Neutral(Neutral(node))).unwrap();
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

#[test]
fn sheet_cells_are_parameters_named_by_their_alias() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="1">
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="6">
<Cell address="A1" alias="w" content="12 mm" alignment="center|vcenter" style="bold"/>
<Cell address="A2" content="=w * 2"/>
<Cell address="A3" alias="label" content="'Plate"/>
<Cell address="A4" content="note"/>
<Cell address="B1" alias="taller" content="=A1 + 1 mm"/>
<Cell address="B2" content="=Sheet.w / 4"/>
</Cells></Property></Properties></Object>
</ObjectData></Document>"#;
    let ir = decode(&archive(document));
    let cell =
        |address: &str| parameter(&ir, &format!("fcstd:design:parameter#Sheet:cell:{address}"));
    let name = |address: &str| {
        cell(address)
            .name
            .as_ref()
            .map(|name| name.as_str().to_owned())
    };
    let width = "fcstd:design:parameter#Sheet:cell:A1".to_owned();
    assert_eq!(name("A1").as_deref(), Some("w"));
    assert_eq!(cell("A1").kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(neutral(cell("A1")), ("12[mm]".to_owned(), Vec::new()));
    let settings = json::to_value(&cell("A1").source_settings).unwrap();
    assert_eq!(
        settings["fcstd:alignment"]["value"]["value"], "center|vcenter",
        "{settings}"
    );
    assert_eq!(
        settings["fcstd:style"]["value"]["value"], "bold",
        "{settings}"
    );

    assert_eq!(name("A2"), None);
    assert_eq!(cell("A2").kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(cell("A2").value, None);
    assert_eq!(
        neutral(cell("A2")),
        ("$0 * 2".to_owned(), vec![width.clone()])
    );

    for (address, text) in [("A3", "Plate"), ("A4", "note")] {
        assert_eq!(cell(address).kind, ValueKind::Text);
        assert_eq!(
            cell(address).value,
            Some(cadmpeg_ir::design::parameter::ParameterValue::Text(
                text.to_owned()
            ))
        );
        assert_eq!(
            cell(address).definition,
            cadmpeg_ir::design::parameter::ParameterDefinition::Expression(Expression::Neutral(
                Neutral(cadmpeg_ir::design::expr::Node::Text(text.to_owned()))
            ))
        );
    }
    assert_eq!(name("A3").as_deref(), Some("label"));

    assert_eq!(
        neutral(cell("B1")),
        ("$0 + 1[mm]".to_owned(), vec![width.clone()])
    );
    assert_eq!(neutral(cell("B2")), ("$0 / 4".to_owned(), vec![width]));
}

#[test]
fn a_lone_reference_to_a_formula_cell_takes_the_cells_quantity() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object name="Sketch" type="Sketcher::SketchObject"/><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="2">
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="2"><Cell address="A1" alias="w" content="6mm"/><Cell address="A2" alias="double" content="=w * 2"/></Cells></Property></Properties></Object>
<Object name="Sketch"><Properties Count="3">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1"><Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="12" EndY="0"/></Geometry></GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="1">
<Constrain Name="width" Type="6" First="0" FirstPos="1" Second="0" SecondPos="2" Value="12"/>
</ConstraintList></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path=".Constraints.width" expression="Sheet.double"/></ExpressionEngine></Property>
</Properties></Object></ObjectData></Document>"#;
    let ir = decode(&archive(document));
    let distance = parameter(&ir, "fcstd:design:parameter#Sketch:constraint:1");
    assert_eq!(
        neutral(distance),
        (
            "$0".to_owned(),
            vec!["fcstd:design:parameter#Sheet:cell:A2".to_owned()]
        )
    );
}
