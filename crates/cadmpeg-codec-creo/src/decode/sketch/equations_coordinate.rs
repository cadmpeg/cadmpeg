// SPDX-License-Identifier: Apache-2.0
//! Section-equation coordinate constraints and linear solvers.

use cadmpeg_core::decode::alloc_filled;
use std::collections::{BTreeMap, BTreeSet};

use super::super::feature_history::feature_dimension_table_complete;
use super::super::sketch_transfer::section_solver_equation_is_disabled;
use super::equations_scalar::SectionScalarVariable;
use super::skamp::SectionPointSource;

const EPS_DISTANCE_AGREEMENT: f64 = 1.0e-9;
const EPS_SOLVER_SCALE: f64 = 1.0e-12;
const EPS_DISCRIMINANT_SCALE: f64 = 1.0e-12;
const EPS_SOLUTION_AGREEMENT: f64 = 1.0e-9;

pub(crate) fn section_equation_function_six_distance_values(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(SectionScalarVariable, f64)> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    let row = |ordinal: Option<u32>| {
        usize::try_from(ordinal?)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
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
            if first_u.variable_type != 1
                || first_v.variable_type != 2
                || first_u.key != first_v.key
                || second_u.variable_type != 1
                || second_v.variable_type != 2
                || second_u.key != second_v.key
                || first_u.key == second_u.key
                || radius.variable_type != 3
                || ambiguous_point_ids.contains(&first_u.key)
                || ambiguous_point_ids.contains(&second_u.key)
            {
                return None;
            }
            let first = coordinates
                .get(&first_u.key)
                .and_then(|point| Some([point[0]?, point[1]?]))?;
            let second = coordinates
                .get(&second_u.key)
                .and_then(|point| Some([point[0]?, point[1]?]))?;
            let delta = [second[0] - first[0], second[1] - first[1]];
            let distance = delta[0].hypot(delta[1]);
            if !distance.is_finite() || distance <= 0.0 {
                return None;
            }
            if radius.value.is_some_and(|stored| {
                !stored.is_finite()
                    || stored <= 0.0
                    || (stored - distance).abs()
                        > EPS_DISTANCE_AGREEMENT * stored.abs().max(distance).max(1.0)
            }) {
                return None;
            }
            Some(((radius.variable_type, radius.key), distance))
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct SectionUnsignedCoordinateDistance {
    pub(crate) first: u32,
    pub(crate) second: u32,
    pub(crate) coordinate: usize,
    pub(crate) scalar: SectionScalarVariable,
    pub(crate) value: f64,
}

#[derive(Clone, Copy)]
pub(crate) struct SectionRadiusDimension {
    pub(crate) radius: u32,
    pub(crate) scalar: SectionScalarVariable,
    pub(crate) value: f64,
}

pub(crate) fn section_equation_dimension_scalar_value(
    scalar: &crate::feature::FeatureVariableRow,
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
    match scalar.value {
        Some(value) if valid(value) && approximately_equal(value, dimension_value) => {
            Some(dimension_value)
        }
        None if scalar.dimension_driven => Some(dimension_value),
        _ => None,
    }
}

pub(crate) fn section_equation_unsigned_coordinate_distances(
    definition: &crate::feature::FeatureDefinition,
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
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter(|equation| equation.function_id == 3 && equation.arguments.len() == 3)
        .filter_map(|equation| {
            let [Some(first), Some(second), Some(dimension)] = equation.arguments.as_slice() else {
                return None;
            };
            let first = variables.rows.get(usize::try_from(*first).ok()?)?;
            let second = variables.rows.get(usize::try_from(*second).ok()?)?;
            let dimension = variables.rows.get(usize::try_from(*dimension).ok()?)?;
            if first.variable_type != second.variable_type
                || !matches!(first.variable_type, 1 | 2)
                || dimension.variable_type != 0
                || ambiguous_point_ids.contains(&first.key)
                || ambiguous_point_ids.contains(&second.key)
                || first.key == second.key
            {
                return None;
            }
            let dimension_row = dimensions.rows.get(usize::try_from(dimension.key).ok()?)?;
            if dimension_row.value_unit != crate::feature::DimensionUnit::Millimeters
                || !matches!(dimension_row.dimension_type, 1..=5)
            {
                return None;
            }
            let value =
                section_equation_dimension_scalar_value(dimension, dimension_row.value?, false)?;
            Some(SectionUnsignedCoordinateDistance {
                first: first.key,
                second: second.key,
                coordinate: usize::from(first.variable_type == 2),
                scalar: (dimension.variable_type, dimension.key),
                value,
            })
        })
        .collect()
}

pub(crate) fn section_equation_radius_dimensions(
    definition: &crate::feature::FeatureDefinition,
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
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
        .filter(|equation| equation.function_id == 2 && equation.arguments.len() == 2)
        .filter_map(|equation| {
            let [Some(first), Some(second)] = equation.arguments.as_slice() else {
                return None;
            };
            let first = variables.rows.get(usize::try_from(*first).ok()?)?;
            let second = variables.rows.get(usize::try_from(*second).ok()?)?;
            let (radius, scalar) = match (first.variable_type, second.variable_type) {
                (3, 0) => (first, second),
                (0, 3) => (second, first),
                _ => return None,
            };
            let dimension = dimensions.rows.get(usize::try_from(scalar.key).ok()?)?;
            let dimension_value = dimension.value?;
            if dimension.dimension_type != 3
                || dimension.value_unit != crate::feature::DimensionUnit::Millimeters
                || radius.value.is_some_and(|value| {
                    !value.is_finite()
                        || value <= 0.0
                        || (value - dimension_value).abs()
                            > EPS_DISTANCE_AGREEMENT
                                * value.abs().max(dimension_value.abs()).max(1.0)
                })
            {
                return None;
            }
            let value = section_equation_dimension_scalar_value(scalar, dimension_value, true)?;
            Some(SectionRadiusDimension {
                radius: radius.key,
                scalar: (scalar.variable_type, scalar.key),
                value,
            })
        })
        .collect()
}

pub(crate) fn section_equation_point_on_line_constraints(
    definition: &crate::feature::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(u32, u32, u32)> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
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
            if target_u.variable_type != 1
                || target_v.variable_type != 2
                || first_u.variable_type != 1
                || first_v.variable_type != 2
                || second_u.variable_type != 1
                || second_v.variable_type != 2
                || target_u.key != target_v.key
                || first_u.key != first_v.key
                || second_u.key != second_v.key
                || target_u.key == first_u.key
                || target_u.key == second_u.key
                || first_u.key == second_u.key
                || line_parameter.variable_type != 4
                || first_zero.variable_type != 5
                || second_zero.variable_type != 5
                || first_zero.value != Some(0.0)
                || second_zero.value != Some(0.0)
                || ambiguous_point_ids.contains(&target_u.key)
                || ambiguous_point_ids.contains(&first_u.key)
                || ambiguous_point_ids.contains(&second_u.key)
            {
                return None;
            }
            Some((target_u.key, first_u.key, second_u.key))
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct SectionEqualLengthConstraint {
    pub(crate) first: [u32; 2],
    pub(crate) second: [u32; 2],
}

pub(crate) fn section_equation_equal_length_constraints(
    definition: &crate::feature::FeatureDefinition,
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
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Vec::new();
    };
    if declared_count != equations.rows.len() + 1 {
        return Vec::new();
    }
    equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
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
            if first_u.variable_type != 1
                || first_v.variable_type != 2
                || second_u.variable_type != 1
                || second_v.variable_type != 2
                || third_u.variable_type != 1
                || third_v.variable_type != 2
                || fourth_u.variable_type != 1
                || fourth_v.variable_type != 2
                || auxiliary.variable_type != 7
                || auxiliary.value != Some(0.0)
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
            Some(SectionEqualLengthConstraint {
                first: [first_u.key, second_u.key],
                second: [third_u.key, fourth_u.key],
            })
        })
        .collect()
}

pub(crate) type SectionCoordinateVariable = (u32, usize);

#[derive(Clone, Default)]
pub(crate) struct SectionCoordinateEquation {
    pub(crate) terms: BTreeMap<SectionCoordinateVariable, f64>,
    pub(crate) rhs: f64,
}

impl SectionCoordinateEquation {
    pub(crate) fn point_value(point: u32, coordinate: usize, value: f64) -> Self {
        let mut equation = Self::default();
        equation.add_point(point, coordinate, 1.0);
        equation.rhs = value;
        equation
    }

    pub(crate) fn point_difference(first: u32, second: u32, coordinate: usize, delta: f64) -> Self {
        let mut equation = Self::default();
        equation.add_point(first, coordinate, -1.0);
        equation.add_point(second, coordinate, 1.0);
        equation.rhs = delta;
        equation
    }

    pub(crate) fn source_difference(
        first: SectionPointSource,
        second: SectionPointSource,
        coordinate: usize,
        delta: f64,
    ) -> Self {
        let mut equation = Self::default();
        equation.add_source(first, coordinate, -1.0);
        equation.add_source(second, coordinate, 1.0);
        equation.rhs += delta;
        equation
    }

    pub(crate) fn add_point(&mut self, point: u32, coordinate: usize, coefficient: f64) {
        *self.terms.entry((point, coordinate)).or_default() += coefficient;
    }

    pub(crate) fn add_source(
        &mut self,
        source: SectionPointSource,
        coordinate: usize,
        coefficient: f64,
    ) {
        match source {
            SectionPointSource::Point(point) => self.add_point(point, coordinate, coefficient),
            SectionPointSource::Value(value) => self.rhs -= coefficient * value[coordinate],
        }
    }
}

pub(crate) fn solve_unsigned_dimension_coordinates(
    equations: &[SectionCoordinateEquation],
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
    distances: &[(u32, u32, usize, f64)],
) -> BTreeMap<SectionCoordinateVariable, f64> {
    const MAX_SIGNED_BRANCHES: usize = 4096;
    if distances.is_empty() {
        return BTreeMap::new();
    }

    let variables = equations
        .iter()
        .flat_map(|equation| equation.terms.keys().copied())
        .chain(
            distances
                .iter()
                .flat_map(|&(first, second, coordinate, _)| {
                    [(first, coordinate), (second, coordinate)]
                }),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let indices = variables
        .iter()
        .enumerate()
        .map(|(index, variable)| (*variable, index))
        .collect::<BTreeMap<_, _>>();
    let Ok(mut adjacency) = alloc_filled(
        variables.len(),
        BTreeSet::new(),
        "creo section equation adjacency",
    ) else {
        return BTreeMap::new();
    };
    let connect = |members: Vec<usize>, adjacency: &mut [BTreeSet<usize>]| {
        for &first in &members {
            adjacency[first].extend(members.iter().copied().filter(|second| *second != first));
        }
    };
    for equation in equations {
        connect(
            equation
                .terms
                .keys()
                .filter_map(|variable| indices.get(variable).copied())
                .collect(),
            &mut adjacency,
        );
    }
    for &(first, second, coordinate, _) in distances {
        connect(
            [
                indices[&(first, coordinate)],
                indices[&(second, coordinate)],
            ]
            .into_iter()
            .collect(),
            &mut adjacency,
        );
    }

    let mut remaining = (0..variables.len()).collect::<BTreeSet<_>>();
    let mut resolved = BTreeMap::new();
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(variable) = pending.pop_front() {
            for &neighbor in &adjacency[variable] {
                if component.insert(neighbor) {
                    remaining.remove(&neighbor);
                    pending.push_back(neighbor);
                }
            }
        }
        let component_distances = distances
            .iter()
            .copied()
            .filter(|&(first, second, coordinate, _)| {
                component.contains(&indices[&(first, coordinate)])
                    && component.contains(&indices[&(second, coordinate)])
            })
            .collect::<Vec<_>>();
        if component_distances.is_empty()
            || component_distances.len() >= usize::BITS as usize
            || (1usize << component_distances.len()) > MAX_SIGNED_BRANCHES
        {
            continue;
        }
        let component_equations = equations
            .iter()
            .filter(|equation| {
                equation
                    .terms
                    .keys()
                    .any(|variable| component.contains(&indices[variable]))
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut solutions = Vec::new();
        for signs in 0..(1usize << component_distances.len()) {
            let mut branched = component_equations.clone();
            for (index, &(first, second, coordinate, magnitude)) in
                component_distances.iter().enumerate()
            {
                let delta = if signs & (1usize << index) == 0 {
                    magnitude
                } else {
                    -magnitude
                };
                branched.push(SectionCoordinateEquation::point_difference(
                    first, second, coordinate, delta,
                ));
            }
            let candidate = solve_section_coordinate_equations(&branched, stored_coordinates);
            let mut values = stored_coordinates.clone();
            for (point, coordinates) in &candidate {
                for (coordinate, value) in coordinates.iter().copied().enumerate() {
                    if let Some(value) = value {
                        values.insert((*point, coordinate), value);
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
                    for (coordinate, value) in coordinates.into_iter().enumerate() {
                        let variable = (point, coordinate);
                        if let (Some(global), Some(value)) = (indices.get(&variable), value) {
                            if component.contains(global)
                                && !stored_coordinates.contains_key(&variable)
                            {
                                candidate_values.insert(variable, value);
                            }
                        }
                    }
                }
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
                resolved.insert(variable, value);
            }
        }
    }
    resolved
}

pub(crate) fn section_equal_length_coordinate_values(
    constraints: &[SectionEqualLengthConstraint],
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> BTreeMap<SectionCoordinateVariable, Option<f64>> {
    let mut candidates = BTreeMap::<SectionCoordinateVariable, Option<f64>>::new();
    for constraint in constraints {
        let variables = constraint
            .first
            .into_iter()
            .chain(constraint.second)
            .flat_map(|point| [(point, 0), (point, 1)])
            .collect::<BTreeSet<_>>();
        let missing = variables
            .iter()
            .copied()
            .filter(|variable| {
                coordinates
                    .get(&variable.0)
                    .and_then(|point| point[variable.1])
                    .is_none()
            })
            .collect::<Vec<_>>();
        let [missing] = missing.as_slice() else {
            continue;
        };

        let component = |first: u32, second: u32, coordinate: usize| -> Option<(f64, f64)> {
            let value = |point: u32| {
                if (point, coordinate) == *missing {
                    Some((1.0, 0.0))
                } else {
                    coordinates
                        .get(&point)
                        .and_then(|coordinates| coordinates.get(coordinate).copied().flatten())
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
            component(constraint.first[0], constraint.first[1], 0)
        else {
            continue;
        };
        let Some((first_v_coefficient, first_v_value)) =
            component(constraint.first[0], constraint.first[1], 1)
        else {
            continue;
        };
        let Some((second_u_coefficient, second_u_value)) =
            component(constraint.second[0], constraint.second[1], 0)
        else {
            continue;
        };
        let Some((second_v_coefficient, second_v_value)) =
            component(constraint.second[0], constraint.second[1], 1)
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
        let quadratic = (
            second_u.0 + second_v.0 - first_u.0 - first_v.0,
            second_u.1 + second_v.1 - first_u.1 - first_v.1,
            second_u.2 + second_v.2 - first_u.2 - first_v.2,
        );
        let roots = quadratic_roots(quadratic);
        let [value] = roots.as_slice() else {
            continue;
        };
        candidates
            .entry(*missing)
            .and_modify(|candidate| {
                if candidate.is_some_and(|candidate| !approximately_equal(candidate, *value)) {
                    *candidate = None;
                }
            })
            .or_insert(Some(*value));
    }
    candidates
}

pub(crate) fn quadratic_roots((quadratic, linear, constant): (f64, f64, f64)) -> Vec<f64> {
    let scale = quadratic
        .abs()
        .max(linear.abs())
        .max(constant.abs())
        .max(1.0);
    let tolerance = EPS_SOLVER_SCALE * scale;
    let mut roots = if quadratic.abs() <= tolerance {
        if linear.abs() <= tolerance {
            Vec::new()
        } else {
            vec![-constant / linear]
        }
    } else {
        let discriminant = linear * linear - 4.0 * quadratic * constant;
        let discriminant_tolerance = EPS_DISCRIMINANT_SCALE
            * (linear * linear + (4.0 * quadratic * constant).abs()).max(1.0);
        if discriminant < -discriminant_tolerance {
            Vec::new()
        } else if discriminant.abs() <= discriminant_tolerance {
            vec![-linear / (2.0 * quadratic)]
        } else {
            let root = discriminant.sqrt();
            vec![
                (-linear - root) / (2.0 * quadratic),
                (-linear + root) / (2.0 * quadratic),
            ]
        }
    };
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
    roots.dedup_by(|first, second| approximately_equal(*first, *second));
    roots
}

pub(crate) fn approximately_equal(first: f64, second: f64) -> bool {
    let scale = first.abs().max(second.abs()).max(1.0);
    (first - second).abs() <= EPS_DISTANCE_AGREEMENT * scale
}

pub(crate) fn solve_section_coordinate_equations(
    equations: &[SectionCoordinateEquation],
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
) -> BTreeMap<u32, [Option<f64>; 2]> {
    let variables = equations
        .iter()
        .flat_map(|equation| equation.terms.keys().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let indices = variables
        .iter()
        .enumerate()
        .map(|(index, variable)| (*variable, index))
        .collect::<BTreeMap<_, _>>();
    let Ok(mut adjacency) = alloc_filled(
        variables.len(),
        BTreeSet::new(),
        "creo section coordinate adjacency",
    ) else {
        return BTreeMap::new();
    };
    let Ok(mut variable_equations) = alloc_filled(
        variables.len(),
        BTreeSet::new(),
        "creo section coordinate equation membership",
    ) else {
        return BTreeMap::new();
    };
    for (equation_index, equation) in equations.iter().enumerate() {
        let members = equation
            .terms
            .keys()
            .filter_map(|variable| indices.get(variable).copied())
            .collect::<Vec<_>>();
        for &first in &members {
            adjacency[first].extend(members.iter().copied().filter(|second| *second != first));
            variable_equations[first].insert(equation_index);
        }
    }
    let mut solved = BTreeMap::<SectionCoordinateVariable, f64>::new();
    let mut remaining = (0..variables.len()).collect::<BTreeSet<_>>();
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(variable) = pending.pop_front() {
            for &neighbor in &adjacency[variable] {
                if component.insert(neighbor) {
                    remaining.remove(&neighbor);
                    pending.push_back(neighbor);
                }
            }
        }
        let columns = component.iter().copied().collect::<Vec<_>>();
        let local_columns = columns
            .iter()
            .enumerate()
            .map(|(local, global)| (*global, local))
            .collect::<BTreeMap<_, _>>();
        let component_equations = component
            .iter()
            .flat_map(|variable| variable_equations[*variable].iter().copied())
            .collect::<BTreeSet<_>>();
        let mut matrix = component_equations
            .into_iter()
            .map(|equation_index| &equations[equation_index])
            .map(|equation| {
                let mut row = SectionLinearRow {
                    coefficients: BTreeMap::new(),
                    rhs: equation.rhs,
                };
                for (variable, coefficient) in &equation.terms {
                    let global = indices[variable];
                    if *coefficient != 0.0 {
                        row.coefficients
                            .insert(local_columns[&global], *coefficient);
                    }
                }
                row
            })
            .collect::<Vec<_>>();
        let Some(component_solution) = uniquely_solved_linear_variables(&mut matrix, columns.len())
        else {
            for global in columns {
                let variable = variables[global];
                if let Some(value) = stored_coordinates.get(&variable) {
                    solved.insert(variable, *value);
                }
            }
            continue;
        };
        for (local, value) in component_solution {
            solved.insert(variables[columns[local]], value);
        }
    }
    let mut points = BTreeMap::<u32, [Option<f64>; 2]>::new();
    for ((point, coordinate), value) in solved {
        points.entry(point).or_insert([None; 2])[coordinate] = Some(value);
    }
    points
}

pub(crate) struct SectionLinearRow {
    pub(crate) coefficients: BTreeMap<usize, f64>,
    pub(crate) rhs: f64,
}

pub(crate) fn uniquely_solved_linear_variables(
    matrix: &mut [SectionLinearRow],
    variable_count: usize,
) -> Option<Vec<(usize, f64)>> {
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
        let pivot_coefficients = matrix[pivot_row].coefficients.clone();
        let pivot_rhs = matrix[pivot_row].rhs;
        for (row, target) in matrix.iter_mut().enumerate() {
            if row == pivot_row {
                continue;
            }
            let factor = target.coefficients.get(&column).copied().unwrap_or(0.0);
            if factor.abs() <= coefficient_tolerance {
                continue;
            }
            for (&index, &pivot_value) in &pivot_coefficients {
                let value = target.coefficients.entry(index).or_default();
                *value -= factor * pivot_value;
                if value.abs() <= coefficient_tolerance {
                    target.coefficients.remove(&index);
                }
            }
            target.rhs -= factor * pivot_rhs;
        }
        pivot_rows.insert(column, pivot_row);
        pivot_row += 1;
    }
    if matrix
        .iter()
        .any(|row| row.coefficients.is_empty() && row.rhs.abs() > residual_tolerance)
    {
        return None;
    }
    let free_columns = (0..variable_count)
        .filter(|column| !pivot_rows.contains_key(column))
        .collect::<Vec<_>>();
    Some(
        pivot_rows
            .into_iter()
            .filter_map(|(column, row)| {
                free_columns
                    .iter()
                    .all(|free| !matrix[row].coefficients.contains_key(free))
                    .then_some((column, matrix[row].rhs))
            })
            .collect(),
    )
}
