// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::features::{DistinctMembers, FeatureId};
use cadmpeg_ir::scalar::{Angle, Length};

use super::*;

fn parameter(
    key: &str,
    name: &str,
    expression: &str,
    value: Option<LegacyValue>,
) -> LegacyParameter {
    LegacyParameter {
        product_definition: None,
        id: ParameterId::mint(format!("creo:featdefs:parameter#{key}")).expect("parameter id"),
        owner: Some(FeatureId::mint("creo:model:feature#40").expect("owner id")),
        ordinal: 0,
        name: name.to_owned(),
        expression: expression.to_owned(),
        display: None,
        value,
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("creo:featdefs:sketch#917".to_owned()),
    }
}

fn length(value: f64) -> LegacyValue {
    LegacyValue::Length(Length::new(value).expect("length"))
}

fn angle(value: f64) -> LegacyValue {
    LegacyValue::Angle(Angle::new(value).expect("angle"))
}

fn with_property(mut parameter: LegacyParameter, key: &str, value: &str) -> LegacyParameter {
    parameter.properties.insert(
        NonBlankString::new(key.to_owned()).expect("key"),
        value.to_owned(),
    );
    parameter
}

fn scope() -> ProductDefinitionId {
    ProductDefinitionId::mint("creo:model:product_definition#root").expect("scope id")
}

fn project_one(parameter: &LegacyParameter, length: Option<UnitSymbol>) -> DesignParameter {
    let names = BTreeMap::from([(parameter.id.clone(), parameter.name.as_str())]);
    project_parameter(parameter, &scope(), &names, length)
}

fn neutral_text(parameter: &DesignParameter) -> String {
    match &parameter.definition {
        ParameterDefinition::Expression(expression @ Expression::Neutral(_)) => {
            serde_json::to_value(expression).expect("serialize")["value"]["text"]
                .as_str()
                .expect("canonical text")
                .to_owned()
        }
        other => panic!("not a neutral expression: {other:?}"),
    }
}

fn source_tokens(parameter: &DesignParameter) -> &[ExpressionToken<ParameterRef>] {
    match &parameter.definition {
        ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::Creo,
            tokens,
        }) => tokens,
        other => panic!("not a Creo source expression: {other:?}"),
    }
}

#[test]
fn a_section_dimension_is_a_literal_in_the_authoring_unit() {
    let projected = project_one(
        &parameter("1", "d1", "1", Some(length(25.4))),
        Some(UnitSymbol::In),
    );
    assert_eq!(neutral_text(&projected), "1[in]");
    assert_eq!(projected.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(
        projected.value,
        Some(ParameterValue::Number(
            FiniteReal::new(25.4).expect("value")
        ))
    );
    assert_eq!(projected.scope, scope());
    assert_eq!(
        projected.owner,
        Some(ParameterOwner::Feature(
            FeatureId::mint("creo:model:feature#40").expect("owner")
        ))
    );
}

#[test]
fn a_literal_falls_back_to_the_stored_unit_only_where_the_value_agrees() {
    let projected = project_one(
        &parameter("1", "d1", "-2.5", Some(length(-2.5))),
        Some(UnitSymbol::In),
    );
    assert_eq!(neutral_text(&projected), "-2.5[mm]");
    let projected = project_one(&parameter("2", "d2", "1.5", Some(angle(1.5))), None);
    assert_eq!(neutral_text(&projected), "1.5[rad]");
    let degrees = 30f64.to_radians();
    let projected = project_one(&parameter("3", "d3", "30", Some(angle(degrees))), None);
    assert_eq!(neutral_text(&projected), "30[deg]");
    let projected = project_one(
        &parameter("4", "d4", "3", Some(length(5.0))),
        Some(UnitSymbol::In),
    );
    assert_eq!(
        source_tokens(&projected),
        [ExpressionToken::Text("3".to_owned())]
    );
}

#[test]
fn a_bracketed_unit_and_a_declared_unit_name_the_literal_unit() {
    let projected = project_one(
        &parameter("1", "copy", "25.4[mm]", Some(length(25.4))),
        Some(UnitSymbol::In),
    );
    assert_eq!(neutral_text(&projected), "25.4[mm]");
    let declared = with_property(
        parameter("2", "span", "2", Some(length(50.8))),
        "declared_unit",
        "inch",
    );
    let projected = project_one(&declared, Some(UnitSymbol::Mm));
    assert_eq!(neutral_text(&projected), "2[in]");
    assert_eq!(projected.unit, Some(single(UnitSymbol::In)));
}

#[test]
fn an_unread_value_keeps_its_empty_text_and_states_no_cache() {
    let projected = project_one(&parameter("1", "d1", "", None), Some(UnitSymbol::Mm));
    assert!(source_tokens(&projected).is_empty());
    assert_eq!(projected.value, None);
}

#[test]
fn resolved_dependencies_are_references_matched_without_case() {
    let target = parameter("1", "span", "2", Some(length(2.0)));
    let mut relation = parameter("2", "copy", "SPAN+d3:1+1[span]", Some(length(3.0)));
    relation.dependencies = DistinctMembers::try_from(vec![target.id.clone()]).expect("deps");
    let names = BTreeMap::from([
        (target.id.clone(), target.name.as_str()),
        (relation.id.clone(), relation.name.as_str()),
    ]);
    let projected = project_parameter(&relation, &scope(), &names, Some(UnitSymbol::Mm));
    assert_eq!(
        source_tokens(&projected),
        [
            ExpressionToken::Reference(ParameterRef {
                target: RefTarget::Parameter(target.id.clone()),
                through: None,
            }),
            ExpressionToken::Text("+d3:1+1[span]".to_owned()),
        ]
    );
}

#[test]
fn an_evaluated_quantity_states_its_exponents_and_a_temperature_is_native() {
    let stress = with_property(
        with_property(
            parameter("1", "stress", "2", None),
            "evaluated_dimension",
            "length:-1,mass:1,time:-2,angle:0,temperature:0",
        ),
        "evaluated_canonical_value",
        "2000",
    );
    let projected = project_one(&stress, Some(UnitSymbol::Mm));
    assert_eq!(
        projected.kind,
        ValueKind::Quantity(Quantity {
            length: -1,
            mass: 1,
            angle: 0,
            time: -2
        })
    );
    assert_eq!(
        projected.value,
        Some(ParameterValue::Number(
            FiniteReal::new(2000.0).expect("value")
        ))
    );
    let freezing = with_property(
        parameter("2", "freezing", "0", None),
        "evaluated_dimension",
        "length:0,mass:0,time:0,angle:0,temperature:1",
    );
    let projected = project_one(&freezing, Some(UnitSymbol::Mm));
    assert!(matches!(projected.kind, ValueKind::Native(_)));
    assert_eq!(projected.value, None);
    assert!(matches!(
        projected.definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
}

#[test]
fn the_authoring_length_follows_the_principal_unit_system() {
    assert_eq!(
        authoring_length(PrincipalUnitSystem::InchPoundMassSecond),
        Some(UnitSymbol::In)
    );
    assert_eq!(
        authoring_length(PrincipalUnitSystem::CentimeterGramSecond),
        Some(UnitSymbol::Cm)
    );
    let scale = cadmpeg_ir::scalar::PositiveReal::new(304.8).expect("scale");
    assert_eq!(
        authoring_length(PrincipalUnitSystem::LegacyLengthScale(scale)),
        Some(UnitSymbol::Ft)
    );
    let odd = cadmpeg_ir::scalar::PositiveReal::new(3.0).expect("scale");
    assert_eq!(
        authoring_length(PrincipalUnitSystem::LegacyLengthScale(odd)),
        None
    );
}
