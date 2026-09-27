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
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(SectionScalarVariable, f64)> {
    section_equation_function_six_distance_rows(definition, coordinates, ambiguous_point_ids)
        .into_iter()
        .filter_map(|equation| Some((equation.radius, equation.coordinate_distance()?)))
        .collect()
}

pub(in crate::decode) fn section_equation_function_six_distance_rows(
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionFunctionSixDistance> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::definitions::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    let scalar_equality_values = section_equation_scalar_equality_values(definition);
    let row = |ordinal: Option<u32>| {
        usize::try_from(ordinal?)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    equations
        .rows
        .iter()
        .filter_map(|equation| {
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
        })
        .collect()
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
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionUnsignedCoordinateDistance> {
    section_equation_unsigned_coordinate_distance_rows(definition, ambiguous_point_ids)
        .into_iter()
        .filter(|constraint| constraint.active)
        .collect()
}

pub(in crate::decode) fn section_equation_unsigned_coordinate_distance_rows(
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionUnsignedCoordinateDistance> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|table| feature_dimension_table_complete(table))
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::definitions::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    let scalar_equality_values = section_equation_scalar_equality_values(definition);
    equations
        .rows
        .iter()
        .filter(|equation| equation.function_id == 3 && equation.arguments.len() == 3)
        .filter_map(|equation| {
            let [Some(first), Some(second), Some(dimension)] = equation.arguments.as_slice() else {
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
        })
        .collect()
}

pub(in crate::decode) fn section_equation_radius_dimensions(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Vec<SectionRadiusDimension> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|table| feature_dimension_table_complete(table))
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::definitions::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    let scalar_equality_values = section_equation_scalar_equality_values(definition);
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
        })
        .collect()
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
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(u32, u32, u32)> {
    section_equation_point_on_line_constraint_rows(definition, ambiguous_point_ids)
        .into_iter()
        .filter(|constraint| constraint.active)
        .map(|constraint| (constraint.target, constraint.first, constraint.second))
        .collect()
}

pub(in crate::decode) fn section_equation_point_on_line_constraint_rows(
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionPointOnLineConstraint> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::definitions::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    let scalar_equality_values = section_equation_scalar_equality_values(definition);
    equations
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
        })
        .collect()
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
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionEqualLengthConstraint> {
    section_equation_equal_length_constraint_rows(definition, ambiguous_point_ids)
        .into_iter()
        .filter(|constraint| constraint.active)
        .collect()
}

pub(in crate::decode) fn section_equation_equal_length_constraint_rows(
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionEqualLengthConstraint> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::definitions::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    let scalar_equality_values = section_equation_scalar_equality_values(definition);
    equations
        .rows
        .iter()
        .filter(|equation| equation.function_id == 33 && equation.arguments.len() == 9)
        .filter_map(|equation| {
            let mut rows = Vec::with_capacity(equation.arguments.len());
            for ordinal in &equation.arguments {
                rows.push(variables.rows.get(usize::try_from((*ordinal)?).ok()?)?);
            }
            let [first_u, first_v, second_u, second_v, third_u, third_v, fourth_u, fourth_v, auxiliary] =
                rows.as_slice()
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
        })
        .collect()
}

pub(super) type SectionCoordinateVariable = (u32, SectionAxis);

#[derive(Clone, Default)]
pub(in crate::decode) struct SectionCoordinateEquation {
    pub(super) terms: BTreeMap<SectionCoordinateVariable, f64>,
    pub(in crate::decode) rhs: f64,
}

impl SectionCoordinateEquation {
    pub(in crate::decode) fn point_value(point: u32, coordinate: SectionAxis, value: f64) -> Self {
        let mut equation = Self::default();
        equation.add_point(point, coordinate, 1.0);
        equation.rhs = value;
        equation
    }

    pub(in crate::decode) fn point_difference(
        first: u32,
        second: u32,
        coordinate: SectionAxis,
        delta: f64,
    ) -> Self {
        let mut equation = Self::default();
        equation.add_point(first, coordinate, -1.0);
        equation.add_point(second, coordinate, 1.0);
        equation.rhs = delta;
        equation
    }

    pub(super) fn source_difference(
        first: SectionPointSource,
        second: SectionPointSource,
        coordinate: SectionAxis,
        delta: f64,
    ) -> Self {
        let mut equation = Self::default();
        equation.add_source(first, coordinate, -1.0);
        equation.add_source(second, coordinate, 1.0);
        equation.rhs += delta;
        equation
    }

    pub(in crate::decode) fn add_point(
        &mut self,
        point: u32,
        coordinate: SectionAxis,
        coefficient: f64,
    ) {
        *self.terms.entry((point, coordinate)).or_default() += coefficient;
    }

    pub(super) fn add_source(
        &mut self,
        source: SectionPointSource,
        coordinate: SectionAxis,
        coefficient: f64,
    ) {
        match source {
            SectionPointSource::Point(point) => self.add_point(point, coordinate, coefficient),
            SectionPointSource::Value(value) => self.rhs -= coefficient * value[coordinate.index()],
        }
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
        if !unique.contains(&variable) {
            ctx.charge_collection_items(1, "creo section unique variables")?;
            unique.insert(variable);
        }
    }
    let mut variables = Vec::new();
    ctx.try_reserve_items(
        &mut variables,
        unique.len(),
        "creo section ordered variables",
    )?;
    variables.extend(unique);
    let mut indices = BTreeMap::new();
    for (index, variable) in variables.iter().enumerate() {
        ctx.charge_collection_items(1, "creo section variable indices")?;
        indices.insert(*variable, index);
    }
    Ok((variables, indices))
}

fn section_remaining_variables(
    ctx: &DecodeContext<'_>,
    count: usize,
) -> Result<BTreeSet<usize>, CodecError> {
    let mut remaining = BTreeSet::new();
    for index in 0..count {
        ctx.charge_collection_items(1, "creo section remaining variables")?;
        remaining.insert(index);
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
    ctx.charge_collection_items(1, "creo section component seed")?;
    component.insert(seed);
    let mut pending = std::collections::VecDeque::new();
    ctx.try_collection(1, "creo section pending seed", || pending.try_reserve(1))?;
    pending.push_back(seed);
    while let Some(variable) = pending.pop_front() {
        for &neighbor in &adjacency[variable] {
            if !component.contains(&neighbor) {
                ctx.charge_collection_items(1, "creo section component neighbors")?;
                component.insert(neighbor);
                remaining.remove(&neighbor);
                ctx.try_collection(1, "creo section pending neighbors", || {
                    pending.try_reserve(1)
                })?;
                pending.push_back(neighbor);
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
    match solved.entry(variable) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            ctx.charge_collection_items(1, "creo section solved coordinates")?;
            entry.insert(value);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            entry.insert(value);
        }
    }
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
                    if second != first && !adjacency[first].contains(&second) {
                        ctx.charge_collection_items(1, "creo section equation adjacency links")?;
                        adjacency[first].insert(second);
                    }
                }
            }
            Ok(())
        };
    for equation in equations {
        let mut members = Vec::new();
        for variable in equation.terms.keys() {
            if let Some(&index) = indices.get(variable) {
                ctx.try_reserve_items(&mut members, 1, "creo section equation members")?;
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
                ctx.try_reserve_items(
                    &mut component_distances,
                    1,
                    "creo section component distances",
                )?;
                component_distances.push((first, second, coordinate, magnitude));
            }
        }
        if component_distances.is_empty()
            || component_distances.len() >= usize::BITS as usize
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
                ctx.try_reserve_items(
                    &mut component_equations,
                    1,
                    "creo section component equation rows",
                )?;
                ctx.charge_collection_items(
                    equation.terms.len() as u64,
                    "creo section component equation terms",
                )?;
                component_equations.push(equation.clone());
            }
        }
        let mut solutions = Vec::new();
        for signs in 0..(1usize << component_distances.len()) {
            ctx.charge_work(1, "explore Creo section distance signs")?;
            let mut branched = Vec::new();
            ctx.try_reserve_items(
                &mut branched,
                component_equations.len(),
                "creo section branch equation rows",
            )?;
            for equation in &component_equations {
                ctx.charge_collection_items(
                    equation.terms.len() as u64,
                    "creo section branch equation terms",
                )?;
                branched.push(equation.clone());
            }
            for (index, &(first, second, coordinate, magnitude)) in
                component_distances.iter().enumerate()
            {
                let delta = if signs & (1usize << index) == 0 {
                    magnitude
                } else {
                    -magnitude
                };
                ctx.try_reserve_items(&mut branched, 1, "creo section signed equation rows")?;
                ctx.charge_collection_items(
                    if first == second { 1 } else { 2 },
                    "creo section signed equation terms",
                )?;
                branched.push(SectionCoordinateEquation::point_difference(
                    first, second, coordinate, delta,
                ));
            }
            let candidate = solve_section_coordinate_equations(ctx, &branched, stored_coordinates)?;
            ctx.charge_collection_items(
                stored_coordinates.len() as u64,
                "creo section stored coordinate copies",
            )?;
            let mut values = stored_coordinates.clone();
            for (point, coordinates) in &candidate {
                for (coordinate, value) in SectionAxis::ALL
                    .into_iter()
                    .zip(coordinates.iter().copied())
                {
                    if let Some(value) = value {
                        match values.entry((*point, coordinate)) {
                            std::collections::btree_map::Entry::Vacant(entry) => {
                                ctx.charge_collection_items(1, "creo section branch values")?;
                                entry.insert(value);
                            }
                            std::collections::btree_map::Entry::Occupied(mut entry) => {
                                entry.insert(value);
                            }
                        }
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
                                ctx.charge_collection_items(1, "creo section candidate values")?;
                                candidate_values.insert(variable, value);
                            }
                        }
                    }
                }
                ctx.try_reserve_items(&mut solutions, 1, "creo section candidate solutions")?;
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
                ctx.charge_collection_items(1, "creo section resolved values")?;
                resolved.insert(variable, value);
            }
        }
    }
    Ok(resolved)
}

pub(super) fn section_equal_length_coordinate_values(
    constraints: &[SectionEqualLengthConstraint],
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> BTreeMap<SectionCoordinateVariable, Option<f64>> {
    let mut candidates = BTreeMap::<SectionCoordinateVariable, Option<f64>>::new();
    for constraint in constraints {
        let variables = constraint
            .first
            .into_iter()
            .chain(constraint.second)
            .flat_map(|point| [(point, SectionAxis::U), (point, SectionAxis::V)])
            .collect::<BTreeSet<_>>();
        let missing = variables
            .iter()
            .copied()
            .filter(|variable| {
                coordinates
                    .get(&variable.0)
                    .and_then(|point| point[variable.1.index()])
                    .is_none()
            })
            .collect::<Vec<_>>();
        let [missing] = missing.as_slice() else {
            continue;
        };

        let component = |first: u32, second: u32, coordinate: SectionAxis| -> Option<(f64, f64)> {
            let value = |point: u32| {
                if (point, coordinate) == *missing {
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
        candidates
            .entry(*missing)
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
    candidates
}

fn quadratic_roots(quadratic: Coefficient, linear: Coefficient, constant: Coefficient) -> Vec<f64> {
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
    roots.sort_by(f64::total_cmp);
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
                ctx.try_reserve_items(&mut members, 1, "creo section coordinate members")?;
                members.push(index);
            }
        }
        for &first in &members {
            for &second in &members {
                if second != first && !adjacency[first].contains(&second) {
                    ctx.charge_collection_items(1, "creo section coordinate adjacency links")?;
                    adjacency[first].insert(second);
                }
            }
            if !variable_equations[first].contains(&equation_index) {
                ctx.charge_collection_items(1, "creo section coordinate equation links")?;
                variable_equations[first].insert(equation_index);
            }
        }
    }
    let mut solved = BTreeMap::<SectionCoordinateVariable, f64>::new();
    let mut remaining = section_remaining_variables(ctx, variables.len())?;
    while let Some(component) = next_section_component(ctx, &mut remaining, &adjacency)? {
        let mut columns = Vec::new();
        ctx.try_reserve_items(
            &mut columns,
            component.len(),
            "creo section component columns",
        )?;
        columns.extend(component.iter().copied());
        let mut local_columns = BTreeMap::new();
        for (local, global) in columns.iter().enumerate() {
            ctx.charge_collection_items(1, "creo section local columns")?;
            local_columns.insert(*global, local);
        }
        let mut component_equations = BTreeSet::new();
        for variable in &component {
            for &equation_index in &variable_equations[*variable] {
                if !component_equations.contains(&equation_index) {
                    ctx.charge_collection_items(1, "creo section component equations")?;
                    component_equations.insert(equation_index);
                }
            }
        }
        let mut matrix = Vec::new();
        ctx.try_reserve_items(
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
                    ctx.charge_collection_items(1, "creo section matrix coefficients")?;
                    row.coefficients
                        .insert(local_columns[&global], *coefficient);
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
        let values = match points.entry(point) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo section solved points")?;
                entry.insert([None; 2])
            }
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
                let value = match target.coefficients.entry(index) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        ctx.charge_collection_items(1, "creo section elimination coefficients")?;
                        entry.insert(0.0)
                    }
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                };
                *value -= factor * pivot_value;
                if value.abs() <= coefficient_tolerance {
                    target.coefficients.remove(&index);
                }
            }
            target.rhs -= factor * pivot_rhs;
        }
        ctx.charge_collection_items(1, "creo section pivot rows")?;
        pivot_rows.insert(column, pivot_row);
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
            ctx.try_reserve_items(&mut free_columns, 1, "creo section free columns")?;
            free_columns.push(column);
        }
    }
    let mut solution = Vec::new();
    for (column, row) in pivot_rows {
        if free_columns
            .iter()
            .all(|free| !matrix[row].coefficients.contains_key(free))
        {
            ctx.try_reserve_items(&mut solution, 1, "creo section solved columns")?;
            solution.push((column, matrix[row].rhs));
        }
    }
    Ok(Some(solution))
}

#[cfg(test)]
mod tests {
    use super::super::axis::SectionAxis;
    use super::{
        SectionCoordinateEquation, SectionCoordinateVariable, SectionEqualLengthConstraint,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeMap;

    fn with_collection_limit<T>(limit: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
        run(&ctx)
    }

    fn solve_matrix_with_limit(
        limit: u64,
        coefficients: Vec<BTreeMap<usize, f64>>,
        variable_count: usize,
    ) -> Result<Option<Vec<(usize, f64)>>, cadmpeg_core::CodecError> {
        let mut matrix = coefficients
            .into_iter()
            .map(|coefficients| super::SectionLinearRow {
                coefficients,
                rhs: 1.0,
            })
            .collect::<Vec<_>>();
        with_collection_limit(limit, |ctx| {
            super::uniquely_solved_linear_variables(ctx, &mut matrix, variable_count)
        })
    }

    #[test]
    fn section_elimination_coefficients_refuse_before_tree_insert() {
        let error = solve_matrix_with_limit(
            0,
            vec![
                BTreeMap::from([(0, 2.0), (1, 1.0)]),
                BTreeMap::from([(0, 1.0)]),
            ],
            2,
        )
        .expect_err("elimination adds the missing second coefficient");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section elimination coefficients")
        );
    }

    #[test]
    fn section_pivot_rows_refuse_before_tree_insert() {
        let error = solve_matrix_with_limit(0, vec![BTreeMap::from([(0, 1.0)])], 1)
            .expect_err("the first pivot needs one tree node");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section pivot rows")
        );
    }

    #[test]
    fn section_free_columns_refuse_before_vector_growth() {
        let error = solve_matrix_with_limit(1, vec![BTreeMap::from([(0, 1.0)])], 2)
            .expect_err("the free column follows one admitted pivot");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section free columns")
        );
    }

    #[test]
    fn section_solved_columns_refuse_before_vector_growth() {
        let error = solve_matrix_with_limit(1, vec![BTreeMap::from([(0, 1.0)])], 1)
            .expect_err("the solved column follows one admitted pivot");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved columns")
        );
    }

    #[test]
    fn section_coordinate_unique_variables_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(0, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("one unique variable exceeds zero collection items");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section unique variables")
        );
    }

    #[test]
    fn section_coordinate_ordered_variables_refuse_before_vector_reserve() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(1, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the ordered copy follows one admitted unique variable");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section ordered variables")
        );
    }

    #[test]
    fn section_coordinate_variable_indices_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(2, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the index node follows the unique and ordered copies");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section variable indices")
        );
    }

    #[test]
    fn unsigned_dimension_unique_variables_refuse_before_tree_insert() {
        let error = with_collection_limit(0, |ctx| {
            super::solve_unsigned_dimension_coordinates(
                ctx,
                &[],
                &BTreeMap::new(),
                &[(1, 2, SectionAxis::U, 1.0)],
            )
        })
        .expect_err("the first distance endpoint needs a variable node");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section unique variables")
        );
    }

    #[test]
    fn section_remaining_variables_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(7, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the remaining set follows variable and equation admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section remaining variables")
        );
    }

    #[test]
    fn section_component_seed_refuses_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(8, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the first component needs its own seed node");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component seed")
        );
    }

    #[test]
    fn section_pending_seed_refuses_before_deque_growth() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(9, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the search queue needs one seed slot");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section pending seed")
        );
    }

    #[test]
    fn section_component_neighbors_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_difference(
            1,
            2,
            SectionAxis::U,
            1.0,
        )];
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .is_ok());
        let error = with_collection_limit(20, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the neighbor needs one component node");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component neighbors")
        );
    }

    #[test]
    fn section_pending_neighbors_refuse_before_deque_growth() {
        let equations = [SectionCoordinateEquation::point_difference(
            1,
            2,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(21, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the admitted neighbor needs one queue slot");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section pending neighbors")
        );
    }

    #[test]
    fn unsigned_dimension_remaining_variables_refuse_before_tree_insert() {
        let error = with_collection_limit(10, |ctx| {
            super::solve_unsigned_dimension_coordinates(
                ctx,
                &[],
                &BTreeMap::new(),
                &[(1, 2, SectionAxis::U, 1.0)],
            )
        })
        .expect_err("remaining nodes follow variable and adjacency admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section remaining variables")
        );
    }

    #[test]
    fn unsigned_component_distances_refuse_before_vector_growth() {
        let error = with_collection_limit(16, |ctx| {
            super::solve_unsigned_dimension_coordinates(
                ctx,
                &[],
                &BTreeMap::new(),
                &[(1, 2, SectionAxis::U, 1.0)],
            )
        })
        .expect_err("the first component distance needs one vector item");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component distances")
        );
    }

    #[test]
    fn unsigned_component_equation_rows_refuse_before_vector_growth() {
        let equations = [SectionCoordinateEquation::point_difference(
            1,
            2,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(19, |ctx| {
            super::solve_unsigned_dimension_coordinates(
                ctx,
                &equations,
                &BTreeMap::new(),
                &[(1, 2, SectionAxis::U, 1.0)],
            )
        })
        .expect_err("the first component equation needs an outer slot");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component equation rows"),
            "{error:?}"
        );
    }

    #[test]
    fn unsigned_component_equation_terms_refuse_before_tree_clone() {
        let equations = [SectionCoordinateEquation::point_difference(
            1,
            2,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(21, |ctx| {
            super::solve_unsigned_dimension_coordinates(
                ctx,
                &equations,
                &BTreeMap::new(),
                &[(1, 2, SectionAxis::U, 1.0)],
            )
        })
        .expect_err("two BTreeMap terms need admission before cloning");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component equation terms"),
            "{error:?}"
        );
    }

    fn unsigned_branch_with_collection_limit(limit: u64) -> cadmpeg_core::CodecError {
        let equations = [SectionCoordinateEquation::point_difference(
            1,
            2,
            SectionAxis::U,
            1.0,
        )];
        with_collection_limit(limit, |ctx| {
            super::solve_unsigned_dimension_coordinates(
                ctx,
                &equations,
                &BTreeMap::new(),
                &[(1, 2, SectionAxis::U, 1.0)],
            )
        })
        .expect_err("the selected branch exceeds its collection allowance")
    }

    #[test]
    fn unsigned_branch_equation_rows_refuse_before_vector_reserve() {
        let error = unsigned_branch_with_collection_limit(22);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section branch equation rows")
        );
    }

    #[test]
    fn unsigned_branch_equation_terms_refuse_before_tree_clone() {
        let error = unsigned_branch_with_collection_limit(24);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section branch equation terms")
        );
    }

    #[test]
    fn unsigned_signed_equation_rows_refuse_before_vector_growth() {
        let error = unsigned_branch_with_collection_limit(25);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section signed equation rows")
        );
    }

    #[test]
    fn unsigned_signed_equation_terms_refuse_before_tree_creation() {
        let error = unsigned_branch_with_collection_limit(27);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section signed equation terms")
        );
    }

    #[test]
    fn unsigned_signed_branch_charges_work_before_expansion() {
        let equations = [SectionCoordinateEquation::point_difference(
            1,
            2,
            SectionAxis::U,
            1.0,
        )];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("small input fits the policy");
        let error = super::solve_unsigned_dimension_coordinates(
            &ctx,
            &equations,
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
        .expect_err("the first signed branch needs one work unit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "explore Creo section distance signs")
        );
    }

    fn unsigned_value_fixture(
        ctx: &DecodeContext<'_>,
    ) -> Result<BTreeMap<SectionCoordinateVariable, f64>, cadmpeg_core::CodecError> {
        let equations = [
            SectionCoordinateEquation::point_value(1, SectionAxis::U, 0.0),
            SectionCoordinateEquation::point_value(2, SectionAxis::U, 1.0),
        ];
        let stored = BTreeMap::from([((1, SectionAxis::U), 0.0)]);
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &equations,
            &stored,
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    }

    fn unsigned_value_with_limit(limit: u64) -> cadmpeg_core::CodecError {
        with_collection_limit(limit, unsigned_value_fixture)
            .expect_err("the selected unsigned value boundary exceeds the allowance")
    }

    #[test]
    fn unsigned_value_fixture_preserves_the_unique_distance_solution() {
        let solved = crate::decode::with_test_decode_ctx(unsigned_value_fixture)
            .expect("service profile admits the distance solution");
        assert_eq!(solved, BTreeMap::from([((2, SectionAxis::U), 1.0)]));
    }

    #[test]
    fn unsigned_stored_coordinates_refuse_before_tree_clone() {
        let error = unsigned_value_with_limit(79);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section stored coordinate copies")
        );
    }

    #[test]
    fn unsigned_branch_values_refuse_before_tree_insert() {
        let error = unsigned_value_with_limit(80);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section branch values")
        );
    }

    #[test]
    fn unsigned_candidate_values_refuse_before_tree_insert() {
        let error = unsigned_value_with_limit(81);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section candidate values")
        );
    }

    #[test]
    fn unsigned_candidate_solutions_refuse_before_vector_growth() {
        let error = unsigned_value_with_limit(82);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section candidate solutions")
        );
    }

    #[test]
    fn unsigned_resolved_values_refuse_before_tree_insert() {
        let error = unsigned_value_with_limit(136);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section resolved values")
        );
    }

    #[test]
    fn section_component_columns_refuse_before_vector_reserve() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(10, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the component needs an ordered column vector");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component columns")
        );
    }

    #[test]
    fn section_local_columns_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(11, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the first local index follows one admitted column");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section local columns")
        );
    }

    #[test]
    fn section_component_equations_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(12, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the component equation follows its local column");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component equations")
        );
    }

    #[test]
    fn section_matrix_rows_refuse_before_vector_reserve() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(13, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the first matrix row follows component equation admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section matrix rows")
        );
    }

    #[test]
    fn section_matrix_coefficients_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(14, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the first sparse coefficient follows one matrix row");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section matrix coefficients")
        );
    }

    #[test]
    fn section_solved_coordinates_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(17, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the solved coordinate follows the admitted matrix result");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved coordinates")
        );
    }

    #[test]
    fn section_stored_fallback_refuses_before_solved_node() {
        let error = with_collection_limit(0, |ctx| {
            let mut solved = BTreeMap::new();
            super::insert_solved_coordinate(ctx, &mut solved, (1, SectionAxis::U), 2.0)
        })
        .expect_err("a stored fallback value needs the same solved-map admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved coordinates")
        );
    }

    #[test]
    fn section_solved_points_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(18, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the first point follows its solved coordinate");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved points")
        );
    }

    #[test]
    fn section_coordinate_adjacency_reports_collection_limit() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(3, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("one adjacency row exceeds the collection limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate adjacency")
        );
    }

    #[test]
    fn section_coordinate_equation_membership_reports_collection_limit() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(4, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("membership row exceeds the remaining collection limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate equation membership")
        );
    }

    #[test]
    fn unsigned_dimension_adjacency_reports_collection_limit() {
        let error = with_collection_limit(7, |ctx| {
            super::solve_unsigned_dimension_coordinates(
                ctx,
                &[],
                &BTreeMap::new(),
                &[(1, 2, SectionAxis::U, 1.0)],
            )
        })
        .expect_err("two adjacency rows exceed the collection limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section equation adjacency")
        );
    }

    #[test]
    fn unsigned_dimension_equation_members_refuse_before_vector_growth() {
        let equations = [SectionCoordinateEquation::point_difference(
            1,
            2,
            SectionAxis::U,
            1.0,
        )];
        let distances = [(1, 2, SectionAxis::U, 1.0)];
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            super::solve_unsigned_dimension_coordinates(
                ctx,
                &equations,
                &BTreeMap::new(),
                &distances,
            )
        })
        .is_ok());
        let error = with_collection_limit(8, |ctx| {
            super::solve_unsigned_dimension_coordinates(
                ctx,
                &equations,
                &BTreeMap::new(),
                &distances,
            )
        })
        .expect_err("the first equation member follows two adjacency rows");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section equation members")
        );
    }

    #[test]
    fn unsigned_dimension_adjacency_links_refuse_before_tree_insert() {
        let error = with_collection_limit(8, |ctx| {
            super::solve_unsigned_dimension_coordinates(
                ctx,
                &[],
                &BTreeMap::new(),
                &[(1, 2, SectionAxis::U, 1.0)],
            )
        })
        .expect_err("the first adjacency link follows two admitted rows");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section equation adjacency links")
        );
    }

    #[test]
    fn section_coordinate_members_refuse_before_vector_growth() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .is_ok());
        let error = with_collection_limit(5, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the first equation member follows two outer rows");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate members")
        );
    }

    #[test]
    fn section_coordinate_adjacency_links_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_difference(
            1,
            2,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(12, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the first adjacency link follows outer and member slots");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate adjacency links")
        );
    }

    #[test]
    fn section_coordinate_equation_links_refuse_before_tree_insert() {
        let equations = [SectionCoordinateEquation::point_value(
            1,
            SectionAxis::U,
            1.0,
        )];
        let error = with_collection_limit(6, |ctx| {
            super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
        })
        .expect_err("the first membership link follows two outer rows and one member");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate equation links")
        );
    }

    #[test]
    fn numerical_followup_equal_length_tangency_keeps_its_single_coordinate() {
        // Segment 1-2 spans (0.3, 0.4). Segment 3-4 is vertical and spans 0.5,
        // so the equal-length constraint has the single solution u3 = 0.0. The
        // squared lengths cancel to -2.7755575615628914e-17 instead of zero,
        // which without the cancellation rule splits the double root into
        // -5.268356063861754e-09 and 5.2683560638617535e-09 and states no
        // coordinate.
        let coordinates = BTreeMap::from([
            (1, [Some(0.0), Some(0.0)]),
            (2, [Some(0.3), Some(0.4)]),
            (3, [None, Some(0.0)]),
            (4, [Some(0.0), Some(0.5)]),
        ]);
        let constraints = [SectionEqualLengthConstraint {
            first: [1, 2],
            second: [3, 4],
            equation_id: 7,
            offset: 0,
            active: true,
        }];
        let expected: BTreeMap<SectionCoordinateVariable, Option<f64>> =
            BTreeMap::from([((3, SectionAxis::U), Some(0.0))]);
        assert_eq!(
            super::section_equal_length_coordinate_values(&constraints, &coordinates),
            expected
        );
    }

    /// The single U coordinate an equal-length tangency states.
    ///
    /// Point 1 sits at the origin and point 2 at `segment`. Point 3 carries the
    /// missing U coordinate and sits on the U axis; point 4 sits at
    /// `(partner_u, length)`, where `length` is the length of segment 1-2. The
    /// equal-length constraint 1-2 against 3-4 is then tangent, with the single
    /// solution u3 = `partner_u`.
    fn equal_length_tangency_u(
        segment: [f64; 2],
        partner_u: f64,
        length: f64,
    ) -> BTreeMap<SectionCoordinateVariable, Option<f64>> {
        let coordinates = BTreeMap::from([
            (1, [Some(0.0), Some(0.0)]),
            (2, [Some(segment[0]), Some(segment[1])]),
            (3, [None, Some(0.0)]),
            (4, [Some(partner_u), Some(length)]),
        ]);
        let constraints = [SectionEqualLengthConstraint {
            first: [1, 2],
            second: [3, 4],
            equation_id: 7,
            offset: 0,
            active: true,
        }];
        super::section_equal_length_coordinate_values(&constraints, &coordinates)
    }

    #[test]
    fn numerical_followup_equal_length_tangency_states_its_off_axis_root() {
        // The exact constant of the quadratic is partner_u^2, and the two
        // squared segment lengths cancel on top of it. That residue is far
        // larger than a discriminant band read from the coefficient values
        // alone admits: the first and third witnesses stated no root at all and
        // the second split the double root into 0.000999992616836453 and
        // 0.0010000073831635471, which no longer agree to one coordinate.
        for (segment, partner_u, length) in [
            ([3.3, 4.3], 0.001, 5.420_332_093_147_061),
            ([0.3, 0.4], 0.001, 0.5),
            ([1.1, 2.2], 0.01, 2.459_674_775_249_769),
        ] {
            let expected: BTreeMap<SectionCoordinateVariable, Option<f64>> =
                BTreeMap::from([((3, SectionAxis::U), Some(partner_u))]);
            assert_eq!(
                equal_length_tangency_u(segment, partner_u, length),
                expected
            );
        }
    }
}
