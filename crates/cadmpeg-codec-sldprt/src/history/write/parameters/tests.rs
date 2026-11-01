// SPDX-License-Identifier: Apache-2.0
use super::order_owner_parameters;
use cadmpeg_ir::features::{DesignParameter, Expression, ExpressionToken, ParameterValue};

fn parameter(name: &str, ordinal: u32, value: i64) -> DesignParameter {
    DesignParameter {
        product_definition: None,
        id: cadmpeg_ir::features::ParameterId::mint(format!("test:writer:parameter#{name}"))
            .unwrap(),
        owner: Some(cadmpeg_ir::features::FeatureId::mint("test:writer:feature#owner").unwrap()),
        ordinal,
        name: name.into(),
        expression: value.to_string().into(),
        driving: true,
        display: None,
        value: Some(ParameterValue::Integer(value)),
        properties: std::collections::BTreeMap::new(),
        native_ref: Some(format!("native-{name}")),
        pmi: None,
    }
}

#[test]
fn repeated_identity_edges_keep_dependencies_before_consumers_and_siblings_in_order() {
    let input = parameter("Input", 2, 2);
    let mut result = parameter("Result", 0, 4);
    result.expression = Expression {
        tokens: vec![
            ExpressionToken::Parameter(input.id.clone()),
            ExpressionToken::Text(" + ".into()),
            ExpressionToken::Parameter(input.id.clone()),
        ],
    };
    let other = parameter("Other", 1, 7);
    let parameters = vec![result, other, input];
    let before = parameters.clone();
    let ordered = order_owner_parameters(parameters.iter().collect());
    assert_eq!(ordered, [&parameters[1], &parameters[2], &parameters[0]]);
    assert_eq!(parameters, before);
}

#[test]
fn text_that_spells_a_parameter_name_is_not_an_identity_edge() {
    let mut result = parameter("Result", 0, 4);
    result.expression = "Input + 2".into();
    let input = parameter("Input", 1, 2);
    let parameters = [result, input];
    assert_eq!(
        order_owner_parameters(parameters.iter().collect()),
        parameters.iter().collect::<Vec<_>>()
    );
}

#[test]
fn cyclic_parameters_remain_exact_held_values_for_the_existing_writer_policy() {
    let mut first = parameter("First", 0, 2);
    let mut second = parameter("Second", 1, 3);
    first.expression = Expression {
        tokens: vec![ExpressionToken::Parameter(second.id.clone())],
    };
    second.expression = Expression {
        tokens: vec![ExpressionToken::Parameter(first.id.clone())],
    };
    let independent = parameter("Independent", 2, 7);
    let parameters = [first, second, independent];
    assert_eq!(
        order_owner_parameters(parameters.iter().collect()),
        [&parameters[2], &parameters[0], &parameters[1]]
    );
}
