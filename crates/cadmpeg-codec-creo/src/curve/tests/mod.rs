// SPDX-License-Identifier: Apache-2.0
use crate::curve::evaluate_expression_program_details;
use crate::curve::CurveExpressionAssignment;
use crate::curve::CurveExpressionLine;
use crate::curve::ExternalRelationSymbols;

mod affine;
mod dimensions;
mod dump;
mod expression_programs;
mod relation_solving;
mod relation_text_functions;
mod relations;
mod rows;
mod scan;

fn evaluate_expression_program(
    lines: &[CurveExpressionLine],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) -> Vec<CurveExpressionAssignment> {
    crate::decode::with_service_context(|ctx| {
        evaluate_expression_program_details(ctx, lines, model_name, external_symbols)
    })
    .expect("test curve expression evaluation")
    .assignments
}
