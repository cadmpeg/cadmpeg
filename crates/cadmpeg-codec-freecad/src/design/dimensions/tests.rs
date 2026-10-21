// SPDX-License-Identifier: Apache-2.0
//! Derived controls retain nonliteral formulas and their source driver identities.

use super::*;
use cadmpeg_ir::{features::ParameterValue, scalar::PositiveLength};

#[test]
fn magnitude_keeps_the_authored_source_formula_and_cache() {
    let owner = FeatureId::mint("test:feature:thickness#0").unwrap();
    let driver = ParameterId::mint("test:parameter:driver#0").unwrap();
    let raw = ParameterId::mint("test:parameter:value#0").unwrap();
    let source = DesignParameter {
        id: raw.clone(),
        owner: Some(owner.clone()),
        ordinal: 0,
        name: "Value".into(),
        expression: Expression {
            tokens: vec![
                ExpressionToken::Text("-2 * ".into()),
                ExpressionToken::Parameter(driver),
            ],
        },
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(-2.0).unwrap(),
        )),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("native:Value".into()),
    };
    let mut parameters = vec![source.clone()];
    let mut dimension = Dimension::Literal(PositiveLength::new(2.0).unwrap());
    computed_control(
        &mut dimension,
        &owner,
        &mut parameters,
        "Value",
        "magnitude",
        "abs(",
    )
    .unwrap();
    assert_eq!(parameters[0], source);
    assert_eq!(
        parameters[1].expression.tokens,
        vec![
            ExpressionToken::Text("abs(".into()),
            ExpressionToken::Parameter(raw),
            ExpressionToken::Text(")".into()),
        ]
    );
    assert_eq!(dimension.parameter(), Some(&parameters[1].id));
    assert_eq!(dimension.value_from(&parameters).unwrap().get(), 2.0);
    assert!(!parameters[1].driving);
}
