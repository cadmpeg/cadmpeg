// SPDX-License-Identifier: Apache-2.0
//! Affine and nonlinear solving of relation equation blocks.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::{
    copy_expression_value, dimension_variable_key, expression_identifier_end,
    parse_relation_expression, quantity_parts_ref, quantity_value, relation_unit, AffineValue,
    CurveExpressionActivation, CurveExpressionRecord, CurveExpressionSolveBlock,
    CurveExpressionValue, DimensionEquality, DimensionProbeKind, DimensionProbeValue,
    ExpressionValue, RelationDimension, RelationEvaluationContext, SimultaneousAffineValue,
    SymbolicRelationDimension,
};

const EPS_DIMENSION_SOLUTION: f64 = 1.0e-9;
const EPS_LINEAR_SYSTEM_COEFFICIENT: f64 = 1.0e-12;
pub(super) const EPS_LINEAR_SYSTEM_RESIDUAL: f64 = 1.0e-9;
pub(super) const MAX_NONLINEAR_SOLVE_VARIABLES: usize = 8;
pub(super) const MAX_NONLINEAR_SOLVE_ITERATIONS: usize = 64;
pub(super) const MAX_NONLINEAR_SOLVE_LINE_SEARCH_STEPS: usize = 16;
pub(super) const NONLINEAR_SOLVE_RESIDUAL_TOLERANCE: f64 = 1.0e-8;
pub(super) const NONLINEAR_SOLVE_DERIVATIVE_STEP: f64 = 1.0e-6;
pub(super) const NONLINEAR_SOLVE_SOLUTION_TOLERANCE: f64 = 1.0e-7;
pub(super) const NONLINEAR_SOLVE_STEP_TOLERANCE: f64 = 1.0e-12;

#[derive(Debug, Clone, Copy)]
pub(super) struct SolveResidual {
    pub(super) value: f64,
    pub(super) scale: f64,
    pub(super) dimension: RelationDimension,
}

pub(super) fn solve_nonlinear_expression_block(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    known_dimensions: &[Option<RelationDimension>],
    initial_values: &[Option<CurveExpressionValue>],
    context: RelationEvaluationContext<'_>,
) -> Result<Option<Vec<CurveExpressionValue>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let variable_count = block.unknowns.len();
    if variable_count == 0
        || variable_count > MAX_NONLINEAR_SOLVE_VARIABLES
        || block.equations.len() < variable_count
    {
        return Ok(None);
    }
    if !nonlinear_equations_are_smooth(ctx, block)? {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo nonlinear solve scratch")?;
    let Some(variable_dimensions) = scratch.with_storage(|| {
        infer_solve_variable_dimensions(ctx, block, values, known_dimensions, context)
    })?
    else {
        return Ok(None);
    };
    let Some(seeds) = scratch
        .with_storage(|| nonlinear_initial_guesses(ctx, initial_values, &variable_dimensions))?
    else {
        return Ok(None);
    };
    let mut seeds = seeds.into_iter();
    let Some(initial_seed) = seeds.next() else {
        return Ok(None);
    };
    let Some(solution) = scratch.with_storage(|| {
        refine_nonlinear_solution(
            ctx,
            block,
            values,
            &variable_dimensions,
            &initial_seed,
            context,
        )
    })?
    else {
        return Ok(None);
    };
    for seed in seeds {
        let (candidate, _candidate_storage) = ctx
            .with_scoped_storage("creo nonlinear candidate scratch", || {
                refine_nonlinear_solution(ctx, block, values, &variable_dimensions, &seed, context)
            })?;
        let Some(candidate) = candidate else {
            continue;
        };
        if !nonlinear_solutions_close(&solution, &candidate) {
            return Ok(None);
        }
    }
    let mut solved = Vec::new();
    ctx.reserve_vec(&mut solved, solution.len(), "creo nonlinear solved values")?;
    for (value, dimension) in solution.into_iter().zip(variable_dimensions) {
        let Some(value) = quantity_value(value, dimension) else {
            return Ok(None);
        };
        solved.push(value);
    }
    Ok(Some(solved))
}

pub(super) fn nonlinear_equations_are_smooth(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    block: &CurveExpressionSolveBlock,
) -> Result<bool, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut equations = block.equations.iter();
    while !equations.as_slice().is_empty() {
        let Some(equation) = ctx.next_charged(&mut equations, "creo relation comparison traversal")? else {
            break;
        };
        if !nonlinear_expression_is_smooth(ctx, &equation.left)?
            || !nonlinear_expression_is_smooth(ctx, &equation.right)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn nonlinear_expression_is_smooth(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    expression: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let bytes = expression.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        ctx.next_charged(
            &mut bytes[cursor..].iter(),
            "creo nonlinear expression scan",
        )?;
        if matches!(bytes[cursor], b'\'' | b'"') {
            let delimiter = bytes[cursor];
            cursor += 1;
            let tail = &bytes[cursor..];
            cursor += ctx
                .position_by(
                    tail,
                    |byte| Ok(*byte == delimiter),
                    "creo nonlinear quoted expression scan",
                )?
                .unwrap_or(tail.len());
            if bytes.get(cursor) != Some(&delimiter) {
                return Ok(false);
            }
            cursor += 1;
            continue;
        }
        if matches!(
            bytes[cursor],
            b'=' | b'!' | b'~' | b'<' | b'>' | b'&' | b'|'
        ) {
            return Ok(false);
        }
        if bytes[cursor] == b'_' || bytes[cursor].is_ascii_alphabetic() {
            let start = cursor;
            let Some(end) = expression_identifier_end(ctx, bytes, start)? else {
                return Ok(false);
            };
            cursor = end;
            let mut following = cursor;
            let tail = &bytes[following..];
            following += ctx
                .position_by(
                    tail,
                    |byte| Ok(!byte.is_ascii_whitespace()),
                    "creo nonlinear following whitespace scan",
                )?
                .unwrap_or(tail.len());
            if bytes.get(following) == Some(&b'(') {
                let name = &expression[start..end];
                let mut smooth = false;
                for candidate in [
                    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh",
                    "log", "ln", "exp", "pow", "sqrt",
                ] {
                    if name.eq_ignore_ascii_case(candidate) {
                        smooth = true;
                        break;
                    }
                }
                if !smooth {
                    return Ok(false);
                }
            }
            continue;
        }
        cursor += 1;
    }
    Ok(true)
}

pub(super) fn nonlinear_initial_guesses(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    initial_values: &[Option<CurveExpressionValue>],
    variable_dimensions: &[RelationDimension],
) -> Result<Option<Vec<Vec<f64>>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let variable_count = variable_dimensions.len();
    if variable_count == 0 || variable_count > MAX_NONLINEAR_SOLVE_VARIABLES {
        return Ok(None);
    }
    let mut seeds = Vec::new();
    let mut add_seed =
        |seed: &[f64], operation: &'static str| -> Result<(), cadmpeg_core::CodecError> {
            if seed.iter().all(|value| value.is_finite())
                && !seeds
                    .iter()
                    .any(|known: &Vec<f64>| known.as_slice() == seed)
            {
                let mut owned = ctx.alloc_filled(seed.len(), 0.0, operation)?;
                owned.copy_from_slice(seed);
                ctx.reserve_vec(&mut seeds, 1, "creo solve seed rows")?;
                seeds.push(owned);
            }
            Ok(())
        };
    if initial_values.len() != variable_count {
        return Ok(None);
    }
    let mut frame = [0.0; MAX_NONLINEAR_SOLVE_VARIABLES];
    let initial = &mut frame[..variable_count];
    for ((slot, value), dimension) in initial
        .iter_mut()
        .zip(initial_values)
        .zip(variable_dimensions)
    {
        let Some((number, value_dimension)) = value.as_ref().and_then(quantity_parts_ref) else {
            return Ok(None);
        };
        if value_dimension != *dimension {
            return Ok(None);
        }
        *slot = number;
    }
    add_seed(initial, "creo solve initial seed")?;
    initial.fill(0.0);
    add_seed(initial, "creo_solve_seed_zero")?;
    for magnitude in [0.01, 0.1, 1.0, 10.0, 100.0] {
        initial.fill(magnitude);
        add_seed(initial, "creo_solve_seed_magnitude")?;
        initial.fill(-magnitude);
        add_seed(initial, "creo_solve_seed_magnitude")?;
    }
    for index in 0..variable_count {
        for magnitude in [0.1, 1.0, 10.0] {
            initial.fill(0.0);
            initial[index] = magnitude;
            add_seed(initial, "creo_solve_seed_axis")?;
            initial[index] = -magnitude;
            add_seed(initial, "creo_solve_seed_axis")?;
        }
    }
    Ok(Some(seeds))
}

pub(super) fn refine_nonlinear_solution(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    variable_dimensions: &[RelationDimension],
    seed: &[f64],
    context: RelationEvaluationContext<'_>,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let variable_count = variable_dimensions.len();
    if variable_count == 0 || variable_count > MAX_NONLINEAR_SOLVE_VARIABLES {
        return Ok(None);
    }
    if seed.len() != variable_count {
        return Ok(None);
    }
    let (initial, mut _current_storage) = ctx.with_scoped_storage(
        "creo nonlinear current point scratch",
        || -> Result<_, cadmpeg_core::CodecError> {
            let mut point = ctx.alloc_filled(seed.len(), 0.0, "creo nonlinear initial point")?;
            point.copy_from_slice(seed);
            Ok(evaluate_nonlinear_residuals(
                ctx,
                block,
                values,
                variable_dimensions,
                &point,
                context,
            )?
            .map(|residuals| (point, residuals)))
        },
    )?;
    let Some((mut point, mut residuals)) = initial else {
        return Ok(None);
    };
    for _ in 0..MAX_NONLINEAR_SOLVE_ITERATIONS {
        let mut iteration_storage = ctx.reserve_scoped(0, "creo nonlinear iteration scratch")?;
        if nonlinear_residuals_converged(ctx, &residuals)? {
            let Some(mut rank_rows) = iteration_storage.with_storage(|| {
                nonlinear_jacobian_rows(
                    ctx,
                    block,
                    values,
                    variable_dimensions,
                    &point,
                    &residuals,
                    context,
                )
            })?
            else {
                return Ok(None);
            };
            if iteration_storage
                .with_storage(|| solve_unique_affine_system(ctx, &mut rank_rows, variable_count))?
                .is_none()
            {
                return Ok(None);
            }
            let mut solved = ctx.alloc_filled(point.len(), 0.0, "creo nonlinear solution point")?;
            solved.copy_from_slice(&point);
            return Ok(Some(solved));
        }
        let Some(mut rows) = iteration_storage.with_storage(|| {
            nonlinear_jacobian_rows(
                ctx,
                block,
                values,
                variable_dimensions,
                &point,
                &residuals,
                context,
            )
        })?
        else {
            return Ok(None);
        };
        for (row, residual) in ctx
            .admit_iter(&mut rows, "creo nonlinear rhs traversal")?
            .zip(&residuals)
        {
            row.rhs = -residual.value;
        }
        let Some(delta) = iteration_storage
            .with_storage(|| solve_unique_affine_system(ctx, &mut rows, variable_count))?
        else {
            return Ok(None);
        };
        let maximum_delta = delta.iter().map(|value| value.abs()).fold(0.0, f64::max);
        let point_scale = point.iter().map(|value| value.abs()).fold(1.0, f64::max);
        if !maximum_delta.is_finite() || maximum_delta > 1e12 * point_scale {
            return Ok(None);
        }
        let base_norm = nonlinear_residual_norm(ctx, &residuals, &residuals)?;
        let mut accepted = None;
        let mut valid_candidate = false;
        let mut scale = 1.0;
        for _ in 0..MAX_NONLINEAR_SOLVE_LINE_SEARCH_STEPS {
            let (trial, trial_storage) = ctx.with_scoped_storage(
                "creo nonlinear trial scratch",
                || -> Result<_, cadmpeg_core::CodecError> {
                    let mut candidate =
                        ctx.alloc_filled(point.len(), 0.0, "creo nonlinear line-search point")?;
                    for ((slot, value), change) in candidate.iter_mut().zip(&point).zip(&delta) {
                        *slot = value + scale * change;
                    }
                    if !candidate.iter().all(|value| value.is_finite()) {
                        return Ok(None);
                    }
                    Ok(evaluate_nonlinear_residuals(
                        ctx,
                        block,
                        values,
                        variable_dimensions,
                        &candidate,
                        context,
                    )?
                    .map(|residuals| (candidate, residuals)))
                },
            )?;
            if let Some((candidate, candidate_residuals)) = trial {
                valid_candidate = true;
                let candidate_norm =
                    nonlinear_residual_norm(ctx, &candidate_residuals, &residuals)?;
                if nonlinear_residuals_converged(ctx, &candidate_residuals)?
                    || candidate_norm < base_norm
                {
                    accepted = Some((candidate, candidate_residuals, trial_storage));
                    break;
                }
            }
            scale *= 0.5;
        }
        let Some((candidate, candidate_residuals, candidate_storage)) = accepted else {
            if !valid_candidate {
                return Ok(None);
            }
            return Err(ctx.refuse_codec_limit(
                "creo nonlinear line-search ceiling",
                cadmpeg_core::decode::u64_from_index(MAX_NONLINEAR_SOLVE_LINE_SEARCH_STEPS),
                cadmpeg_core::decode::u64_from_index(MAX_NONLINEAR_SOLVE_LINE_SEARCH_STEPS) + 1,
            ));
        };
        point = candidate;
        residuals = candidate_residuals;
        _current_storage = candidate_storage;
        if maximum_delta * scale <= NONLINEAR_SOLVE_STEP_TOLERANCE * point_scale
            && !nonlinear_residuals_converged(ctx, &residuals)?
        {
            return Ok(None);
        }
    }
    if !nonlinear_residuals_converged(ctx, &residuals)? {
        return Err(ctx.refuse_codec_limit(
            "creo nonlinear iteration ceiling",
            cadmpeg_core::decode::u64_from_index(MAX_NONLINEAR_SOLVE_ITERATIONS),
            cadmpeg_core::decode::u64_from_index(MAX_NONLINEAR_SOLVE_ITERATIONS) + 1,
        ));
    }
    let mut iteration_storage = ctx.reserve_scoped(0, "creo nonlinear final rank scratch")?;
    let Some(mut rank_rows) = iteration_storage.with_storage(|| {
        nonlinear_jacobian_rows(
            ctx,
            block,
            values,
            variable_dimensions,
            &point,
            &residuals,
            context,
        )
    })?
    else {
        return Ok(None);
    };
    if iteration_storage
        .with_storage(|| solve_unique_affine_system(ctx, &mut rank_rows, variable_count))?
        .is_none()
    {
        return Ok(None);
    }
    let mut solved = ctx.alloc_filled(point.len(), 0.0, "creo nonlinear solution point")?;
    solved.copy_from_slice(&point);
    Ok(Some(solved))
}

pub(super) fn nonlinear_jacobian_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    variable_dimensions: &[RelationDimension],
    point: &[f64],
    residuals: &[SolveResidual],
    context: RelationEvaluationContext<'_>,
) -> Result<Option<Vec<AffineEquationRow>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let variable_count = variable_dimensions.len();
    if variable_count == 0 || variable_count > MAX_NONLINEAR_SOLVE_VARIABLES {
        return Ok(None);
    }
    if point.len() != variable_count {
        return Ok(None);
    }
    let mut rows = Vec::new();
    for _ in ctx.admit_iter(residuals, "creo nonlinear Jacobian row initialization")? {
        let coefficients =
            ctx.alloc_filled(variable_count, 0.0, "creo nonlinear Jacobian coefficients")?;
        ctx.push_vec(
            &mut rows,
            AffineEquationRow {
                coefficients,
                rhs: 0.0,
            },
            "creo nonlinear Jacobian rows",
        )?;
    }
    for column in 0..variable_count {
        let mut probes = ctx.reserve_scoped(0, "creo nonlinear Jacobian probes")?;
        let step = NONLINEAR_SOLVE_DERIVATIVE_STEP * point[column].abs().max(1.0);
        let mut plus = probes
            .with_storage(|| ctx.alloc_filled(point.len(), 0.0, "creo nonlinear positive probe"))?;
        let mut minus = probes
            .with_storage(|| ctx.alloc_filled(point.len(), 0.0, "creo nonlinear negative probe"))?;
        plus.copy_from_slice(point);
        minus.copy_from_slice(point);
        plus[column] += step;
        minus[column] -= step;
        let Some(plus_residuals) = probes.with_storage(|| {
            evaluate_nonlinear_residuals(ctx, block, values, variable_dimensions, &plus, context)
        })?
        else {
            return Ok(None);
        };
        let Some(minus_residuals) = probes.with_storage(|| {
            evaluate_nonlinear_residuals(ctx, block, values, variable_dimensions, &minus, context)
        })?
        else {
            return Ok(None);
        };
        let mut input = rows.iter_mut().zip(residuals).enumerate();
        while input.len() != 0 {
            let Some((row_index, (row, residual))) = ctx.next_charged(&mut input, "creo nonlinear Jacobian column traversal")? else {
                break;
            };
            let Some(plus_residual) = plus_residuals.get(row_index) else {
                return Ok(None);
            };
            let Some(minus_residual) = minus_residuals.get(row_index) else {
                return Ok(None);
            };
            if plus_residual.dimension != residual.dimension
                || minus_residual.dimension != residual.dimension
            {
                return Ok(None);
            }
            let derivative = (plus_residual.value - minus_residual.value) / (2.0 * step);
            if !derivative.is_finite() {
                return Ok(None);
            }
            row.coefficients[column] = derivative;
        }
    }
    Ok(Some(rows))
}

pub(super) fn evaluate_nonlinear_residuals(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    variable_dimensions: &[RelationDimension],
    point: &[f64],
    context: RelationEvaluationContext<'_>,
) -> Result<Option<Vec<SolveResidual>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let variable_count = variable_dimensions.len();
    if variable_count == 0 || variable_count > MAX_NONLINEAR_SOLVE_VARIABLES {
        return Ok(None);
    }
    if variable_dimensions.len() != block.unknowns.len() || point.len() != variable_dimensions.len()
    {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo nonlinear evaluation scratch")?;
    let mut evaluation_values = BTreeMap::new();
    for (name, value) in ctx.admit_iter(values, "creo nonlinear known value traversal")? {
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut evaluation_values,
                ctx.copy_retained_text(name, "creo nonlinear known value names")?,
                copy_expression_value(ctx, value, "creo nonlinear known string values")?,
                "creo nonlinear known value nodes",
            )
        })?;
    }
    for ((variable, dimension), value) in block
        .unknowns
        .iter()
        .map(|unknown| &unknown.name)
        .zip(variable_dimensions)
        .zip(point)
    {
        if !value.is_finite() {
            return Ok(None);
        }
        let mut key = scratch.with_storage(|| {
            ctx.copy_retained_text(variable, "creo nonlinear unknown value names")
        })?;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        let Some(value) = quantity_value(*value, *dimension) else {
            return Ok(None);
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut evaluation_values,
                key,
                value,
                "creo nonlinear unknown value nodes",
            )
        })?;
    }
    let mut residuals = Vec::new();
    let mut equations = block.equations.iter();
    while equations.len() != 0 {
        let Some(equation) = ctx.next_charged(&mut equations, "creo nonlinear equation traversal")? else {
            break;
        };
        let Some(left) = parse_relation_expression::<CurveExpressionValue>(
            ctx,
            &equation.left,
            &evaluation_values,
            context,
        )?
        else {
            return Ok(None);
        };
        let Some(right) = parse_relation_expression::<CurveExpressionValue>(
            ctx,
            &equation.right,
            &evaluation_values,
            context,
        )?
        else {
            return Ok(None);
        };
        let Some((left, left_dimension)) = quantity_parts_ref(&left) else {
            return Ok(None);
        };
        let Some((right, right_dimension)) = quantity_parts_ref(&right) else {
            return Ok(None);
        };
        if left_dimension != right_dimension {
            return Ok(None);
        }
        let value = left - right;
        let scale = left.abs().max(right.abs()).max(1.0);
        if !value.is_finite() || !scale.is_finite() {
            return Ok(None);
        }
        ctx.reserve_vec(&mut residuals, 1, "creo nonlinear residual rows")?;
        residuals.push(SolveResidual {
            value,
            scale,
            dimension: left_dimension,
        });
    }
    Ok(Some(residuals))
}

pub(super) fn nonlinear_residual_norm(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    residuals: &[SolveResidual],
    reference: &[SolveResidual],
) -> Result<f64, cadmpeg_core::CodecError> {
    Ok(ctx
        .admit_iter(residuals, "creo nonlinear residual norm")?
        .zip(reference)
        .map(|(residual, reference)| (residual.value / reference.scale).abs())
        .fold(0.0, f64::max))
}

pub(super) fn nonlinear_residuals_converged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    residuals: &[SolveResidual],
) -> Result<bool, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut residuals = residuals.iter();
    while !residuals.as_slice().is_empty() {
        let Some(residual) = ctx.next_charged(&mut residuals, "creo nonlinear residual convergence")? else {
            break;
        };
        let converged = residual.value.abs() <= NONLINEAR_SOLVE_RESIDUAL_TOLERANCE * residual.scale;
        if !converged {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn nonlinear_solutions_close(left: &[f64], right: &[f64]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            (left - right).abs()
                <= NONLINEAR_SOLVE_SOLUTION_TOLERANCE * left.abs().max(right.abs()).max(1.0)
        })
}

pub(super) struct AffineEquationRow {
    pub(super) coefficients: Vec<f64>,
    pub(super) rhs: f64,
}

pub(super) fn eliminate_pivot_column(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &mut [AffineEquationRow],
    pivot_row: usize,
    column: usize,
    coefficient_tolerance: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let (before, pivot_and_after) = rows.split_at_mut(pivot_row);
    let Some((pivot, after)) = pivot_and_after.split_first_mut() else {
        return Ok(());
    };
    for row in ctx
        .admit_iter(before, "creo matrix elimination rows")?
        .chain(ctx.admit_iter(after, "creo matrix elimination rows")?)
    {
        let factor = row.coefficients[column];
        if factor.abs() <= coefficient_tolerance {
            continue;
        }
        if row.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
            for (coefficient, pivot_coefficient) in
                row.coefficients.iter_mut().zip(&pivot.coefficients)
            {
                *coefficient -= factor * pivot_coefficient;
                if coefficient.abs() <= coefficient_tolerance {
                    *coefficient = 0.0;
                }
            }
        } else {
            for (coefficient, pivot_coefficient) in ctx
                .admit_iter(&mut row.coefficients, "creo matrix elimination")?
                .zip(&pivot.coefficients)
            {
                *coefficient -= factor * pivot_coefficient;
                if coefficient.abs() <= coefficient_tolerance {
                    *coefficient = 0.0;
                }
            }
        }
        row.rhs -= factor * pivot.rhs;
    }
    Ok(())
}

pub(super) fn solve_unique_affine_system(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &mut [AffineEquationRow],
    variable_count: usize,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if variable_count == 0 || rows.len() < variable_count {
        return Ok(None);
    }
    for row in ctx.admit_iter(&mut *rows, "creo matrix row normalization")? {
        let scale = if row.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
            row.coefficients
                .iter()
                .map(|value| value.abs())
                .fold(0.0, f64::max)
        } else {
            ctx.admit_iter(&row.coefficients, "creo matrix row normalization")?
                .map(|value| value.abs())
                .fold(0.0, f64::max)
        };
        if scale > 0.0 {
            if row.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
                for coefficient in &mut row.coefficients {
                    *coefficient /= scale;
                }
            } else {
                for coefficient in
                    ctx.admit_iter(&mut row.coefficients, "creo matrix row normalization")?
                {
                    *coefficient /= scale;
                }
            }
            row.rhs /= scale;
        }
    }
    let rhs_scale = ctx
        .admit_iter(&*rows, "creo matrix rhs scale")?
        .map(|row| row.rhs.abs())
        .fold(1.0, f64::max);
    let coefficient_tolerance = EPS_LINEAR_SYSTEM_COEFFICIENT;
    let residual_tolerance = EPS_LINEAR_SYSTEM_RESIDUAL * rhs_scale;
    let mut columns = 0..variable_count;
    while columns.len() != 0 {
        let Some(column) = (if variable_count <= MAX_NONLINEAR_SOLVE_VARIABLES {
            columns.next()
        } else {
            ctx.next_charged(&mut columns, "creo matrix column traversal")?
        }) else {
            break;
        };
        let pivot_row = column;
        let Some(selected) = ctx
            .admit_iter(pivot_row..rows.len(), "creo matrix pivot scan")?
            .max_by(|&first, &second| {
                rows[first].coefficients[column]
                    .abs()
                    .total_cmp(&rows[second].coefficients[column].abs())
            })
        else {
            return Ok(None);
        };
        let divisor = rows[selected].coefficients[column];
        if divisor.abs() <= coefficient_tolerance {
            return Ok(None);
        }
        rows.swap(pivot_row, selected);
        if rows[pivot_row].coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
            for coefficient in &mut rows[pivot_row].coefficients {
                *coefficient /= divisor;
            }
        } else {
            for coefficient in ctx.admit_iter(
                &mut rows[pivot_row].coefficients,
                "creo matrix pivot normalization",
            )? {
                *coefficient /= divisor;
            }
        }
        rows[pivot_row].rhs /= divisor;
        eliminate_pivot_column(ctx, rows, pivot_row, column, coefficient_tolerance)?;
    }
    if !ctx.all_by(
        &rows[variable_count..],
        |row| {
            Ok(
                (if row.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
                    row.coefficients
                        .iter()
                        .all(|coefficient| coefficient.abs() <= coefficient_tolerance)
                } else {
                    ctx.all_by(
                        &row.coefficients,
                        |coefficient| Ok(coefficient.abs() <= coefficient_tolerance),
                        "creo matrix residual coefficients",
                    )?
                }) && row.rhs.abs() <= residual_tolerance,
            )
        },
        "creo matrix residual scan",
    )? {
        return Ok(None);
    }
    let mut solution = ctx.alloc_filled(variable_count, 0.0, "creo affine unique solution")?;
    if solution.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
        for (slot, row) in solution.iter_mut().zip(rows.iter()) {
            *slot = row.rhs;
        }
        Ok(solution
            .iter()
            .all(|value| value.is_finite())
            .then_some(solution))
    } else {
        for (slot, row) in ctx
            .admit_iter(&mut solution, "creo affine solution traversal")?
            .zip(rows.iter())
        {
            *slot = row.rhs;
        }
        Ok(ctx
            .all_by(
                &solution,
                |value| Ok(value.is_finite()),
                "creo affine solution finite scan",
            )?
            .then_some(solution))
    }
}

pub(super) fn evaluate_affine_program(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &CurveExpressionRecord,
) -> Result<BTreeMap<String, AffineValue>, cadmpeg_core::CodecError> {
    let mut values = BTreeMap::new();
    ctx.insert_btree_map(
        &mut values,
        ctx.copy_retained_text("t", "creo affine time value name")?,
        AffineValue {
            constant: 0.0,
            linear: 1.0,
        },
        "creo affine time value node",
    )?;
    let mut defined_symbols = BTreeSet::new();
    ctx.insert_btree_set(
        &mut defined_symbols,
        ctx.copy_retained_text("t", "creo affine defined time name")?,
        "creo affine defined time node",
    )?;
    for assignment in ctx.admit_iter(&record.assignments, "creo affine assignment traversal")? {
        let Some((name, declared_unit)) = assignment.parameter_target() else {
            continue;
        };
        let mut key = ctx.copy_retained_text(name, "creo affine assignment names")?;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        let declaration_is_valid = declared_unit.is_none()
            || !ctx.contains_btree_set(
                &defined_symbols,
                &key,
                "creo affine defined symbol lookup",
            )?;
        if !ctx.contains_btree_set(&defined_symbols, &key, "creo affine defined symbol lookup")? {
            ctx.insert_btree_set(
                &mut defined_symbols,
                ctx.copy_retained_text(&key, "creo affine defined symbol names")?,
                "creo affine defined symbol nodes",
            )?;
        }
        match assignment.activation {
            CurveExpressionActivation::Active => {
                let value = if declaration_is_valid {
                    parse_relation_expression::<crate::curve::AffineValue>(
                        ctx,
                        &assignment.expression,
                        &values,
                        RelationEvaluationContext::default(),
                    )?
                    .map(|value| match declared_unit {
                        Some(unit) => match relation_unit(ctx, unit)? {
                            Some(unit) => value.with_unit_checked(unit, ctx),
                            None => Ok(None),
                        },
                        None => Ok(Some(value)),
                    })
                    .transpose()?
                    .flatten()
                } else {
                    None
                };
                if let Some(value) = value {
                    ctx.insert_btree_map(&mut values, key, value, "creo affine value nodes")?;
                } else {
                    ctx.remove_btree_map(&mut values, &key, "creo affine value removal")?;
                }
            }
            CurveExpressionActivation::Inactive => {}
            CurveExpressionActivation::Conditional => {
                ctx.remove_btree_map(&mut values, &key, "creo affine value removal")?;
            }
        }
    }
    Ok(values)
}

pub(super) fn infer_solve_variable_dimensions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    known_dimensions: &[Option<RelationDimension>],
    context: RelationEvaluationContext<'_>,
) -> Result<Option<Vec<RelationDimension>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if known_dimensions.len() != block.unknowns.len() {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo solve matrix scratch")?;
    let mut variable_keys = Vec::new();
    let mut known_keys = HashSet::new();
    let mut unknowns = block.unknowns.iter();
    while unknowns.len() != 0 {
        let Some(unknown) = (if block.unknowns.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
            unknowns.next()
        } else {
            ctx.next_charged(&mut unknowns, "creo solve variable traversal")?
        }) else {
            break;
        };
        scratch.with_storage(|| {
            ctx.reserve_vec(&mut variable_keys, 1, "creo dimension variable keys")
        })?;
        let mut key = scratch.with_storage(|| {
            ctx.copy_retained_text(&unknown.name, "creo dimension variable key text")
        })?;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        if ctx.contains_hash_set(&known_keys, &key, "creo dimension duplicate checks")? {
            return Ok(None);
        }
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut known_keys,
                ctx.copy_retained_text(&key, "creo dimension key index text")?,
                "creo dimension key index nodes",
            )
        })?;
        variable_keys.push(key);
    }

    let mut probe_values = BTreeMap::new();
    for (name, value) in ctx.admit_iter(values, "creo solve known value traversal")? {
        let probe = match value {
            CurveExpressionValue::String(value) => DimensionProbeValue::text(Some(
                scratch
                    .with_storage(|| ctx.copy_retained_text(value, "creo dimension known text"))?,
            )),
            _ => match DimensionProbeValue::from_relation_value(value) {
                Some(probe) => probe,
                None => continue,
            },
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut probe_values,
                ctx.copy_retained_text(name, "creo dimension known value names")?,
                probe,
                "creo dimension known value nodes",
            )
        })?;
    }
    let mut dimension_variables = variable_keys.iter().zip(known_dimensions);
    while dimension_variables.len() != 0 {
        let Some((key, dimension)) = (if variable_keys.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
            dimension_variables.next()
        } else {
            ctx.next_charged(
                &mut dimension_variables,
                "creo dimension variable traversal",
            )?
        }) else {
            break;
        };
        let value = match dimension {
            Some(dimension) => DimensionProbeValue {
                dimension: SymbolicRelationDimension::from_relation_dimension(*dimension),
                kind: DimensionProbeKind::Numeric(None),
                constraints: Vec::new(),
            },
            None => scratch.with_storage(|| DimensionProbeValue::variable(ctx, key))?,
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut probe_values,
                ctx.copy_retained_text(key, "creo dimension unknown value names")?,
                value,
                "creo dimension unknown value nodes",
            )
        })?;
    }

    let mut constraints = Vec::new();
    let mut equations = block.equations.iter();
    while equations.len() != 0 {
        let Some(equation) = ctx.next_charged(&mut equations, "creo solve equation traversal")? else {
            break;
        };
        let Some(left) = scratch.with_storage(|| {
            parse_relation_expression::<DimensionProbeValue>(
                ctx,
                &equation.left,
                &probe_values,
                context,
            )
        })?
        else {
            return Ok(None);
        };
        let Some(right) = scratch.with_storage(|| {
            parse_relation_expression::<DimensionProbeValue>(
                ctx,
                &equation.right,
                &probe_values,
                context,
            )
        })?
        else {
            return Ok(None);
        };
        for constraint in ctx
            .admit_iter(left.constraints, "creo dimension constraint traversal")?
            .chain(ctx.admit_iter(right.constraints, "creo dimension constraint traversal")?)
        {
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut constraints,
                    constraint,
                    "creo dimension constraint rows",
                )
            })?;
        }
        scratch.with_storage(|| {
            ctx.reserve_vec(&mut constraints, 1, "creo dimension constraint rows")
        })?;
        constraints.push(DimensionEquality {
            left: left.dimension,
            right: right.dimension,
        });
    }

    let mut axis_rows: [Vec<AffineEquationRow>; 5] =
        std::array::from_fn(|_| Vec::<AffineEquationRow>::new());
    let mut axis_variable_keys: [Vec<String>; 5] = std::array::from_fn(|_| Vec::new());
    for (axis, keys) in axis_variable_keys.iter_mut().enumerate() {
        let mut axis_keys = variable_keys.iter();
        while axis_keys.len() != 0 {
            let Some(variable) = (if variable_keys.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
                axis_keys.next()
            } else {
                ctx.next_charged(&mut axis_keys, "creo dimension axis key traversal")?
            }) else {
                break;
            };
            scratch
                .with_storage(|| ctx.reserve_vec(keys, 1, "creo dimension axis variable keys"))?;
            keys.push(scratch.with_storage(|| {
                dimension_variable_key(ctx, variable, axis, "creo dimension axis variable names")
            })?);
        }
    }
    let mut equalities = constraints.into_iter();
    while equalities.len() != 0 {
        let Some(equality) = ctx.next_charged(&mut equalities, "creo dimension equality traversal")? else {
            break;
        };
        for ((rows, keys), (left, right)) in axis_rows
            .iter_mut()
            .zip(&axis_variable_keys)
            .zip(equality.left.axes.into_iter().zip(equality.right.axes))
        {
            let Some(difference) =
                scratch.with_storage(|| left.combine_admitted(ctx, right, true))?
            else {
                return Ok(None);
            };
            let mut coefficients = scratch.with_storage(|| {
                ctx.alloc_filled(keys.len(), 0.0, "creo dimension equation coefficients")
            })?;
            let mut coefficient_rows = coefficients.iter_mut().zip(keys);
            let mut has_coefficients = false;
            while coefficient_rows.len() != 0 {
                let Some((coefficient, variable)) = (if keys.len() <= MAX_NONLINEAR_SOLVE_VARIABLES
                {
                    coefficient_rows.next()
                } else {
                    ctx.next_charged(
                        &mut coefficient_rows,
                        "creo dimension equation coefficient work",
                    )?
                }) else {
                    break;
                };
                *coefficient = ctx
                    .get_btree_map(
                        &difference.variables,
                        variable,
                        "creo dimension coefficient lookup",
                    )?
                    .copied()
                    .unwrap_or_default()
                    .as_f64()
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Creo dimension coefficient cannot be represented exactly",
                        )
                    })?;
                has_coefficients |= *coefficient != 0.0;
            }
            let rhs = -difference.constant.as_f64().ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(
                    "Creo dimension constant cannot be represented exactly",
                )
            })?;
            if has_coefficients || rhs != 0.0 {
                scratch
                    .with_storage(|| ctx.reserve_vec(rows, 1, "creo dimension equation rows"))?;
                rows.push(AffineEquationRow { coefficients, rhs });
            }
        }
    }

    let axis_len = variable_keys.len();
    let mut components: [Vec<i8>; 5] = std::array::from_fn(|_| Vec::new());
    for component in &mut components {
        *component = scratch
            .with_storage(|| ctx.alloc_filled(axis_len, 0i8, "creo_solve_dimension_components"))?;
    }
    let mut required_columns = BTreeSet::new();
    let mut known_components = known_dimensions.iter().enumerate();
    while known_components.len() != 0 {
        let Some((index, dimension)) = (if known_dimensions.len() <= MAX_NONLINEAR_SOLVE_VARIABLES
        {
            known_components.next()
        } else {
            ctx.next_charged(&mut known_components, "creo known dimension traversal")?
        }) else {
            break;
        };
        if let Some(dimension) = dimension {
            components[0][index] = dimension.length;
            components[1][index] = dimension.mass;
            components[2][index] = dimension.time;
            components[3][index] = dimension.angle;
            components[4][index] = dimension.temperature;
        } else {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut required_columns,
                    index,
                    "creo dimension required column nodes",
                )
            })?;
        }
    }
    for (axis, rows) in axis_rows.iter_mut().enumerate() {
        let Some(solution) = scratch.with_storage(|| {
            solve_dimension_axis(ctx, rows, variable_keys.len(), &required_columns)
        })?
        else {
            return Ok(None);
        };
        let mut solution_values = solution.into_iter().enumerate();
        while solution_values.len() != 0 {
            let Some((index, value)) = (if variable_keys.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
                solution_values.next()
            } else {
                ctx.next_charged(&mut solution_values, "creo inferred component traversal")?
            }) else {
                break;
            };
            if known_dimensions[index].is_some() {
                continue;
            }
            let rounded = value.round();
            if !value.is_finite()
                || (value - rounded).abs() > EPS_DIMENSION_SOLUTION
                || rounded < f64::from(i8::MIN)
                || rounded > f64::from(i8::MAX)
            {
                return Ok(None);
            }
            components[axis][index] = i8::try_from(
                cadmpeg_core::convert::truncate_f64_to_i32(rounded).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Creo numeric value cannot be represented exactly",
                    )
                })?,
            )
            .map_err(|_| {
                cadmpeg_core::CodecError::malformed("Creo numeric value exceeds dimension range")
            })?;
        }
    }
    let mut dimensions = ctx.alloc_filled(
        variable_keys.len(),
        RelationDimension::default(),
        "creo inferred variable dimensions",
    )?;
    let mut dimension_rows = dimensions.iter_mut().enumerate();
    while dimension_rows.len() != 0 {
        let Some((index, dimension)) = (if variable_keys.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
            dimension_rows.next()
        } else {
            ctx.next_charged(&mut dimension_rows, "creo inferred dimension traversal")?
        }) else {
            break;
        };
        *dimension = RelationDimension {
            length: components[0][index],
            mass: components[1][index],
            time: components[2][index],
            angle: components[3][index],
            temperature: components[4][index],
        };
    }
    Ok(Some(dimensions))
}

pub(super) fn solve_dimension_axis(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &mut [AffineEquationRow],
    variable_count: usize,
    required_columns: &BTreeSet<usize>,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo dimension pivot scratch")?;
    let mut pivot_row = 0;
    let mut pivot_rows = Vec::new();
    let coefficient_tolerance = EPS_LINEAR_SYSTEM_COEFFICIENT;
    let mut columns = 0..variable_count;
    while columns.len() != 0 {
        let Some(column) = (if variable_count <= MAX_NONLINEAR_SOLVE_VARIABLES {
            columns.next()
        } else {
            ctx.next_charged(&mut columns, "creo matrix column traversal")?
        }) else {
            break;
        };
        let Some(selected) = ctx
            .admit_iter(pivot_row..rows.len(), "creo matrix pivot scan")?
            .max_by(|&first, &second| {
                rows[first].coefficients[column]
                    .abs()
                    .total_cmp(&rows[second].coefficients[column].abs())
            })
        else {
            break;
        };
        let divisor = rows[selected].coefficients[column];
        if divisor.abs() <= coefficient_tolerance {
            continue;
        }
        rows.swap(pivot_row, selected);
        if rows[pivot_row].coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
            for coefficient in &mut rows[pivot_row].coefficients {
                *coefficient /= divisor;
            }
        } else {
            for coefficient in ctx.admit_iter(
                &mut rows[pivot_row].coefficients,
                "creo matrix pivot normalization",
            )? {
                *coefficient /= divisor;
            }
        }
        rows[pivot_row].rhs /= divisor;
        eliminate_pivot_column(ctx, rows, pivot_row, column, coefficient_tolerance)?;
        scratch.with_storage(|| {
            ctx.reserve_vec(&mut pivot_rows, 1, "creo solve dimension pivot rows")
        })?;
        pivot_rows.push((column, pivot_row));
        pivot_row += 1;
    }
    let residual_tolerance = EPS_LINEAR_SYSTEM_RESIDUAL
        * ctx
            .admit_iter(&*rows, "creo matrix rhs scale")?
            .map(|row| row.rhs.abs())
            .fold(1.0, f64::max);
    if !ctx.all_by(
        &*rows,
        |row| {
            Ok(
                (if row.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
                    row.coefficients
                        .iter()
                        .any(|coefficient| coefficient.abs() > coefficient_tolerance)
                } else {
                    ctx.any_by(
                        &row.coefficients,
                        |coefficient| Ok(coefficient.abs() > coefficient_tolerance),
                        "creo matrix residual coefficients",
                    )?
                }) || row.rhs.abs() <= residual_tolerance,
            )
        },
        "creo matrix residual scan",
    )? {
        return Ok(None);
    }
    let required_present = if required_columns.len() <= MAX_NONLINEAR_SOLVE_VARIABLES
        && pivot_rows.len() <= MAX_NONLINEAR_SOLVE_VARIABLES
    {
        required_columns.iter().all(|required| {
            pivot_rows
                .binary_search_by_key(required, |&(column, _)| column)
                .is_ok()
        })
    } else {
        ctx.all_by(
            required_columns,
            |required| {
                Ok(ctx
                    .binary_search_by_key(
                        &pivot_rows,
                        required,
                        |&(column, _)| Ok(column),
                        "creo dimension pivot column lookup",
                    )?
                    .is_ok())
            },
            "creo required dimension column traversal",
        )?
    };
    if !required_present {
        return Ok(None);
    }
    let mut solution = ctx.alloc_filled(variable_count, 0.0, "creo_solve_dimension_axis")?;
    let mut dimension_solution = pivot_rows.into_iter();
    while dimension_solution.len() != 0 {
        let Some((column, row)) = (if variable_count <= MAX_NONLINEAR_SOLVE_VARIABLES {
            dimension_solution.next()
        } else {
            ctx.next_charged(&mut dimension_solution, "creo dimension solution traversal")?
        }) else {
            break;
        };
        solution[column] = rows[row].rhs;
    }
    Ok(Some(solution))
}

pub(super) fn solve_affine_expression_block(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    variable_dimensions: &[RelationDimension],
    context: RelationEvaluationContext<'_>,
) -> Result<Option<Vec<CurveExpressionValue>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if variable_dimensions.len() != block.unknowns.len() {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo solve matrix scratch")?;
    let mut variable_keys = Vec::new();
    let mut unknowns = block.unknowns.iter();
    while unknowns.len() != 0 {
        let Some(unknown) = (if block.unknowns.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
            unknowns.next()
        } else {
            ctx.next_charged(&mut unknowns, "creo solve variable traversal")?
        }) else {
            break;
        };
        scratch
            .with_storage(|| ctx.reserve_vec(&mut variable_keys, 1, "creo affine variable keys"))?;
        let mut key = scratch
            .with_storage(|| ctx.copy_retained_text(&unknown.name, "creo affine variable names"))?;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        variable_keys.push(key);
    }
    let mut affine_values = BTreeMap::new();
    for (name, value) in ctx.admit_iter(values, "creo solve known value traversal")? {
        let Some((value, dimension)) = quantity_parts_ref(value) else {
            continue;
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut affine_values,
                ctx.copy_retained_text(name, "creo affine known value names")?,
                SimultaneousAffineValue::constant(value, dimension),
                "creo affine known value nodes",
            )
        })?;
    }
    let mut affine_variables = variable_keys.iter().zip(variable_dimensions);
    while affine_variables.len() != 0 {
        let Some((variable, dimension)) = (if variable_keys.len() <= MAX_NONLINEAR_SOLVE_VARIABLES
        {
            affine_variables.next()
        } else {
            ctx.next_charged(&mut affine_variables, "creo affine variable traversal")?
        }) else {
            break;
        };
        let mut coefficients = BTreeMap::new();
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut coefficients,
                ctx.copy_retained_text(variable, "creo affine coefficient names")?,
                1.0,
                "creo affine coefficient nodes",
            )
        })?;
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut affine_values,
                ctx.copy_retained_text(variable, "creo affine unknown value names")?,
                SimultaneousAffineValue {
                    dimension: *dimension,
                    constant: 0.0,
                    coefficients,
                },
                "creo affine unknown value nodes",
            )
        })?;
    }
    let mut rows = Vec::new();
    let mut equations = block.equations.iter();
    while equations.len() != 0 {
        let Some(equation) = ctx.next_charged(&mut equations, "creo solve equation traversal")? else {
            break;
        };
        let Some(left) = scratch.with_storage(|| {
            parse_relation_expression::<SimultaneousAffineValue>(
                ctx,
                &equation.left,
                &affine_values,
                context,
            )
        })?
        else {
            return Ok(None);
        };
        let Some(right) = scratch.with_storage(|| {
            parse_relation_expression::<SimultaneousAffineValue>(
                ctx,
                &equation.right,
                &affine_values,
                context,
            )
        })?
        else {
            return Ok(None);
        };
        let Some(difference) = scratch.with_storage(|| left.combine_admitted(right, true, ctx))?
        else {
            return Ok(None);
        };
        let mut coefficients = scratch.with_storage(|| {
            ctx.alloc_filled(
                variable_keys.len(),
                0.0,
                "creo affine equation coefficients",
            )
        })?;
        let mut affine_coefficients = coefficients.iter_mut().zip(&variable_keys);
        while affine_coefficients.len() != 0 {
            let Some((coefficient, variable)) = (if variable_keys.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
                    affine_coefficients.next()
                } else {
                    ctx.next_charged(
                        &mut affine_coefficients,
                        "creo affine coefficient traversal",
                    )?
                }) else {
                break;
            };
            *coefficient = ctx
                .get_btree_map(
                    &difference.coefficients,
                    variable,
                    "creo affine coefficient lookup",
                )?
                .copied()
                .unwrap_or(0.0);
        }
        scratch.with_storage(|| ctx.reserve_vec(&mut rows, 1, "creo affine equation rows"))?;
        rows.push(AffineEquationRow {
            coefficients,
            rhs: -difference.constant,
        });
    }
    let Some(solution) =
        scratch.with_storage(|| solve_unique_affine_system(ctx, &mut rows, variable_keys.len()))?
    else {
        return Ok(None);
    };
    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, solution.len(), "creo affine solved values")?;
    let mut solved_values = solution.into_iter().zip(variable_dimensions);
    while solved_values.len() != 0 {
        let Some((value, dimension)) = (if variable_dimensions.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
                solved_values.next()
            } else {
                ctx.next_charged(&mut solved_values, "creo affine solved value traversal")?
            }) else {
            break;
        };
        let Some(value) = quantity_value(value, *dimension) else {
            return Ok(None);
        };
        values.push(value);
    }
    Ok(Some(values))
}

#[cfg(test)]
mod tests;
