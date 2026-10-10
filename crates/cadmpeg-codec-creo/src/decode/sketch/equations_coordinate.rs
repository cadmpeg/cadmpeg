// SPDX-License-Identifier: Apache-2.0
//! Section-equation coordinate constraints and linear solvers.

use super::axis::SectionAxis;

use crate::feature::definitions::VariableType;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::super::feature_history::dimensions::feature_dimension_table_complete;
use super::equations_scalar::{
    reconcile_equation_value, section_equation_scalar_equality_values, SectionScalarVariable,
};
use super::skamp::SectionPointSource;
use crate::decode::quadratic::Coefficient;
use crate::decode::sketch_transfer::solver_links::EquationIncidences;

const EPS_DIMENSION_BINDING: f64 = 1.0e-9;
const EPS_DISTANCE_AGREEMENT: f64 = 1.0e-9;
const EPS_SOLVER_SCALE: f64 = 1.0e-12;
const EPS_SOLUTION_AGREEMENT: f64 = 1.0e-9;

#[derive(Clone, Copy)]
enum SectionSixDistance {
    Measured(PositiveLength),
    ActiveIncomplete,
    Inactive(Option<PositiveLength>),
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionFunctionSixDistance {
    pub(in crate::decode) first: u32,
    pub(in crate::decode) second: u32,
    pub(in crate::decode) radius: SectionScalarVariable,
    distance: SectionSixDistance,
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
}

impl SectionFunctionSixDistance {
    fn coordinate_distance(self) -> Option<f64> {
        match self.distance {
            SectionSixDistance::Measured(distance) => Some(distance.get()),
            SectionSixDistance::ActiveIncomplete | SectionSixDistance::Inactive(_) => None,
        }
    }

    /// The distance for an equation with both endpoints resolved.
    pub(in crate::decode) fn constraint_distance(self) -> Option<PositiveLength> {
        match self.distance {
            SectionSixDistance::Measured(distance) => Some(distance),
            SectionSixDistance::Inactive(distance) => distance,
            SectionSixDistance::ActiveIncomplete => None,
        }
    }

    /// Whether the equation is enabled.
    pub(in crate::decode) fn active(self) -> bool {
        !matches!(self.distance, SectionSixDistance::Inactive(_))
    }
}

pub(super) fn section_equation_function_six_distance_values(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<(SectionScalarVariable, f64)>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation function six distance values scratch",
    )?;
    let source_rows = scratch.with_storage(|| {
        section_equation_function_six_distance_rows(
            ctx,
            definition,
            coordinates,
            ambiguous_point_ids,
        )
    })?;
    ctx.collect_vec(
        ctx.admit_iter(&source_rows, "creo section projected equation rows")?
            .copied()
            .filter_map(|equation| Some((equation.radius, equation.coordinate_distance()?))),
        "creo section equation six distance values",
    )
}

pub(in crate::decode) fn section_equation_function_six_distance_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionFunctionSixDistance>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation function six distance rows scratch",
    )?;
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(Vec::new());
    };
    let Some(equations) = scratch.with_storage(|| {
        crate::feature::definitions::equation_table(ctx, &definition.body, 0, definition.body.len())
    })?
    else {
        return Ok(Vec::new());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(Vec::new());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(Vec::new());
    }
    let scalar_equality_values =
        scratch.with_storage(|| section_equation_scalar_equality_values(ctx, definition))?;
    let row = |ordinal: Option<u32>| {
        usize::try_from(ordinal?)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    let mut rows = Vec::new();
    let equation_solver_rows = ctx.admit_iter(&equations.rows, "creo section source equation rows")?;
    let equation_solver = EquationIncidences::new(ctx, definition)?;
    for equation in equation_solver_rows {
        if equation.function_id != 6 {
            continue;
        }
        let [Some(first_u), Some(first_v), Some(second_u), Some(second_v), Some(radius)] =
            equation.arguments.as_slice()
        else {
            continue;
        };
        let (Some(first_u), Some(first_v), Some(second_u), Some(second_v), Some(radius)) = (
            row(Some(*first_u)),
            row(Some(*first_v)),
            row(Some(*second_u)),
            row(Some(*second_v)),
            row(Some(*radius)),
        ) else {
            continue;
        };
        if first_u.variable_type != VariableType::U
            || first_v.variable_type != VariableType::V
            || first_u.key != first_v.key
            || second_u.variable_type != VariableType::U
            || second_v.variable_type != VariableType::V
            || second_u.key != second_v.key
            || first_u.key == second_u.key
            || radius.variable_type != VariableType::Radius
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &first_u.key,
                "creo section ambiguous point ids contains",
            )?
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &second_u.key,
                "creo section ambiguous point ids contains",
            )?
        {
            continue;
        }
        let Ok(radius_equality) = ctx
            .get_btree_map(
                &scalar_equality_values,
                &(radius.variable_type, radius.key),
                "creo section scalar equality values get",
            )?
            .copied()
            .unwrap_or(Ok(None))
        else {
            continue;
        };
        let Ok(radius_value) = reconcile_equation_value(radius.value.value(), radius_equality)
        else {
            continue;
        };
        let stored_distance = radius_value.and_then(PositiveLength::new);
        if radius_value.is_some() && stored_distance.is_none() {
            continue;
        }
        let active = !equation_solver.is_disabled(equation.equation_id);
        let first_point = ctx
            .get_btree_map(coordinates, &first_u.key, "creo section coordinates get")?
            .and_then(|point| Some([point[0]?, point[1]?]));
        let second_point = ctx
            .get_btree_map(coordinates, &second_u.key, "creo section coordinates get")?
            .and_then(|point| Some([point[0]?, point[1]?]));
        let points_complete = first_point.is_some() && second_point.is_some();
        let distance = if active {
            match (first_point, second_point) {
                (Some(first), Some(second)) => {
                    let delta = [second[0] - first[0], second[1] - first[1]];
                    let Some(distance) = PositiveLength::new(delta[0].hypot(delta[1])) else {
                        continue;
                    };
                    if stored_distance.is_some_and(|stored| {
                        !(FiniteReal::new(stored.get()))
                            .zip(FiniteReal::new(distance.get()))
                            .is_some_and(|(first, second)| approximately_equal(first, second))
                    }) {
                        continue;
                    }
                    SectionSixDistance::Measured(distance)
                }
                _ => SectionSixDistance::ActiveIncomplete,
            }
        } else {
            SectionSixDistance::Inactive(if points_complete {
                stored_distance
            } else {
                None
            })
        };
        ctx.push_vec(
            &mut rows,
            SectionFunctionSixDistance {
                first: first_u.key,
                second: second_u.key,
                radius: (radius.variable_type, radius.key),
                distance,
                equation_id: equation.equation_id,
                offset: equation.offset,
            },
            "creo section equation function six distance rows",
        )?;
    }
    Ok(rows)
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionUnsignedCoordinateDistance {
    pub(in crate::decode) first: u32,
    pub(in crate::decode) second: u32,
    pub(in crate::decode) coordinate: SectionAxis,
    pub(in crate::decode) scalar: SectionScalarVariable,
    pub(in crate::decode) value: f64,
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
    pub(in crate::decode) active: bool,
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionRadiusDimension {
    pub(in crate::decode) radius_variable: SectionScalarVariable,
    pub(in crate::decode) radius: u32,
    pub(in crate::decode) scalar: SectionScalarVariable,
    pub(in crate::decode) value: PositiveLength,
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
    pub(in crate::decode) active: bool,
}

fn section_equation_dimension_scalar_value(
    scalar: &crate::feature::definitions::FeatureVariableRow,
    equality_value: Option<f64>,
    dimension_value: f64,
    strictly_positive: bool,
) -> Option<f64> {
    let valid = |value: f64| {
        value.is_finite()
            && (strictly_positive && value > 0.0 || !strictly_positive && value >= 0.0)
    };
    if !valid(dimension_value) {
        return None;
    }
    let scalar_value = reconcile_equation_value(scalar.value.value(), equality_value).ok()?;
    match scalar_value {
        Some(value)
            if valid(value)
                && (FiniteReal::new(value))
                    .zip(FiniteReal::new(dimension_value))
                    .is_some_and(|(first, second)| approximately_equal(first, second)) =>
        {
            Some(dimension_value)
        }
        None if scalar.value == crate::feature::definitions::ScalarLane::DimensionDriven => {
            Some(dimension_value)
        }
        _ => None,
    }
}

pub(super) fn section_equation_unsigned_coordinate_distances(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionUnsignedCoordinateDistance>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation unsigned coordinate distances scratch",
    )?;
    let source_rows = scratch.with_storage(|| {
        section_equation_unsigned_coordinate_distance_rows(ctx, definition, ambiguous_point_ids)
    })?;
    ctx.collect_vec(
        ctx.admit_iter(&source_rows, "creo section projected equation rows")?
            .copied()
            .filter(|constraint| constraint.active),
        "creo section unsigned coordinate distances",
    )
}

pub(in crate::decode) fn section_equation_unsigned_coordinate_distance_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionUnsignedCoordinateDistance>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation unsigned coordinate distance rows scratch",
    )?;
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(Vec::new());
    };
    let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|table| feature_dimension_table_complete(table))
    else {
        return Ok(Vec::new());
    };
    let Some(equations) = scratch.with_storage(|| {
        crate::feature::definitions::equation_table(ctx, &definition.body, 0, definition.body.len())
    })?
    else {
        return Ok(Vec::new());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(Vec::new());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(Vec::new());
    }
    let scalar_equality_values =
        scratch.with_storage(|| section_equation_scalar_equality_values(ctx, definition))?;
    let mut rows = Vec::new();
    let equation_solver_rows = ctx
        .admit_iter(&equations.rows, "creo section source equation rows")?;
    let equation_solver = EquationIncidences::new(ctx, definition)?;
    for equation in equation_solver_rows
        .filter(|equation| equation.function_id == 3 && equation.arguments.len() == 3)
    {
        let [Some(first), Some(second), Some(dimension)] = equation.arguments.as_slice() else {
            continue;
        };
        let Some(first) = usize::try_from(*first)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(second) = usize::try_from(*second)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(dimension) = usize::try_from(*dimension)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        if first.variable_type != second.variable_type
            || !matches!(first.variable_type, VariableType::U | VariableType::V)
            || dimension.variable_type != VariableType::Dimension
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &first.key,
                "creo section ambiguous point ids contains",
            )?
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &second.key,
                "creo section ambiguous point ids contains",
            )?
            || first.key == second.key
        {
            continue;
        }
        let Some(dimension_row) = usize::try_from(dimension.key)
            .ok()
            .and_then(|ordinal| dimensions.rows.get(ordinal))
        else {
            continue;
        };
        if !matches!(dimension_row.dimension_type, 1..=5) {
            continue;
        }
        let Ok(equality_value) = ctx
            .get_btree_map(
                &scalar_equality_values,
                &(dimension.variable_type, dimension.key),
                "creo section scalar equality values get",
            )?
            .copied()
            .unwrap_or(Ok(None))
        else {
            continue;
        };
        let Some(dimension_value) = dimension_row.value.resolved() else {
            continue;
        };
        let Some(value) = section_equation_dimension_scalar_value(
            dimension,
            equality_value,
            dimension_value.abs(),
            false,
        ) else {
            continue;
        };
        let Some(coordinate) = SectionAxis::from_variable(first.variable_type) else {
            continue;
        };
        ctx.push_vec(
            &mut rows,
            SectionUnsignedCoordinateDistance {
                first: first.key,
                second: second.key,
                coordinate,
                scalar: (dimension.variable_type, dimension.key),
                value,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !equation_solver.is_disabled(equation.equation_id),
            },
            "creo section equation unsigned coordinate distance rows",
        )?;
    }
    Ok(rows)
}

pub(in crate::decode) fn section_equation_radius_dimensions(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<SectionRadiusDimension>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo section equation radius dimensions scratch")?;
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(Vec::new());
    };
    let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|table| feature_dimension_table_complete(table))
    else {
        return Ok(Vec::new());
    };
    let Some(equations) = scratch.with_storage(|| {
        crate::feature::definitions::equation_table(ctx, &definition.body, 0, definition.body.len())
    })?
    else {
        return Ok(Vec::new());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(Vec::new());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(Vec::new());
    }
    let scalar_equality_values =
        scratch.with_storage(|| section_equation_scalar_equality_values(ctx, definition))?;
    let mut rows = Vec::new();
    let equation_solver_rows = ctx
        .admit_iter(&equations.rows, "creo section source equation rows")?;
    let equation_solver = EquationIncidences::new(ctx, definition)?;
    for equation in equation_solver_rows
        .filter(|equation| equation.function_id == 2 && equation.arguments.len() == 2)
    {
        let [Some(first), Some(second)] = equation.arguments.as_slice() else {
            continue;
        };
        let Some(first) = usize::try_from(*first)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(second) = usize::try_from(*second)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let (radius, scalar) = match (first.variable_type, second.variable_type) {
            (VariableType::Radius, VariableType::Dimension) => (first, second),
            (VariableType::Dimension, VariableType::Radius) => (second, first),
            _ => continue,
        };
        let Some(dimension) = usize::try_from(scalar.key)
            .ok()
            .and_then(|ordinal| dimensions.rows.get(ordinal))
        else {
            continue;
        };
        let Some(dimension_value) = dimension.value.resolved() else {
            continue;
        };
        let Ok(radius_equality) = ctx
            .get_btree_map(
                &scalar_equality_values,
                &(radius.variable_type, radius.key),
                "creo section scalar equality values get",
            )?
            .copied()
            .unwrap_or(Ok(None))
        else {
            continue;
        };
        let Ok(radius_value) = reconcile_equation_value(radius.value.value(), radius_equality)
        else {
            continue;
        };
        if dimension.dimension_type != 3
            || radius_value.is_some_and(|value| {
                !value.is_finite()
                    || value <= 0.0
                    || (value - dimension_value).abs()
                        > EPS_DIMENSION_BINDING * value.abs().max(dimension_value.abs()).max(1.0)
            })
        {
            continue;
        }
        let Ok(equality_value) = ctx
            .get_btree_map(
                &scalar_equality_values,
                &(scalar.variable_type, scalar.key),
                "creo section scalar equality values get",
            )?
            .copied()
            .unwrap_or(Ok(None))
        else {
            continue;
        };
        let Some(value) =
            section_equation_dimension_scalar_value(scalar, equality_value, dimension_value, true)
        else {
            continue;
        };
        let Some(value) = PositiveLength::new(value) else {
            continue;
        };
        ctx.push_vec(
            &mut rows,
            SectionRadiusDimension {
                radius_variable: (radius.variable_type, radius.key),
                radius: radius.key,
                scalar: (scalar.variable_type, scalar.key),
                value,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !equation_solver.is_disabled(equation.equation_id),
            },
            "creo section equation radius dimensions",
        )?;
    }
    Ok(rows)
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionPointOnLineConstraint {
    pub(in crate::decode) target: u32,
    pub(in crate::decode) first: u32,
    pub(in crate::decode) second: u32,
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
    pub(in crate::decode) active: bool,
}

pub(super) fn section_equation_point_on_line_constraints(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<(u32, u32, u32)>, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "creo section equation point on line constraints scratch")?;
    let source_rows = scratch.with_storage(|| {
        section_equation_point_on_line_constraint_rows(ctx, definition, ambiguous_point_ids)
    })?;
    ctx.collect_vec(
        ctx.admit_iter(&source_rows, "creo section projected equation rows")?
            .copied()
            .filter(|constraint| constraint.active)
            .map(|constraint| (constraint.target, constraint.first, constraint.second)),
        "creo section point on line constraints",
    )
}

pub(in crate::decode) fn section_equation_point_on_line_constraint_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionPointOnLineConstraint>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation point on line constraint rows scratch",
    )?;
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(Vec::new());
    };
    let Some(equations) = scratch.with_storage(|| {
        crate::feature::definitions::equation_table(ctx, &definition.body, 0, definition.body.len())
    })?
    else {
        return Ok(Vec::new());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(Vec::new());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(Vec::new());
    }
    let scalar_equality_values =
        scratch.with_storage(|| section_equation_scalar_equality_values(ctx, definition))?;
    let mut rows = Vec::new();
    let equation_solver_rows = ctx
        .admit_iter(&equations.rows, "creo section source equation rows")?;
    let equation_solver = EquationIncidences::new(ctx, definition)?;
    for equation in equation_solver_rows
        .filter(|equation| equation.function_id == 35 && equation.arguments.len() == 9)
    {
        let [Some(target_u), Some(target_v), Some(first_u), Some(first_v), Some(second_u), Some(second_v), Some(line_parameter), Some(first_zero), Some(second_zero)] =
            equation.arguments.as_slice()
        else {
            continue;
        };
        let Some(target_u) = usize::try_from(*target_u)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(target_v) = usize::try_from(*target_v)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(first_u) = usize::try_from(*first_u)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(first_v) = usize::try_from(*first_v)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(second_u) = usize::try_from(*second_u)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(second_v) = usize::try_from(*second_v)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(line_parameter) = usize::try_from(*line_parameter)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(first_zero) = usize::try_from(*first_zero)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(second_zero) = usize::try_from(*second_zero)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        if target_u.variable_type != VariableType::U
            || target_v.variable_type != VariableType::V
            || first_u.variable_type != VariableType::U
            || first_v.variable_type != VariableType::V
            || second_u.variable_type != VariableType::U
            || second_v.variable_type != VariableType::V
            || target_u.key != target_v.key
            || first_u.key != first_v.key
            || second_u.key != second_v.key
            || target_u.key == first_u.key
            || target_u.key == second_u.key
            || first_u.key == second_u.key
            || line_parameter.variable_type != VariableType::Parameter
            || first_zero.variable_type != VariableType::Selector
            || second_zero.variable_type != VariableType::Selector
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &target_u.key,
                "creo section ambiguous point ids contains",
            )?
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &first_u.key,
                "creo section ambiguous point ids contains",
            )?
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &second_u.key,
                "creo section ambiguous point ids contains",
            )?
        {
            continue;
        }
        let zero_value = |row: &crate::feature::definitions::FeatureVariableRow| -> Result<Option<f64>, CodecError> {
                let equality = ctx.get_btree_map(&scalar_equality_values, &(row.variable_type, row.key), "creo section scalar equality lookup")?.copied().unwrap_or(Ok(None));
                Ok(equality.ok().and_then(|value| reconcile_equation_value(row.value.value(), value).ok()).flatten())
            };
        if zero_value(first_zero)? != Some(0.0) || zero_value(second_zero)? != Some(0.0) {
            continue;
        }
        ctx.push_vec(
            &mut rows,
            SectionPointOnLineConstraint {
                target: target_u.key,
                first: first_u.key,
                second: second_u.key,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !equation_solver.is_disabled(equation.equation_id),
            },
            "creo section equation point on line constraint rows",
        )?;
    }
    Ok(rows)
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionEqualLengthConstraint {
    pub(in crate::decode) first: [u32; 2],
    pub(in crate::decode) second: [u32; 2],
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
    pub(in crate::decode) active: bool,
}

pub(super) fn section_equation_equal_length_constraints(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionEqualLengthConstraint>, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "creo section equation equal length constraints scratch")?;
    let source_rows = scratch.with_storage(|| {
        section_equation_equal_length_constraint_rows(ctx, definition, ambiguous_point_ids)
    })?;
    ctx.collect_vec(
        ctx.admit_iter(&source_rows, "creo section projected equation rows")?
            .copied()
            .filter(|constraint| constraint.active),
        "creo section equal length constraints",
    )
}

pub(in crate::decode) fn section_equation_equal_length_constraint_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionEqualLengthConstraint>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation equal length constraint rows scratch",
    )?;
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(Vec::new());
    };
    let Some(equations) = scratch.with_storage(|| {
        crate::feature::definitions::equation_table(ctx, &definition.body, 0, definition.body.len())
    })?
    else {
        return Ok(Vec::new());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(Vec::new());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(Vec::new());
    }
    let scalar_equality_values =
        scratch.with_storage(|| section_equation_scalar_equality_values(ctx, definition))?;
    let mut rows = Vec::new();
    let equation_solver_rows = ctx
        .admit_iter(&equations.rows, "creo section source equation rows")?;
    let equation_solver = EquationIncidences::new(ctx, definition)?;
    for equation in equation_solver_rows
        .filter(|equation| equation.function_id == 33 && equation.arguments.len() == 9)
    {
        let argument_rows: [Option<&crate::feature::definitions::FeatureVariableRow>; 9] =
            std::array::from_fn(|index| {
                let ordinal = equation.arguments[index]?;
                variables.rows.get(usize::try_from(ordinal).ok()?)
            });
        let [Some(first_u), Some(first_v), Some(second_u), Some(second_v), Some(third_u), Some(third_v), Some(fourth_u), Some(fourth_v), Some(auxiliary)] =
            argument_rows
        else {
            continue;
        };
        if first_u.variable_type != VariableType::U
            || first_v.variable_type != VariableType::V
            || second_u.variable_type != VariableType::U
            || second_v.variable_type != VariableType::V
            || third_u.variable_type != VariableType::U
            || third_v.variable_type != VariableType::V
            || fourth_u.variable_type != VariableType::U
            || fourth_v.variable_type != VariableType::V
            || auxiliary.variable_type != VariableType::Auxiliary
            || first_u.key != first_v.key
            || second_u.key != second_v.key
            || third_u.key != third_v.key
            || fourth_u.key != fourth_v.key
            || first_u.key == second_u.key
            || third_u.key == fourth_u.key
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &first_u.key,
                "creo section ambiguous point lookup",
            )?
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &second_u.key,
                "creo section ambiguous point lookup",
            )?
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &third_u.key,
                "creo section ambiguous point lookup",
            )?
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &fourth_u.key,
                "creo section ambiguous point lookup",
            )?
        {
            continue;
        }
        let Ok(equality_value) = ctx
            .get_btree_map(
                &scalar_equality_values,
                &(auxiliary.variable_type, auxiliary.key),
                "creo section scalar equality values get",
            )?
            .copied()
            .unwrap_or(Ok(None))
        else {
            continue;
        };
        let Ok(auxiliary_value) = reconcile_equation_value(auxiliary.value.value(), equality_value)
        else {
            continue;
        };
        if auxiliary_value != Some(0.0) {
            continue;
        }
        ctx.push_vec(
            &mut rows,
            SectionEqualLengthConstraint {
                first: [first_u.key, second_u.key],
                second: [third_u.key, fourth_u.key],
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !equation_solver.is_disabled(equation.equation_id),
            },
            "creo section equation equal length constraint rows",
        )?;
    }
    Ok(rows)
}

pub(super) type SectionCoordinateVariable = (u32, SectionAxis);

#[derive(Clone, Default)]
pub(in crate::decode) struct SectionCoordinateEquation {
    pub(super) terms: BTreeMap<SectionCoordinateVariable, f64>,
    pub(in crate::decode) rhs: f64,
}

impl SectionCoordinateEquation {
    pub(in crate::decode) fn point_value(
        ctx: &DecodeContext<'_>,
        point: u32,
        coordinate: SectionAxis,
        value: f64,
    ) -> Result<Self, CodecError> {
        let mut equation = Self::default();
        equation.add_point(ctx, point, coordinate, 1.0)?;
        equation.rhs = value;
        Ok(equation)
    }

    pub(in crate::decode) fn point_difference(
        ctx: &DecodeContext<'_>,
        first: u32,
        second: u32,
        coordinate: SectionAxis,
        delta: f64,
    ) -> Result<Self, CodecError> {
        Self::point_difference_with_operation(
            ctx,
            first,
            second,
            coordinate,
            delta,
            "creo coordinate equation term nodes",
        )
    }

    fn point_difference_with_operation(
        ctx: &DecodeContext<'_>,
        first: u32,
        second: u32,
        coordinate: SectionAxis,
        delta: f64,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut equation = Self::default();
        ctx.insert_btree_map(&mut equation.terms, (first, coordinate), -1.0, operation)?;

        *ctx.entry_btree_map(&mut equation.terms, (second, coordinate), operation)?
            .or_default() += 1.0;
        equation.rhs = delta;
        Ok(equation)
    }

    pub(super) fn source_difference(
        ctx: &DecodeContext<'_>,
        first: SectionPointSource,
        second: SectionPointSource,
        coordinate: SectionAxis,
        delta: f64,
    ) -> Result<Self, CodecError> {
        let mut equation = Self::default();
        equation.add_source(ctx, first, coordinate, -1.0)?;
        equation.add_source(ctx, second, coordinate, 1.0)?;
        equation.rhs += delta;
        Ok(equation)
    }

    pub(in crate::decode) fn add_point(
        &mut self,
        ctx: &DecodeContext<'_>,
        point: u32,
        coordinate: SectionAxis,
        coefficient: f64,
    ) -> Result<(), CodecError> {
        *ctx.entry_btree_map(
            &mut self.terms,
            (point, coordinate),
            "creo coordinate equation term nodes",
        )?
        .or_default() += coefficient;
        Ok(())
    }

    pub(super) fn add_source(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: SectionPointSource,
        coordinate: SectionAxis,
        coefficient: f64,
    ) -> Result<(), CodecError> {
        match source {
            SectionPointSource::Point(point) => {
                self.add_point(ctx, point, coordinate, coefficient)?;
            }
            SectionPointSource::Value(value) => self.rhs -= coefficient * value[coordinate.index()],
        }
        Ok(())
    }
}

fn admitted_coordinate_variables(
    ctx: &DecodeContext<'_>,
    equations: &[SectionCoordinateEquation],
    distances: &[(u32, u32, SectionAxis, f64)],
) -> Result<
    (
        Vec<SectionCoordinateVariable>,
        HashMap<SectionCoordinateVariable, usize>,
    ),
    CodecError,
> {
    let mut scratch = ctx.reserve_scoped(0, "creo admitted coordinate variables scratch")?;
    let mut unique = BTreeSet::new();
    for equation in ctx.admit_iter(equations, "creo coordinate variable equations")? {
        for (variable, _) in
            ctx.admit_iter(&equation.terms, "creo coordinate equation variables")?
        {
            scratch.with_storage(|| {
                ctx.insert_btree_set(&mut unique, *variable, "creo section unique variables")
            })?;
        }
    }
    for &(first, second, coordinate, _) in
        ctx.admit_iter(distances, "creo coordinate variable distances")?
    {
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut unique,
                (first, coordinate),
                "creo section unique variables",
            )
        })?;
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut unique,
                (second, coordinate),
                "creo section unique variables",
            )
        })?;
    }
    let mut variables = Vec::new();
    ctx.reserve_vec(
        &mut variables,
        unique.len(),
        "creo section ordered variables",
    )?;
    variables.extend(ctx.admit_iter(unique, "creo section ordered variable copy")?);
    let mut indices = HashMap::new();
    for (index, variable) in ctx
        .admit_iter(&variables, "creo ordered section variables")?
        .enumerate()
    {
        ctx.insert_hash_map(
            &mut indices,
            *variable,
            index,
            "creo section variable indices",
        )?;
    }
    Ok((variables, indices))
}

fn section_remaining_variables(
    ctx: &DecodeContext<'_>,
    count: usize,
) -> Result<BTreeSet<usize>, CodecError> {
    let mut remaining = BTreeSet::new();
    for index in ctx.admit_iter(&(0..count), "creo section remaining variable scan")? {
        ctx.insert_btree_set(&mut remaining, index, "creo section remaining variables")?;
    }
    Ok(remaining)
}

fn next_section_component(
    ctx: &DecodeContext<'_>,
    remaining: &mut BTreeSet<usize>,
    adjacency: &[BTreeSet<usize>],
) -> Result<Option<BTreeSet<usize>>, CodecError> {
    if let Some(original) = ctx.resource_refusal() {
        return Err(CodecError::ResourceLimit(original));
    }
    if remaining.is_empty() {
        return Ok(None);
    }
    let Some(seed) = ctx.find_map(
        &*remaining,
        |seed| Ok(Some(*seed)),
        "creo section component seed scan",
    )?
    else {
        return Ok(None);
    };
    ctx.remove_btree_set(remaining, &seed, "creo section remaining remove")?;
    let mut component = BTreeSet::new();
    ctx.insert_btree_set(&mut component, seed, "creo section component seed")?;
    let mut queue_storage = ctx.reserve_scoped(0, "creo coordinate pending scratch")?;
    let mut pending = std::collections::VecDeque::new();
    queue_storage
        .with_storage(|| ctx.push_back(&mut pending, seed, "creo section pending seed"))?;
    while !pending.is_empty() {
        let Some(variable) = ctx
            .next_charged(&mut pending.iter(), "creo section coordinate graph visits")?
            .copied()
        else {
            break;
        };
        pending.pop_front();
        for &neighbor in ctx.admit_iter(&adjacency[variable], "creo component adjacency links")? {
            if ctx.insert_btree_set(&mut component, neighbor, "creo section component neighbors")? {
                ctx.remove_btree_set(remaining, &neighbor, "creo section remaining remove")?;
                queue_storage.with_storage(|| {
                    ctx.push_back(&mut pending, neighbor, "creo section pending neighbors")
                })?;
            }
        }
    }
    Ok(Some(component))
}

pub(in crate::decode) fn solve_unsigned_dimension_coordinates(
    ctx: &DecodeContext<'_>,
    equations: &[SectionCoordinateEquation],
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
    distances: &[(u32, u32, SectionAxis, f64)],
) -> Result<BTreeMap<SectionCoordinateVariable, f64>, CodecError> {
    const MAX_SIGNED_BRANCHES: usize = 4096;
    let mut scratch = ctx.reserve_scoped(0, "creo solve unsigned dimension coordinates scratch")?;
    if distances.is_empty() {
        return Ok(BTreeMap::new());
    }

    let (variables, indices) =
        scratch.with_storage(|| admitted_coordinate_variables(ctx, equations, distances))?;
    let mut adjacency = scratch.with_storage(|| {
        ctx.collect_indexed_vec(variables.len(), "creo section equation adjacency", |_| {
            Ok(BTreeSet::new())
        })
    })?;
    let mut variable_equations = scratch.with_storage(|| {
        ctx.collect_indexed_vec(variables.len(), "creo unsigned equation membership", |_| {
            Ok(Vec::new())
        })
    })?;
    let mut variable_distances = scratch.with_storage(|| {
        ctx.collect_indexed_vec(variables.len(), "creo unsigned distance membership", |_| {
            Ok(Vec::new())
        })
    })?;
    for (equation_index, equation) in ctx
        .admit_iter(equations, "creo unsigned coordinate equations")?
        .enumerate()
    {
        let mut seed: Option<usize> = None;
        for (variable, _) in ctx.admit_iter(&equation.terms, "creo unsigned equation terms")? {
            let Some(&index) = indices.get(variable) else {
                continue;
            };
            if let Some(first) = seed {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut adjacency[first],
                        index,
                        "creo section equation adjacency links",
                    )
                })?;
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut adjacency[index],
                        first,
                        "creo section equation adjacency links",
                    )
                })?;
            } else {
                seed = Some(index);
            }
        }
        if let Some(seed) = seed {
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut variable_equations[seed],
                    equation_index,
                    "creo unsigned equation links",
                )
            })?;
        }
    }
    for (distance_index, &(first, second, coordinate, _)) in ctx
        .admit_iter(distances, "creo unsigned distance rows")?
        .enumerate()
    {
        let first = indices[&(first, coordinate)];
        let second = indices[&(second, coordinate)];
        if first != second {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut adjacency[first],
                    second,
                    "creo section equation adjacency links",
                )
            })?;
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut adjacency[second],
                    first,
                    "creo section equation adjacency links",
                )
            })?;
        }
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut variable_distances[first],
                distance_index,
                "creo unsigned distance links",
            )
        })?;
    }

    let mut remaining =
        scratch.with_storage(|| section_remaining_variables(ctx, variables.len()))?;
    let mut resolved = BTreeMap::new();
    loop {
        let mut component_storage = ctx.reserve_scoped(0, "creo coordinate component scratch")?;
        let Some(component) = component_storage
            .with_storage(|| next_section_component(ctx, &mut remaining, &adjacency))?
        else {
            break;
        };
        ctx.charge_work(1, "creo unsigned coordinate components")?;
        let mut distance_indices = BTreeSet::new();
        let mut equation_indices = BTreeSet::new();
        for &variable in ctx.admit_iter(&component, "creo unsigned component memberships")? {
            for &index in ctx.admit_iter(
                &variable_distances[variable],
                "creo unsigned member distances",
            )? {
                component_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut distance_indices,
                        index,
                        "creo unsigned component distance indexes",
                    )
                })?;
            }
            for &index in ctx.admit_iter(
                &variable_equations[variable],
                "creo unsigned member equations",
            )? {
                component_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut equation_indices,
                        index,
                        "creo unsigned component equation indexes",
                    )
                })?;
            }
        }
        let mut component_distances = Vec::new();
        for &index in ctx.admit_iter(&distance_indices, "creo component distance rows")? {
            component_storage.with_storage(|| {
                ctx.push_vec(
                    &mut component_distances,
                    distances[index],
                    "creo section component distances",
                )
            })?;
        }
        if component_distances.is_empty()
            || cadmpeg_core::decode::u64_from_index(component_distances.len())
                >= u64::from(usize::BITS)
            || (1usize << component_distances.len()) > MAX_SIGNED_BRANCHES
        {
            continue;
        }
        let mut component_equations = Vec::new();
        for &index in ctx.admit_iter(&equation_indices, "creo component source equations")? {
            component_storage.with_storage(|| {
                ctx.push_vec(
                    &mut component_equations,
                    &equations[index],
                    "creo section component equation rows",
                )
            })?;
        }
        let mut solutions = Vec::new();
        for signs in 0..(1usize << component_distances.len()) {
            let mut branch_storage =
                ctx.reserve_scoped(0, "creo signed coordinate branch scratch")?;
            let mut branched = Vec::new();
            branch_storage.with_storage(|| {
                ctx.reserve_vec(
                    &mut branched,
                    component_equations.len(),
                    "creo section branch equation rows",
                )
            })?;
            for equation in ctx.admit_iter(&component_equations, "creo component equations")? {
                let mut terms = BTreeMap::new();
                for (variable, coefficient) in
                    ctx.admit_iter(&equation.terms, "creo branch source equation terms")?
                {
                    branch_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut terms,
                            *variable,
                            *coefficient,
                            "creo section branch equation terms",
                        )
                    })?;
                }
                branched.push(SectionCoordinateEquation {
                    terms,
                    rhs: equation.rhs,
                });
            }
            for (index, &(first, second, coordinate, magnitude)) in ctx
                .admit_iter(&component_distances, "creo branch distance rows")?
                .enumerate()
            {
                let delta = if signs & (1usize << index) == 0 {
                    magnitude
                } else {
                    -magnitude
                };
                branch_storage.with_storage(|| {
                    ctx.reserve_vec(&mut branched, 1, "creo section signed equation rows")
                })?;
                branched.push(branch_storage.with_storage(|| {
                    SectionCoordinateEquation::point_difference_with_operation(
                        ctx,
                        first,
                        second,
                        coordinate,
                        delta,
                        "creo section signed equation terms",
                    )
                })?);
            }
            let candidate = branch_storage.with_storage(|| {
                solve_section_coordinate_equations(ctx, &branched, stored_coordinates)
            })?;
            let value = |variable: &SectionCoordinateVariable| -> Result<Option<f64>, CodecError> {
                let candidate = ctx
                    .get_btree_map(
                        &candidate,
                        &variable.0,
                        "creo unsigned candidate coordinate lookup",
                    )?
                    .and_then(|point| point[variable.1.index()]);
                if candidate.is_some() {
                    return Ok(candidate);
                }
                Ok(ctx
                    .get_btree_map(
                        stored_coordinates,
                        variable,
                        "creo unsigned stored coordinate lookup",
                    )?
                    .copied())
            };
            let equations_valid = ctx.all_by(
                &component_equations,
                |equation| {
                    let mut terms = equation.terms.iter();
                    let mut lhs = 0.0;
                    while terms.len() != 0 {
                        let Some((variable, coefficient)) =
                            ctx.next_charged(&mut terms, "creo coordinate validation terms")?
                        else {
                            break;
                        };
                        let Some(value) = value(variable)? else {
                            return Ok(true);
                        };
                        lhs += value * coefficient;
                    }
                    let scale = lhs.abs().max(equation.rhs.abs()).max(1.0);
                    Ok(crate::vecmath::within(
                        (lhs - equation.rhs).abs(),
                        EPS_SOLUTION_AGREEMENT * scale,
                    ))
                },
                "creo coordinate validation equations",
            )?;
            let valid = equations_valid
                && ctx.all_by(
                    &component_distances,
                    |&(first, second, coordinate, magnitude)| {
                        let Some(first) = value(&(first, coordinate))? else {
                            return Ok(false);
                        };
                        let Some(second) = value(&(second, coordinate))? else {
                            return Ok(false);
                        };
                        let scale = first.abs().max(second.abs()).max(magnitude).max(1.0);
                        Ok(((second - first).abs() - magnitude).abs()
                            <= EPS_DISTANCE_AGREEMENT * scale)
                    },
                    "creo coordinate distance validations",
                )?;
            if valid {
                let mut candidate_values = BTreeMap::new();
                for (&point, coordinates) in
                    ctx.admit_iter(&candidate, "creo candidate solution points")?
                {
                    for (coordinate, value) in SectionAxis::ALL
                        .into_iter()
                        .zip(coordinates.iter().copied())
                    {
                        let variable = (point, coordinate);
                        if let (Some(global), Some(value)) = (indices.get(&variable), value) {
                            if ctx.contains_btree_set(
                                &component,
                                global,
                                "creo section component contains",
                            )? && !ctx.contains_key_btree_map(
                                stored_coordinates,
                                &variable,
                                "creo section stored coordinates contains_key",
                            )? {
                                component_storage.with_storage(|| {
                                    ctx.insert_btree_map(
                                        &mut candidate_values,
                                        variable,
                                        value,
                                        "creo section candidate values",
                                    )
                                })?;
                            }
                        }
                    }
                }
                component_storage.with_storage(|| {
                    ctx.reserve_vec(&mut solutions, 1, "creo section candidate solutions")
                })?;
                solutions.push(candidate_values);
            }
        }
        for &global in ctx.admit_iter(&component, "creo resolved component variables")? {
            let variable = variables[global];
            let Some(solution) = solutions.first() else {
                continue;
            };
            let Some(value) = ctx
                .get_btree_map(solution, &variable, "creo unsigned first solution lookup")?
                .copied()
            else {
                continue;
            };
            let scale = value.abs().max(1.0);
            if ctx.all_by(
                &solutions,
                |solution| {
                    Ok(ctx
                        .get_btree_map(
                            solution,
                            &variable,
                            "creo unsigned solution agreement lookup",
                        )?
                        .is_some_and(|candidate| {
                            (*candidate - value).abs() <= EPS_DISTANCE_AGREEMENT * scale
                        }))
                },
                "creo coordinate candidate solutions",
            )? {
                ctx.insert_btree_map(
                    &mut resolved,
                    variable,
                    value,
                    "creo section resolved values",
                )?;
            }
        }
    }
    Ok(resolved)
}

pub(super) fn section_equal_length_coordinate_values(
    ctx: &DecodeContext<'_>,
    constraints: &[SectionEqualLengthConstraint],
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Result<BTreeMap<SectionCoordinateVariable, Option<f64>>, CodecError> {
    let mut candidates = BTreeMap::<SectionCoordinateVariable, Option<f64>>::new();
    for constraint in ctx.admit_iter(constraints, "creo equal length coordinate constraints")? {
        let mut missing = None;
        let mut ambiguous = false;
        for variable in constraint
            .first
            .into_iter()
            .chain(constraint.second)
            .flat_map(|point| [(point, SectionAxis::U), (point, SectionAxis::V)])
        {
            if ctx
                .get_btree_map(coordinates, &variable.0, "creo section coordinates get")?
                .and_then(|point| point[variable.1.index()])
                .is_some()
            {
                continue;
            }
            if missing.is_some_and(|earlier| earlier != variable) {
                ambiguous = true;
                break;
            }
            missing = Some(variable);
        }
        let Some(missing) = missing.filter(|_| !ambiguous) else {
            continue;
        };

        let component = |first: u32,
                         second: u32,
                         coordinate: SectionAxis|
         -> Result<Option<(f64, f64)>, CodecError> {
            let value = |point: u32| -> Result<Option<(f64, f64)>, CodecError> {
                if (point, coordinate) == missing {
                    Ok(Some((1.0, 0.0)))
                } else {
                    Ok(ctx
                        .get_btree_map(coordinates, &point, "creo section coordinates get")?
                        .and_then(|coordinates| {
                            coordinates.get(coordinate.index()).copied().flatten()
                        })
                        .map(|value| (0.0, value)))
                }
            };
            let Some((first_coefficient, first_value)) = value(first)? else {
                return Ok(None);
            };
            let Some((second_coefficient, second_value)) = value(second)? else {
                return Ok(None);
            };
            Ok(Some((
                second_coefficient - first_coefficient,
                second_value - first_value,
            )))
        };
        let Some((first_u_coefficient, first_u_value)) =
            component(constraint.first[0], constraint.first[1], SectionAxis::U)?
        else {
            continue;
        };
        let Some((first_v_coefficient, first_v_value)) =
            component(constraint.first[0], constraint.first[1], SectionAxis::V)?
        else {
            continue;
        };
        let Some((second_u_coefficient, second_u_value)) =
            component(constraint.second[0], constraint.second[1], SectionAxis::U)?
        else {
            continue;
        };
        let Some((second_v_coefficient, second_v_value)) =
            component(constraint.second[0], constraint.second[1], SectionAxis::V)?
        else {
            continue;
        };

        let square = |coefficient: f64, value: f64| {
            (
                coefficient * coefficient,
                2.0 * coefficient * value,
                value * value,
            )
        };
        let first_u = square(first_u_coefficient, first_u_value);
        let first_v = square(first_v_coefficient, first_v_value);
        let second_u = square(second_u_coefficient, second_u_value);
        let second_v = square(second_v_coefficient, second_v_value);
        // The squared coefficients are exactly 0.0 or 1.0 and the linear terms
        // are exactly zero except on the one axis that carries the missing
        // coordinate, so the first two sums are exact. The constant sum is the
        // difference of two squared lengths: it cancels against the four
        // squares, so it carries their magnitudes and the root solver reads the
        // discriminant against the error those magnitudes admit. Without that,
        // the residue of the cancellation turns a tangency into two roots or
        // into none.
        let roots = quadratic_roots(
            Coefficient::single(second_u.0 + second_v.0 - first_u.0 - first_v.0),
            Coefficient::single(second_u.1 + second_v.1 - first_u.1 - first_v.1),
            Coefficient::summed(
                second_u.2 + second_v.2 - first_u.2 - first_v.2,
                second_u.2 + second_v.2 + first_u.2 + first_v.2,
            ),
        );
        let [value] = roots.as_slice() else {
            continue;
        };
        ctx.entry_btree_map(
            &mut candidates,
            missing,
            "creo equal-length coordinate candidates",
        )?
        .and_modify(|candidate| {
            if candidate.is_some_and(|candidate| {
                !(FiniteReal::new(candidate))
                    .zip(FiniteReal::new(*value))
                    .is_some_and(|(first, second)| approximately_equal(first, second))
            }) {
                *candidate = None;
            }
        })
        .or_insert(Some(*value));
    }
    Ok(candidates)
}

fn quadratic_roots(
    quadratic: Coefficient,
    linear: Coefficient,
    constant: Coefficient,
) -> crate::decode::quadratic::QuadraticRoots {
    let mut roots = crate::decode::quadratic::real_roots(quadratic, linear, constant);
    let quadratic = quadratic.stated();
    let linear = linear.stated();
    let constant = constant.stated();
    let scale = quadratic.abs().max(linear.abs()).max(constant.abs());
    let quadratic = quadratic / scale;
    let linear = linear / scale;
    let constant = constant / scale;
    roots.retain(|root| {
        root.is_finite()
            && (quadratic * root * root + linear * root + constant).abs()
                <= EPS_SOLUTION_AGREEMENT
                    * (quadratic * root * root)
                        .abs()
                        .max((linear * root).abs())
                        .max(constant.abs())
                        .max(1.0)
    });
    roots.dedup_by(|first, second| {
        (FiniteReal::new(*first))
            .zip(FiniteReal::new(*second))
            .is_some_and(|(first, second)| approximately_equal(first, second))
    });
    roots
}

pub(in crate::decode) fn approximately_equal(first: FiniteReal, second: FiniteReal) -> bool {
    let scale = first.get().abs().max(second.get().abs()).max(1.0);
    (first.get() - second.get()).abs() <= EPS_DISTANCE_AGREEMENT * scale
}

pub(in crate::decode) fn solve_section_coordinate_equations(
    ctx: &DecodeContext<'_>,
    equations: &[SectionCoordinateEquation],
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
) -> Result<BTreeMap<u32, [Option<f64>; 2]>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo solve section coordinate equations scratch")?;
    let (variables, indices) =
        scratch.with_storage(|| admitted_coordinate_variables(ctx, equations, &[]))?;
    let mut adjacency = scratch.with_storage(|| {
        ctx.collect_indexed_vec(variables.len(), "creo section coordinate adjacency", |_| {
            Ok(BTreeSet::new())
        })
    })?;
    let mut variable_equations = scratch.with_storage(|| {
        ctx.collect_indexed_vec(
            variables.len(),
            "creo section coordinate equation membership",
            |_| Ok(BTreeSet::new()),
        )
    })?;
    for (equation_index, equation) in ctx
        .admit_iter(equations, "creo section coordinate source equations")?
        .enumerate()
    {
        let mut seed: Option<usize> = None;
        for (variable, _) in
            ctx.admit_iter(&equation.terms, "creo section coordinate source terms")?
        {
            let Some(&index) = indices.get(variable) else {
                continue;
            };
            if let Some(first) = seed {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut adjacency[first],
                        index,
                        "creo section coordinate adjacency links",
                    )
                })?;
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut adjacency[index],
                        first,
                        "creo section coordinate adjacency links",
                    )
                })?;
            } else {
                seed = Some(index);
            }
        }
        if let Some(seed) = seed {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut variable_equations[seed],
                    equation_index,
                    "creo section coordinate equation links",
                )
            })?;
        }
    }
    let mut solved = BTreeMap::<SectionCoordinateVariable, f64>::new();
    let mut remaining =
        scratch.with_storage(|| section_remaining_variables(ctx, variables.len()))?;
    loop {
        let mut component_storage = ctx.reserve_scoped(0, "creo coordinate component scratch")?;
        let Some(component) = component_storage
            .with_storage(|| next_section_component(ctx, &mut remaining, &adjacency))?
        else {
            break;
        };
        ctx.charge_work(1, "creo section coordinate components")?;
        let mut columns = Vec::new();
        component_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut columns,
                component.len(),
                "creo section component columns",
            )
        })?;
        columns.extend(
            ctx.admit_iter(&component, "creo section coordinate component columns")?
                .copied(),
        );
        let mut local_columns = HashMap::new();
        for (local, global) in ctx
            .admit_iter(&columns, "creo section local coordinate columns")?
            .enumerate()
        {
            component_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut local_columns,
                    *global,
                    local,
                    "creo section local columns",
                )
            })?;
        }
        let mut component_equations = BTreeSet::new();
        for variable in ctx.admit_iter(&component, "creo section component variables")? {
            for &equation_index in ctx.admit_iter(
                &variable_equations[*variable],
                "creo section variable equations",
            )? {
                component_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut component_equations,
                        equation_index,
                        "creo section component equations",
                    )
                })?;
            }
        }
        let mut matrix = Vec::new();
        component_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut matrix,
                component_equations.len(),
                "creo section matrix rows",
            )
        })?;
        for &equation_index in
            ctx.admit_iter(&component_equations, "creo section component equations")?
        {
            let equation = &equations[equation_index];
            let mut row = SectionLinearRow {
                coefficients: BTreeMap::new(),
                rhs: equation.rhs,
            };
            for (variable, coefficient) in
                ctx.admit_iter(&equation.terms, "creo section equation coefficients")?
            {
                let global = indices[variable];
                if *coefficient != 0.0 {
                    component_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut row.coefficients,
                            local_columns[&global],
                            *coefficient,
                            "creo section matrix coefficients",
                        )
                    })?;
                }
            }
            matrix.push(row);
        }
        let Some(component_solution) = component_storage
            .with_storage(|| uniquely_solved_linear_variables(ctx, &mut matrix, columns.len()))?
        else {
            for &global in ctx.admit_iter(&columns, "creo section unresolved component columns")? {
                let variable = variables[global];
                if let Some(value) = ctx.get_btree_map(
                    stored_coordinates,
                    &variable,
                    "creo section stored coordinates get",
                )? {
                    scratch.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut solved,
                            variable,
                            *value,
                            "creo section solved coordinates",
                        )
                    })?;
                }
            }
            continue;
        };
        for &(local, value) in
            ctx.admit_iter(&component_solution, "creo section solved component columns")?
        {
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut solved,
                    variables[columns[local]],
                    value,
                    "creo section solved coordinates",
                )
            })?;
        }
    }
    let mut points = BTreeMap::<u32, [Option<f64>; 2]>::new();
    for (&(point, coordinate), &value) in
        ctx.admit_iter(&solved, "creo section solved coordinates")?
    {
        let values = ctx
            .entry_btree_map(&mut points, point, "creo section solved points")?
            .or_insert([None; 2]);
        values[coordinate.index()] = Some(value);
    }
    Ok(points)
}

struct SectionLinearRow {
    coefficients: BTreeMap<usize, f64>,
    rhs: f64,
}

fn uniquely_solved_linear_variables(
    ctx: &DecodeContext<'_>,
    matrix: &mut [SectionLinearRow],
    variable_count: usize,
) -> Result<Option<Vec<(usize, f64)>>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo uniquely solved linear variables scratch")?;
    let mut coefficient_scale: f64 = 1.0;
    let mut rhs_scale: f64 = 1.0;
    for row in ctx.admit_iter(&*matrix, "creo linear solver matrix scale rows")? {
        rhs_scale = rhs_scale.max(row.rhs.abs());
        for (_, value) in ctx.admit_iter(
            &row.coefficients,
            "creo linear solver matrix scale coefficients",
        )? {
            coefficient_scale = coefficient_scale.max(value.abs());
        }
    }
    let coefficient_tolerance = EPS_SOLVER_SCALE * coefficient_scale;
    let residual_tolerance = EPS_SOLUTION_AGREEMENT * rhs_scale;
    let mut pivot_rows = BTreeMap::new();
    let mut pivot_row = 0;
    let mut columns = 0..variable_count;
    while pivot_row < matrix.len() && !columns.is_empty() {
        let Some(column) = ctx.next_charged(&mut columns, "creo section pivot column scan")? else {
            break;
        };
        let mut selected = pivot_row;
        let mut selected_value = ctx
            .get_btree_map(
                &matrix[selected].coefficients,
                &column,
                "creo section pivot selected coefficient lookup",
            )?
            .copied()
            .unwrap_or(0.0)
            .abs();
        for (offset, row) in ctx
            .admit_iter(
                &matrix[pivot_row + 1..],
                "creo section pivot candidate rows",
            )?
            .enumerate()
        {
            let candidate_value = ctx
                .get_btree_map(
                    &row.coefficients,
                    &column,
                    "creo section pivot candidate coefficient lookup",
                )?
                .copied()
                .unwrap_or(0.0)
                .abs();
            if selected_value.total_cmp(&candidate_value) != std::cmp::Ordering::Greater {
                selected = pivot_row + 1 + offset;
                selected_value = candidate_value;
            }
        }
        let divisor = ctx
            .get_btree_map(
                &matrix[selected].coefficients,
                &column,
                "creo section pivot divisor lookup",
            )?
            .copied()
            .unwrap_or(0.0);
        if divisor.abs() <= coefficient_tolerance {
            continue;
        }
        matrix.swap(pivot_row, selected);
        for (_, value) in ctx.admit_iter(
            &mut matrix[pivot_row].coefficients,
            "creo section pivot normalization terms",
        )? {
            *value /= divisor;
        }
        matrix[pivot_row].rhs /= divisor;
        let (before, pivot_and_after) = matrix.split_at_mut(pivot_row);
        let (pivot, after) = pivot_and_after.split_at_mut(1);
        let pivot = &pivot[0];
        let pivot_rhs = pivot.rhs;
        for target in ctx
            .admit_iter(before, "creo section elimination preceding rows")?
            .chain(ctx.admit_iter(after, "creo section elimination following rows")?)
        {
            let factor = ctx
                .get_btree_map(
                    &target.coefficients,
                    &column,
                    "creo section elimination factor lookup",
                )?
                .copied()
                .unwrap_or(0.0);
            if factor.abs() <= coefficient_tolerance {
                continue;
            }
            for (&index, &pivot_value) in
                ctx.admit_iter(&pivot.coefficients, "creo section pivot coefficients")?
            {
                let value = ctx
                    .entry_btree_map(
                        &mut target.coefficients,
                        index,
                        "creo section elimination coefficients",
                    )?
                    .or_insert(0.0);
                *value -= factor * pivot_value;
                if value.abs() <= coefficient_tolerance {
                    ctx.remove_btree_map(
                        &mut target.coefficients,
                        &index,
                        "creo section elimination zero removal",
                    )?;
                }
            }
            target.rhs -= factor * pivot_rhs;
        }
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut pivot_rows,
                column,
                pivot_row,
                "creo section pivot rows",
            )
        })?;
        pivot_row += 1;
    }
    if ctx.any_by(
        &*matrix,
        |row| Ok(row.coefficients.is_empty() && row.rhs.abs() > residual_tolerance),
        "creo section residual matrix rows",
    )? {
        return Ok(None);
    }
    let mut free_columns = Vec::new();
    for column in ctx.admit_iter(&(0..variable_count), "creo section free column scan")? {
        if !ctx.contains_key_btree_map(
            &pivot_rows,
            &column,
            "creo section pivot rows contains_key",
        )? {
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut free_columns, 1, "creo section free columns")
            })?;
            free_columns.push(column);
        }
    }
    let mut solution = Vec::new();
    for (&column, &row) in ctx.admit_iter(&pivot_rows, "creo section pivot rows")? {
        if ctx.all_by(
            &free_columns,
            |free| {
                Ok(!ctx.contains_key_btree_map(
                    &matrix[row].coefficients,
                    free,
                    "creo section free coefficient lookup",
                )?)
            },
            "creo section free columns",
        )? {
            ctx.reserve_vec(&mut solution, 1, "creo section solved columns")?;
            solution.push((column, matrix[row].rhs));
        }
    }
    Ok(Some(solution))
}

#[cfg(test)]
pub(in crate::decode) struct SectionEquationFixture;

#[cfg(test)]
impl SectionEquationFixture {
    pub(in crate::decode) fn point_value(
        point: u32,
        coordinate: SectionAxis,
        value: f64,
    ) -> SectionCoordinateEquation {
        crate::decode::with_test_decode_ctx(|ctx| {
            SectionCoordinateEquation::point_value(ctx, point, coordinate, value)
        })
        .expect("test coordinate equation")
    }

    pub(in crate::decode) fn point_difference(
        first: u32,
        second: u32,
        coordinate: SectionAxis,
        delta: f64,
    ) -> SectionCoordinateEquation {
        crate::decode::with_test_decode_ctx(|ctx| {
            SectionCoordinateEquation::point_difference(ctx, first, second, coordinate, delta)
        })
        .expect("test coordinate equation")
    }

    pub(in crate::decode) fn add_point(
        equation: &mut SectionCoordinateEquation,
        point: u32,
        coordinate: SectionAxis,
        coefficient: f64,
    ) {
        crate::decode::with_test_decode_ctx(|ctx| {
            equation.add_point(ctx, point, coordinate, coefficient)
        })
        .expect("test coordinate equation term");
    }
}

#[cfg(test)]
mod tests;
