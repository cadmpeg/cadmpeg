//! Resource refusal at recursive formula expression boundaries.

use std::collections::BTreeMap;

#[test]
fn formula_unary_descent_propagates_caller_depth_refusal() {
    crate::test_support::with_depth_limit(0, |ctx| {
        let result =
            super::super::evaluate_formula_expression_charged(ctx, "++++1", &BTreeMap::new());
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("resource refusal required")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RecursionDepth
        );
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn formula_power_descent_propagates_caller_depth_refusal() {
    crate::test_support::with_depth_limit(0, |ctx| {
        let result =
            super::super::evaluate_formula_expression_charged(ctx, "1**1**1", &BTreeMap::new());
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("resource refusal required")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RecursionDepth
        );
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn formula_local_unary_depth_refuses_instead_of_returning_absence() {
    let expression = format!("{}1", "+".repeat(129));
    crate::test_support::with_depth_limit(256, |ctx| {
        let result =
            super::super::evaluate_formula_expression_charged(ctx, &expression, &BTreeMap::new());
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("resource refusal required")
        };
        assert_eq!(limit.operation, "catia_formula_expression_local_depth");
        assert_eq!(limit.limit, 128);
        assert_eq!(limit.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn formula_bound_string_operations_admit_operand_work() {
    use super::super::{EvaluatedFormulaString, EvaluatedFormulaValue};
    for (source, operation) in [
        ("#1_.Length()", "catia_formula_string_length"),
        ("#1_.Search(\"x\")", "catia_formula_string_search"),
        ("#1_.Extract(0,1)", "catia_formula_string_boundary"),
        ("#1_.ToReal()", "catia_formula_string_real"),
        (
            "ReplaceSubText(#1_,\"x\",\"y\")",
            "catia_formula_replace_subtext",
        ),
        ("ToUpper(#1_)", "catia_formula_string_case"),
    ] {
        let bindings = BTreeMap::from([(
            "#1_",
            EvaluatedFormulaValue::String(EvaluatedFormulaString::from_parts(
                "x".repeat(4096),
                true,
            )),
        )]);
        let result = crate::test_support::with_work_refusal(operation, |ctx| {
            let result = super::super::evaluate_formula_expression_charged(ctx, source, &bindings);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        });
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == operation)
        );
    }
}

#[test]
fn formula_literal_scan_propagates_caller_work_refusal() {
    crate::test_support::with_work_limit(0, |ctx| {
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            super::super::string_literal_expression(ctx, "text")
        else {
            panic!("literal scan must refuse")
        };
        assert_eq!(limit.operation, "catia_formula_string_literal_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn legacy_symbol_ordinal_scan_propagates_caller_work_refusal() {
    crate::test_support::with_work_limit(8, |ctx| {
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            super::super::legacy_symbol_matches_input(ctx, "#1_/12", "#1_")
        else {
            panic!("ordinal scan must refuse")
        };
        assert_eq!(limit.operation, "catia_legacy_symbol_ordinal_visits");
        assert_eq!((limit.used, limit.additional), (8, 1));
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
    // Three prefix bytes, three suffix bytes, two digits and the end probe.
    crate::test_support::with_work_limit(9, |ctx| {
        assert!(
            super::super::legacy_symbol_matches_input(ctx, "#1_/12", "#1_")
                .expect("exact ordinal budget")
        );
    });
}

#[test]
fn legacy_symbol_ordinal_scan_preserves_matching_rules() {
    crate::test_support::with_service_context(|ctx| {
        for symbol in ["#1_", "#1_ /12"] {
            assert!(
                super::super::legacy_symbol_matches_input(ctx, symbol, "#1_")
                    .expect("service work budget")
            );
        }
        for symbol in ["#1_/", "#1_/x", "#2_"] {
            assert!(
                !super::super::legacy_symbol_matches_input(ctx, symbol, "#1_")
                    .expect("service work budget")
            );
        }
    });
}

#[test]
fn formula_string_boundary_admission_preserves_unicode_indices() {
    crate::test_support::with_service_context(|ctx| {
        let bindings = std::collections::BTreeMap::new();
        let mut parser = super::super::FormulaExpressionParser {
            source: "",
            at: 0,
            bindings: &bindings,
            ctx,
            evaluate: false,
            static_check: false,
        };
        for (index, expected) in [
            (0, Some(0)),
            (1, Some(1)),
            (2, Some(3)),
            (3, Some(7)),
            (4, None),
        ] {
            assert_eq!(
                parser
                    .string_boundary("aé😀", index)
                    .expect("service work budget"),
                expected
            );
        }
    });
}

#[test]
fn formula_string_boundary_scan_propagates_resource_refusal() {
    crate::test_support::with_work_limit(0, |ctx| {
        let bindings = std::collections::BTreeMap::new();
        let mut parser = super::super::FormulaExpressionParser {
            source: "",
            at: 0,
            bindings: &bindings,
            ctx,
            evaluate: false,
            static_check: false,
        };
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = parser.string_boundary("é", 0)
        else {
            panic!("boundary scan must refuse")
        };
        assert_eq!(limit.operation, "catia_formula_string_boundary");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn formula_string_length_scan_propagates_resource_refusal() {
    let refusal = crate::test_support::with_work_refusal("catia_formula_string_length", |ctx| {
        let bindings = std::collections::BTreeMap::new();
        let mut parser = super::super::FormulaExpressionParser {
            source: "\"a\".Length()",
            at: 0,
            bindings: &bindings,
            ctx,
            evaluate: true,
            static_check: false,
        };
        parser.postfix(0)
    });
    assert!(
        matches!(refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "catia_formula_string_length")
    );
}

#[test]
fn formula_unsupported_first_token_does_not_admit_the_suffix() {
    let source = format!("!{}", "x".repeat(4096));
    // The whitespace probes at unary and primary, then the first numeric scan step.
    crate::test_support::with_work_limit(3, |ctx| {
        assert!(
            super::super::evaluate_formula_expression_charged(ctx, &source, &BTreeMap::new())
                .expect("unvisited suffix costs no work")
                .is_none()
        );
    });
}

#[test]
fn formula_numeric_parse_and_binding_lookup_refuse_before_execution() {
    use super::super::{EvaluatedFormulaString, EvaluatedFormulaValue};
    let bindings = BTreeMap::from([(
        "#1_",
        EvaluatedFormulaValue::String(EvaluatedFormulaString::from_parts("abc".to_owned(), true)),
    )]);
    for (source, operation) in [
        ("1234", "catia_formula_numeric_parse"),
        ("#1_", "catia_formula_symbol_lookup"),
    ] {
        let refusal = crate::test_support::with_work_refusal(operation, |ctx| {
            super::super::evaluate_formula_expression_charged(ctx, source, &bindings)
        });
        assert!(
            matches!(refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation)
        );
    }
}

#[test]
fn formula_search_admits_only_the_resolved_range() {
    for (start, cap, expected) in [(4, 5, -1), (2, 7, 2)] {
        crate::test_support::with_work_limit(cap, |ctx| {
            let bindings = BTreeMap::new();
            let mut parser = super::super::FormulaExpressionParser {
                source: "",
                at: 0,
                bindings: &bindings,
                ctx,
                evaluate: true,
                static_check: false,
            };
            let needle = if start == 4 {
                "x".repeat(4096)
            } else {
                "c".to_owned()
            };
            assert_eq!(
                parser
                    .search_string("abc", &needle, start, true)
                    .expect("range budget"),
                Some(expected)
            );
        });
    }
}

#[test]
fn formula_case_conversion_preserves_per_character_lowercase() {
    assert_eq!(
        super::super::evaluate_formula_expression("ToLower(\"ΟΣİ\")", &BTreeMap::new())
            .and_then(super::super::EvaluatedFormulaValue::string),
        Some("οσi\u{307}".to_owned())
    );
}
