// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::text::NonBlankString;

use super::{parameter_cut, Restatements};
use crate::design::dimension::{ParameterRef, RefTarget};
use crate::design::expr::{parse, Expression, ExpressionDialect, ExpressionToken, Neutral};
use crate::design::options::NativeOptions;
use crate::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use crate::design::units::{Quantity, ValueKind};
use crate::features::parameters::{
    DesignParameter as ModelParameter, ParameterValue as ModelValue,
};
use crate::features::selections::DistinctMembers;
use crate::features::ParameterId;
use crate::ids::ProductDefinitionId;
use crate::scalar::{FiniteReal, Length};

const WIDTH: &str = "test:design:parameter#width";
const DEPTH: &str = "test:design:parameter#depth";
const NO_REFS: &[ParameterRef] = &[];

fn id(key: &str) -> ParameterId {
    ParameterId::mint(key).unwrap()
}

fn scope() -> ProductDefinitionId {
    ProductDefinitionId::mint("test:design:product-definition#root").unwrap()
}

/// A model length parameter `depth` of 12 mm with the expression text
/// `text`, which references `width`.
fn model(text: &str) -> ModelParameter {
    ModelParameter {
        product_definition: None,
        id: id(DEPTH),
        owner: None,
        ordinal: 0,
        name: "depth".into(),
        expression: text.into(),
        display: None,
        value: Some(ModelValue::Length(Length::new(12.0).unwrap())),
        dependencies: [id(WIDTH)].into_iter().collect::<DistinctMembers<_>>(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

/// The design parameter `depth` of 12 mm with the definition `definition`.
fn design(definition: ParameterDefinition) -> DesignParameter {
    DesignParameter {
        id: id(DEPTH),
        scope: scope(),
        owner: None,
        name: NonBlankString::new("depth".to_owned()),
        kind: ValueKind::Quantity(Quantity::LENGTH),
        unit: None,
        definition,
        value: Some(ParameterValue::Number(FiniteReal::new(12.0).unwrap())),
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    }
}

fn width() -> ParameterRef {
    ParameterRef {
        target: RefTarget::Parameter(id(WIDTH)),
        through: None,
    }
}

fn cut(model: &ModelParameter, design: &DesignParameter) -> BTreeSet<&'static str> {
    parameter_cut(model, design, None, false, &Restatements::default())
}

/// A design parameter whose tree states the expression and its reference
/// carries the model parameter whole.
#[test]
fn a_parameter_whose_tree_states_its_expression_is_carried_whole() {
    let tree = parse("$0 + 2", &[width()], &[]).unwrap();
    let design = design(ParameterDefinition::Expression(Expression::Neutral(
        Neutral(tree),
    )));
    assert!(cut(&model("width + 2"), &design).is_empty());
}

/// A driven design parameter leaves out the expression and the parameters
/// it references.
#[test]
fn a_driven_parameter_leaves_out_the_expression_and_its_references() {
    assert_eq!(
        cut(&model("width + 2"), &design(ParameterDefinition::Driven)),
        BTreeSet::from(["dependencies", "expression"])
    );
}

/// Source tokens state the text they spell, white space aside, with a
/// reference in place of each name.
#[test]
fn source_tokens_state_the_text_they_spell() {
    let source = |text: &str| {
        design(ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::Fusion,
            tokens: vec![
                ExpressionToken::Reference(width()),
                ExpressionToken::Text(text.to_owned()),
            ],
        }))
    };
    assert!(cut(&model("width + 2"), &source(" + 2")).is_empty());
    assert_eq!(
        cut(&model("width + 2"), &source(" + 3")),
        BTreeSet::from(["dependencies", "expression"])
    );
}

/// A one-literal tree states a literal text of the same number in any unit
/// spelling, and no other text.
#[test]
fn a_literal_tree_states_the_literal_of_its_number() {
    let literal = design(ParameterDefinition::Expression(Expression::Neutral(
        Neutral(parse("12", NO_REFS, &[]).unwrap()),
    )));
    let mut plain = model("12 mm");
    plain.dependencies = DistinctMembers::default();
    assert!(cut(&plain, &literal).is_empty());
    plain.expression = "13 mm".into();
    assert_eq!(cut(&plain, &literal), BTreeSet::from(["expression"]));
}

/// A design parameter of another value, name or position leaves out each
/// field it does not hold.
#[test]
fn a_parameter_leaves_out_each_field_it_does_not_hold() {
    let tree = parse("$0 + 2", &[width()], &[]).unwrap();
    let mut design = design(ParameterDefinition::Expression(Expression::Neutral(
        Neutral(tree),
    )));
    design.value = Some(ParameterValue::Number(FiniteReal::new(12.5).unwrap()));
    design.name = NonBlankString::new("other".to_owned());
    let model = model("width + 2");
    assert_eq!(
        parameter_cut(&model, &design, None, true, &Restatements::default()),
        BTreeSet::from(["name", "ordinal", "value"])
    );
}

/// A design parameter states the driven flag of the model properties only
/// through a driven definition.
#[test]
fn the_driven_flag_agrees_with_the_definition() {
    let mut driven = model("");
    driven.dependencies = DistinctMembers::default();
    driven.native_ref = Some("native".to_owned());
    driven.properties.insert(
        NonBlankString::new("is_driving".to_owned()).unwrap(),
        "0".to_owned(),
    );
    let native = |definition| DesignParameter {
        native_ref: Some("native".to_owned()),
        ..design(definition)
    };
    assert!(cut(&driven, &native(ParameterDefinition::Driven)).is_empty());
    let assigned = native(ParameterDefinition::Expression(Expression::Neutral(
        Neutral(parse("12", NO_REFS, &[]).unwrap()),
    )));
    assert_eq!(cut(&driven, &assigned), BTreeSet::from(["properties"]));
}

#[test]
fn the_same_solidworks_scalar_is_carried_in_canonical_units() {
    let mut model = model("12");
    model.dependencies = DistinctMembers::default();
    model.value = Some(ModelValue::Real(FiniteReal::new(0.012).unwrap()));
    model.native_ref = Some("sldprt:feature-input:scalar#test".into());
    let mut design = design(ParameterDefinition::Expression(Expression::Neutral(
        Neutral(parse("12[mm]", NO_REFS, &[]).unwrap()),
    )));
    design.native_ref = model.native_ref.clone();
    assert!(cut(&model, &design).is_empty());

    design.value = Some(ParameterValue::Number(FiniteReal::new(0.012).unwrap()));
    assert!(cut(&model, &design).contains("value"));

    design.value = Some(ParameterValue::Number(FiniteReal::new(12.0).unwrap()));
    design.native_ref = Some("sldprt:feature-input:scalar#other".into());
    assert!(cut(&model, &design).contains("value"));
}

#[test]
fn a_display_modifier_is_carried_by_the_matching_typed_field() {
    let mut model = model("<MOD-DIAM>12");
    model.dependencies = DistinctMembers::default();
    model.display = Some(super::ModelDisplay::Diameter);
    let mut design = design(ParameterDefinition::Expression(Expression::Neutral(
        Neutral(parse("12[mm]", NO_REFS, &[]).unwrap()),
    )));
    design.display = Some(super::DimensionDisplay::Diameter);
    assert!(cut(&model, &design).is_empty());

    model.expression = "<MOD-DIAM>13".into();
    assert!(cut(&model, &design).contains("expression"));

    model.expression = "<MOD-DIAM>12 in".into();
    assert!(cut(&model, &design).contains("expression"));

    model.expression = "<MOD-DIAM>12".into();
    design.display = Some(super::DimensionDisplay::Radius);
    assert!(cut(&model, &design).contains("expression"));
}

#[test]
fn a_parenthesized_negation_carries_the_signed_literal() {
    let mut model = model("-(12 mm)");
    model.dependencies = DistinctMembers::default();
    model.value = Some(ModelValue::Length(Length::new(-12.0).unwrap()));
    let mut design = design(ParameterDefinition::Expression(Expression::Neutral(
        Neutral(parse("-12[mm]", NO_REFS, &[]).unwrap()),
    )));
    design.value = Some(ParameterValue::Number(FiniteReal::new(-12.0).unwrap()));
    assert!(cut(&model, &design).is_empty());

    model.expression = "-(-12 mm)".into();
    assert!(cut(&model, &design).contains("expression"));

    model.expression = "-(12 mm + 2 mm)".into();
    assert!(cut(&model, &design).contains("expression"));

    model.expression = "-(12 m)".into();
    assert!(cut(&model, &design).contains("expression"));
}
