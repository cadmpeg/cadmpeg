// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::selections::DistinctMembers;

use super::*;

const TABLE: &str = "nx:om-entry-0:feature#equations-0";

fn parameter(key: &str, name: &str, unit: &str, expression: &str) -> LegacyParameter {
    let mut properties = BTreeMap::new();
    properties.insert(
        NonBlankString::new("unit".to_owned()).expect("key"),
        unit.to_owned(),
    );
    LegacyParameter {
        product_definition: None,
        id: ParameterId::mint(format!("nx:om-entry-0:parameter#{key}")).expect("id"),
        owner: Some(FeatureId::mint(TABLE).expect("id")),
        ordinal: 0,
        name: name.to_owned(),
        expression: expression.to_owned(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
        properties,
        pmi: None,
        native_ref: Some(format!("nx:om-entry-0:expression#{key}")),
    }
}

fn projected(parameters: Vec<LegacyParameter>) -> Vec<DesignParameter> {
    let mut ir = CadIr::empty();
    ir.model.parameters = parameters;
    let definition = ProductDefinitionId::mint("nx:document:product-definition#root").unwrap();
    let mut losses = Vec::new();
    project(&mut ir, &definition, &mut losses).unwrap();
    assert!(losses.is_empty());
    ir.design.parameters
}

/// Each native expression `(key, table, offset)` of `expressions` in the
/// `expressions` arena of `ir`.
fn with_expressions(ir: &mut CadIr, expressions: &[(&str, &str, u64)]) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let records = expressions
        .iter()
        .map(|(key, table, offset)| {
            json!({
                "id": format!("nx:om-entry-0:expression#{key}"),
                "object_id": null,
                "record": null,
                "name": key,
                "parameter_index": null,
                "qualifier": null,
                "unit": "millimeter",
                "expression": "1",
                "source_entry": "om",
                "source_table": table,
                "source_offset": offset,
            })
        })
        .collect::<Vec<_>>();
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("nx"))
        .set_arena(&ctx, "expressions", &records)
        .unwrap();
}

fn positions(ir: &CadIr) -> Vec<(&str, Option<u32>)> {
    ir.design
        .parameters
        .iter()
        .map(|parameter| {
            (
                parameter.name.as_ref().unwrap().as_str(),
                parameter.position,
            )
        })
        .collect()
}

/// An expression's position is its place in the record order of its table,
/// whatever its dependency rank, and expressions of two tables have none.
#[test]
fn an_expression_is_placed_by_the_record_order_of_its_table() {
    let definition = ProductDefinitionId::mint("nx:document:product-definition#root").unwrap();
    let parameters = || {
        let mut a = parameter("a", "a", "mm", "b + 1");
        a.ordinal = 1;
        let b = parameter("b", "b", "mm", "2");
        vec![a, b]
    };
    let mut ir = CadIr::empty();
    ir.model.parameters = parameters();
    with_expressions(&mut ir, &[("a", TABLE, 10), ("b", TABLE, 20)]);
    project(&mut ir, &definition, &mut Vec::new()).unwrap();
    assert_eq!(positions(&ir), [("a", Some(0)), ("b", Some(1))]);
    let mut ir = CadIr::empty();
    ir.model.parameters = parameters();
    with_expressions(
        &mut ir,
        &[
            ("a", TABLE, 10),
            ("b", "nx:om-entry-0:feature#equations-1", 20),
        ],
    );
    project(&mut ir, &definition, &mut Vec::new()).unwrap();
    assert_eq!(positions(&ir), [("a", None), ("b", None)]);
}

fn definition_json(parameter: &DesignParameter) -> serde_json::Value {
    json::to_value(&parameter.definition).expect("definition")
}

fn reference_json(key: &str) -> serde_json::Value {
    json!({"target": {"kind": "parameter", "value": format!("nx:om-entry-0:parameter#{key}")}})
}

fn neutral(text: &str, refs: &[&str]) -> serde_json::Value {
    json!({"kind": "expression", "value": {"kind": "neutral", "value": {
        "text": text,
        "refs": refs.iter().map(|key| reference_json(key)).collect::<Vec<_>>(),
        "measures": [],
    }}})
}

fn source(tokens: &serde_json::Value) -> serde_json::Value {
    json!({"kind": "expression", "value": {"kind": "source", "value": {
        "dialect": {"kind": "nx"}, "tokens": tokens,
    }}})
}

#[test]
fn a_literal_is_in_the_declared_unit_and_states_its_value() {
    let projected = projected(vec![
        parameter("1", "p1", "millimeter", "10"),
        parameter("2", "p2", "degree", "-30"),
        parameter("3", "p3", "inch", " 2.5 "),
    ]);

    assert_eq!(projected[0].owner, None);
    assert_eq!(
        projected[0].name.as_ref().map(NonBlankString::as_str),
        Some("p1")
    );
    assert_eq!(projected[0].kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(projected[0].unit, unit_of(UnitSymbol::Mm));
    assert_eq!(definition_json(&projected[0]), neutral("10[mm]", &[]));
    assert_eq!(definition_json(&projected[1]), neutral("-30[deg]", &[]));
    assert_eq!(definition_json(&projected[2]), neutral("2.5[in]", &[]));
    assert_eq!(
        projected[0].value,
        Some(ParameterValue::Number(FiniteReal::new(10.0).unwrap()))
    );
    assert_eq!(projected[1].kind, ValueKind::Quantity(Quantity::ANGLE));
    assert_eq!(
        projected[1].value,
        Some(ParameterValue::Number(
            FiniteReal::new((-30.0f64).to_radians()).unwrap()
        ))
    );
    assert_eq!(
        projected[2].value,
        Some(ParameterValue::Number(FiniteReal::new(63.5).unwrap()))
    );
    assert_eq!(
        projected[0].native_ref.as_deref(),
        Some("nx:om-entry-0:expression#1")
    );
}

#[test]
fn a_unit_of_unit_symbols_declares_its_quantity() {
    let projected = projected(vec![
        parameter("1", "p1", "kg", "2"),
        parameter("2", "p2", "mm^3", "8"),
        parameter("3", "p3", "N/mm^2(MPa)", "3"),
        parameter("4", "p4", "1/mm", "4"),
        parameter("5", "p5", "C", "20"),
    ]);
    let kinds = projected
        .iter()
        .map(|parameter| json::to_value(&parameter.kind).unwrap())
        .collect::<Vec<_>>();
    let quantity = |length: i8, mass: i8, time: i8| json!({"kind": "quantity", "value": {"length": length, "mass": mass, "angle": 0, "time": time}});
    assert_eq!(
        kinds,
        [
            quantity(0, 1, 0),
            quantity(3, 0, 0),
            quantity(-1, 1, -2),
            quantity(-1, 0, 0),
            json!({"kind": "native", "value": "nx:C"}),
        ]
    );
    assert_eq!(
        json::to_value(&projected[2].unit).unwrap(),
        json!([
            {"symbol": "kg", "power": 1},
            {"symbol": "m", "power": 1},
            {"symbol": "s", "power": -2},
            {"symbol": "mm", "power": -2},
        ])
    );
    assert_eq!(
        projected[2].value,
        Some(ParameterValue::Number(FiniteReal::new(3000.0).unwrap()))
    );
    assert_eq!(projected[4].unit, None);
    assert_eq!(projected[4].value, None);
}

#[test]
fn a_formula_over_references_is_a_neutral_tree() {
    let projected = projected(vec![
        parameter("1", "p1", "millimeter", "10"),
        parameter("2", "p2", "millimeter", "p1 + p3"),
        parameter("3", "p3", "inch", "1"),
        parameter("4", "p4", "millimeter", "p1*2 + p3 / .5"),
    ]);
    assert_eq!(
        definition_json(&projected[1]),
        neutral("$0 + $1", &["1", "3"])
    );
    assert_eq!(projected[1].value, None);
    assert!(matches!(
        projected[3].definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
}

#[test]
fn nx_trigonometry_reads_degrees() {
    let projected = projected(vec![
        parameter("1", "p1", "millimeter", "10"),
        parameter("2", "p2", "millimeter", "sin(p1 / p5) * p1 * deg(p1 / p5)"),
        parameter("3", "p3", "degree", "45"),
        parameter("4", "p4", "millimeter", "cos(p3) * p1 + pi() * p1"),
        parameter("5", "p5", "millimeter", "4"),
    ]);
    assert_eq!(
        definition_json(&projected[1]),
        neutral(
            "sin($0 / $1 * 1[deg]) * $0 * ($0 / $1 * 1[rad] / 1[deg])",
            &["1", "5"]
        )
    );
    assert_eq!(
        definition_json(&projected[3]),
        neutral("cos($0) * $1 + pi * $1", &["3", "1"])
    );
}

#[test]
fn a_formula_with_a_literal_among_its_terms_keeps_the_source_text() {
    let projected = projected(vec![
        parameter("1", "p1", "millimeter", "10"),
        parameter("2", "p2", "millimeter", "p1 + 5"),
        parameter("3", "p3", "millimeter", "max(p1, 2)"),
    ]);
    assert_eq!(
        definition_json(&projected[1]),
        source(&json!([
            {"kind": "reference", "value": reference_json("1")},
            {"kind": "text", "value": " + 5"},
        ]))
    );
    assert_eq!(
        definition_json(&projected[2]),
        source(&json!([
            {"kind": "text", "value": "max("},
            {"kind": "reference", "value": reference_json("1")},
            {"kind": "text", "value": ", 2)"},
        ]))
    );
}

#[test]
fn an_untranslated_construct_keeps_the_source_text() {
    let projected = projected(vec![
        parameter("1", "p1", "millimeter", "10"),
        parameter("2", "p2", "millimeter", "tan(p1) * q"),
        parameter("3", "p3", "C", "p1 * p1"),
        parameter("4", "p4", "degree", "p1"),
        parameter("5", "p5", "mm^2", "p1 * p1"),
    ]);
    assert_eq!(
        definition_json(&projected[1]),
        source(&json!([
            {"kind": "text", "value": "tan("},
            {"kind": "reference", "value": reference_json("1")},
            {"kind": "text", "value": ") * q"},
        ]))
    );
    assert_eq!(
        projected[2].kind,
        ValueKind::Native(NonBlankString::new("nx:C".to_owned()).unwrap())
    );
    assert_eq!(projected[2].unit, None);
    assert!(matches!(
        projected[2].definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
    assert!(matches!(
        projected[3].definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
    assert_eq!(definition_json(&projected[4]), neutral("$0 * $0", &["1"]));
}

#[test]
fn a_name_repeated_in_one_table_resolves_no_reference() {
    let projected = projected(vec![
        parameter("1", "p1", "millimeter", "10"),
        parameter("2", "p1", "millimeter", "20"),
        parameter("3", "p3", "millimeter", "p1"),
    ]);
    assert_eq!(
        definition_json(&projected[2]),
        source(&json!([{"kind": "text", "value": "p1"}]))
    );
}
