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

#[test]
fn cacheless_law_differential_applies_elementary_functions_and_composition() {
    let inner = LawExpression::Algebraic {
        operator: "MUL".into(),
        operands: vec![
            LawExpression::Double { value: 2.0 },
            LawExpression::Text { value: "X".into() },
        ],
    };
    let law = LawExpression::Algebraic {
        operator: "SIN".into(),
        operands: vec![inner.clone()],
    };
    let differential = scalar_sweep_law_differential(&law, 0.75).expect("sine law");
    assert!((differential.value - 1.5f64.sin()).abs() <= f64::EPSILON * 64.0);
    assert!((differential.derivative - 2.0 * 1.5f64.cos()).abs() <= f64::EPSILON * 64.0);

    let composition = LawExpression::Algebraic {
        operator: "O".into(),
        operands: vec![
            LawExpression::Algebraic {
                operator: "COS".into(),
                operands: vec![LawExpression::Text { value: "X".into() }],
            },
            inner,
        ],
    };
    let differential =
        scalar_sweep_law_differential(&composition, 0.75).expect("composed cosine law");
    assert!((differential.value - 1.5f64.cos()).abs() <= f64::EPSILON * 64.0);
    assert!((differential.derivative + 2.0 * 1.5f64.sin()).abs() <= f64::EPSILON * 64.0);
}

#[test]
fn cacheless_law_differential_rejects_undefined_domains() {
    let absolute = LawExpression::Algebraic {
        operator: "ABS".into(),
        operands: vec![LawExpression::Text { value: "X".into() }],
    };
    assert!(scalar_sweep_law_differential(&absolute, 0.0).is_none());

    let inverse = LawExpression::Algebraic {
        operator: "ARCSIN".into(),
        operands: vec![LawExpression::Text { value: "X".into() }],
    };
    assert!(scalar_sweep_law_differential(&inverse, 1.0).is_none());
}
