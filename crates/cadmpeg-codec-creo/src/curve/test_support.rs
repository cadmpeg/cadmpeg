// SPDX-License-Identifier: Apache-2.0

use crate::curve::{CurveExpressionAssignment, CurveExpressionLine, ExternalRelationSymbols};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub(super) fn with_expression_policy<T>(
    policy: DecodePolicy,
    run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test input is admitted");
    run(&ctx)
}

pub(super) fn expression_lines(source: &[&str]) -> Vec<crate::curve::CurveExpressionLine> {
    source
        .iter()
        .enumerate()
        .map(|(offset, text)| crate::curve::CurveExpressionLine {
            text: (*text).to_owned(),
            offset,
        })
        .collect()
}

pub(super) fn external_symbol(
    value: Option<crate::curve::CurveExpressionValue>,
) -> crate::curve::ExternalRelationSymbols {
    named_external_symbol("external", value)
}

pub(super) fn named_external_symbol(
    name: &str,
    value: Option<crate::curve::CurveExpressionValue>,
) -> crate::curve::ExternalRelationSymbols {
    crate::curve::ExternalRelationSymbols {
        values: BTreeMap::from([(name.to_owned(), value)]),
    }
}

pub(super) fn solve_phase_inputs(
    source: &[&str],
    external_symbols: &crate::curve::ExternalRelationSymbols,
) -> (
    crate::curve::CurveExpressionSolveBlock,
    BTreeMap<String, crate::curve::CurveExpressionValue>,
    crate::curve::CurveExpressionEvaluation,
) {
    let lines = expression_lines(source);
    let evaluation = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::test_support::evaluate_program_details(ctx, &lines, None, external_symbols)
    })
    .expect("service profile evaluates the original expression");
    let block = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::test_support::compile_solve_program(ctx, &lines)
    })
    .expect("solve program")
    .blocks
    .pop()
    .expect("one solve block");
    let preceding: Vec<_> = lines
        .iter()
        .take_while(|line| line.offset < block.offset)
        .cloned()
        .collect();
    let preceding = with_expression_policy(DecodePolicy::service(), |ctx| {
        crate::curve::test_support::evaluate_program_details(
            ctx,
            &preceding,
            None,
            external_symbols,
        )
    })
    .expect("preceding assignments");
    let mut values = BTreeMap::new();
    for (name, value) in &external_symbols.values {
        if let Some(value) = value {
            values.insert(name.clone(), value.clone());
        }
    }
    for assignment in preceding.assignments {
        if let (Some((name, _)), Some(value)) =
            (assignment.scalar_target(), assignment.value.as_ref())
        {
            values.insert(name.to_ascii_lowercase(), value.clone());
        }
    }
    (block, values, evaluation)
}

pub(super) fn with_policy<T>(
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

pub(super) fn evaluate_expression_program(
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

pub(super) fn compile_solve_program(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lines: &[CurveExpressionLine],
) -> Result<super::CurveExpressionSolveProgram, cadmpeg_core::CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "creo solve program index scratch")?;
    super::curve_expression_solve_program(ctx, lines, &mut index_storage)
}

pub(super) fn evaluate_program_details(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lines: &[CurveExpressionLine],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) -> Result<super::CurveExpressionEvaluation, cadmpeg_core::CodecError> {
    let mut program_storage = ctx.reserve_scoped(0, "creo solve program scratch")?;
    let mut index_storage = ctx.reserve_scoped(0, "creo solve program index scratch")?;
    let program = program_storage
        .with_storage(|| super::curve_expression_solve_program(ctx, lines, &mut index_storage))?;
    let mut solution_storage = ctx.reserve_scoped(0, "creo expression solution scratch")?;
    super::evaluate_expression_program_details(
        ctx,
        lines,
        model_name,
        external_symbols,
        &mut solution_storage,
        &program,
        true,
    )
}
