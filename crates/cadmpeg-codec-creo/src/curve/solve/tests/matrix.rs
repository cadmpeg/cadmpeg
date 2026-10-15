// SPDX-License-Identifier: Apache-2.0

use crate::curve::solve::{
    refine_nonlinear_solution, solve_unique_affine_system, AffineEquationRow,
    NONLINEAR_SOLVE_SOLUTION_TOLERANCE,
};
use crate::curve::test_support::with_expression_policy;
use crate::curve::RelationEvaluationContext;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
const EPS_INDEPENDENT_EQUATION_SCALE: f64 = 1.0e-12;

#[test]
fn affine_solver_is_invariant_under_independent_equation_scaling() {
    let mut rows = [
        AffineEquationRow {
            coefficients: vec![1e-15, 0.0],
            rhs: 6e-15,
        },
        AffineEquationRow {
            coefficients: vec![0.0, 1e15],
            rhs: 4e15,
        },
    ];

    let solution =
        crate::decode::with_test_decode_ctx(|ctx| solve_unique_affine_system(ctx, &mut rows, 2))
            .expect("service profile")
            .expect("independently scaled unique system");
    assert!((solution[0] - 6.0).abs() <= EPS_INDEPENDENT_EQUATION_SCALE);
    assert!((solution[1] - 4.0).abs() <= EPS_INDEPENDENT_EQUATION_SCALE);
}

#[test]
fn affine_matrix_elimination_refuses_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = crate::test_support::allocation_limit_at(
        ResourceDimension::WorkUnits,
        Some("creo matrix row normalization"),
        |cap| {
            let mut trial = policy;
            trial.limits.max_work_units = cap;
            with_expression_policy(trial, |ctx| {
                solve_unique_affine_system(
                    ctx,
                    &mut [AffineEquationRow {
                        coefficients: vec![1.0],
                        rhs: 2.0,
                    }],
                    1,
                )
            })
        },
    );
    let error = with_expression_policy(policy, |ctx| {
        solve_unique_affine_system(
            ctx,
            &mut [AffineEquationRow {
                coefficients: vec![1.0],
                rhs: 2.0,
            }],
            1,
        )
    })
    .expect_err("matrix work must refuse");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo matrix row normalization")
    );
    crate::decode::with_test_decode_ctx(|ctx| {
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

fn nonlinear_ceiling_error(left: &str, seed: f64) -> CodecError {
    use crate::curve::{
        CurveExpressionEquation, CurveExpressionSolveBlock, RelationDimension, SolveUnknown,
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
    with_expression_policy(DecodePolicy::service(), |ctx| {
        refine_nonlinear_solution(
            ctx,
            &block,
            &BTreeMap::new(),
            &[RelationDimension::default()],
            &[seed],
            RelationEvaluationContext::default(),
        )
    })
    .expect_err("local solver ceiling refuses")
}

#[test]
fn nonlinear_iteration_ceiling_refuses_a_progressing_smooth_system() {
    let error = nonlinear_ceiling_error("x+100000000000000000000*x*x*x+0.001", 0.0);
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo nonlinear iteration ceiling")
    );
}

#[test]
fn nonlinear_line_search_ceiling_refuses_valid_non_improving_probes() {
    const SMALL_NONZERO_SEED: f64 = 0.000_000_1;
    let error = nonlinear_ceiling_error("x*x+1", SMALL_NONZERO_SEED);
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo nonlinear line-search ceiling")
    );
}

#[test]
fn affine_pivot_and_coefficient_elimination_refuse_work() {
    let solution = crate::test_support::assert_work_boundaries(
        &["creo matrix row normalization", "creo matrix pivot scan"],
        |ctx| {
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
        },
    );
    assert_eq!(solution, Some(vec![2.0, 3.0]));
}

#[test]
fn nonlinear_cubic_negative_seeds_preserve_progress_with_one_residual_scale() {
    use crate::curve::{
        CurveExpressionEquation, CurveExpressionSolveBlock, RelationDimension, SolveUnknown,
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
        let solution = with_expression_policy(DecodePolicy::service(), |ctx| {
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

#[test]
fn large_affine_width_keeps_coefficient_work_admitted() {
    let expected: Vec<_> = (1..=9).map(f64::from).collect();
    let solution = crate::test_support::assert_work_boundaries(
        &["creo matrix pivot normalization", "creo matrix elimination"],
        |ctx| {
            let mut rows: Vec<_> = (0..9)
                .map(|row| {
                    let coefficients: Vec<_> = (0..9)
                        .map(|column| if column == row { 2.0 } else { 1.0 })
                        .collect();
                    AffineEquationRow {
                        rhs: 45.0 + f64::from(row + 1),
                        coefficients,
                    }
                })
                .collect();
            solve_unique_affine_system(ctx, &mut rows, 9)
        },
    );
    let solution = solution.expect("unique nine-variable system");
    assert_eq!(solution.len(), expected.len());
    for (actual, expected) in solution.iter().zip(expected) {
        assert!((actual - expected).abs() <= crate::curve::solve::EPS_LINEAR_SYSTEM_RESIDUAL);
    }
}
