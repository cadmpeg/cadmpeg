// SPDX-License-Identifier: Apache-2.0
//! Exact integer values in expression completeness checks.

#[test]
fn expression_completeness_refuses_inexact_integer_value() {
    let mut ir = super::one_incomplete_expression_parameter();
    ir.model.parameters[0].expression = "9007199254740993".into();
    ir.model.parameters[0].value = Some(cadmpeg_ir::features::ParameterValue::Integer(
        9_007_199_254_740_993,
    ));
    let incomplete = ({ super::super::incomplete_expression_parameters(&ir) })
        .expect("expression check is admitted");
    assert_eq!(incomplete, [ir.model.parameters[0].id.clone()].into());
}
