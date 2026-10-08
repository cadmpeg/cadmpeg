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
fn flat_relation_scans_refuse_caller_work() {
    for source in ["1+1+1", "123456789", "       1", "----1"] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let error = with_policy(policy, |ctx| {
            parse_relation_expression::<CurveExpressionValue>(
                ctx,
                source,
                &BTreeMap::new(),
                RelationEvaluationContext::default(),
            )
        })
        .expect_err("source scan must refuse");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation source scan")
        );
    }
}

#[test]
fn relation_unit_recursion_refuses_caller_and_local_depth() {
    for depth in [0, 1024] {
        let source = if depth == 0 {
            "((mm))".to_owned()
        } else {
            format!("{}mm{}", "(".repeat(129), ")".repeat(129))
        };
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = depth;
        let error = with_policy(policy, |ctx| {
            relation_unit(ctx, &source).map(|unit| unit.is_some())
        })
        .expect_err("unit nesting must refuse");
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }
}

#[test]
fn relation_local_nesting_ceiling_refuses() {
    let source = format!("{}1{}", "(".repeat(129), ")".repeat(129));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    let error = with_policy(policy, |ctx| {
        parse_relation_expression::<CurveExpressionValue>(
            ctx,
            &source,
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        )
    })
    .expect_err("local nesting ceiling");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation nesting ceiling")
    );
}

#[test]
fn affine_matrix_elimination_refuses_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = with_policy(policy, |ctx| {
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

fn nonlinear_ceiling_error(left: &str, seed: f64) -> CodecError {
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
fn relation_local_exponent_nesting_ceiling_refuses() {
    let source = format!("{}1", "1^".repeat(129));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    let error = with_policy(policy, |ctx| {
        parse_relation_expression::<CurveExpressionValue>(
            ctx,
            &source,
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        )
    })
    .expect_err("local exponent nesting ceiling");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation nesting ceiling")
    );
}

#[test]
fn relation_local_function_nesting_ceiling_refuses() {
    let source = format!("{}1{}", "abs(".repeat(129), ")".repeat(129));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    let error = with_policy(policy, |ctx| {
        parse_relation_expression::<CurveExpressionValue>(
            ctx,
            &source,
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        )
    })
    .expect_err("local function nesting ceiling");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation nesting ceiling")
    );
}

#[test]
fn relation_unit_source_scan_refuses_work_before_nonrecursive_parse() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = with_policy(policy, |ctx| {
        relation_unit(ctx, "mm*mm/mm").map(|unit| unit.is_some())
    })
    .expect_err("unit source work");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation unit source scan")
    );
}

#[test]
fn affine_pivot_and_coefficient_elimination_refuse_work() {
    let solution = crate::test_support::assert_work_boundaries(
        &[
            "creo matrix row normalization",
            "creo matrix pivot scan",
            "creo matrix pivot normalization",
            "creo matrix elimination",
        ],
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
fn relation_stack_operators_refuse_work_after_source_admission() {
    let value = crate::test_support::assert_work_boundaries(
        &["creo relation source scan"],
        |ctx| {
            parse_relation_expression::<CurveExpressionValue>(
                ctx,
                "1+2*3",
                &BTreeMap::new(),
                RelationEvaluationContext::default(),
            )
        },
    );
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
