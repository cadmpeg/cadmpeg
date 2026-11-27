// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use std::io::Cursor;

use cadmpeg_ir::design::expr::{Expression, Neutral};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterOwner, ParameterValue};
use cadmpeg_ir::design::units::{Quantity, ValueKind};
use cadmpeg_ir::features::parameters::ParameterValue as LegacyValue;
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

use crate::test_support::test_archive::{archive, archive_entries, assert_valid_document};
use crate::FcstdCodec;

mod oracle;

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

fn unresolved_kind() -> ValueKind {
    ValueKind::Native(cadmpeg_core::nonblank_literal!(
        "fcstd:unresolved-expression-result"
    ))
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
    assert_eq!(cell.kind, unresolved_kind());
    let assumed = crate::loss::FreecadLossCode::ParameterExpressionResultUnresolved.note("");
    assert_eq!(
        assumed.code.to_string(),
        "fcstd/parameter.expression-result-unresolved"
    );
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
    let wire = json::to_value(&distance.definition).unwrap();
    assert_eq!(wire["value"]["kind"], "source");
    assert_eq!(
        wire["value"]["value"]["tokens"][0]["value"]["target"]["value"],
        "fcstd:design:parameter#Sheet:cell:A1"
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
    decode_archive_with_losses(&archive(document))
}

fn decode_archive_with_losses(bytes: &[u8]) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_valid_document(decoded.ir());
    (decoded.ir().clone(), decoded.report().losses.clone())
}

#[test]
fn a_python_property_takes_its_json_class_from_the_named_member() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object name="Sheet" type="Spreadsheet::Sheet"/><Object name="Vars" type="App::VarSet"/></Objects>
<ObjectData Count="2">
<Object name="Vars"><Properties Count="5">
<Property name="Flag" type="App::PropertyPythonObject"><Python file="flag.json" value="0" json="yes"/></Property>
<Property name="List" type="App::PropertyPythonObject"><Python file="items.json"/></Property>
<Property name="Integer" type="App::PropertyPythonObject"><Python file="integer.json"/></Property>
<Property name="Missing" type="App::PropertyPythonObject"><Python file="missing.json"/></Property>
<Property name="Legacy" type="App::PropertyPythonObject"><Python value="\nfalse" json="yes"/></Property>
</Properties></Object>
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="6">
<Cell address="A1" content="=Vars.Flag"/>
<Cell address="A2" content="=sum(Vars.List; 2 mm)"/>
<Cell address="A3" content="=cos(Vars.List) == 1"/>
<Cell address="A4" content="=&lt;&lt;x&gt;&gt; * (Vars.Integer - Vars.Integer)"/>
<Cell address="A5" content="=Vars.Missing == 1"/>
<Cell address="A6" content="=Vars.Legacy"/>
</Cells></Property></Properties></Object>
</ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("flag.json", b"true"),
        (
            "items.json",
            br#"[{"label":"NaN","value":-Infinity},Infinity,NaN]"#,
        ),
        (
            "integer.json",
            b"99999999999999999999999999999999999999999999999",
        ),
    ]);
    let (ir, losses) = decode_archive_with_losses(&bytes);
    for (address, expected, assumed) in [
        ("A1", ValueKind::Boolean, false),
        ("A2", ValueKind::Quantity(Quantity::LENGTH), false),
        ("A3", unresolved_kind(), true),
        ("A4", ValueKind::Text, false),
        ("A5", unresolved_kind(), true),
        ("A6", ValueKind::Boolean, false),
    ] {
        let id = format!("fcstd:design:parameter#Sheet:cell:{address}");
        assert_eq!(parameter(&ir, &id).kind, expected, "{address}");
        assert_eq!(
            charge_count(
                &losses,
                crate::loss::FreecadLossCode::ParameterExpressionResultUnresolved,
                &id
            ),
            usize::from(assumed),
            "{address}"
        );
    }
}

#[test]
fn a_formula_reads_typed_values_through_local_links_and_shape_paths() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4"><Object name="Sheet" type="Spreadsheet::Sheet"/><Object name="Vars" type="App::VarSet"/><Object name="Stock" type="Part::Feature"/><Object name="Relay" type="App::FeaturePython"/></Objects>
<ObjectData Count="4">
<Object name="Vars"><Properties Count="2">
<Property name="Ref" type="App::PropertyLink"><Link value="Stock"/></Property>
<Property name="Again" type="App::PropertyXLink"><XLink file="" name="Relay"/></Property>
</Properties></Object>
<Object name="Relay"><Properties Count="1">
<Property name="Ref" type="App::PropertyLink"><Link value="Stock"/></Property>
</Properties></Object>
<Object name="Stock"><Properties Count="3">
<Property name="Width" type="App::PropertyLength"><Float value="12"/></Property>
<Property name="Shape" type="Part::PropertyPartShape"><Part file="stock.brp"/></Property>
<Property name="Empty" type="Part::PropertyPartShape"><Part file="empty.brp"/></Property>
</Properties></Object>
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="6">
<Cell address="A1" content="=sum(Vars.Again.Ref.Width; 2 mm)"/>
<Cell address="A2" content="=&lt;&lt;%s&gt;&gt; % Vars.Ref.Width"/>
<Cell address="A3" content="=str(Stock.Shape.Length)"/>
<Cell address="A4" content="=str(Stock.Empty.Length)"/>
<Cell address="A5" content="=round(Vars.Ref.Shape) == 1"/>
<Cell address="A6" content="=sum(Sheet.cells; 3 mm)"/>
</Cells></Property></Properties></Object>
</ObjectData></Document>"#;
    let shape = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nVe 0.001 1 2 3 0 0 1001000 *\n+1 0 *";
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("stock.brp", shape),
        ("empty.brp", b""),
    ]);
    let (ir, losses) = decode_archive_with_losses(&bytes);
    for (address, expected, assumed) in [
        ("A1", ValueKind::Quantity(Quantity::LENGTH), false),
        ("A2", ValueKind::Text, false),
        ("A3", ValueKind::Text, false),
        ("A4", unresolved_kind(), true),
        ("A5", unresolved_kind(), true),
        ("A6", ValueKind::Quantity(Quantity::LENGTH), false),
    ] {
        let id = format!("fcstd:design:parameter#Sheet:cell:{address}");
        assert_eq!(parameter(&ir, &id).kind, expected, "{address}");
        assert_eq!(
            charge_count(
                &losses,
                crate::loss::FreecadLossCode::ParameterExpressionResultUnresolved,
                &id
            ),
            usize::from(assumed),
            "{address}"
        );
    }
}

fn charged(
    losses: &[cadmpeg_ir::report::loss::LossNote],
    code: crate::loss::FreecadLossCode,
    id: &str,
) -> bool {
    charge_count(losses, code, id) != 0
}

fn charge_count(
    losses: &[cadmpeg_ir::report::loss::LossNote],
    code: crate::loss::FreecadLossCode,
    id: &str,
) -> usize {
    let code = code.note("").code;
    losses
        .iter()
        .filter(|loss| {
            loss.code == code && loss.message.split_ascii_whitespace().any(|word| word == id)
        })
        .count()
}

#[test]
fn spreadsheet_formulas_keep_proven_kinds_and_distinguish_syntax_from_errors() {
    use crate::loss::FreecadLossCode::ParameterExpressionResultUnresolved;

    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object name="Sheet" type="Spreadsheet::Sheet"/><Object name="Vars" type="App::VarSet"/><Object name="Stock" type="Part::Feature"/></Objects>
<ObjectData Count="3">
<Object name="Vars"><Properties Count="10">
<Property name="Mode" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="2"><Enum value="one"/><Enum value="two"/></CustomEnumList></Property>
<Property name="SpringRate" type="App::PropertyStiffness"><Float value="7"/></Property>
<Property name="Heat" type="App::PropertyHeatFlux"><Float value="5"/></Property>
<Property name="Density" type="App::PropertyStiffnessDensity"><Float value="3"/></Property>
<Property name="Path" type="App::PropertyPath"><Path value="model"/></Property>
<Property name="Target" type="App::PropertyLink"><Link value="Missing"/></Property>
<Property name="Self" type="App::PropertyLink"><Link value="Vars"/></Property>
<Property name="Flag" type="App::PropertyPythonObject"><Python value="dHJ1ZQ" encoded="yes" json="yes"/></Property>
<Property name="Other" type="App::PropertyPythonObject"><Python value="ZmFsc2U" encoded="yes" json="yes"/></Property>
<Property name="Current" type="App::PropertyElectricCurrent"><Float value="4"/></Property>
</Properties></Object>
<Object name="Stock"><Properties Count="1">
<Property name="Shape" type="Part::PropertyPartShape"><Part file=""/></Property>
</Properties></Object>
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="31">
<Cell address="A1" content="2"/>
<Cell address="A3" content="=sum(list(1; 2) + list(3); 2 mm)"/>
<Cell address="A4" content="=sum(create(&lt;&lt;vector&gt;&gt;); 2 mm)"/>
<Cell address="A7" content="=pow(4 mm; A1)"/>
<Cell address="A8" content="=cos(0; 0) == 1"/>
<Cell address="A9" content="=&lt;&lt;%d&gt;&gt; % &lt;&lt;bad&gt;&gt;"/>
<Cell address="A10" content="=(1 / 0) == 1"/>
<Cell address="A11" content="=&lt;&lt;x&gt;&gt; * average(A1; 4)"/>
<Cell address="A21" content="=(vector(1; 2; 3) * vector(4; 5; 6)) + 2"/>
<Cell address="A22" content="=vangle(vector(1; 2; 3) + vector(1; 2; 3); vector(1; 0; 0))"/>
<Cell address="A24" content="=vangle(create(&lt;&lt;vector&gt;&gt;; 1; 2; 3); vector(1; 0; 0))"/>
<Cell address="A28" content="=(2 &gt; 1 ? 4 mm : 5)"/>
<Cell address="A29" content="=sum(no_such_function(1); 2 mm)"/>
<Cell address="B1" content="=sum(translationm(vector(1; 2; 3)); 2 mm)"/>
<Cell address="B2" content="=sum(translationm(tuple(1; 2; 3)); 2 mm)"/>
<Cell address="B3" content="=parsequant(&lt;&lt;2 mm&gt;&gt;)"/>
<Cell address="B4" content="=address(1; 1; 5)"/>
<Cell address="B5" content="=(4 A / 2 A)"/>
<Cell address="B6" content="=pow(4 mm; count(A1; 4))"/>
<Cell address="C1" content="=Vars.Mode"/>
<Cell address="C2" content="=Vars.SpringRate"/>
<Cell address="C3" content="=Vars.Heat"/>
<Cell address="C4" content="=Vars.Density"/>
<Cell address="C5" content="=Vars.Path + &lt;&lt;suffix&gt;&gt;"/>
<Cell address="C6" content="=Vars.Target == 1"/>
<Cell address="C7" content="=Vars.Self == 1"/>
<Cell address="C8" content="=Stock.Shape.Length"/>
<Cell address="C9" content="=sum(C8; 2 mm)"/>
<Cell address="D1" content="=Vars.Flag == 1"/>
<Cell address="D2" content="=Vars.Other"/>
<Cell address="D3" content="=Vars.Current / 2 A"/>
</Cells></Property></Properties></Object>
</ObjectData></Document>"#;
    let (ir, losses) = decode_with_losses(document);
    let length = ValueKind::Quantity(Quantity::LENGTH);
    let real = ValueKind::Quantity(Quantity::REAL);
    let angle = ValueKind::Quantity(Quantity::ANGLE);
    let area = ValueKind::Quantity(Quantity::AREA);
    for (address, expected, assumed) in [
        ("A3", length.clone(), 0),
        ("A4", length.clone(), 0),
        ("A7", area.clone(), 0),
        ("A9", unresolved_kind(), 1),
        ("A10", unresolved_kind(), 1),
        ("A11", ValueKind::Text, 0),
        ("A21", real.clone(), 0),
        ("A22", angle.clone(), 0),
        ("A24", angle, 0),
        ("A28", length.clone(), 0),
        ("B1", length.clone(), 0),
        ("B2", length.clone(), 0),
        ("B3", length.clone(), 0),
        ("B5", real.clone(), 0),
        ("B6", area, 0),
        ("C1", real.clone(), 0),
        ("C2", ValueKind::Quantity(Quantity::of(0, 1, 0, -2)), 0),
        ("C3", ValueKind::Quantity(Quantity::of(0, 1, 0, -3)), 0),
        ("C4", ValueKind::Quantity(Quantity::of(-2, 1, 0, -2)), 0),
        ("C5", ValueKind::Text, 0),
        ("C6", unresolved_kind(), 1),
        ("C7", unresolved_kind(), 1),
        ("C8", unresolved_kind(), 1),
        ("C9", length, 0),
        ("D1", ValueKind::Boolean, 0),
        ("D2", ValueKind::Boolean, 0),
        ("D3", ValueKind::Quantity(Quantity::REAL), 0),
    ] {
        let id = format!("fcstd:design:parameter#Sheet:cell:{address}");
        let cell = parameter(&ir, &id);
        assert_eq!(cell.kind, expected, "{address}");
        assert_eq!(cell.value, None, "{address}");
        assert_eq!(
            charge_count(&losses, ParameterExpressionResultUnresolved, &id),
            assumed,
            "{address}"
        );
    }
    for (address, original) in [
        ("A8", "=cos(0; 0) == 1"),
        ("B4", "=address(1; 1; 5)"),
        ("A29", "=sum(no_such_function(1); 2 mm)"),
    ] {
        let id = format!("fcstd:design:parameter#Sheet:cell:{address}");
        let cell = parameter(&ir, &id);
        assert_eq!(cell.kind, ValueKind::Text, "{address}");
        assert_eq!(
            cell.value,
            Some(ParameterValue::Text(original.to_owned())),
            "{address}"
        );
        assert_eq!(
            charge_count(&losses, ParameterExpressionResultUnresolved, &id),
            0,
            "{address}"
        );
    }
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
                crate::loss::FreecadLossCode::ParameterExpressionResultUnresolved,
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
    assert_eq!(cell("A15").kind, unresolved_kind());
    assert!(charged(
        &losses,
        crate::loss::FreecadLossCode::ParameterExpressionResultUnresolved,
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
            crate::loss::FreecadLossCode::ParameterExpressionResultUnresolved,
            &id(address),
        )
    };

    // Python repeats text by an integer or a bool, and a link property's
    // label is the named object's label. An integer property and a whole
    // value cell are integers.
    for address in ["B1", "B2", "B3", "B4", "B5", "G1", "G2"] {
        assert_eq!(cell(address).kind, ValueKind::Text, "{address}");
        assert!(!assumed(address), "{address}: {losses:?}");
    }

    // The referenced quotient folds to a whole number and FreeCAD stores
    // it as an integer before the repetition.
    assert_eq!(cell("G3").kind, ValueKind::Text);
    assert!(!assumed("G3"), "{losses:?}");
    assert_eq!(cell("G7").kind, ValueKind::Text);
    assert!(!assumed("G7"), "{losses:?}");

    // A comparison, a truth constant, and a Bool property are bools. `==`
    // compares any two values `FreeCAD` evaluates.
    for address in ["B6", "B7", "E1", "E2", "E3"] {
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
    // a comparison with a tuple or `None`. FreeCAD reads another property of
    // a linked object at run time, which may hold `None`.
    for address in [
        "B9", "D1", "D2", "E4", "E5", "E6", "E7", "E8", "G4", "G5", "G6",
    ] {
        assert_eq!(cell(address).kind, unresolved_kind(), "{address}");
        assert!(assumed(address), "{address}: {losses:?}");
    }
}

#[test]
fn a_comparison_freecad_refuses_an_aggregate_and_a_remainder_take_freecads_kind() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object name="Sheet" type="Spreadsheet::Sheet"/><Object name="Vars" type="App::VarSet"/></Objects>
<ObjectData Count="2">
<Object name="Vars"><Properties Count="11">
<Property name="Empty" type="App::PropertyLink" status="1"><Link value=""/></Property>
<Property name="Gone" type="App::PropertyXLink" status="1"><XLink file="" stamp="" name=""/></Property>
<Property name="Proxy" type="App::PropertyPythonObject" status="1"><Python value="bnVsbA==" encoded="yes"/></Property>
<Property name="Ids" type="App::PropertyIntegerSet" status="1"><IntegerSet count="2"><I v="1"/><I v="2"/></IntegerSet></Property>
<Property name="Flag" type="App::PropertyPythonObject" status="1"><Python value="dHJ1ZQ==" encoded="yes" json="yes"/></Property>
<Property name="Three" type="App::PropertyPythonObject" status="1"><Python value="Mw==" encoded="yes" json="yes"/></Property>
<Property name="Half" type="App::PropertyPythonObject" status="1"><Python value="Mi41" encoded="yes" json="yes"/></Property>
<Property name="Name" type="App::PropertyPythonObject" status="1"><Python value="Ingi" encoded="yes" json="yes"/></Property>
<Property name="Pair" type="App::PropertyPythonObject" status="1"><Python value="WzEsIDJd" encoded="yes" json="yes"/></Property>
<Property name="Instance" type="App::PropertyPythonObject" status="1"><Python value="e30=" encoded="yes" module="Lib" class="Proxy"/></Property>
<Property name="Placement" type="App::PropertyPlacement" status="1"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
</Properties></Object>
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="24">
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
<Cell address="L1" content="=Vars.Flag == 1"/>
<Cell address="L2" content="=Vars.Flag"/>
<Cell address="L3" content="=str(A1) * Vars.Three"/>
<Cell address="L4" content="=str(A1) * Vars.Half"/>
<Cell address="L5" content="=Vars.Name + &lt;&lt;y&gt;&gt;"/>
<Cell address="L6" content="=Vars.Pair == 1"/>
<Cell address="L7" content="=Vars.Instance == 1"/>
<Cell address="M1" content="=cos(Vars.Placement) == 1"/>
<Cell address="M2" content="=sum(Vars.Placement; 2 mm)"/>
<Cell address="M3" content="=round(Vars.Placement) == 1"/>
<Cell address="M4" content="=Vars.Placement == 1"/>
<Cell address="M5" content="=sum(Vars.Pair; Vars.Half; 2)"/>
</Cells></Property></Properties></Object>
</ObjectData></Document>"#;
    let (ir, losses) = decode_with_losses(document);
    let id = |address: &str| format!("fcstd:design:parameter#Sheet:cell:{address}");
    let cell = |address: &str| parameter(&ir, &id(address));
    let assumed = |address: &str| {
        charged(
            &losses,
            crate::loss::FreecadLossCode::ParameterExpressionResultUnresolved,
            &id(address),
        )
    };

    // `FreeCAD` refuses `None`, a tuple, a set, and an instance of a Python
    // class as an operand of `==`, a function with a unit error raises, and
    // a numeric function raises on an object that is no number: the cell is
    // in error and holds no value. A Python object property that stores no
    // JSON and no class holds `None`. Python refuses text times a `float`.
    for address in [
        "H1", "H2", "H3", "H4", "H5", "H6", "H7", "J3", "L4", "L7", "M1", "M3",
    ] {
        assert_eq!(cell(address).kind, unresolved_kind(), "{address}");
        assert!(assumed(address), "{address}: {losses:?}");
    }

    // An aggregate passes over a value that is no number; a quantity object
    // keeps its unit under `%`.
    for (address, quantity) in [
        ("J1", Quantity::LENGTH),
        ("J2", Quantity::LENGTH),
        ("K1", Quantity::REAL),
        ("M2", Quantity::LENGTH),
        ("M5", Quantity::REAL),
    ] {
        assert_eq!(
            cell(address).kind,
            ValueKind::Quantity(quantity),
            "{address}"
        );
        assert!(!assumed(address), "{address}: {losses:?}");
    }

    // A Python object property stored as JSON holds the value its JSON
    // states: a bool, an `int`, a string, or a list.
    for (address, kind) in [
        ("L1", ValueKind::Boolean),
        ("L2", ValueKind::Boolean),
        ("L3", ValueKind::Text),
        ("L5", ValueKind::Text),
        ("L6", ValueKind::Boolean),
        ("M4", ValueKind::Boolean),
    ] {
        assert_eq!(cell(address).kind, kind, "{address}");
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
