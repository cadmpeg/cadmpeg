// SPDX-License-Identifier: Apache-2.0
//! Formula unit tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::{
    evaluate_formula_expression, evaluate_formula_expression_with_mode, outer_container_in_scope,
    EvaluatedFormulaValue, LegacyModelingScope,
};
use crate::native::CatiaOuterContainerBinding;
use std::collections::BTreeMap;

mod evaluate;
mod expression_admission;
mod legacy_admission;
mod transfer;

pub(super) fn evaluated<'a>(
    source: &'a str,
    bindings: &BTreeMap<&'a str, EvaluatedFormulaValue>,
) -> Option<EvaluatedFormulaValue> {
    crate::test_support::with_service_context(|ctx| {
        evaluate_formula_expression(ctx, source, bindings)
    })
    .expect("evaluate formula expression")
}

pub(super) fn evaluated_with_mode<'a>(
    source: &'a str,
    bindings: &BTreeMap<&'a str, EvaluatedFormulaValue>,
    evaluate: bool,
) -> Option<EvaluatedFormulaValue> {
    crate::test_support::with_service_context(|ctx| {
        evaluate_formula_expression_with_mode(ctx, source, bindings, evaluate)
    })
    .expect("evaluate formula expression with mode")
}

fn binding(stream_name: &str) -> CatiaOuterContainerBinding {
    CatiaOuterContainerBinding {
        data_offset: 10,
        ordinal: 2,
        class_name: "CATPrtCont".to_string(),
        base_class: "CATProdCont".to_string(),
        stream_name: stream_name.to_string(),
    }
}

#[test]
fn legacy_parameter_scope_requires_the_exact_modeling_container() {
    let part = binding("part");
    let other_part = binding("other-part");

    assert!(outer_container_in_scope(
        Some(&part),
        LegacyModelingScope::Container(&part)
    ));
    assert!(!outer_container_in_scope(
        Some(&other_part),
        LegacyModelingScope::Container(&part)
    ));
    assert!(!outer_container_in_scope(
        None,
        LegacyModelingScope::Container(&part)
    ));
    assert!(!outer_container_in_scope(
        Some(&part),
        LegacyModelingScope::Unresolved
    ));
}

#[test]
fn legacy_parameter_scope_admits_unbound_fragment_runs() {
    assert!(outer_container_in_scope(
        None,
        LegacyModelingScope::Unbounded
    ));
}

fn inactive_string_index(value: f64) -> Option<usize> {
    crate::test_support::with_service_context(|ctx| {
        let bindings = std::collections::BTreeMap::new();
        let parser = super::FormulaExpressionParser {
            source: "",
            at: 0,
            bindings: &bindings,
            ctx,
            refusal: None,
            evaluate: false,
            static_check: false,
        };
        parser.string_index(super::EvaluatedFormulaScalar::from_parts(
            value,
            super::FormulaDimension::SCALAR,
            Some(true),
            Some(value),
        ))
    })
}

#[test]
fn inactive_string_index_uses_placeholder_for_positive_infinity() {
    assert_eq!(inactive_string_index(f64::INFINITY), Some(0));
}

#[test]
fn inactive_string_index_preserves_negative_placeholder() {
    assert_eq!(inactive_string_index(-1.0), Some(0));
}

#[test]
fn inactive_string_index_preserves_nan_placeholder() {
    assert_eq!(inactive_string_index(f64::NAN), Some(0));
}

#[test]
fn active_formula_refuses_unknown_integer_binding() {
    let bindings = std::collections::BTreeMap::from([(
        "#1_",
        super::static_formula_value(super::FormulaParameterType::Integer),
    )]);
    assert!(evaluated("#1_ + 1", &bindings).is_none());
}

#[test]
fn inactive_formula_preserves_unknown_integer_type() {
    let bindings = std::collections::BTreeMap::from([(
        "#1_",
        super::static_formula_value(super::FormulaParameterType::Integer),
    )]);
    let result = evaluated("false ? #1_ + 1 ; 2", &bindings)
        .and_then(super::EvaluatedFormulaValue::scalar)
        .expect("known active result");
    assert_eq!(result.known_value(), Some(2.0));
    assert!(evaluated_with_mode("#1_ + 1", &bindings, false).is_some());
}
