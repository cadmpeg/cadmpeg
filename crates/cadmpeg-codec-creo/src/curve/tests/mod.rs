// SPDX-License-Identifier: Apache-2.0
use crate::curve::evaluate_expression_program_details;
use crate::curve::CurveExpressionAssignment;
use crate::curve::CurveExpressionLine;
use crate::curve::ExternalRelationSymbols;

mod affine;
mod allocation;
mod dimension_admission;
mod dump;
mod relations;
mod rows;
mod scan;
mod text_function_admission;
mod work_admission;

fn evaluate_expression_program(
    lines: &[CurveExpressionLine],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) -> Vec<CurveExpressionAssignment> {
    crate::decode::with_test_decode_ctx(|ctx| {
        { let mut solution_storage = ctx.reserve_scoped(0, "creo expression solution scratch")?; evaluate_expression_program_details(ctx, lines, model_name, external_symbols, &mut solution_storage) }
    })
    .expect("test curve expression evaluation")
    .assignments
}
