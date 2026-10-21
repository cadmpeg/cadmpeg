// SPDX-License-Identifier: Apache-2.0
//! Native scalar cache integration preserves authored expression tokens.

use super::super::set_typed_parameter_cache;
use cadmpeg_ir::features::{
    DesignParameter, Expression, ExpressionToken, ParameterId, ParameterValue,
};
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
use std::collections::BTreeMap;

fn parameter(expression: Expression, value: ParameterValue) -> DesignParameter {
    DesignParameter {
        id: ParameterId::mint("test:model:parameter#cache").unwrap(),
        owner: None,
        ordinal: 0,
        name: "Control".into(),
        expression,
        value: Some(value),
        properties: BTreeMap::default(),
        driving: true,
        display: None,
        native_ref: None,
        pmi: None,
    }
}

#[test]
fn agreeing_scalar_cache_keeps_authored_literal_spelling() {
    for (text, old, admitted) in [
        (
            "12.5mm",
            ParameterValue::Length(Length::new(12.5).unwrap()),
            ParameterValue::Length(Length::new(12.5).unwrap()),
        ),
        (
            "0°",
            ParameterValue::Angle(Angle::new(0.0).unwrap()),
            ParameterValue::Angle(Angle::new(0.0).unwrap()),
        ),
        (
            "5deg",
            ParameterValue::Angle(Angle::new(5.0 * (std::f64::consts::PI / 180.0)).unwrap()),
            ParameterValue::Angle(Angle::new(5.0 * (std::f64::consts::PI / 180.0)).unwrap()),
        ),
        (
            "1.25",
            ParameterValue::Real(FiniteReal::new(1.25).unwrap()),
            ParameterValue::Length(Length::new(1.25).unwrap()),
        ),
    ] {
        let authored = Expression::text(text);
        let mut control = parameter(authored.clone(), old);
        set_typed_parameter_cache(&mut control, admitted.clone());
        assert_eq!(control.expression, authored);
        assert_eq!(control.value, Some(admitted));
    }
}

#[test]
fn conflicting_literal_keeps_actual_admitted_cache_instead_of_text_value() {
    let admitted = ParameterValue::Length(Length::new(7.0).unwrap());
    let mut control = parameter(Expression::text("8mm"), admitted.clone());
    set_typed_parameter_cache(&mut control, admitted.clone());
    assert_eq!(control.value, Some(admitted.clone()));
    assert_eq!(control.expression, admitted.literal_expression());
    assert_eq!(control.expression, Expression::text("7 mm"));
}

#[test]
fn scalar_cache_does_not_replace_authored_parameter_reference() {
    let dependency = ParameterId::mint("test:model:parameter#dependency").unwrap();
    let authored = Expression {
        tokens: vec![ExpressionToken::Parameter(dependency)],
    };
    let mut control = parameter(
        authored.clone(),
        ParameterValue::Length(Length::new(7.0).unwrap()),
    );
    let admitted = ParameterValue::Length(Length::new(8.0).unwrap());
    set_typed_parameter_cache(&mut control, admitted.clone());
    assert_eq!(control.expression, authored);
    assert_eq!(control.value, Some(admitted));
}
