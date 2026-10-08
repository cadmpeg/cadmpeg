// SPDX-License-Identifier: Apache-2.0
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
        evaluate_program_details(ctx, lines, model_name, external_symbols)
    })
    .expect("test curve expression evaluation")
    .assignments
}

fn compile_solve_program(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lines: &[CurveExpressionLine],
) -> Result<super::CurveExpressionSolveProgram, cadmpeg_core::CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "creo solve program index scratch")?;
    super::curve_expression_solve_program(ctx, lines, &mut index_storage)
}

fn evaluate_program_details(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lines: &[CurveExpressionLine],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) -> Result<super::CurveExpressionEvaluation, cadmpeg_core::CodecError> {
    let mut program_storage = ctx.reserve_scoped(0, "creo solve program scratch")?;
    let mut index_storage = ctx.reserve_scoped(0, "creo solve program index scratch")?;
    let program = program_storage.with_storage(|| super::curve_expression_solve_program(ctx, lines, &mut index_storage))?;
    let mut solution_storage = ctx.reserve_scoped(0, "creo expression solution scratch")?;
    super::evaluate_expression_program_details(ctx, lines, model_name, external_symbols, &mut solution_storage, &program)
}
