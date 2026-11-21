// SPDX-License-Identifier: Apache-2.0
//! Part parameters and formulas in the v11 design layer.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    self, BinaryOp, Expression, ExpressionDialect, ExpressionToken, Neutral, Node,
};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use cadmpeg_ir::design::units::{Quantity, ValueKind};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::test_formula::{
    standard_catpart_with_formula_relation, standard_catpart_with_typed_formula_inputs,
    standard_catpart_with_typed_formula_inputs_and_object_payload,
    standard_catpart_with_typed_formula_relation,
};
use crate::CatiaCodec;

fn decode(bytes: Vec<u8>) -> EditableDecodeResult {
    EditableDecodeResult::from(
        CatiaCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap(),
    )
}

fn parameter<'a>(result: &'a EditableDecodeResult, name: &str) -> &'a DesignParameter {
    result
        .ir()
        .design
        .parameters
        .iter()
        .find(|parameter| {
            parameter
                .name
                .as_ref()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                == Some(name)
        })
        .unwrap()
}

fn reference(id: &ParameterId) -> ParameterRef {
    ParameterRef {
        target: RefTarget::Parameter(id.clone()),
        through: None,
    }
}

fn neutral(text: &str, refs: &[ParameterRef]) -> ParameterDefinition {
    ParameterDefinition::Expression(Expression::Neutral(Neutral(
        expr::parse(text, refs, &[]).unwrap(),
    )))
}

#[test]
fn a_literal_parameter_holds_its_stored_value() {
    let result = decode(standard_catpart_with_formula_relation(4, false));
    let thickness = parameter(&result, "Thickness");
    assert_eq!(thickness.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(
        thickness.value,
        Some(ParameterValue::Number(FiniteReal::new(35.0).unwrap()))
    );
    assert_eq!(thickness.definition, neutral("35[mm]", &[]));
    assert_eq!(thickness.owner, None);
    assert_eq!(thickness.unit, None);
    let legacy = &result.ir().model.parameters[0];
    assert_eq!(thickness.native_ref, legacy.native_ref);
}

#[test]
fn a_formula_of_unit_arithmetic_is_neutral() {
    let result = decode(standard_catpart_with_formula_relation(4, false));
    let thickness = parameter(&result, "Thickness");
    let output = parameter(&result, "Result");
    assert_eq!(output.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(
        output.value,
        Some(ParameterValue::Number(FiniteReal::new(33.0).unwrap()))
    );
    let ParameterDefinition::Expression(Expression::Neutral(Neutral(Node::Binary {
        op: BinaryOp::Subtract,
        left,
        ..
    }))) = &output.definition
    else {
        panic!("a neutral difference: {:?}", output.definition);
    };
    assert_eq!(**left, Node::Reference(reference(&thickness.id)));
    assert_eq!(
        output.definition,
        neutral("$0 - 2[mm]", &[reference(&thickness.id)])
    );
}

#[test]
fn a_constant_formula_keeps_its_formula() {
    let result = decode(standard_catpart_with_typed_formula_inputs(
        3,
        false,
        &[],
        "LENGTH",
        Some(12.0),
        "10mm+2mm",
    ));
    let output = parameter(&result, "Result");
    assert_eq!(output.definition, neutral("10[mm] + 2[mm]", &[]));
}

#[test]
fn a_predicate_keeps_its_source_text_with_resolved_bindings() {
    let result = decode(standard_catpart_with_typed_formula_inputs(
        5,
        false,
        &[
            ("#1_", "Real", "X", "#1_ /2", 5.0),
            ("#2_", "Real", "Y", "#2_ /2", 3.0),
        ],
        "Boolean",
        None,
        "(#1_ /2>#2_ /2) and (#1_ /2>=0)",
    ));
    let x = reference(&parameter(&result, "X").id);
    let y = reference(&parameter(&result, "Y").id);
    let output = parameter(&result, "Result");
    assert_eq!(output.kind, ValueKind::Boolean);
    assert_eq!(output.value, None);
    assert_eq!(
        output.definition,
        ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::Catia,
            tokens: vec![
                ExpressionToken::Text("(".into()),
                ExpressionToken::Reference(x.clone()),
                ExpressionToken::Text(">".into()),
                ExpressionToken::Reference(y),
                ExpressionToken::Text(") and (".into()),
                ExpressionToken::Reference(x),
                ExpressionToken::Text(">=0)".into()),
            ],
        })
    );
}

#[test]
fn a_formula_of_an_integer_and_an_angle_is_neutral() {
    let result = decode(standard_catpart_with_typed_formula_relation(
        4,
        false,
        "Integer",
        "ANGLE",
        2.0,
        0.5,
        "#1_ /2*0.25rad",
    ));
    let input = parameter(&result, "Thickness");
    assert_eq!(input.kind, ValueKind::Integer);
    assert_eq!(input.value, Some(ParameterValue::Integer(2)));
    assert_eq!(input.definition, neutral("2", &[]));
    let output = parameter(&result, "Result");
    assert_eq!(output.kind, ValueKind::Quantity(Quantity::ANGLE));
    assert_eq!(
        output.definition,
        neutral("$0 * 0.25[rad]", &[reference(&input.id)])
    );
}

#[test]
fn a_formula_whose_tree_disagrees_with_its_stored_value_keeps_its_source_text() {
    let result = decode(
        standard_catpart_with_typed_formula_inputs_and_object_payload(
            4,
            false,
            &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
            "LENGTH",
            None,
            "#1_ /2+1mm",
            (&[0xfe], Some(0)),
        ),
    );
    let width = reference(&parameter(&result, "Width").id);
    let formula = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| !parameter.dependencies.is_empty())
        .unwrap();
    let output = result
        .ir()
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.id == formula.id)
        .unwrap();
    assert_eq!(
        output.value,
        Some(ParameterValue::Number(FiniteReal::new(1.0).unwrap()))
    );
    assert_eq!(
        output.definition,
        ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::Catia,
            tokens: vec![
                ExpressionToken::Reference(width),
                ExpressionToken::Text("+1mm".into()),
            ],
        })
    );
}

#[test]
fn a_formula_that_is_one_text_input_is_its_reference() {
    let result = decode(
        standard_catpart_with_typed_formula_inputs_and_object_payload(
            4,
            false,
            &[("#1_", "String", "Value", "#1_", 1.0)],
            "String",
            None,
            "#1_",
            (&[0xfe], Some(0)),
        ),
    );
    let [input, formula] = result.ir().model.parameters.as_slice() else {
        panic!("an input and a formula");
    };
    let output = result
        .ir()
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.id == formula.id)
        .unwrap();
    assert_eq!(output.kind, ValueKind::Text);
    assert_eq!(
        output.definition,
        ParameterDefinition::Expression(Expression::Neutral(Neutral(Node::Reference(reference(
            &input.id
        )))))
    );
}

#[test]
fn a_formula_that_is_one_input_of_another_kind_keeps_its_text() {
    let result = decode(
        standard_catpart_with_typed_formula_inputs_and_object_payload(
            4,
            false,
            &[("#1_", "String", "Value", "#1_", 1.0)],
            "String",
            None,
            "#1_",
            (&[0xfe], Some(0)),
        ),
    );
    let [input, _] = result.ir().model.parameters.as_slice() else {
        panic!("an input and a formula");
    };
    let parameters = BTreeMap::from([(&input.id, input)]);
    for kind in [ValueKind::Boolean, ValueKind::Quantity(Quantity::LENGTH)] {
        assert_eq!(
            super::whole_reference(&reference(&input.id), "#1_", &kind, &parameters),
            Expression::Source {
                dialect: ExpressionDialect::Catia,
                tokens: vec![ExpressionToken::Text("#1_".into())],
            }
        );
    }
}
