// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn cacheless_law_differential_applies_algebraic_product_rule() {
    let law = LawExpression::Algebraic {
        operator: "MUL".into(),
        operands: vec![
            LawExpression::Double { value: 2.0 },
            LawExpression::Text { value: "X".into() },
        ],
    };
    let differential = scalar_sweep_law_differential(&law, 3.0).expect("law differential");
    assert_eq!(differential.value, 6.0);
    assert_eq!(differential.derivative, 2.0);
}
