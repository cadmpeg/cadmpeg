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
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="11">
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
<Cell address="A11" content="3 dm" displayUnit="dm"/>
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

    // A unit with no symbol keeps the source's text. A display unit of a
    // quantity cell that is no neutral unit is a native option; that of a
    // text cell is a source setting.
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
    for address in ["A8", "A11"] {
        let native = json::to_value(&cell(address).native_options).unwrap();
        assert!(native.get("fcstd:displayUnit").is_some(), "{native}");
        assert!(cell(address).source_settings.is_empty(), "{address}");
    }
    let settings = json::to_value(&cell("A10").source_settings).unwrap();
    assert!(settings.get("fcstd:displayUnit").is_some(), "{settings}");
    assert!(cell("A10").native_options.is_empty());

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

/// Decodes `document` and returns its IR and loss notes.
fn decode_with_losses(document: &str) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_valid_document(decoded.ir());
    (decoded.ir().clone(), decoded.report().losses.clone())
}

fn charged(
    losses: &[cadmpeg_ir::report::loss::LossNote],
    code: crate::loss::FreecadLossCode,
    id: &str,
) -> bool {
    let code = code.note("").code;
    losses
        .iter()
        .any(|loss| loss.code == code && loss.message.contains(id))
}

#[test]
fn a_formula_cell_takes_the_kind_freecad_computes() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object name="Sheet" type="Spreadsheet::Sheet"/><Object name="Vars" type="App::VarSet"/></Objects>
<ObjectData Count="2">
<Object name="Vars"><Properties Count="4">
<Property name="Label" type="App::PropertyString"><String value="Settings"/></Property>
<Property name="L" type="App::PropertyLength" status="1"><Float value="4"/></Property>
<Property name="Face" type="App::PropertyArea" status="1"><Float value="9"/></Property>
<Property name="Count" type="App::PropertyInteger" status="1"><Integer value="3"/></Property>
</Properties></Object>
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="16">
<Cell address="A1" alias="w" content="5 mm"/>
<Cell address="A2" content="=abs(A1)"/>
<Cell address="A3" content="=A1^2"/>
<Cell address="A4" content="=str(A1)"/>
<Cell address="A5" content="=Vars.L"/>
<Cell address="A6" content="=.A1 * 2"/>
<Cell address="A7" content="=&lt;&lt;w&gt;&gt;"/>
<Cell address="A8" content="=Vars.Label"/>
<Cell address="A9" content="=Vars.Face"/>
<Cell address="A10" content="=&lt;&lt;Settings&gt;&gt;.L"/>
<Cell address="A11" content="=Vars.Count * 2"/>
<Cell address="A12" content="=A3 / A2"/>
<Cell address="A13" content="=trunc(A1 / 1 mm)"/>
<Cell address="A14" content="=2 +"/>
<Cell address="A15" content="=A15 + 1 mm"/>
<Cell address="A16" content="inf"/>
</Cells></Property></Properties></Object>
</ObjectData></Document>"#;
    let (ir, losses) = decode_with_losses(document);
    let id = |address: &str| format!("fcstd:design:parameter#Sheet:cell:{address}");
    let cell = |address: &str| parameter(&ir, &id(address));
    let quantity = |address: &str, quantity: Quantity| {
        assert_eq!(
            cell(address).kind,
            ValueKind::Quantity(quantity),
            "{address}"
        );
        assert!(
            !charged(
                &losses,
                crate::loss::FreecadLossCode::ParameterQuantityAssumed,
                &id(address)
            ),
            "{address}: {losses:?}"
        );
    };
    quantity("A2", Quantity::LENGTH);
    quantity("A3", Quantity::AREA);
    quantity("A5", Quantity::LENGTH);
    quantity("A6", Quantity::LENGTH);
    quantity("A9", Quantity::AREA);
    quantity("A10", Quantity::LENGTH);
    quantity("A11", Quantity::REAL);
    quantity("A12", Quantity::LENGTH);
    quantity("A13", Quantity::REAL);
    for address in ["A4", "A7", "A8"] {
        assert_eq!(cell(address).kind, ValueKind::Text, "{address}");
    }
    assert_eq!(neutral(cell("A3")), ("$0^2".to_owned(), vec![id("A1")]));

    // A property of an object that is no parameter stays the source's text.
    for address in ["A5", "A8", "A9", "A10", "A11"] {
        assert!(
            charged(
                &losses,
                crate::loss::FreecadLossCode::ParameterDependencyDropped,
                &id(address)
            ),
            "{address}: {losses:?}"
        );
    }

    // FreeCAD keeps a formula it does not parse as the cell's text.
    assert_eq!(cell("A14").kind, ValueKind::Text);
    assert_eq!(
        cell("A14").value,
        Some(cadmpeg_ir::design::parameter::ParameterValue::Text(
            "=2 +".to_owned()
        ))
    );

    // A formula in a cycle has no kind the file determines.
    assert_eq!(cell("A15").kind, ValueKind::Quantity(Quantity::REAL));
    assert!(charged(
        &losses,
        crate::loss::FreecadLossCode::ParameterQuantityAssumed,
        &id("A15")
    ));

    assert_eq!(cell("A16").kind, ValueKind::Quantity(Quantity::REAL));
    assert_eq!(cell("A16").value, None);
    assert!(charged(
        &losses,
        crate::loss::FreecadLossCode::ParameterValueNonFinite,
        &id("A16")
    ));
}

#[test]
fn text_times_a_number_a_linked_label_a_comparison_and_an_aggregate_take_freecads_kind() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object name="Sheet" type="Spreadsheet::Sheet"/><Object name="Vars" type="App::VarSet"/><Object name="Steel" type="App::FeaturePython"/></Objects>
<ObjectData Count="3">
<Object name="Steel"><Properties Count="1">
<Property name="Label" type="App::PropertyString"><String value="S235"/></Property>
</Properties></Object>
<Object name="Vars"><Properties Count="6">
<Property name="Count" type="App::PropertyInteger" status="1"><Integer value="3"/></Property>
<Property name="Ratio" type="App::PropertyFloat" status="1"><Float value="2"/></Property>
<Property name="Flag" type="App::PropertyBool" status="1"><Bool value="true"/></Property>
<Property name="Material" type="App::PropertyLink" status="1"><Link value="Steel"/></Property>
<Property name="Other" type="App::PropertyXLink" status="1"><XLink file="" name="Steel"/></Property>
<Property name="Part" type="App::PropertyXLink" status="1"><XLink file="" name="Steel" sub=""/></Property>
</Properties></Object>
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="33">
<Cell address="A1" content="5 mm"/>
<Cell address="B1" content="=str(A1) * Vars.Flag + &lt;&lt;-&gt;&gt; * (1 - Vars.Flag)"/>
<Cell address="B2" content="=Vars.Material.Label"/>
<Cell address="B3" content="=Vars.Other.Label2"/>
<Cell address="B4" content="=(&lt;&lt;%s x %s&gt;&gt; % tuple(1; 2) + &lt;&lt;a&gt;&gt;) * Vars.Flag"/>
<Cell address="B5" content="=Vars.Flag * (&lt;&lt;a&gt;&gt; + &lt;&lt;b&gt;&gt;)"/>
<Cell address="B6" content="=&lt;&lt;a&gt;&gt; == 1"/>
<Cell address="B7" content="=A1 &gt; 2"/>
<Cell address="B8" content="=sum(&lt;&lt;a&gt;&gt;; 2 mm)"/>
<Cell address="B9" content="=sum(C1:C2)"/>
<Cell address="D1" content="=str(A1) * 2 mm"/>
<Cell address="D2" content="=Vars.Material.Shape"/>
<Cell address="C1" content="'abc"/>
<Cell address="C2" content="3 mm"/>
<Cell address="C3" content="=sum(C2:C2)"/>
<Cell address="E1" content="=True"/>
<Cell address="E2" content="=Vars.Flag"/>
<Cell address="E3" content="=E1 != E2"/>
<Cell address="E4" content="=str(A1) * 1.5"/>
<Cell address="E5" content="=Vars.Part.Label"/>
<Cell address="E6" content="=&lt;&lt;a&gt;&gt; &lt; 1"/>
<Cell address="E7" content="=Vars.Material.Shape == 1"/>
<Cell address="E8" content="=A1 &gt; cos(0)"/>
<Cell address="G1" content="=str(A1) * Vars.Count"/>
<Cell address="G2" content="=str(A1) * F1"/>
<Cell address="G3" content="=str(A1) * F2"/>
<Cell address="G4" content="=str(A1) * Vars.Ratio"/>
<Cell address="G5" content="=Vars.Part == 1"/>
<Cell address="G6" content="=F3 == 1"/>
<Cell address="G7" content="=G3 + &lt;&lt;!&gt;&gt;"/>
<Cell address="F1" content="3"/>
<Cell address="F2" content="=F1 / 3"/>
<Cell address="F3" content="=None"/>
</Cells></Property></Properties></Object>
</ObjectData></Document>"#;
    let (ir, losses) = decode_with_losses(document);
    let id = |address: &str| format!("fcstd:design:parameter#Sheet:cell:{address}");
    let cell = |address: &str| parameter(&ir, &id(address));
    let assumed = |address: &str| {
        charged(
            &losses,
            crate::loss::FreecadLossCode::ParameterQuantityAssumed,
            &id(address),
        )
    };

    let integer = |address: &str| {
        charged(
            &losses,
            crate::loss::FreecadLossCode::ParameterIntegerAssumed,
            &id(address),
        )
    };

    // Python repeats text by an integer or a bool, and a link property's
    // label is the named object's label. An integer property and a whole
    // value cell are integers.
    for address in ["B1", "B2", "B3", "B4", "B5", "G1", "G2"] {
        assert_eq!(cell(address).kind, ValueKind::Text, "{address}");
        assert!(!assumed(address), "{address}: {losses:?}");
        assert!(!integer(address), "{address}: {losses:?}");
    }

    // A formula cell holds an integer where its value is whole; the file
    // does not store that value.
    assert_eq!(cell("G3").kind, ValueKind::Text);
    assert!(!assumed("G3"), "{losses:?}");
    assert!(integer("G3"), "{losses:?}");
    // The cell that takes the number as an `int` carries the one charge; a
    // cell that references it takes its text.
    assert_eq!(cell("G7").kind, ValueKind::Text);
    assert!(!assumed("G7"), "{losses:?}");
    assert!(!integer("G7"), "{losses:?}");

    // A comparison, a truth constant, and a Bool property are bools. `==`
    // compares any two values `FreeCAD` evaluates.
    for address in ["B6", "B7", "E1", "E2", "E3", "E7"] {
        assert_eq!(cell(address).kind, ValueKind::Boolean, "{address}");
        assert!(!assumed(address), "{address}: {losses:?}");
    }

    // An aggregate passes over a text value.
    for (address, quantity) in [("B8", Quantity::LENGTH), ("C3", Quantity::LENGTH)] {
        assert_eq!(
            cell(address).kind,
            ValueKind::Quantity(quantity),
            "{address}"
        );
        assert!(!assumed(address), "{address}: {losses:?}");
    }

    // FreeCAD raises on a range with a text cell, text times a length or a
    // float, the label of a link with sub-names, text ordered against a
    // number, a length ordered against a dimensionless function result, and
    // a comparison with a tuple or `None`. It reads another property of a
    // linked object at run time.
    for address in ["B9", "D1", "D2", "E4", "E5", "E6", "E8", "G4", "G5", "G6"] {
        assert_eq!(
            cell(address).kind,
            ValueKind::Quantity(Quantity::REAL),
            "{address}"
        );
        assert!(assumed(address), "{address}: {losses:?}");
    }
}

#[test]
fn a_comparison_freecad_refuses_an_aggregate_and_a_remainder_take_freecads_kind() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object name="Sheet" type="Spreadsheet::Sheet"/><Object name="Vars" type="App::VarSet"/></Objects>
<ObjectData Count="2">
<Object name="Vars"><Properties Count="4">
<Property name="Empty" type="App::PropertyLink" status="1"><Link value=""/></Property>
<Property name="Gone" type="App::PropertyXLink" status="1"><XLink file="" stamp="" name=""/></Property>
<Property name="Proxy" type="App::PropertyPythonObject" status="1"><Python value="bnVsbA==" encoded="yes"/></Property>
<Property name="Ids" type="App::PropertyIntegerSet" status="1"><IntegerSet count="2"><I v="1"/><I v="2"/></IntegerSet></Property>
</Properties></Object>
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="12">
<Cell address="A1" content="5 mm"/>
<Cell address="H1" content="=tuple(1; 2) == 1"/>
<Cell address="H2" content="=cos(5 mm) == 1"/>
<Cell address="H3" content="=Vars.Empty == 1"/>
<Cell address="H4" content="=Vars.Gone != 1"/>
<Cell address="H5" content="=Vars.Proxy == 1"/>
<Cell address="H6" content="=Vars.Ids == 1"/>
<Cell address="H7" content="=(A1 &gt; 2 ? None : 1) == 1"/>
<Cell address="J1" content="=sum(Vars.Empty; Vars.Ids; 2 mm)"/>
<Cell address="J2" content="=sum(Vars.Proxy; 2 mm)"/>
<Cell address="J3" content="=sum(1 mm + 2 °; 2 mm)"/>
<Cell address="K1" content="=cos(0) % 2 mm"/>
</Cells></Property></Properties></Object>
</ObjectData></Document>"#;
    let (ir, losses) = decode_with_losses(document);
    let id = |address: &str| format!("fcstd:design:parameter#Sheet:cell:{address}");
    let cell = |address: &str| parameter(&ir, &id(address));
    let assumed = |address: &str| {
        charged(
            &losses,
            crate::loss::FreecadLossCode::ParameterQuantityAssumed,
            &id(address),
        )
    };

    // `FreeCAD` refuses `None`, a tuple, a set, and a Python object as an
    // operand of `==`, and a function with a unit error raises: the cell is
    // in error and holds no value.
    for address in ["H1", "H2", "H3", "H4", "H5", "H6", "H7", "J2", "J3"] {
        assert_eq!(
            cell(address).kind,
            ValueKind::Quantity(Quantity::REAL),
            "{address}"
        );
        assert!(assumed(address), "{address}: {losses:?}");
    }

    // An aggregate passes over a value that is no number; a quantity object
    // keeps its unit under `%`.
    for (address, quantity) in [("J1", Quantity::LENGTH), ("K1", Quantity::REAL)] {
        assert_eq!(
            cell(address).kind,
            ValueKind::Quantity(quantity),
            "{address}"
        );
        assert!(!assumed(address), "{address}: {losses:?}");
    }
}

/// A sketch outside a body whose one distance constraint, with the driving
/// flag `is_driving`, is bound to a sheet alias.
fn bound_constraint(is_driving: &str) -> CadIr {
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object name="Sketch" type="Sketcher::SketchObject"/><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="2">
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="1"><Cell address="A1" alias="w" content="12"/></Cells></Property></Properties></Object>
<Object name="Sketch"><Properties Count="3">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1"><Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="12" EndY="0"/></Geometry></GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="1">
<Constrain Name="" Type="6" First="0" FirstPos="1" Second="0" SecondPos="2" Value="12" IsDriving="{is_driving}"/>
</ConstraintList></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Constraints[0]" expression="Sheet.w"/></ExpressionEngine></Property>
</Properties></Object></ObjectData></Document>"#
    );
    decode(&archive(&document))
}

/// The fields a CADIR write of `ir` names for the model parameter `id`:
/// none where it carries the parameter whole.
fn parameter_charge(ir: &CadIr, id: &str) -> Vec<String> {
    use cadmpeg_ir::codec::write::{CadirEncoder, EncodeInput, Encoder};

    let plan = CadirEncoder
        .plan(EncodeInput::new(ir, None))
        .expect("the CADIR writer drops what it cannot write");
    let Some(loss) = plan.report().losses.iter().find(|loss| {
        loss.code.local_code() == "arena.omitted"
            && loss.message.contains("model arena `parameters`")
    }) else {
        return Vec::new();
    };
    let (omitted, part) = loss
        .message
        .split_once(" stated in part: ")
        .unwrap_or((loss.message.as_str(), ""));
    assert!(!omitted.contains(id), "{}", loss.message);
    part.split_once(&format!("{id} ("))
        .and_then(|(_, fields)| fields.split_once(')'))
        .map(|(fields, _)| fields.split(", ").map(str::to_owned).collect())
        .unwrap_or_default()
}

/// A driven constraint states no expression, so a CADIR write names the
/// expression of its model parameter and the parameters it references; the
/// same constraint driving states its expression, and the write carries the
/// model parameter whole.
#[test]
fn a_write_names_the_expression_a_driven_constraint_leaves_out() {
    const DISTANCE: &str = "fcstd:design:parameter#Sketch:constraint:1";
    let driven = bound_constraint("0");
    assert_eq!(
        parameter(&driven, DISTANCE).definition,
        cadmpeg_ir::design::parameter::ParameterDefinition::Driven
    );
    let charge = parameter_charge(&driven, DISTANCE);
    assert!(
        charge.iter().any(|field| field == "expression"),
        "{charge:?}"
    );
    let driving = bound_constraint("1");
    assert!(parameter_charge(&driving, DISTANCE).is_empty());
}

#[test]
fn spreadsheet_constants_keep_their_kind_and_units() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="1"><Object name="Sheet"><Properties Count="1">
<Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="4">
<Cell address="A1" content="=pi mm"/>
<Cell address="A2" content="=true"/>
<Cell address="A3" content="=false"/>
<Cell address="A4" content="=true mm"/>
</Cells></Property></Properties></Object></ObjectData></Document>"#;
    let (ir, losses) = decode_with_losses(document);
    for (address, kind) in [
        ("A1", ValueKind::Quantity(Quantity::LENGTH)),
        ("A2", ValueKind::Boolean),
        ("A3", ValueKind::Boolean),
        ("A4", ValueKind::Quantity(Quantity::LENGTH)),
    ] {
        let id = format!("fcstd:design:parameter#Sheet:cell:{address}");
        assert_eq!(parameter(&ir, &id).kind, kind, "{address}");
        assert!(
            !charged(
                &losses,
                crate::loss::FreecadLossCode::ParameterQuantityAssumed,
                &id
            ),
            "{address}: {losses:?}"
        );
    }
    let pi = parameter(&ir, "fcstd:design:parameter#Sheet:cell:A1");
    assert_eq!(neutral(pi), ("pi * 1[mm]".to_owned(), vec![]));
}

#[test]
fn spreadsheet_range_tokens_bind_endpoints_and_report_implicit_members() {
    use cadmpeg_ir::design::dimension::RefTarget;
    use cadmpeg_ir::design::expr::ExpressionToken;
    use cadmpeg_ir::design::parameter::ParameterDefinition;

    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="1"><Object name="Sheet"><Properties Count="1">
<Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="6">
<Cell address="A1" alias="Start" content="2 mm"/>
<Cell address="A2" content="3 mm"/>
<Cell address="A3" content="4 mm"/>
<Cell address="B1" content="=sum(Start : A2)"/>
<Cell address="B2" content="=sum($A$1:A3)"/>
<Cell address="B3" content="=sum(A1:Missing)"/>
</Cells></Property></Properties></Object></ObjectData></Document>"#;
    let (ir, losses) = decode_with_losses(document);
    let id = |address| format!("fcstd:design:parameter#Sheet:cell:{address}");
    let check = |address, expected: &[&str], kind, dropped| {
        let key = id(address);
        let cell = parameter(&ir, &key);
        assert_eq!(cell.kind, ValueKind::Quantity(kind), "{address}");
        let ParameterDefinition::Expression(Expression::Source { tokens, .. }) = &cell.definition
        else {
            panic!("{address}: {:?}", cell.definition);
        };
        let parts = tokens
            .iter()
            .map(|token| match token {
                ExpressionToken::Text(text) => text.as_str(),
                ExpressionToken::Reference(reference) => match &reference.target {
                    RefTarget::Parameter(id) => id.as_str(),
                    target @ RefTarget::External { .. } => panic!("{target:?}"),
                },
            })
            .collect::<Vec<_>>();
        assert_eq!(parts, expected, "{address}");
        assert_eq!(
            charged(
                &losses,
                crate::loss::FreecadLossCode::ParameterDependencyDropped,
                &key
            ),
            dropped,
            "{address}: {losses:?}"
        );
    };
    check(
        "B1",
        &["sum(", &id("A1"), " : ", &id("A2"), ")"],
        Quantity::LENGTH,
        false,
    );
    check(
        "B2",
        &["sum(", &id("A1"), ":", &id("A3"), ")"],
        Quantity::LENGTH,
        true,
    );
    check(
        "B3",
        &["sum(", &id("A1"), ":Missing)"],
        Quantity::REAL,
        true,
    );
}

#[test]
fn spreadsheet_expressions_bind_unicode_aliases() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects>
<ObjectData Count="1"><Object name="Sheet"><Properties Count="1">
<Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="3">
<Cell address="A1" alias="Länge" content="2 mm"/>
<Cell address="A2" content="=Länge + 1 mm"/>
<Cell address="A3" content="=Sheet.Länge + 1 mm&#160;"/>
</Cells></Property></Properties></Object></ObjectData></Document>"#;
    let (ir, losses) = decode_with_losses(document);
    let target = "fcstd:design:parameter#Sheet:cell:A1";
    for address in ["A2", "A3"] {
        let key = format!("fcstd:design:parameter#Sheet:cell:{address}");
        let cell = parameter(&ir, &key);
        assert_eq!(
            cell.kind,
            ValueKind::Quantity(Quantity::LENGTH),
            "{address}"
        );
        assert_eq!(
            neutral(cell),
            ("$0 + 1[mm]".to_owned(), vec![target.to_owned()])
        );
        assert!(
            !charged(
                &losses,
                crate::loss::FreecadLossCode::ParameterDependencyDropped,
                &key
            ),
            "{address}: {losses:?}"
        );
    }
}

#[test]
fn a_spreadsheet_path_through_a_local_link_takes_the_stored_property_kind() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object name="Sheet" type="Spreadsheet::Sheet"/><Object name="Vars" type="App::VarSet"/><Object name="Segment" type="App::FeaturePython"/></Objects>
<ObjectData Count="3">
<Object name="Vars"><Properties Count="2">
<Property name="Member" type="App::PropertyLink"><Link value="Segment"/></Property>
<Property name="Empty" type="App::PropertyLink"><Link value=""/></Property>
</Properties></Object>
<Object name="Segment"><Properties Count="1">
<Property name="Length" type="App::PropertyLength"><Float value="12"/></Property>
</Properties></Object>
<Object name="Sheet"><Properties Count="1">
<Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="2">
<Cell address="A1" content="=Vars.Member.Length"/>
<Cell address="A2" content="=Vars.Empty.Length"/>
</Cells></Property></Properties></Object></ObjectData></Document>"#;
    let (ir, losses) = decode_with_losses(document);
    let linked = "fcstd:design:parameter#Sheet:cell:A1";
    let missing = "fcstd:design:parameter#Sheet:cell:A2";
    assert_eq!(
        parameter(&ir, linked).kind,
        ValueKind::Quantity(Quantity::LENGTH)
    );
    assert_eq!(parameter(&ir, linked).value, None);
    let expression = json::to_value(&parameter(&ir, linked).definition).unwrap();
    assert_eq!(
        expression["value"]["value"]["tokens"][0]["value"],
        "Vars.Member.Length"
    );
    assert!(!charged(
        &losses,
        crate::loss::FreecadLossCode::ParameterQuantityAssumed,
        linked
    ));
    assert_eq!(
        parameter(&ir, missing).kind,
        ValueKind::Quantity(Quantity::REAL)
    );
    assert!(charged(
        &losses,
        crate::loss::FreecadLossCode::ParameterQuantityAssumed,
        missing
    ));
}
