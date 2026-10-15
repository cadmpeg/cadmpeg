// SPDX-License-Identifier: Apache-2.0
use crate::curve::{
    parse_relation_expression, relation_unit, solve_unique_affine_system, AffineEquationRow,
    CurveExpressionValue, RelationEvaluationContext,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn with_policy<T>(
    policy: DecodePolicy,
    run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test root");
    let result = run(&ctx);
    if let Err(CodecError::ResourceLimit(limit)) = &result {
        assert_eq!(ctx.resource_refusal(), Some(*limit));
    }
    result
}

#[test]
fn relation_unit_local_depth_ceiling_leaves_the_unit_unresolved() {
    let source = format!("{}mm{}", "(".repeat(129), ")".repeat(129));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    let resolved = with_policy(policy, |ctx| {
        let resolved = relation_unit(ctx, &source).map(|unit| unit.is_some());
        assert!(ctx.resource_refusal().is_none());
        resolved
    })
    .expect("the local unit ceiling is not a decode failure");
    assert!(!resolved);
    let admitted = format!("{}mm{}", "(".repeat(128), ")".repeat(128));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    assert!(with_policy(policy, |ctx| {
        relation_unit(ctx, &admitted).map(|unit| unit.is_some())
    })
    .expect("nesting at the ceiling parses"));
}

/// Parse `source` with session depth above the local ceiling and assert
/// the local ceiling leaves the expression unevaluated without a refusal.
fn assert_local_nesting_ceiling_skips(source: &str, admitted: &str) {
    for (source, evaluated) in [(source, false), (admitted, true)] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1024;
        let value = with_policy(policy, |ctx| {
            let value = parse_relation_expression::<CurveExpressionValue>(
                ctx,
                source,
                &BTreeMap::new(),
                RelationEvaluationContext::default(),
            );
            assert!(ctx.resource_refusal().is_none());
            value
        })
        .expect("the local nesting ceiling is not a decode failure");
        assert_eq!(value.is_some(), evaluated, "{source}");
    }
}

#[test]
fn relation_local_nesting_ceiling_leaves_the_expression_unevaluated() {
    assert_local_nesting_ceiling_skips(
        &format!("{}1{}", "(".repeat(129), ")".repeat(129)),
        &format!("{}1{}", "(".repeat(128), ")".repeat(128)),
    );
}

#[test]
fn affine_single_row_system_solves() {
    crate::decode::with_service_context(|ctx| {
        assert_eq!(
            solve_unique_affine_system(
                ctx,
                &mut [AffineEquationRow {
                    coefficients: vec![1.0],
                    rhs: 2.0
                }],
                1
            )
            .expect("service"),
            Some(vec![2.0])
        );
    });
}

#[test]
fn relation_values_reject_nonfinite_numbers_and_dedicated_residual_dimensions() {
    use crate::curve::{quantity_value, CurveExpressionQuantity, RelationDimension};
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for dimension in [
            RelationDimension::default(),
            RelationDimension::LENGTH,
            RelationDimension::ANGLE,
            RelationDimension::TIME,
        ] {
            assert!(quantity_value(value, dimension).is_none());
        }
        assert!(CurveExpressionQuantity::new(value, [0, 0, 1, 0, 0]).is_none());
    }
    for powers in [[0; 5], [1, 0, 0, 0, 0], [0, 0, 0, 1, 0]] {
        assert!(CurveExpressionQuantity::new(1.0, powers).is_none());
    }
    let value = quantity_value(1.0, RelationDimension::default()).expect("finite number");
    assert!(matches!(value, CurveExpressionValue::Number(_)));
    assert_eq!(value.truth(), Some(true));
    assert!(matches!(
        quantity_value(1.0, RelationDimension::LENGTH),
        Some(CurveExpressionValue::Length(_))
    ));
    assert!(matches!(
        quantity_value(1.0, RelationDimension::ANGLE),
        Some(CurveExpressionValue::Angle(_))
    ));
}

#[test]
fn relation_quantity_serialization_preserves_base_power_fields() {
    let value = crate::curve::CurveExpressionQuantity::new(3.5, [1, 2, 0, 0, 0])
        .expect("finite residual quantity");
    assert_eq!(
        serde_json::to_string(&value).expect("quantity JSON"),
        r#"{"value":3.5,"length_power":1,"mass_power":2,"time_power":0,"angle_power":0,"temperature_power":0}"#
    );
}

fn nonlinear_ceiling_solution(left: &str, seed: f64) -> Option<Vec<f64>> {
    use crate::curve::{
        refine_nonlinear_solution, CurveExpressionEquation, CurveExpressionSolveBlock,
        RelationDimension, SolveUnknown,
    };
    let block = CurveExpressionSolveBlock {
        equations: vec![CurveExpressionEquation {
            left: left.to_owned(),
            right: "0".to_owned(),
            dependencies: vec!["x".to_owned()],
            offset: 0,
        }],
        assignments: Vec::new(),
        unknowns: vec![SolveUnknown {
            name: "x".to_owned(),
            solution: None,
        }],
        offset: 0,
        for_offset: 1,
    };
    with_policy(DecodePolicy::service(), |ctx| {
        let solution = refine_nonlinear_solution(
            ctx,
            &block,
            &BTreeMap::new(),
            &[RelationDimension::default()],
            &[seed],
            RelationEvaluationContext::default(),
        );
        assert!(ctx.resource_refusal().is_none());
        solution
    })
    .expect("a local solver ceiling is not a decode failure")
}

#[test]
fn nonlinear_iteration_ceiling_leaves_a_progressing_smooth_system_unsolved() {
    assert!(nonlinear_ceiling_solution("x+100000000000000000000*x*x*x+0.001", 0.0).is_none());
    assert!(nonlinear_ceiling_solution("x*x*x+x-2", 0.5).is_some());
}

#[test]
fn nonlinear_line_search_ceiling_leaves_valid_non_improving_probes_unsolved() {
    const SMALL_NONZERO_SEED: f64 = 0.000_000_1;
    assert!(nonlinear_ceiling_solution("x*x+1", SMALL_NONZERO_SEED).is_none());
}

#[test]
fn relation_local_exponent_nesting_ceiling_leaves_the_expression_unevaluated() {
    assert_local_nesting_ceiling_skips(
        &format!("{}1", "1^".repeat(129)),
        &format!("{}1", "1^".repeat(128)),
    );
}

#[test]
fn relation_local_function_nesting_ceiling_leaves_the_expression_unevaluated() {
    assert_local_nesting_ceiling_skips(
        &format!("{}1{}", "abs(".repeat(129), ")".repeat(129)),
        &format!("{}1{}", "abs(".repeat(128), ")".repeat(128)),
    );
}

#[test]
fn affine_two_row_system_solves() {
    let solution = crate::decode::with_service_context(|ctx| {
        solve_unique_affine_system(
            ctx,
            &mut [
                AffineEquationRow {
                    coefficients: vec![1.0, 1.0],
                    rhs: 5.0,
                },
                AffineEquationRow {
                    coefficients: vec![1.0, 2.0],
                    rhs: 8.0,
                },
            ],
            2,
        )
    })
    .expect("service");
    assert_eq!(solution, Some(vec![2.0, 3.0]));
}

#[test]
fn relation_operators_evaluate_with_precedence() {
    let value = crate::decode::with_service_context(|ctx| {
        parse_relation_expression::<CurveExpressionValue>(
            ctx,
            "1+2*3",
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        )
    })
    .expect("service");
    assert_eq!(
        value,
        crate::curve::quantity_value(7.0, crate::curve::RelationDimension::default())
    );
}

#[test]
fn nonlinear_cubic_negative_seeds_preserve_progress_with_one_residual_scale() {
    use crate::curve::{
        refine_nonlinear_solution, CurveExpressionEquation, CurveExpressionSolveBlock,
        RelationDimension, SolveUnknown, NONLINEAR_SOLVE_SOLUTION_TOLERANCE,
    };
    let block = CurveExpressionSolveBlock {
        equations: vec![CurveExpressionEquation {
            left: "x*x*x".to_owned(),
            right: "8".to_owned(),
            dependencies: vec!["x".to_owned()],
            offset: 0,
        }],
        assignments: Vec::new(),
        unknowns: vec![SolveUnknown {
            name: "x".to_owned(),
            solution: None,
        }],
        offset: 0,
        for_offset: 1,
    };
    for seed in [-100.0, -10.0, -1.0, -0.1, -0.01] {
        let solution = with_policy(DecodePolicy::service(), |ctx| {
            refine_nonlinear_solution(
                ctx,
                &block,
                &BTreeMap::new(),
                &[RelationDimension::default()],
                &[seed],
                RelationEvaluationContext::default(),
            )
        })
        .expect("negative cubic seed admits refinement")
        .expect("unique cubic root");
        assert_eq!(solution.len(), 1);
        assert!((solution[0] - 2.0).abs() <= NONLINEAR_SOLVE_SOLUTION_TOLERANCE);
    }
}
