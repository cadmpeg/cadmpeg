// SPDX-License-Identifier: Apache-2.0
use crate::curve::{DimensionProbeKind, DimensionProbeValue, RelationEvaluationContext};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn dimension_arithmetic_preserves_service_values() {
    let values = BTreeMap::new();
    let result = crate::decode::with_service_context(|ctx| {
        crate::curve::parse_relation_expression::<DimensionProbeValue>(
            ctx,
            "2+3*4",
            &values,
            RelationEvaluationContext::default(),
        )
    })
    .expect("service expression")
    .expect("numeric result");
    assert!(matches!(
        result.kind,
        DimensionProbeKind::Numeric(Some(14.0))
    ));
    assert_eq!(result.constraints.len(), 1);
}

fn dimension_conversion_result(
    name: &str,
) -> Result<Option<Vec<crate::curve::RelationDimension>>, CodecError> {
    use crate::curve::{
        CurveExpressionEquation, CurveExpressionSolveBlock, CurveExpressionValue, SolveUnknown,
    };
    let mut expression = name.to_owned();
    for _ in 0..8 {
        expression = format!("({expression}^127)");
    }
    let block = CurveExpressionSolveBlock {
        equations: vec![CurveExpressionEquation {
            left: expression,
            right: "1".to_owned(),
            dependencies: vec![name.to_owned()],
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
    let values = BTreeMap::from([(
        "length".to_owned(),
        CurveExpressionValue::Length(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture"),
        ),
    )]);
    crate::decode::with_service_context(|ctx| {
        crate::curve::infer_solve_variable_dimensions(
            ctx,
            &block,
            &values,
            &[None],
            RelationEvaluationContext::default(),
        )
    })
}

#[test]
fn dimension_inference_refuses_inexact_integer_coefficient() {
    let error =
        dimension_conversion_result("x").expect_err("127 to the eighth power is not exact in f64");
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn dimension_inference_refuses_inexact_integer_constant() {
    let error = dimension_conversion_result("length")
        .expect_err("127 to the eighth power is not exact in f64");
    assert!(matches!(error, CodecError::Malformed(_)));
}
