// SPDX-License-Identifier: Apache-2.0
use crate::curve::evaluate_expression_program_details;
use crate::curve::CurveExpressionAssignment;
use crate::curve::CurveExpressionLine;
use crate::curve::ExternalRelationSymbols;

mod affine;
mod allocation;
mod dump;
mod relations;
mod rows;
mod scan;

fn evaluate_expression_program(
    lines: &[CurveExpressionLine],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) -> Vec<CurveExpressionAssignment> {
    crate::decode::with_test_decode_ctx(|ctx| {
        evaluate_expression_program_details(ctx, lines, model_name, external_symbols)
    })
    .expect("test curve expression evaluation")
    .assignments
}
