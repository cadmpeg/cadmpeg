// SPDX-License-Identifier: Apache-2.0
use super::super::{
    eliminate_pivot_column, evaluate_nonlinear_residuals, infer_solve_variable_dimensions,
    nonlinear_equations_are_smooth, nonlinear_expression_is_smooth, nonlinear_initial_guesses,
    nonlinear_jacobian_rows, nonlinear_residuals_converged, refine_nonlinear_solution,
    solve_affine_expression_block, solve_nonlinear_expression_block, solve_unique_affine_system,
    SolveResidual, EPS_LINEAR_SYSTEM_COEFFICIENT,
};
use crate::curve::{CurveExpressionSolveBlock, RelationDimension, RelationEvaluationContext};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn visit_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    policy
}

fn empty_block() -> CurveExpressionSolveBlock {
    CurveExpressionSolveBlock {
        equations: Vec::new(),
        assignments: Vec::new(),
        unknowns: Vec::new(),
        offset: 0,
        for_offset: 0,
    }
}

#[test]
fn solver_fixed_precondition_rejections_preserve_original_refusal_without_work() {
    let block = empty_block();
    let values = BTreeMap::new();
    let arena = DecodeArena::new();
    let policy = visit_policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let check = |refused: bool| {
        // These are helper precondition rejections, not a parsed solve block.
        let results = [
            solve_nonlinear_expression_block(
                &ctx,
                &block,
                &values,
                &[],
                &[],
                RelationEvaluationContext::default(),
            )
            .map(|value| value.is_none()),
            nonlinear_expression_is_smooth(&ctx, ""),
            nonlinear_equations_are_smooth(&ctx, &block),
            nonlinear_initial_guesses(&ctx, &[], &[]).map(|value| value.is_none()),
            refine_nonlinear_solution(
                &ctx,
                &block,
                &values,
                &[],
                &[],
                RelationEvaluationContext::default(),
            )
            .map(|value| value.is_none()),
            nonlinear_jacobian_rows(
                &ctx,
                &block,
                &values,
                &[],
                &[],
                &[],
                RelationEvaluationContext::default(),
            )
            .map(|value| value.is_none()),
            evaluate_nonlinear_residuals(
                &ctx,
                &block,
                &values,
                &[],
                &[],
                RelationEvaluationContext::default(),
            )
            .map(|value| value.is_none()),
            nonlinear_residuals_converged(&ctx, &[]),
            eliminate_pivot_column(&ctx, &mut [], 0, 0, EPS_LINEAR_SYSTEM_COEFFICIENT)
                .map(|()| true),
            solve_unique_affine_system(&ctx, &mut [], 0).map(|value| value.is_none()),
            infer_solve_variable_dimensions(
                &ctx,
                &block,
                &values,
                &[None],
                RelationEvaluationContext::default(),
            )
            .map(|value| value.is_none()),
            solve_affine_expression_block(
                &ctx,
                &block,
                &values,
                &[RelationDimension::default()],
                RelationEvaluationContext::default(),
            )
            .map(|value| value.is_none()),
        ];
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("original seeded refusal");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            } else {
                assert!(result.expect("fixed precondition route"));
            }
        }
    };
    check(false);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx
        .charge_work_limit(1, "after fixed solver rejection")
        .expect_err("zero cap");
    assert_eq!(
        (original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1)
    );
    check(true);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn nonlinear_residual_convergence_visits_present_rows_until_first_failure() {
    let row = |value| SolveResidual {
        value,
        scale: 1.0,
        dimension: RelationDimension::default(),
    };
    let mut first = vec![row(1.0)];
    first.extend(std::iter::repeat_n(row(0.0), 128));
    let mut second = vec![row(0.0), row(1.0)];
    second.extend(std::iter::repeat_n(row(0.0), 128));
    for (rows, visits, expected) in [
        (Vec::new(), 0, true),
        (vec![row(0.0)], 1, true),
        (vec![row(0.0), row(0.0)], 2, true),
        (first, 1, false),
        (second, 2, false),
        (vec![row(f64::NAN)], 1, false),
    ] {
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
            let arena = DecodeArena::new();
            let policy = visit_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = nonlinear_residuals_converged(&ctx, &rows);
            let (original, admitted) = if cap == visits {
                assert_eq!(result.expect("present residuals"), expected);
                let original = ctx
                    .charge_work_limit(1, "after residual convergence")
                    .expect_err("exact visits");
                assert_eq!(
                    (original.dimension, original.used, original.additional),
                    (ResourceDimension::WorkUnits, visits, 1)
                );
                (original, true)
            } else {
                let original = ctx.resource_refusal().expect("residual visit refusal");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                assert_eq!(
                    (
                        original.dimension,
                        original.limit,
                        original.used,
                        original.additional,
                        original.operation
                    ),
                    (
                        ResourceDimension::WorkUnits,
                        cap,
                        cap,
                        1,
                        "creo nonlinear residual convergence"
                    )
                );
                (original, false)
            };
            assert!(
                matches!(nonlinear_residuals_converged(&ctx, &rows), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
            assert_eq!(ctx.resource_refusal(), Some(original));
            if admitted {
                Ok(())
            } else {
                Err(CodecError::ResourceLimit(original))
            }
        });
    }
}

#[test]
fn nonlinear_smooth_expression_stops_before_unneeded_tail() {
    let expression = format!("!{}", "x".repeat(128));
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
        let arena = DecodeArena::new();
        let policy = visit_policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = nonlinear_expression_is_smooth(&ctx, &expression);
        let (original, admitted) = if cap == 1 {
            assert!(!result.expect("first nonsmooth byte"));
            let original = ctx
                .charge_work_limit(1, "after nonsmooth expression")
                .expect_err("exact visit");
            assert_eq!(
                (original.dimension, original.used, original.additional),
                (ResourceDimension::WorkUnits, 1, 1)
            );
            (original, true)
        } else {
            let original = ctx.resource_refusal().expect("first byte refusal");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(
                (
                    original.dimension,
                    original.used,
                    original.additional,
                    original.operation
                ),
                (
                    ResourceDimension::WorkUnits,
                    0,
                    1,
                    "creo nonlinear expression scan"
                )
            );
            (original, false)
        };
        assert!(
            matches!(nonlinear_expression_is_smooth(&ctx, &expression), Err(CodecError::ResourceLimit(actual)) if actual == original)
        );
        assert_eq!(ctx.resource_refusal(), Some(original));
        if admitted {
            Ok(())
        } else {
            Err(CodecError::ResourceLimit(original))
        }
    });
}

#[test]
fn nonlinear_equation_smoothness_visits_present_rows_until_first_failure() {
    let equation = |left: &str| crate::curve::CurveExpressionEquation {
        left: left.to_owned(),
        right: "".to_owned(),
        dependencies: Vec::new(),
        offset: 0,
    };
    let mut early = vec![equation("!")];
    early[0].right = "x".repeat(128);
    early.extend(std::iter::repeat_n(equation("x"), 128));
    for (equations, visits, expected) in [
        (Vec::new(), 0, true),
        (vec![equation("")], 1, true),
        (vec![equation(""), equation("")], 2, true),
        (early, 2, false),
    ] {
        let mut block = empty_block();
        block.equations = equations;
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
            let arena = DecodeArena::new();
            let policy = visit_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = nonlinear_equations_are_smooth(&ctx, &block);
            let (original, admitted) = if cap == visits {
                assert_eq!(result.expect("present equations"), expected);
                let original = ctx
                    .charge_work_limit(1, "after equation smoothness")
                    .expect_err("exact visits");
                assert_eq!(
                    (original.dimension, original.used, original.additional),
                    (ResourceDimension::WorkUnits, visits, 1)
                );
                (original, true)
            } else {
                let original = ctx
                    .resource_refusal()
                    .expect("equation or byte visit refusal");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                let operation = if cap == 1 && !expected {
                    "creo nonlinear expression scan"
                } else {
                    "creo relation comparison traversal"
                };
                assert_eq!(
                    (
                        original.dimension,
                        original.limit,
                        original.used,
                        original.additional,
                        original.operation
                    ),
                    (ResourceDimension::WorkUnits, cap, cap, 1, operation)
                );
                (original, false)
            };
            assert!(
                matches!(nonlinear_equations_are_smooth(&ctx, &block), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
            assert_eq!(ctx.resource_refusal(), Some(original));
            if admitted {
                Ok(())
            } else {
                Err(CodecError::ResourceLimit(original))
            }
        });
    }
}
