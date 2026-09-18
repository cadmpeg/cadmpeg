// SPDX-License-Identifier: Apache-2.0
use crate::curve::evaluate_expression_program_details;
use crate::curve::CurveExpressionAssignment;
use crate::curve::CurveExpressionLine;
use crate::curve::ExternalRelationSymbols;

mod affine;
mod dump;
mod relations;
mod rows;
mod scan;

fn evaluate_expression_program(
    lines: &[CurveExpressionLine],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) -> Vec<CurveExpressionAssignment> {
    evaluate_expression_program_details(lines, model_name, external_symbols).assignments
}
