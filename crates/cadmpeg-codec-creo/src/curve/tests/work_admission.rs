// SPDX-License-Identifier: Apache-2.0
use crate::curve::{parse_relation_expression, relation_unit, solve_unique_affine_system, AffineEquationRow, CurveExpressionValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn with_policy<T>(policy: DecodePolicy, run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test root");
    let result = run(&ctx);
    if let Err(CodecError::ResourceLimit(limit)) = &result { assert_eq!(ctx.resource_refusal(), Some(limit.clone())); }
    result
}

#[test]
fn flat_relation_scans_refuse_caller_work() {
    for source in ["1+1+1", "123456789", "       1", "----1"] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let error = with_policy(policy, |ctx| parse_relation_expression::<CurveExpressionValue>(ctx, source, &BTreeMap::new(), Default::default())).expect_err("source scan must refuse");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation source scan"));
    }
}

#[test]
fn relation_unit_recursion_refuses_caller_and_local_depth() {
    for depth in [0, 1024] {
        let source = if depth == 0 { "((mm))".to_owned() } else { format!("{}mm{}", "(".repeat(129), ")".repeat(129)) };
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = depth;
        let error = with_policy(policy, |ctx| relation_unit(ctx, &source).map(|unit| unit.is_some())).expect_err("unit nesting must refuse");
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }
}

#[test]
fn relation_local_nesting_ceiling_refuses() {
    let source = format!("{}1{}", "(".repeat(129), ")".repeat(129));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    let error = with_policy(policy, |ctx| parse_relation_expression::<CurveExpressionValue>(ctx, &source, &BTreeMap::new(), Default::default())).expect_err("local nesting ceiling");
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation nesting ceiling"));
}

#[test]
fn affine_matrix_elimination_refuses_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = with_policy(policy, |ctx| solve_unique_affine_system(ctx, &mut [AffineEquationRow { coefficients: vec![1.0], rhs: 2.0 }], 1)).expect_err("matrix work must refuse");
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo matrix row normalization"));
    crate::decode::with_test_decode_ctx(|ctx| assert_eq!(solve_unique_affine_system(ctx, &mut [AffineEquationRow { coefficients: vec![1.0], rhs: 2.0 }], 1).expect("service"), Some(vec![2.0])));
}
