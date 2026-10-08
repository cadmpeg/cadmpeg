// SPDX-License-Identifier: Apache-2.0
//! Affine and nonlinear solving of relation equation blocks.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    copy_expression_value, expression_identifier_end, infer_solve_variable_dimensions,
    parse_relation_expression, quantity_parts_ref, quantity_value, relation_unit, AffineValue,
    CurveExpressionActivation, CurveExpressionRecord, CurveExpressionSolveBlock,
    CurveExpressionValue, ExpressionValue, RelationDimension, RelationEvaluationContext,
    EPS_LINEAR_SYSTEM_COEFFICIENT, EPS_LINEAR_SYSTEM_RESIDUAL,
};

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
    let Some(variable_dimensions) =
        infer_solve_variable_dimensions(ctx, block, values, known_dimensions, context)?
    else {
        return Ok(None);
    };
    let mut scratch = ctx.reserve_scoped(0, "creo nonlinear solve scratch")?;
    let Some(seeds) = scratch.with_storage(|| nonlinear_initial_guesses(ctx, initial_values, &variable_dimensions))? else {
        return Ok(None);
    };
    let mut seeds = seeds.into_iter();
    let Some(initial_seed) = seeds.next() else {
        return Ok(None);
    };
    let Some(solution) = refine_nonlinear_solution(
        ctx,
        block,
        values,
        &variable_dimensions,
        &initial_seed,
        context,
    )?
    else {
        return Ok(None);
    };
    for seed in seeds {
        let Some(candidate) =
            refine_nonlinear_solution(ctx, block, values, &variable_dimensions, &seed, context)?
        else {
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
    ctx.all_by(
        &block.equations,
        |equation| {
            Ok({
                nonlinear_expression_is_smooth(ctx, &equation.left)? && nonlinear_expression_is_smooth(ctx, &equation.right)?
            })
        },
        "creo relation comparison traversal",
    )
}

pub(super) fn nonlinear_expression_is_smooth(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    expression: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    let bytes = expression.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        ctx.next_charged(&mut bytes[cursor..].iter(), "creo nonlinear expression scan")?;
        if matches!(bytes[cursor], b'\'' | b'"') {
            let delimiter = bytes[cursor];
            cursor += 1;
            let tail = &bytes[cursor..];
            cursor += ctx.position_by(tail, |byte| Ok(*byte == delimiter), "creo nonlinear quoted expression scan")?.unwrap_or(tail.len());
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
            following += ctx.position_by(tail, |byte| Ok(!byte.is_ascii_whitespace()), "creo nonlinear following whitespace scan")?.unwrap_or(tail.len());
            if bytes.get(following) == Some(&b'(') {
                let name = &expression[start..end];
                let smooth = ctx.any_by(
                    &[
                        "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh",
                        "tanh", "log", "ln", "exp", "pow", "sqrt",
                    ],
                    |candidate| {
                        ctx.eq_ignore_ascii_case(name, candidate, "creo relation text comparison")
                    },
                    "creo relation comparison traversal",
                )?;
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
    let variable_count = variable_dimensions.len();
    let mut seeds = Vec::new();
    let mut add_seed = |seed: Vec<f64>| -> Result<(), cadmpeg_core::CodecError> {
        if seed.iter().all(|value| value.is_finite()) && !seeds.iter().any(|known| known == &seed) {
            ctx.reserve_vec(&mut seeds, 1, "creo solve seed rows")?;
            seeds.push(seed);
        }
        Ok(())
    };
    if initial_values.len() != variable_count {
        return Ok(None);
    }
    let mut initial = ctx.alloc_filled(variable_count, 0.0, "creo solve initial seed")?;
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
    add_seed(initial)?;
    add_seed(ctx.alloc_filled(variable_count, 0.0, "creo_solve_seed_zero")?)?;
    for magnitude in [0.01, 0.1, 1.0, 10.0, 100.0] {
        add_seed(ctx.alloc_filled(variable_count, magnitude, "creo_solve_seed_magnitude")?)?;
        add_seed(ctx.alloc_filled(variable_count, -magnitude, "creo_solve_seed_magnitude")?)?;
    }
    for index in 0..variable_count {
        for magnitude in [0.1, 1.0, 10.0] {
            let mut positive = ctx.alloc_filled(variable_count, 0.0, "creo_solve_seed_axis")?;
            positive[index] = magnitude;
            add_seed(positive)?;
            let mut negative = ctx.alloc_filled(variable_count, 0.0, "creo_solve_seed_axis")?;
            negative[index] = -magnitude;
            add_seed(negative)?;
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
    let variable_count = variable_dimensions.len();
    let mut point = ctx.alloc_filled(seed.len(), 0.0, "creo nonlinear initial point")?;
    point.copy_from_slice(seed);
    let Some(mut residuals) =
        evaluate_nonlinear_residuals(ctx, block, values, variable_dimensions, &point, context)?
    else {
        return Ok(None);
    };
    for _ in 0..MAX_NONLINEAR_SOLVE_ITERATIONS {
        if nonlinear_residuals_converged(ctx, &residuals)? {
            let Some(mut rank_rows) = nonlinear_jacobian_rows(
                ctx,
                block,
                values,
                variable_dimensions,
                &point,
                &residuals,
                context,
            )?
            else {
                return Ok(None);
            };
            if solve_unique_affine_system(ctx, &mut rank_rows, variable_count)?.is_none() {
                return Ok(None);
            }
            return Ok(Some(point));
        }
        let Some(mut rows) = nonlinear_jacobian_rows(
            ctx,
            block,
            values,
            variable_dimensions,
            &point,
            &residuals,
            context,
        )?
        else {
            return Ok(None);
        };
        for (row, residual) in rows.iter_mut().zip(&residuals) {
            row.rhs = -residual.value;
        }
        let Some(delta) = solve_unique_affine_system(ctx, &mut rows, variable_count)? else {
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
            ctx.charge_work(1, "creo nonlinear line-search work")?;
            let mut candidate =
                ctx.alloc_filled(point.len(), 0.0, "creo nonlinear line-search point")?;
            for ((slot, value), change) in candidate.iter_mut().zip(&point).zip(&delta) {
                *slot = value + scale * change;
            }
            if candidate.iter().all(|value| value.is_finite()) {
                if let Some(candidate_residuals) = evaluate_nonlinear_residuals(
                    ctx,
                    block,
                    values,
                    variable_dimensions,
                    &candidate,
                    context,
                )? {
                    valid_candidate = true;
                    let candidate_norm = nonlinear_residual_norm(ctx, &candidate_residuals, &residuals)?;
                    if nonlinear_residuals_converged(ctx, &candidate_residuals)?
                        || candidate_norm < base_norm
                    {
                        accepted = Some((candidate, candidate_residuals));
                        break;
                    }
                }
            }
            scale *= 0.5;
        }
        let Some((candidate, candidate_residuals)) = accepted else {
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
    let Some(mut rank_rows) = nonlinear_jacobian_rows(
        ctx,
        block,
        values,
        variable_dimensions,
        &point,
        &residuals,
        context,
    )?
    else {
        return Ok(None);
    };
    if solve_unique_affine_system(ctx, &mut rank_rows, variable_count)?.is_none() {
        return Ok(None);
    }
    Ok(Some(point))
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
    let variable_count = variable_dimensions.len();
    if variable_count > MAX_NONLINEAR_SOLVE_VARIABLES || point.len() != variable_count { return Ok(None); }
    let mut rows = Vec::new();
    for _ in ctx.admit_iter(residuals, "creo nonlinear Jacobian row initialization")? {
        let coefficients = ctx.alloc_filled(variable_count, 0.0, "creo nonlinear Jacobian coefficients")?;
        ctx.push_vec(&mut rows, AffineEquationRow { coefficients, rhs: 0.0 }, "creo nonlinear Jacobian rows")?;
    }
    for column in 0..variable_count {
        let mut probes = ctx.reserve_scoped(0, "creo nonlinear Jacobian probes")?;
        let step = NONLINEAR_SOLVE_DERIVATIVE_STEP * point[column].abs().max(1.0);
        let mut plus = probes.with_storage(|| ctx.alloc_filled(point.len(), 0.0, "creo nonlinear positive probe"))?;
        let mut minus = probes.with_storage(|| ctx.alloc_filled(point.len(), 0.0, "creo nonlinear negative probe"))?;
        plus.copy_from_slice(point);
        minus.copy_from_slice(point);
        plus[column] += step;
        minus[column] -= step;
        let Some(plus_residuals) = probes.with_storage(|| evaluate_nonlinear_residuals(ctx, block, values, variable_dimensions, &plus, context))? else { return Ok(None); };
        let Some(minus_residuals) = probes.with_storage(|| evaluate_nonlinear_residuals(ctx, block, values, variable_dimensions, &minus, context))? else { return Ok(None); };
        let mut input = rows.iter_mut().zip(residuals).enumerate();
        while let Some((row_index, (row, residual))) = ctx.next_charged(&mut input, "creo nonlinear Jacobian column traversal")? {
            let Some(plus_residual) = plus_residuals.get(row_index) else { return Ok(None); };
            let Some(minus_residual) = minus_residuals.get(row_index) else { return Ok(None); };
            if plus_residual.dimension != residual.dimension || minus_residual.dimension != residual.dimension { return Ok(None); }
            let derivative = (plus_residual.value - minus_residual.value) / (2.0 * step);
            if !derivative.is_finite() { return Ok(None); }
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
    if variable_dimensions.len() != block.unknowns.len() || point.len() != variable_dimensions.len()
    {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo nonlinear evaluation scratch")?;
    let mut evaluation_values = BTreeMap::new();
    for (name, value) in ctx.admit_iter(values, "creo nonlinear known value traversal")? {
        scratch.with_storage(|| ctx.insert_btree_map(
            &mut evaluation_values,
            ctx.copy_retained_text(name, "creo nonlinear known value names")?,
            copy_expression_value(ctx, value, "creo nonlinear known string values")?,
            "creo nonlinear known value nodes",
        ))?;
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
        let mut key = scratch.with_storage(|| ctx.copy_retained_text(variable, "creo nonlinear unknown value names"))?;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        let Some(value) = quantity_value(*value, *dimension) else {
            return Ok(None);
        };
        scratch.with_storage(|| ctx.insert_btree_map(
            &mut evaluation_values,
            key,
            value,
            "creo nonlinear unknown value nodes",
        ))?;
    }
    let mut residuals = Vec::new();
    let mut equations = block.equations.iter();
    while let Some(equation) = ctx.next_charged(&mut equations, "creo nonlinear equation traversal")? {
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

pub(super) fn nonlinear_residual_norm(ctx: &cadmpeg_core::decode::DecodeContext<'_>, residuals: &[SolveResidual], reference: &[SolveResidual]) -> Result<f64, cadmpeg_core::CodecError> {
    Ok(ctx.admit_iter(residuals, "creo nonlinear residual norm")?.zip(reference).map(|(residual, reference)| (residual.value / reference.scale).abs()).fold(0.0, f64::max))
}

pub(super) fn nonlinear_residuals_converged(ctx: &cadmpeg_core::decode::DecodeContext<'_>, residuals: &[SolveResidual]) -> Result<bool, cadmpeg_core::CodecError> {
    ctx.all_by(residuals, |residual| Ok(residual.value.abs() <= NONLINEAR_SOLVE_RESIDUAL_TOLERANCE * residual.scale), "creo nonlinear residual convergence")
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
    let (before, pivot_and_after) = rows.split_at_mut(pivot_row);
    let Some((pivot, after)) = pivot_and_after.split_first_mut() else {
        return Ok(());
    };
    for row in ctx.admit_iter(before, "creo matrix elimination rows")?.chain(ctx.admit_iter(after, "creo matrix elimination rows")?) {
        let factor = row.coefficients[column];
        if factor.abs() <= coefficient_tolerance {
            continue;
        }
        for (coefficient, pivot_coefficient) in ctx.admit_iter(&mut row.coefficients, "creo matrix elimination")?.zip(&pivot.coefficients)
        {
            *coefficient -= factor * pivot_coefficient;
            if coefficient.abs() <= coefficient_tolerance {
                *coefficient = 0.0;
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
    if variable_count == 0 || rows.len() < variable_count {
        return Ok(None);
    }
    for row in ctx.admit_iter(&mut *rows, "creo matrix row normalization")? {
        let scale = ctx.admit_iter(&row.coefficients, "creo matrix row normalization")?
            .map(|value| value.abs())
            .fold(0.0, f64::max);
        if scale > 0.0 {
            for coefficient in ctx.admit_iter(&mut row.coefficients, "creo matrix row normalization")? {
                *coefficient /= scale;
            }
            row.rhs /= scale;
        }
    }
    let rhs_scale = ctx.admit_iter(&*rows, "creo matrix rhs scale")?.map(|row| row.rhs.abs()).fold(1.0, f64::max);
    let coefficient_tolerance = EPS_LINEAR_SYSTEM_COEFFICIENT;
    let residual_tolerance = EPS_LINEAR_SYSTEM_RESIDUAL * rhs_scale;
    let mut columns = 0..variable_count;
    while let Some(column) = ctx.next_charged(&mut columns, "creo matrix column traversal")? {
        let pivot_row = column;
        let Some(selected) = ctx.admit_iter(pivot_row..rows.len(), "creo matrix pivot scan")?.max_by(|&first, &second| {
            rows[first].coefficients[column]
                .abs()
                .total_cmp(&rows[second].coefficients[column].abs())
        }) else {
            return Ok(None);
        };
        let divisor = rows[selected].coefficients[column];
        if divisor.abs() <= coefficient_tolerance {
            return Ok(None);
        }
        rows.swap(pivot_row, selected);
        for coefficient in ctx.admit_iter(&mut rows[pivot_row].coefficients, "creo matrix pivot normalization")? {
            *coefficient /= divisor;
        }
        rows[pivot_row].rhs /= divisor;
        eliminate_pivot_column(ctx, rows, pivot_row, column, coefficient_tolerance)?;
    }
    if !ctx.all_by(&rows[variable_count..], |row| {
        Ok(ctx.all_by(&row.coefficients, |coefficient| Ok(coefficient.abs() <= coefficient_tolerance), "creo matrix residual coefficients")? && row.rhs.abs() <= residual_tolerance)
    }, "creo matrix residual scan")? {
        return Ok(None);
    }
    let mut solution = ctx.alloc_filled(variable_count, 0.0, "creo affine unique solution")?;
    for (slot, row) in ctx.admit_iter(&mut solution, "creo affine solution traversal")?.zip(rows.iter()) {
        *slot = row.rhs;
    }
    Ok(ctx.all_by(&solution, |value| Ok(value.is_finite()), "creo affine solution finite scan")?.then_some(solution))
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
        let declaration_is_valid = declared_unit.is_none() || !ctx.contains_btree_set(&defined_symbols, &key, "creo affine defined symbol lookup")?;
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
