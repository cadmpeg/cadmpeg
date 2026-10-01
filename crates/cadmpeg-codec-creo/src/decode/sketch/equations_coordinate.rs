// SPDX-License-Identifier: Apache-2.0
//! Section-equation coordinate constraints and linear solvers.

use super::axis::SectionAxis;

use crate::feature::definitions::VariableType;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
use std::collections::{BTreeMap, BTreeSet};

use super::super::feature_history::dimensions::feature_dimension_table_complete;
use super::equations_scalar::{
    reconcile_equation_value, section_equation_scalar_equality_values, SectionScalarVariable,
};
use super::skamp::SectionPointSource;
use crate::decode::quadratic::Coefficient;
use crate::decode::sketch_transfer::constraints::section_solver_equation_is_disabled;

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
    ctx.collect_vec(
        section_equation_function_six_distance_rows(
            ctx,
            definition,
            coordinates,
            ambiguous_point_ids,
        )?
        .into_iter()
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
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(Vec::new());
    };
    let Some(equations) = crate::feature::definitions::equation_table(
        ctx,
        &definition.body,
        0,
        definition.body.len(),
    )?
    else {
        return Ok(Vec::new());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(Vec::new());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(Vec::new());
    }
    let scalar_equality_values = section_equation_scalar_equality_values(ctx, definition)?;
    let row = |ordinal: Option<u32>| {
        usize::try_from(ordinal?)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    ctx.collect_vec(
        equations.rows.iter().filter_map(|equation| {
            if equation.function_id != 6 {
                return None;
            }
            let [Some(first_u), Some(first_v), Some(second_u), Some(second_v), Some(radius)] =
                equation.arguments.as_slice()
            else {
                return None;
            };
            let (Some(first_u), Some(first_v), Some(second_u), Some(second_v), Some(radius)) = (
                row(Some(*first_u)),
                row(Some(*first_v)),
                row(Some(*second_u)),
                row(Some(*second_v)),
                row(Some(*radius)),
            ) else {
                return None;
            };
            if first_u.variable_type != VariableType::U
                || first_v.variable_type != VariableType::V
                || first_u.key != first_v.key
                || second_u.variable_type != VariableType::U
                || second_v.variable_type != VariableType::V
                || second_u.key != second_v.key
                || first_u.key == second_u.key
                || radius.variable_type != VariableType::Radius
                || ambiguous_point_ids.contains(&first_u.key)
                || ambiguous_point_ids.contains(&second_u.key)
            {
                return None;
            }
            let radius_equality = scalar_equality_values
                .get(&(radius.variable_type, radius.key))
                .copied()
                .unwrap_or(Ok(None))
                .ok()?;
            let radius_value =
                reconcile_equation_value(radius.value.value(), radius_equality).ok()?;
            let stored_distance = radius_value.and_then(PositiveLength::new);
            if radius_value.is_some() && stored_distance.is_none() {
                return None;
            }
            let active = !section_solver_equation_is_disabled(definition, equation.equation_id);
            let first_point = coordinates
                .get(&first_u.key)
                .and_then(|point| Some([point[0]?, point[1]?]));
            let second_point = coordinates
                .get(&second_u.key)
                .and_then(|point| Some([point[0]?, point[1]?]));
            let points_complete = first_point.is_some() && second_point.is_some();
            let distance = if active {
                match (first_point, second_point) {
                    (Some(first), Some(second)) => {
                        let delta = [second[0] - first[0], second[1] - first[1]];
                        let distance = PositiveLength::new(delta[0].hypot(delta[1]))?;
                        if stored_distance.is_some_and(|stored| {
                            !(FiniteReal::new(stored.get()))
                                .zip(FiniteReal::new(distance.get()))
                                .is_some_and(|(first, second)| approximately_equal(first, second))
                        }) {
                            return None;
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
            Some(SectionFunctionSixDistance {
                first: first_u.key,
                second: second_u.key,
                radius: (radius.variable_type, radius.key),
                distance,
                equation_id: equation.equation_id,
                offset: equation.offset,
            })
        }),
        "creo section equation function six distance rows",
    )
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
    ctx.collect_vec(
        section_equation_unsigned_coordinate_distance_rows(ctx, definition, ambiguous_point_ids)?
            .into_iter()
            .filter(|constraint| constraint.active),
        "creo section unsigned coordinate distances",
    )
}

pub(in crate::decode) fn section_equation_unsigned_coordinate_distance_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionUnsignedCoordinateDistance>, CodecError> {
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
    let Some(equations) = crate::feature::definitions::equation_table(
        ctx,
        &definition.body,
        0,
        definition.body.len(),
    )?
    else {
        return Ok(Vec::new());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(Vec::new());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(Vec::new());
    }
    let scalar_equality_values = section_equation_scalar_equality_values(ctx, definition)?;
    ctx.collect_vec(
        equations
            .rows
            .iter()
            .filter(|equation| equation.function_id == 3 && equation.arguments.len() == 3)
            .filter_map(|equation| {
                let [Some(first), Some(second), Some(dimension)] = equation.arguments.as_slice()
                else {
                    return None;
                };
                let first = variables.rows.get(usize::try_from(*first).ok()?)?;
                let second = variables.rows.get(usize::try_from(*second).ok()?)?;
                let dimension = variables.rows.get(usize::try_from(*dimension).ok()?)?;
                if first.variable_type != second.variable_type
                    || !matches!(first.variable_type, VariableType::U | VariableType::V)
                    || dimension.variable_type != VariableType::Dimension
                    || ambiguous_point_ids.contains(&first.key)
                    || ambiguous_point_ids.contains(&second.key)
                    || first.key == second.key
                {
                    return None;
                }
                let dimension_row = dimensions.rows.get(usize::try_from(dimension.key).ok()?)?;
                if !matches!(dimension_row.dimension_type, 1..=5) {
                    return None;
                }
                let equality_value = scalar_equality_values
                    .get(&(dimension.variable_type, dimension.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?;
                let value = section_equation_dimension_scalar_value(
                    dimension,
                    equality_value,
                    dimension_row.value.resolved()?.abs(),
                    false,
                )?;
                Some(SectionUnsignedCoordinateDistance {
                    first: first.key,
                    second: second.key,
                    coordinate: SectionAxis::from_variable(first.variable_type)?,
                    scalar: (dimension.variable_type, dimension.key),
                    value,
                    equation_id: equation.equation_id,
                    offset: equation.offset,
                    active: !section_solver_equation_is_disabled(definition, equation.equation_id),
                })
            }),
        "creo section equation unsigned coordinate distance rows",
    )
}

pub(in crate::decode) fn section_equation_radius_dimensions(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<SectionRadiusDimension>, CodecError> {
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
    let Some(equations) = crate::feature::definitions::equation_table(
        ctx,
        &definition.body,
        0,
        definition.body.len(),
    )?
    else {
        return Ok(Vec::new());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(Vec::new());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(Vec::new());
    }
    let scalar_equality_values = section_equation_scalar_equality_values(ctx, definition)?;
    ctx.collect_vec(
        equations
            .rows
            .iter()
            .filter(|equation| equation.function_id == 2 && equation.arguments.len() == 2)
            .filter_map(|equation| {
                let [Some(first), Some(second)] = equation.arguments.as_slice() else {
                    return None;
                };
                let first = variables.rows.get(usize::try_from(*first).ok()?)?;
                let second = variables.rows.get(usize::try_from(*second).ok()?)?;
                let (radius, scalar) = match (first.variable_type, second.variable_type) {
                    (VariableType::Radius, VariableType::Dimension) => (first, second),
                    (VariableType::Dimension, VariableType::Radius) => (second, first),
                    _ => return None,
                };
                let dimension = dimensions.rows.get(usize::try_from(scalar.key).ok()?)?;
                let dimension_value = dimension.value.resolved()?;
                let radius_equality = scalar_equality_values
                    .get(&(radius.variable_type, radius.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?;
                let radius_value =
                    reconcile_equation_value(radius.value.value(), radius_equality).ok()?;
                if dimension.dimension_type != 3
                    || radius_value.is_some_and(|value| {
                        !value.is_finite()
                            || value <= 0.0
                            || (value - dimension_value).abs()
                                > EPS_DIMENSION_BINDING
                                    * value.abs().max(dimension_value.abs()).max(1.0)
                    })
                {
                    return None;
                }
                let equality_value = scalar_equality_values
                    .get(&(scalar.variable_type, scalar.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?;
                let value = section_equation_dimension_scalar_value(
                    scalar,
                    equality_value,
                    dimension_value,
                    true,
                )?;
                Some(SectionRadiusDimension {
                    radius_variable: (radius.variable_type, radius.key),
                    radius: radius.key,
                    scalar: (scalar.variable_type, scalar.key),
                    value: PositiveLength::new(value)?,
                    equation_id: equation.equation_id,
                    offset: equation.offset,
                    active: !section_solver_equation_is_disabled(definition, equation.equation_id),
                })
            }),
        "creo section equation radius dimensions",
    )
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
    ctx.collect_vec(
        section_equation_point_on_line_constraint_rows(ctx, definition, ambiguous_point_ids)?
            .into_iter()
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
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(Vec::new());
    };
    let Some(equations) = crate::feature::definitions::equation_table(
        ctx,
        &definition.body,
        0,
        definition.body.len(),
    )?
    else {
        return Ok(Vec::new());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(Vec::new());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(Vec::new());
    }
    let scalar_equality_values = section_equation_scalar_equality_values(ctx, definition)?;
    ctx.collect_vec(equations
        .rows
        .iter()
        .filter(|equation| equation.function_id == 35 && equation.arguments.len() == 9)
        .filter_map(|equation| {
            let [
                Some(target_u),
                Some(target_v),
                Some(first_u),
                Some(first_v),
                Some(second_u),
                Some(second_v),
                Some(line_parameter),
                Some(first_zero),
                Some(second_zero),
            ] = equation.arguments.as_slice()
            else {
                return None;
            };
            let target_u = variables.rows.get(usize::try_from(*target_u).ok()?)?;
            let target_v = variables.rows.get(usize::try_from(*target_v).ok()?)?;
            let first_u = variables.rows.get(usize::try_from(*first_u).ok()?)?;
            let first_v = variables.rows.get(usize::try_from(*first_v).ok()?)?;
            let second_u = variables.rows.get(usize::try_from(*second_u).ok()?)?;
            let second_v = variables.rows.get(usize::try_from(*second_v).ok()?)?;
            let line_parameter = variables
                .rows
                .get(usize::try_from(*line_parameter).ok()?)?;
            let first_zero = variables.rows.get(usize::try_from(*first_zero).ok()?)?;
            let second_zero = variables.rows.get(usize::try_from(*second_zero).ok()?)?;
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
                || ambiguous_point_ids.contains(&target_u.key)
                || ambiguous_point_ids.contains(&first_u.key)
                || ambiguous_point_ids.contains(&second_u.key)
            {
                return None;
            }
            let zero_value = |row: &crate::feature::definitions::FeatureVariableRow| {
                let equality_value = scalar_equality_values
                    .get(&(row.variable_type, row.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?;
                reconcile_equation_value(row.value.value(), equality_value).ok()?
            };
            if zero_value(first_zero) != Some(0.0) || zero_value(second_zero) != Some(0.0) {
                return None;
            }
            Some(SectionPointOnLineConstraint {
                target: target_u.key,
                first: first_u.key,
                second: second_u.key,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !section_solver_equation_is_disabled(definition, equation.equation_id),
            })
        }), "creo section equation point on line constraint rows")
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
    ctx.collect_vec(
        section_equation_equal_length_constraint_rows(ctx, definition, ambiguous_point_ids)?
            .into_iter()
            .filter(|constraint| constraint.active),
        "creo section equal length constraints",
    )
}

pub(in crate::decode) fn section_equation_equal_length_constraint_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionEqualLengthConstraint>, CodecError> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(Vec::new());
    };
    let Some(equations) = crate::feature::definitions::equation_table(
        ctx,
        &definition.body,
        0,
        definition.body.len(),
    )?
    else {
        return Ok(Vec::new());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(Vec::new());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(Vec::new());
    }
    let scalar_equality_values = section_equation_scalar_equality_values(ctx, definition)?;
    ctx.collect_vec(equations
        .rows
        .iter()
        .filter(|equation| equation.function_id == 33 && equation.arguments.len() == 9)
        .filter_map(|equation| {
            let rows: [Option<&crate::feature::definitions::FeatureVariableRow>; 9] =
                std::array::from_fn(|index| {
                    let ordinal = equation.arguments[index]?;
                    variables.rows.get(usize::try_from(ordinal).ok()?)
                });
            let [Some(first_u), Some(first_v), Some(second_u), Some(second_v), Some(third_u), Some(third_v), Some(fourth_u), Some(fourth_v), Some(auxiliary)] = rows
            else {
                return None;
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
                || [first_u.key, second_u.key, third_u.key, fourth_u.key]
                    .into_iter()
                    .any(|point_id| ambiguous_point_ids.contains(&point_id))
            {
                return None;
            }
            let equality_value = scalar_equality_values
                .get(&(auxiliary.variable_type, auxiliary.key))
                .copied()
                .unwrap_or(Ok(None))
                .ok()?;
            let auxiliary_value = reconcile_equation_value(auxiliary.value.value(), equality_value).ok()?;
            if auxiliary_value != Some(0.0) {
                return None;
            }
            Some(SectionEqualLengthConstraint {
                first: [first_u.key, second_u.key],
                second: [third_u.key, fourth_u.key],
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !section_solver_equation_is_disabled(definition, equation.equation_id),
            })
        }), "creo section equation equal length constraint rows")
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
        if first != second {
            ctx.admit_btree_entry(&equation.terms, &(second, coordinate), operation)?;
        }
        *equation.terms.entry((second, coordinate)).or_default() += 1.0;
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
        ctx.admit_btree_entry(
            &self.terms,
            &(point, coordinate),
            "creo coordinate equation term nodes",
        )?;
        *self.terms.entry((point, coordinate)).or_default() += coefficient;
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
    candidates: impl IntoIterator<Item = SectionCoordinateVariable>,
) -> Result<
    (
        Vec<SectionCoordinateVariable>,
        BTreeMap<SectionCoordinateVariable, usize>,
    ),
    CodecError,
> {
    let mut unique = BTreeSet::new();
    for variable in candidates {
        ctx.insert_btree_set(&mut unique, variable, "creo section unique variables")?;
    }
    let mut variables = Vec::new();
    ctx.reserve_vec(
        &mut variables,
        unique.len(),
        "creo section ordered variables",
    )?;
    variables.extend(unique);
    let mut indices = BTreeMap::new();
    for (index, variable) in variables.iter().enumerate() {
        ctx.insert_btree_map(
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
    for index in 0..count {
        ctx.insert_btree_set(&mut remaining, index, "creo section remaining variables")?;
    }
    Ok(remaining)
}

fn next_section_component(
    ctx: &DecodeContext<'_>,
    remaining: &mut BTreeSet<usize>,
    adjacency: &[BTreeSet<usize>],
) -> Result<Option<BTreeSet<usize>>, CodecError> {
    let Some(seed) = remaining.pop_first() else {
        return Ok(None);
    };
    let mut component = BTreeSet::new();
    ctx.insert_btree_set(&mut component, seed, "creo section component seed")?;
    let mut pending = std::collections::VecDeque::new();
    ctx.push_back(&mut pending, seed, "creo section pending seed")?;
    while let Some(variable) = pending.pop_front() {
        for &neighbor in &adjacency[variable] {
            if ctx.insert_btree_set(&mut component, neighbor, "creo section component neighbors")? {
                remaining.remove(&neighbor);
                ctx.push_back(&mut pending, neighbor, "creo section pending neighbors")?;
            }
        }
    }
    Ok(Some(component))
}

fn insert_solved_coordinate(
    ctx: &DecodeContext<'_>,
    solved: &mut BTreeMap<SectionCoordinateVariable, f64>,
    variable: SectionCoordinateVariable,
    value: f64,
) -> Result<(), CodecError> {
    ctx.insert_btree_map(solved, variable, value, "creo section solved coordinates")?;
    Ok(())
}

pub(in crate::decode) fn solve_unsigned_dimension_coordinates(
    ctx: &DecodeContext<'_>,
    equations: &[SectionCoordinateEquation],
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
    distances: &[(u32, u32, SectionAxis, f64)],
) -> Result<BTreeMap<SectionCoordinateVariable, f64>, CodecError> {
    const MAX_SIGNED_BRANCHES: usize = 4096;
    if distances.is_empty() {
        return Ok(BTreeMap::new());
    }

    let (variables, indices) = admitted_coordinate_variables(
        ctx,
        equations
            .iter()
            .flat_map(|equation| equation.terms.keys().copied())
            .chain(
                distances
                    .iter()
                    .flat_map(|&(first, second, coordinate, _)| {
                        [(first, coordinate), (second, coordinate)]
                    }),
            ),
    )?;
    let mut adjacency = ctx.alloc_filled(
        variables.len(),
        BTreeSet::new(),
        "creo section equation adjacency",
    )?;
    let connect =
        |members: &[usize], adjacency: &mut [BTreeSet<usize>]| -> Result<(), CodecError> {
            for &first in members {
                for &second in members {
                    if second != first {
                        ctx.insert_btree_set(
                            &mut adjacency[first],
                            second,
                            "creo section equation adjacency links",
                        )?;
                    }
                }
            }
            Ok(())
        };
    for equation in equations {
        let mut members = Vec::new();
        for variable in equation.terms.keys() {
            if let Some(&index) = indices.get(variable) {
                ctx.reserve_vec(&mut members, 1, "creo section equation members")?;
                members.push(index);
            }
        }
        connect(&members, &mut adjacency)?;
    }
    for &(first, second, coordinate, _) in distances {
        let members = [
            indices[&(first, coordinate)],
            indices[&(second, coordinate)],
        ];
        connect(&members, &mut adjacency)?;
    }

    let mut remaining = section_remaining_variables(ctx, variables.len())?;
    let mut resolved = BTreeMap::new();
    while let Some(component) = next_section_component(ctx, &mut remaining, &adjacency)? {
        let mut component_distances = Vec::new();
        for &(first, second, coordinate, magnitude) in distances {
            if component.contains(&indices[&(first, coordinate)])
                && component.contains(&indices[&(second, coordinate)])
            {
                ctx.reserve_vec(
                    &mut component_distances,
                    1,
                    "creo section component distances",
                )?;
                component_distances.push((first, second, coordinate, magnitude));
            }
        }
        if component_distances.is_empty()
            || cadmpeg_core::decode::u64_from_index(component_distances.len())
                >= u64::from(usize::BITS)
            || (1usize << component_distances.len()) > MAX_SIGNED_BRANCHES
        {
            continue;
        }
        let mut component_equations = Vec::new();
        for equation in equations {
            if equation
                .terms
                .keys()
                .any(|variable| component.contains(&indices[variable]))
            {
                ctx.reserve_vec(
                    &mut component_equations,
                    1,
                    "creo section component equation rows",
                )?;
                let mut terms = BTreeMap::new();
                for (variable, coefficient) in &equation.terms {
                    ctx.insert_btree_map(
                        &mut terms,
                        *variable,
                        *coefficient,
                        "creo section component equation terms",
                    )?;
                }
                component_equations.push(SectionCoordinateEquation {
                    terms,
                    rhs: equation.rhs,
                });
            }
        }
        let mut solutions = Vec::new();
        for signs in 0..(1usize << component_distances.len()) {
            ctx.charge_work(1, "explore Creo section distance signs")?;
            let mut branched = Vec::new();
            ctx.reserve_vec(
                &mut branched,
                component_equations.len(),
                "creo section branch equation rows",
            )?;
            for equation in &component_equations {
                let mut terms = BTreeMap::new();
                for (variable, coefficient) in &equation.terms {
                    ctx.insert_btree_map(
                        &mut terms,
                        *variable,
                        *coefficient,
                        "creo section branch equation terms",
                    )?;
                }
                branched.push(SectionCoordinateEquation {
                    terms,
                    rhs: equation.rhs,
                });
            }
            for (index, &(first, second, coordinate, magnitude)) in
                component_distances.iter().enumerate()
            {
                let delta = if signs & (1usize << index) == 0 {
                    magnitude
                } else {
                    -magnitude
                };
                ctx.reserve_vec(&mut branched, 1, "creo section signed equation rows")?;
                branched.push(SectionCoordinateEquation::point_difference_with_operation(
                    ctx,
                    first,
                    second,
                    coordinate,
                    delta,
                    "creo section signed equation terms",
                )?);
            }
            let candidate = solve_section_coordinate_equations(ctx, &branched, stored_coordinates)?;
            let mut values = BTreeMap::new();
            for (variable, value) in stored_coordinates {
                ctx.insert_btree_map(
                    &mut values,
                    *variable,
                    *value,
                    "creo section stored coordinate copies",
                )?;
            }
            for (point, coordinates) in &candidate {
                for (coordinate, value) in SectionAxis::ALL
                    .into_iter()
                    .zip(coordinates.iter().copied())
                {
                    if let Some(value) = value {
                        ctx.insert_btree_map(
                            &mut values,
                            (*point, coordinate),
                            value,
                            "creo section branch values",
                        )?;
                    }
                }
            }
            let valid = component_equations.iter().all(|equation| {
                let Some(lhs) = equation
                    .terms
                    .iter()
                    .try_fold(0.0, |lhs, (variable, coefficient)| {
                        Some(lhs + values.get(variable)? * coefficient)
                    })
                else {
                    return true;
                };
                let scale = lhs.abs().max(equation.rhs.abs()).max(1.0);
                (lhs - equation.rhs).abs() <= EPS_SOLUTION_AGREEMENT * scale
            }) && component_distances.iter().all(
                |&(first, second, coordinate, magnitude)| {
                    let Some(first) = values.get(&(first, coordinate)).copied() else {
                        return false;
                    };
                    let Some(second) = values.get(&(second, coordinate)).copied() else {
                        return false;
                    };
                    let scale = first.abs().max(second.abs()).max(magnitude).max(1.0);
                    ((second - first).abs() - magnitude).abs() <= EPS_DISTANCE_AGREEMENT * scale
                },
            );
            if valid {
                let mut candidate_values = BTreeMap::new();
                for (point, coordinates) in candidate {
                    for (coordinate, value) in SectionAxis::ALL.into_iter().zip(coordinates) {
                        let variable = (point, coordinate);
                        if let (Some(global), Some(value)) = (indices.get(&variable), value) {
                            if component.contains(global)
                                && !stored_coordinates.contains_key(&variable)
                            {
                                ctx.insert_btree_map(
                                    &mut candidate_values,
                                    variable,
                                    value,
                                    "creo section candidate values",
                                )?;
                            }
                        }
                    }
                }
                ctx.reserve_vec(&mut solutions, 1, "creo section candidate solutions")?;
                solutions.push(candidate_values);
            }
        }
        for &global in &component {
            let variable = variables[global];
            let Some(value) = solutions
                .first()
                .and_then(|solution| solution.get(&variable))
                .copied()
            else {
                continue;
            };
            let scale = value.abs().max(1.0);
            if solutions.iter().all(|solution| {
                solution.get(&variable).is_some_and(|candidate| {
                    (*candidate - value).abs() <= EPS_DISTANCE_AGREEMENT * scale
                })
            }) {
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
    for constraint in constraints {
        let mut missing = None;
        let mut ambiguous = false;
        for variable in constraint
            .first
            .into_iter()
            .chain(constraint.second)
            .flat_map(|point| [(point, SectionAxis::U), (point, SectionAxis::V)])
        {
            if coordinates
                .get(&variable.0)
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

        let component = |first: u32, second: u32, coordinate: SectionAxis| -> Option<(f64, f64)> {
            let value = |point: u32| {
                if (point, coordinate) == missing {
                    Some((1.0, 0.0))
                } else {
                    coordinates
                        .get(&point)
                        .and_then(|coordinates| {
                            coordinates.get(coordinate.index()).copied().flatten()
                        })
                        .map(|value| (0.0, value))
                }
            };
            let (first_coefficient, first_value) = value(first)?;
            let (second_coefficient, second_value) = value(second)?;
            Some((
                second_coefficient - first_coefficient,
                second_value - first_value,
            ))
        };
        let Some((first_u_coefficient, first_u_value)) =
            component(constraint.first[0], constraint.first[1], SectionAxis::U)
        else {
            continue;
        };
        let Some((first_v_coefficient, first_v_value)) =
            component(constraint.first[0], constraint.first[1], SectionAxis::V)
        else {
            continue;
        };
        let Some((second_u_coefficient, second_u_value)) =
            component(constraint.second[0], constraint.second[1], SectionAxis::U)
        else {
            continue;
        };
        let Some((second_v_coefficient, second_v_value)) =
            component(constraint.second[0], constraint.second[1], SectionAxis::V)
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
            ctx,
            Coefficient::single(second_u.0 + second_v.0 - first_u.0 - first_v.0),
            Coefficient::single(second_u.1 + second_v.1 - first_u.1 - first_v.1),
            Coefficient::summed(
                second_u.2 + second_v.2 - first_u.2 - first_v.2,
                second_u.2 + second_v.2 + first_u.2 + first_v.2,
            ),
        )?;
        let [value] = roots.as_slice() else {
            continue;
        };
        ctx.admit_btree_entry(
            &candidates,
            &missing,
            "creo equal-length coordinate candidates",
        )?;
        candidates
            .entry(missing)
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
    ctx: &DecodeContext<'_>,
    quadratic: Coefficient,
    linear: Coefficient,
    constant: Coefficient,
) -> Result<crate::decode::quadratic::QuadraticRoots, CodecError> {
    let mut roots = crate::decode::quadratic::real_roots(ctx, quadratic, linear, constant)?;
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
    ctx.stable_sort_by(
        &mut roots,
        f64::total_cmp,
        |_| 0,
        "creo sketch coordinate quadratic roots sort",
    )?;
    roots.dedup_by(|first, second| {
        (FiniteReal::new(*first))
            .zip(FiniteReal::new(*second))
            .is_some_and(|(first, second)| approximately_equal(first, second))
    });
    Ok(roots)
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
    let (variables, indices) = admitted_coordinate_variables(
        ctx,
        equations
            .iter()
            .flat_map(|equation| equation.terms.keys().copied()),
    )?;
    let mut adjacency = ctx.alloc_filled(
        variables.len(),
        BTreeSet::new(),
        "creo section coordinate adjacency",
    )?;
    let mut variable_equations = ctx.alloc_filled(
        variables.len(),
        BTreeSet::new(),
        "creo section coordinate equation membership",
    )?;
    for (equation_index, equation) in equations.iter().enumerate() {
        let mut members = Vec::new();
        for variable in equation.terms.keys() {
            if let Some(&index) = indices.get(variable) {
                ctx.reserve_vec(&mut members, 1, "creo section coordinate members")?;
                members.push(index);
            }
        }
        for &first in &members {
            for &second in &members {
                if second != first {
                    ctx.insert_btree_set(
                        &mut adjacency[first],
                        second,
                        "creo section coordinate adjacency links",
                    )?;
                }
            }
            ctx.insert_btree_set(
                &mut variable_equations[first],
                equation_index,
                "creo section coordinate equation links",
            )?;
        }
    }
    let mut solved = BTreeMap::<SectionCoordinateVariable, f64>::new();
    let mut remaining = section_remaining_variables(ctx, variables.len())?;
    while let Some(component) = next_section_component(ctx, &mut remaining, &adjacency)? {
        let mut columns = Vec::new();
        ctx.reserve_vec(
            &mut columns,
            component.len(),
            "creo section component columns",
        )?;
        columns.extend(component.iter().copied());
        let mut local_columns = BTreeMap::new();
        for (local, global) in columns.iter().enumerate() {
            ctx.insert_btree_map(
                &mut local_columns,
                *global,
                local,
                "creo section local columns",
            )?;
        }
        let mut component_equations = BTreeSet::new();
        for variable in &component {
            for &equation_index in &variable_equations[*variable] {
                ctx.insert_btree_set(
                    &mut component_equations,
                    equation_index,
                    "creo section component equations",
                )?;
            }
        }
        let mut matrix = Vec::new();
        ctx.reserve_vec(
            &mut matrix,
            component_equations.len(),
            "creo section matrix rows",
        )?;
        for equation_index in component_equations {
            let equation = &equations[equation_index];
            let mut row = SectionLinearRow {
                coefficients: BTreeMap::new(),
                rhs: equation.rhs,
            };
            for (variable, coefficient) in &equation.terms {
                let global = indices[variable];
                if *coefficient != 0.0 {
                    ctx.insert_btree_map(
                        &mut row.coefficients,
                        local_columns[&global],
                        *coefficient,
                        "creo section matrix coefficients",
                    )?;
                }
            }
            matrix.push(row);
        }
        let Some(component_solution) =
            uniquely_solved_linear_variables(ctx, &mut matrix, columns.len())?
        else {
            for global in columns {
                let variable = variables[global];
                if let Some(value) = stored_coordinates.get(&variable) {
                    insert_solved_coordinate(ctx, &mut solved, variable, *value)?;
                }
            }
            continue;
        };
        for (local, value) in component_solution {
            insert_solved_coordinate(ctx, &mut solved, variables[columns[local]], value)?;
        }
    }
    let mut points = BTreeMap::<u32, [Option<f64>; 2]>::new();
    for ((point, coordinate), value) in solved {
        ctx.admit_btree_entry(&points, &point, "creo section solved points")?;
        let values = match points.entry(point) {
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert([None; 2]),
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
        };
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
    let coefficient_scale = matrix
        .iter()
        .flat_map(|row| row.coefficients.values())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let rhs_scale = matrix.iter().map(|row| row.rhs.abs()).fold(1.0, f64::max);
    let coefficient_tolerance = EPS_SOLVER_SCALE * coefficient_scale;
    let residual_tolerance = EPS_SOLUTION_AGREEMENT * rhs_scale;
    let mut pivot_rows = BTreeMap::new();
    let mut pivot_row = 0;
    for column in 0..variable_count {
        let Some(selected) = (pivot_row..matrix.len()).max_by(|&first, &second| {
            matrix[first]
                .coefficients
                .get(&column)
                .copied()
                .unwrap_or(0.0)
                .abs()
                .total_cmp(
                    &matrix[second]
                        .coefficients
                        .get(&column)
                        .copied()
                        .unwrap_or(0.0)
                        .abs(),
                )
        }) else {
            break;
        };
        let divisor = matrix[selected]
            .coefficients
            .get(&column)
            .copied()
            .unwrap_or(0.0);
        if divisor.abs() <= coefficient_tolerance {
            continue;
        }
        matrix.swap(pivot_row, selected);
        for value in matrix[pivot_row].coefficients.values_mut() {
            *value /= divisor;
        }
        matrix[pivot_row].rhs /= divisor;
        let (before, pivot_and_after) = matrix.split_at_mut(pivot_row);
        let Some((pivot, after)) = pivot_and_after.split_first_mut() else {
            return Ok(None);
        };
        let pivot_rhs = pivot.rhs;
        for target in before.iter_mut().chain(after.iter_mut()) {
            let factor = target.coefficients.get(&column).copied().unwrap_or(0.0);
            if factor.abs() <= coefficient_tolerance {
                continue;
            }
            for (&index, &pivot_value) in &pivot.coefficients {
                ctx.admit_btree_entry(
                    &target.coefficients,
                    &index,
                    "creo section elimination coefficients",
                )?;
                let value = match target.coefficients.entry(index) {
                    std::collections::btree_map::Entry::Vacant(entry) => entry.insert(0.0),
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                };
                *value -= factor * pivot_value;
                if value.abs() <= coefficient_tolerance {
                    target.coefficients.remove(&index);
                }
            }
            target.rhs -= factor * pivot_rhs;
        }
        ctx.insert_btree_map(
            &mut pivot_rows,
            column,
            pivot_row,
            "creo section pivot rows",
        )?;
        pivot_row += 1;
    }
    if matrix
        .iter()
        .any(|row| row.coefficients.is_empty() && row.rhs.abs() > residual_tolerance)
    {
        return Ok(None);
    }
    let mut free_columns = Vec::new();
    for column in 0..variable_count {
        if !pivot_rows.contains_key(&column) {
            ctx.reserve_vec(&mut free_columns, 1, "creo section free columns")?;
            free_columns.push(column);
        }
    }
    let mut solution = Vec::new();
    for (column, row) in pivot_rows {
        if free_columns
            .iter()
            .all(|free| !matrix[row].coefficients.contains_key(free))
        {
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
