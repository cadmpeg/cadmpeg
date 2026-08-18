// SPDX-License-Identifier: Apache-2.0
//! Section-equation scalar constraints, seeds, and resolved scalar values.

use std::collections::{BTreeMap, BTreeSet};

use super::super::feature_history::{
    feature_dimension_table_complete, feature_relation_table_complete,
};
use super::super::sketch_transfer::{
    section_solver_equation_is_disabled, section_solver_relation_is_disabled,
};
use super::coordinates::resolved_section_coordinates;
use super::equations_coordinate::{
    approximately_equal, section_equation_function_six_distance_values,
    section_equation_radius_dimensions, section_equation_unsigned_coordinate_distances,
    SectionCoordinateEquation, SectionCoordinateVariable,
};

const EPS_RADIAL_VALUE: f64 = 1e-9;
const EPS_RADIAL_ANGLE: f64 = 1e-9;
const EPS_RADIAL_ZERO: f64 = 1e-12;
const EPS_AXIS_DISTANCE: f64 = 1e-9;
const EPS_AXIS_ZERO: f64 = 1e-12;
const EPS_SCALAR_EQUALITY: f64 = 1e-9;

pub(crate) fn section_equation_coordinate_equalities(
    definition: &crate::feature::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(u32, u32, usize)> {
    section_equation_coordinate_equality_rows(definition, ambiguous_point_ids)
        .into_iter()
        .filter(|constraint| constraint.active)
        .map(|constraint| (constraint.first, constraint.second, constraint.axis))
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct SectionEquationCoordinateEquality {
    pub(crate) first: u32,
    pub(crate) second: u32,
    pub(crate) axis: usize,
    pub(crate) function_id: u32,
    pub(crate) equation_id: u32,
    pub(crate) offset: usize,
    pub(crate) active: bool,
}

pub(crate) fn section_equation_coordinate_equality_rows(
    definition: &crate::feature::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionEquationCoordinateEquality> {
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
    let scalar_equality_values = section_equation_scalar_equality_values(definition);
    equations
        .rows
        .iter()
        .filter_map(|equation| {
            let (first, second, auxiliary) = match equation.function_id {
                2 if equation.arguments.len() == 2 => {
                    let [Some(first), Some(second)] = equation.arguments.as_slice() else {
                        return None;
                    };
                    (*first, *second, None)
                }
                13 if equation.arguments.len() == 3 => {
                    let [Some(first), Some(second), Some(auxiliary)] =
                        equation.arguments.as_slice()
                    else {
                        return None;
                    };
                    (*first, *second, Some(*auxiliary))
                }
                _ => return None,
            };
            let first = variables.rows.get(usize::try_from(first).ok()?)?;
            let second = variables.rows.get(usize::try_from(second).ok()?)?;
            if let Some(auxiliary) = auxiliary {
                let auxiliary = variables.rows.get(usize::try_from(auxiliary).ok()?)?;
                let equality_value = scalar_equality_values
                    .get(&(auxiliary.variable_type, auxiliary.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?;
                if auxiliary.variable_type != 7
                    || reconcile_equation_value(auxiliary.value, equality_value).ok()? != Some(0.0)
                {
                    return None;
                }
            }
            if first.variable_type != second.variable_type
                || !matches!(first.variable_type, 1 | 2)
                || auxiliary.is_some() && first.variable_type != 2
                || ambiguous_point_ids.contains(&first.key)
                || ambiguous_point_ids.contains(&second.key)
                || first.key == second.key
            {
                return None;
            }
            Some(SectionEquationCoordinateEquality {
                first: first.key,
                second: second.key,
                axis: usize::from(first.variable_type == 2),
                function_id: equation.function_id,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !section_solver_equation_is_disabled(definition, equation.equation_id),
            })
        })
        .collect()
}

pub(crate) type SectionScalarVariable = (u32, u32);

#[derive(Clone, Copy)]
pub(crate) struct SectionEquationMidpointConstraint {
    pub(crate) first: SectionCoordinateVariable,
    pub(crate) second: SectionCoordinateVariable,
    pub(crate) result: SectionScalarVariable,
}

#[derive(Clone, Copy)]
pub(crate) struct SectionEquationPointBinding {
    pub(crate) point: u32,
    pub(crate) coordinates: [SectionScalarVariable; 2],
}

#[derive(Default)]
pub(crate) struct SectionEquationAuxiliaryConstraints {
    pub(crate) midpoints: Vec<SectionEquationMidpointConstraint>,
    pub(crate) point_bindings: Vec<SectionEquationPointBinding>,
}

pub(crate) fn section_equation_auxiliary_constraints(
    definition: &crate::feature::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> SectionEquationAuxiliaryConstraints {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return SectionEquationAuxiliaryConstraints::default();
    };
    let Some(equations) =
        crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return SectionEquationAuxiliaryConstraints::default();
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return SectionEquationAuxiliaryConstraints::default();
    };
    if declared_count != equations.rows.len() + 1 {
        return SectionEquationAuxiliaryConstraints::default();
    }

    let row = |ordinal: Option<u32>| {
        usize::try_from(ordinal?)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    let mut constraints = SectionEquationAuxiliaryConstraints::default();
    for equation in equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
    {
        match (equation.function_id, equation.arguments.as_slice()) {
            (42, [Some(first), Some(second), Some(result)]) => {
                let (Some(first), Some(second), Some(result)) =
                    (row(Some(*first)), row(Some(*second)), row(Some(*result)))
                else {
                    continue;
                };
                if first.variable_type != second.variable_type
                    || !matches!(first.variable_type, 1 | 2)
                    || result.variable_type != 6
                    || ambiguous_point_ids.contains(&first.key)
                    || ambiguous_point_ids.contains(&second.key)
                {
                    continue;
                }
                let coordinate = usize::from(first.variable_type == 2);
                constraints
                    .midpoints
                    .push(SectionEquationMidpointConstraint {
                        first: (first.key, coordinate),
                        second: (second.key, coordinate),
                        result: (result.variable_type, result.key),
                    });
            }
            (31, [Some(first_u), Some(first_v), Some(second_u), Some(second_v)]) => {
                let (Some(first_u), Some(first_v), Some(second_u), Some(second_v)) = (
                    row(Some(*first_u)),
                    row(Some(*first_v)),
                    row(Some(*second_u)),
                    row(Some(*second_v)),
                ) else {
                    continue;
                };
                if first_u.variable_type != 1
                    || first_v.variable_type != 2
                    || first_u.key != first_v.key
                    || second_u.variable_type != 6
                    || second_v.variable_type != 6
                    || second_u.key == second_v.key
                    || ambiguous_point_ids.contains(&first_u.key)
                {
                    continue;
                }
                constraints
                    .point_bindings
                    .push(SectionEquationPointBinding {
                        point: first_u.key,
                        coordinates: [
                            (second_u.variable_type, second_u.key),
                            (second_v.variable_type, second_v.key),
                        ],
                    });
            }
            _ => {}
        }
    }
    constraints
}

#[derive(Clone, Copy)]
pub(crate) struct SectionFunctionFortyTwoMidpointCoordinate {
    pub(crate) first: u32,
    pub(crate) second: u32,
    pub(crate) coordinate: usize,
    pub(crate) value: Option<f64>,
    pub(crate) equation_id: u32,
    pub(crate) offset: usize,
    pub(crate) active: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct SectionFunctionThirtyOnePointCoordinates {
    pub(crate) point: u32,
    pub(crate) values: [Option<f64>; 2],
    pub(crate) equation_id: u32,
    pub(crate) offset: usize,
    pub(crate) active: bool,
}

pub(crate) fn reconcile_equation_value(
    stored: Option<f64>,
    solved: Option<f64>,
) -> Result<Option<f64>, ()> {
    if stored.is_some_and(|value| !value.is_finite())
        || solved.is_some_and(|value| !value.is_finite())
    {
        return Err(());
    }
    match (stored, solved) {
        (Some(stored), Some(solved)) if !approximately_equal(stored, solved) => Err(()),
        (Some(stored), _) => Ok(Some(stored)),
        (_, Some(solved)) => Ok(Some(solved)),
        (None, None) => Ok(None),
    }
}

pub(crate) fn section_equation_function_forty_two_midpoint_coordinate_rows(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionFunctionFortyTwoMidpointCoordinate> {
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
            let [Some(first), Some(second), Some(result)] = equation.arguments.as_slice() else {
                return None;
            };
            if equation.function_id != 42 {
                return None;
            }
            let (Some(first), Some(second), Some(result)) =
                (row(Some(*first)), row(Some(*second)), row(Some(*result)))
            else {
                return None;
            };
            if first.variable_type != second.variable_type
                || !matches!(first.variable_type, 1 | 2)
                || result.variable_type != 6
                || ambiguous_point_ids.contains(&first.key)
                || ambiguous_point_ids.contains(&second.key)
            {
                return None;
            }
            let coordinate = usize::from(first.variable_type == 2);
            let solved = coordinates
                .get(&first.key)
                .and_then(|point| point[coordinate])
                .zip(
                    coordinates
                        .get(&second.key)
                        .and_then(|point| point[coordinate]),
                )
                .map(|(first, second)| f64::midpoint(first, second));
            let result_variable = (result.variable_type, result.key);
            let equality_value = scalar_equality_values
                .get(&result_variable)
                .copied()
                .unwrap_or(Ok(None))
                .ok()?;
            let stored = reconcile_equation_value(result.value, equality_value).ok()?;
            let value = reconcile_equation_value(stored, solved).ok()?;
            Some(SectionFunctionFortyTwoMidpointCoordinate {
                first: first.key,
                second: second.key,
                coordinate,
                value,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !section_solver_equation_is_disabled(definition, equation.equation_id),
            })
        })
        .collect()
}

pub(crate) fn section_equation_function_thirty_one_point_coordinate_rows(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionFunctionThirtyOnePointCoordinates> {
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
            let [Some(first_u), Some(first_v), Some(second_u), Some(second_v)] =
                equation.arguments.as_slice()
            else {
                return None;
            };
            if equation.function_id != 31 {
                return None;
            }
            let (Some(first_u), Some(first_v), Some(second_u), Some(second_v)) = (
                row(Some(*first_u)),
                row(Some(*first_v)),
                row(Some(*second_u)),
                row(Some(*second_v)),
            ) else {
                return None;
            };
            if first_u.variable_type != 1
                || first_v.variable_type != 2
                || first_u.key != first_v.key
                || second_u.variable_type != 6
                || second_v.variable_type != 6
                || second_u.key == second_v.key
                || ambiguous_point_ids.contains(&first_u.key)
            {
                return None;
            }
            let point = coordinates.get(&first_u.key).copied().unwrap_or([None; 2]);
            let u_equality = scalar_equality_values
                .get(&(second_u.variable_type, second_u.key))
                .copied()
                .unwrap_or(Ok(None))
                .ok()?;
            let v_equality = scalar_equality_values
                .get(&(second_v.variable_type, second_v.key))
                .copied()
                .unwrap_or(Ok(None))
                .ok()?;
            let values = [
                reconcile_equation_value(
                    reconcile_equation_value(second_u.value, u_equality).ok()?,
                    point[0],
                )
                .ok()?,
                reconcile_equation_value(
                    reconcile_equation_value(second_v.value, v_equality).ok()?,
                    point[1],
                )
                .ok()?,
            ];
            Some(SectionFunctionThirtyOnePointCoordinates {
                point: first_u.key,
                values,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !section_solver_equation_is_disabled(definition, equation.equation_id),
            })
        })
        .collect()
}

pub(crate) fn merge_scalar_value_candidate(
    values: &mut BTreeMap<SectionScalarVariable, Option<f64>>,
    variable: SectionScalarVariable,
    value: f64,
) {
    if !value.is_finite() {
        return;
    }
    match values.entry(variable) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(Some(value));
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            let Some(stored) = *entry.get() else {
                return;
            };
            if !approximately_equal(stored, value) {
                *entry.get_mut() = None;
            }
        }
    }
}

pub(crate) fn section_relation_radius_scalar_values(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<(SectionScalarVariable, f64)> {
    let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|table| feature_dimension_table_complete(table))
    else {
        return Vec::new();
    };
    definition
        .relations
        .iter()
        .filter(|table| feature_relation_table_complete(table))
        .flat_map(|table| &table.rows)
        .filter_map(|relation| {
            if section_solver_relation_is_disabled(definition, relation.relation_id)
                || relation.relation_type != 14
                || relation.sign != 1
            {
                return None;
            }
            let vectors = relation.operand_vectors?;
            let [Some(radius), Some(0), Some(0), Some(0)] = vectors[0] else {
                return None;
            };
            if vectors[1] != [Some(0); 4] || vectors[2] != [Some(15), Some(0), Some(0), Some(0)] {
                return None;
            }
            let dimension = dimensions
                .rows
                .get(usize::try_from(relation.dimension_id).ok()?)?;
            if dimension.value_unit != crate::feature::DimensionUnit::Millimeters
                || !matches!(dimension.dimension_type, 1..=5)
            {
                return None;
            }
            let value = dimension
                .value
                .filter(|value| value.is_finite() && *value > 0.0)?;
            let value = if dimension.dimension_type == 4 {
                value / 2.0
            } else {
                value
            };
            value.is_finite().then_some(((3, radius), value))
        })
        .collect()
}

pub(crate) fn section_equation_scalar_seed_values(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<SectionScalarVariable, Option<f64>> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return BTreeMap::new();
    };
    let ambiguous_point_ids = variables.reconciled_points().1;
    let mut values = BTreeMap::new();
    for row in &variables.rows {
        if matches!(row.variable_type, 1 | 2) {
            continue;
        }
        let variable = (row.variable_type, row.key);
        match row.value {
            Some(value) if value.is_finite() => {
                merge_scalar_value_candidate(&mut values, variable, value);
            }
            Some(_) => {
                values.insert(variable, None);
            }
            None => {}
        }
    }
    for (variable, value) in section_equation_scalar_equalities(definition) {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for constraint in
        section_equation_unsigned_coordinate_distances(definition, &ambiguous_point_ids)
    {
        merge_scalar_value_candidate(&mut values, constraint.scalar, constraint.value);
    }
    for constraint in section_equation_radius_dimensions(definition)
        .into_iter()
        .filter(|constraint| constraint.active)
    {
        merge_scalar_value_candidate(&mut values, constraint.radius_variable, constraint.value);
        merge_scalar_value_candidate(&mut values, constraint.scalar, constraint.value);
    }
    for (variable, value) in section_relation_radius_scalar_values(definition) {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for (variable, value) in section_equation_function_sixteen_angle_difference_values(definition) {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    values
}

pub(crate) fn propagate_section_equation_scalar_equality_values(
    definition: &crate::feature::FeatureDefinition,
    values: &mut BTreeMap<SectionScalarVariable, Option<f64>>,
) -> bool {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return false;
    };
    let components = section_equation_scalar_equality_components(definition);
    let mut changed = false;
    for component in components {
        let mut component_value = None;
        let mut conflicting = false;
        for variable in &component {
            let invalid_row = variables.rows.iter().any(|row| {
                (row.variable_type, row.key) == *variable
                    && row.value.is_some_and(|value| !value.is_finite())
            });
            if invalid_row {
                conflicting = true;
                break;
            }
            let Some(Some(value)) = values.get(variable) else {
                continue;
            };
            if !value.is_finite()
                || component_value.is_some_and(|stored| !approximately_equal(stored, *value))
            {
                conflicting = true;
                break;
            }
            component_value = Some(*value);
        }
        if conflicting {
            for variable in component {
                if values.get(&variable) != Some(&None) {
                    values.insert(variable, None);
                    changed = true;
                }
            }
        } else if let Some(value) = component_value {
            for variable in component {
                if values.get(&variable) != Some(&Some(value)) {
                    values.insert(variable, Some(value));
                    changed = true;
                }
            }
        }
    }
    changed
}

pub(crate) fn append_section_equation_auxiliary_coordinate_constraints(
    constraints: &SectionEquationAuxiliaryConstraints,
    scalar_values: &BTreeMap<SectionScalarVariable, Option<f64>>,
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
    equations: &mut Vec<SectionCoordinateEquation>,
) {
    for constraint in &constraints.midpoints {
        let Some(Some(value)) = scalar_values.get(&constraint.result) else {
            continue;
        };
        if stored_coordinates
            .get(&constraint.first)
            .zip(stored_coordinates.get(&constraint.second))
            .is_some_and(|(first, second)| {
                !approximately_equal(f64::midpoint(*first, *second), *value)
            })
        {
            continue;
        }
        let mut equation = SectionCoordinateEquation::default();
        equation.add_point(constraint.first.0, constraint.first.1, 1.0);
        equation.add_point(constraint.second.0, constraint.second.1, 1.0);
        equation.rhs = 2.0 * value;
        equations.push(equation);
    }
    for constraint in &constraints.point_bindings {
        let mut values = [None; 2];
        let mut underdetermined = false;
        let mut invalid = false;
        for (coordinate, variable) in constraint.coordinates.into_iter().enumerate() {
            match scalar_values.get(&variable) {
                Some(Some(value)) => {
                    if stored_coordinates
                        .get(&(constraint.point, coordinate))
                        .is_some_and(|stored| !approximately_equal(*stored, *value))
                    {
                        invalid = true;
                        break;
                    }
                    values[coordinate] = Some(*value);
                }
                Some(None) => {
                    invalid = true;
                    break;
                }
                None => {
                    underdetermined |=
                        !stored_coordinates.contains_key(&(constraint.point, coordinate));
                }
            }
        }
        if invalid || underdetermined {
            continue;
        }
        for (coordinate, value) in values
            .into_iter()
            .enumerate()
            .filter_map(|(coordinate, value)| Some((coordinate, value?)))
        {
            equations.push(SectionCoordinateEquation::point_value(
                constraint.point,
                coordinate,
                value,
            ));
        }
    }
}

pub(crate) fn section_equation_scalar_values_from_coordinates(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> BTreeMap<SectionScalarVariable, f64> {
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .map_or_else(BTreeSet::new, |variables| variables.reconciled_points().1);
    let constraints = section_equation_auxiliary_constraints(definition, &ambiguous_point_ids);
    let seed_values = section_equation_scalar_seed_values(definition);
    let mut derived = BTreeMap::<SectionScalarVariable, Option<f64>>::new();
    let compatible = |variable: SectionScalarVariable, value: f64| {
        !seed_values.contains_key(&variable)
            || seed_values[&variable].is_some_and(|stored| approximately_equal(stored, value))
    };
    for constraint in constraints.midpoints {
        let (Some(Some(first)), Some(Some(second))) = (
            coordinates
                .get(&constraint.first.0)
                .map(|point| point[constraint.first.1]),
            coordinates
                .get(&constraint.second.0)
                .map(|point| point[constraint.second.1]),
        ) else {
            continue;
        };
        let value = f64::midpoint(first, second);
        if compatible(constraint.result, value) {
            merge_scalar_value_candidate(&mut derived, constraint.result, value);
        }
    }
    for constraint in constraints.point_bindings {
        let Some(point) = coordinates.get(&constraint.point) else {
            continue;
        };
        let mut invalid = false;
        let mut candidates = Vec::new();
        for (coordinate, variable) in constraint.coordinates.into_iter().enumerate() {
            let Some(value) = point[coordinate] else {
                continue;
            };
            if !compatible(variable, value) {
                invalid = true;
                break;
            }
            if !seed_values.contains_key(&variable) {
                candidates.push((variable, value));
            }
        }
        if !invalid {
            for (variable, value) in candidates {
                merge_scalar_value_candidate(&mut derived, variable, value);
            }
        }
    }
    for (variable, value) in
        section_equation_function_six_distance_values(definition, coordinates, &ambiguous_point_ids)
    {
        merge_scalar_value_candidate(&mut derived, variable, value);
    }
    for (variable, value) in section_equation_function_forty_three_axis_distance_values(
        definition,
        coordinates,
        &ambiguous_point_ids,
    ) {
        merge_scalar_value_candidate(&mut derived, variable, value);
    }
    for constraint in
        section_equation_radial_constraints(definition, coordinates, &ambiguous_point_ids)
    {
        for (variable, value) in [
            (constraint.radius, constraint.radius_value),
            (constraint.angle, constraint.angle_value),
        ] {
            let Some(value) = value else {
                continue;
            };
            merge_scalar_value_candidate(&mut derived, variable, value);
        }
    }
    derived
        .into_iter()
        .filter_map(|(variable, value)| Some((variable, value?)))
        .collect()
}

fn direct_function_five_scalar_rows<'a>(
    function_id: u32,
    arguments: &[Option<u32>],
    variables: &'a [crate::feature::FeatureVariableRow],
) -> Option<(
    &'a crate::feature::FeatureVariableRow,
    &'a crate::feature::FeatureVariableRow,
    &'a crate::feature::FeatureVariableRow,
)> {
    if function_id != 5 || arguments.len() != 3 {
        return None;
    }
    let [Some(first), Some(second), Some(selector)] = arguments else {
        return None;
    };
    let row = |ordinal: u32| {
        usize::try_from(ordinal)
            .ok()
            .and_then(|ordinal| variables.get(ordinal))
    };
    let (Some(first), Some(second), Some(selector)) = (row(*first), row(*second), row(*selector))
    else {
        return None;
    };
    (first.variable_type == 6
        && second.variable_type == 6
        && selector.variable_type == 5
        && first.key != second.key)
        .then_some((first, second, selector))
}

pub(crate) fn section_equation_scalar_equality_components(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<BTreeSet<SectionScalarVariable>> {
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

    let mut adjacency = BTreeMap::<SectionScalarVariable, BTreeSet<SectionScalarVariable>>::new();
    let mut deferred_function_five = Vec::<(
        SectionScalarVariable,
        SectionScalarVariable,
        SectionScalarVariable,
        Option<f64>,
    )>::new();
    for equation in equations
        .rows
        .iter()
        .filter(|equation| !section_solver_equation_is_disabled(definition, equation.equation_id))
    {
        if let Some((first, second, selector)) = direct_function_five_scalar_rows(
            equation.function_id,
            &equation.arguments,
            &variables.rows,
        ) {
            deferred_function_five.push((
                (first.variable_type, first.key),
                (second.variable_type, second.key),
                (selector.variable_type, selector.key),
                selector.value,
            ));
            continue;
        }
        let (2, [Some(first), Some(second)]) =
            (equation.function_id, equation.arguments.as_slice())
        else {
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
        if first.variable_type != second.variable_type
            || matches!(first.variable_type, 1 | 2)
            || first.key == second.key
        {
            continue;
        }
        let first = (first.variable_type, first.key);
        let second = (second.variable_type, second.key);
        adjacency.entry(first).or_default().insert(second);
        adjacency.entry(second).or_default().insert(first);
    }

    let base_components = scalar_equality_components(&adjacency);
    let base_values = scalar_equality_values_for_components(&variables.rows, &base_components);
    for (first, second, selector, stored_selector) in deferred_function_five {
        let equality_value = base_values.get(&selector).copied().unwrap_or(Ok(None));
        if !matches!(
            equality_value
                .ok()
                .and_then(|value| reconcile_equation_value(stored_selector, value).ok()),
            Some(Some(value)) if value == 0.0
        ) {
            continue;
        }
        adjacency.entry(first).or_default().insert(second);
        adjacency.entry(second).or_default().insert(first);
    }
    scalar_equality_components(&adjacency)
}

fn scalar_equality_components(
    adjacency: &BTreeMap<SectionScalarVariable, BTreeSet<SectionScalarVariable>>,
) -> Vec<BTreeSet<SectionScalarVariable>> {
    let mut remaining = adjacency.keys().copied().collect::<BTreeSet<_>>();
    let mut components = Vec::new();
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(variable) = pending.pop_front() {
            for neighbor in adjacency
                .get(&variable)
                .into_iter()
                .flat_map(|neighbors| neighbors.iter())
                .copied()
            {
                if component.insert(neighbor) {
                    remaining.remove(&neighbor);
                    pending.push_back(neighbor);
                }
            }
        }
        components.push(component);
    }
    components
}

fn scalar_equality_values_for_components(
    rows: &[crate::feature::FeatureVariableRow],
    components: &[BTreeSet<SectionScalarVariable>],
) -> BTreeMap<SectionScalarVariable, Result<Option<f64>, ()>> {
    let mut values = BTreeMap::<SectionScalarVariable, Vec<f64>>::new();
    let mut invalid = BTreeSet::<SectionScalarVariable>::new();
    for row in rows {
        if matches!(row.variable_type, 1 | 2) {
            continue;
        }
        let variable = (row.variable_type, row.key);
        match row.value {
            Some(value) if value.is_finite() => values.entry(variable).or_default().push(value),
            Some(_) => {
                invalid.insert(variable);
            }
            None => {}
        }
    }

    let mut resolved = BTreeMap::new();
    for component in components {
        let value = if component.iter().any(|variable| invalid.contains(variable)) {
            Err(())
        } else {
            let component_values = component
                .iter()
                .flat_map(|variable| values.get(variable).into_iter().flatten().copied())
                .collect::<Vec<_>>();
            match component_values.first().copied() {
                None => Ok(None),
                Some(first) => {
                    let scale = component_values
                        .iter()
                        .map(|value| value.abs())
                        .fold(1.0, f64::max);
                    component_values
                        .iter()
                        .all(|value| (*value - first).abs() <= EPS_SCALAR_EQUALITY * scale)
                        .then_some(Some(first))
                        .ok_or(())
                }
            }
        };
        resolved.extend(component.iter().copied().map(|variable| (variable, value)));
    }
    resolved
}

pub(crate) fn section_equation_scalar_equalities(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<SectionScalarVariable, f64> {
    section_equation_scalar_equality_values(definition)
        .into_iter()
        .filter_map(|(variable, value)| match value {
            Ok(Some(value)) => Some((variable, value)),
            Ok(None) | Err(()) => None,
        })
        .collect()
}

pub(crate) fn section_equation_scalar_equality_values(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<SectionScalarVariable, Result<Option<f64>, ()>> {
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return BTreeMap::new();
    };
    let components = section_equation_scalar_equality_components(definition);
    scalar_equality_values_for_components(&variables.rows, &components)
}

#[derive(Clone, Copy)]
pub(crate) struct SectionRadialConstraint {
    pub(crate) first: u32,
    pub(crate) second: u32,
    pub(crate) radius: (u32, u32),
    pub(crate) angle: (u32, u32),
    pub(crate) radius_value: Option<f64>,
    pub(crate) angle_value: Option<f64>,
    pub(crate) equation_id: u32,
    pub(crate) offset: usize,
    pub(crate) active: bool,
}

impl SectionRadialConstraint {
    pub(crate) fn offset(self) -> Option<[f64; 2]> {
        let radius = self.radius_value?;
        if radius.abs() <= EPS_RADIAL_ZERO {
            return Some([0.0; 2]);
        }
        let angle = self.angle_value?;
        Some([radius * angle.cos(), radius * angle.sin()])
    }
}

pub(crate) fn section_equation_radial_constraints(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionRadialConstraint> {
    section_equation_radial_constraint_rows(definition, coordinates, ambiguous_point_ids)
        .into_iter()
        .filter(|constraint| constraint.active)
        .collect()
}

pub(crate) fn section_equation_radial_constraints_with_scalar_values(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
    scalar_values: &BTreeMap<SectionScalarVariable, Option<f64>>,
) -> Vec<SectionRadialConstraint> {
    section_equation_radial_constraint_rows_with_scalar_values(
        definition,
        coordinates,
        ambiguous_point_ids,
        Some(scalar_values),
    )
    .into_iter()
    .filter(|constraint| constraint.active)
    .collect()
}

pub(crate) fn section_equation_radial_constraint_rows(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionRadialConstraint> {
    section_equation_radial_constraint_rows_with_scalar_values(
        definition,
        coordinates,
        ambiguous_point_ids,
        None,
    )
}

fn section_equation_radial_constraint_rows_with_scalar_values(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
    scalar_values: Option<&BTreeMap<SectionScalarVariable, Option<f64>>>,
) -> Vec<SectionRadialConstraint> {
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
    let scalar_equality_values = section_equation_scalar_equality_values(definition);
    equations
        .rows
        .iter()
        .filter(|equation| equation.function_id == 0 && equation.arguments.len() == 6)
        .filter_map(|equation| {
            let [
                Some(first_u),
                Some(first_v),
                Some(second_u),
                Some(second_v),
                Some(radius),
                Some(angle),
            ] = equation.arguments.as_slice()
            else {
                return None;
            };
            let first_u = variables.rows.get(usize::try_from(*first_u).ok()?)?;
            let first_v = variables.rows.get(usize::try_from(*first_v).ok()?)?;
            let second_u = variables.rows.get(usize::try_from(*second_u).ok()?)?;
            let second_v = variables.rows.get(usize::try_from(*second_v).ok()?)?;
            let radius = variables.rows.get(usize::try_from(*radius).ok()?)?;
            let angle = variables.rows.get(usize::try_from(*angle).ok()?)?;
            if first_u.variable_type != 1
                || first_v.variable_type != 2
                || second_u.variable_type != 1
                || second_v.variable_type != 2
                || first_u.key != first_v.key
                || second_u.key != second_v.key
                || first_u.key == second_u.key
                || !matches!(radius.variable_type, 0 | 3)
                || !matches!(angle.variable_type, 4 | 6)
                || ambiguous_point_ids.contains(&first_u.key)
                || ambiguous_point_ids.contains(&second_u.key)
            {
                return None;
            }
            let scalar_value = |row: &crate::feature::FeatureVariableRow| {
                let equality_value = scalar_equality_values
                    .get(&(row.variable_type, row.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?;
                let resolved = reconcile_equation_value(row.value, equality_value).ok()?;
                let Some(scalar_values) = scalar_values else {
                    return Some(resolved);
                };
                let Some(Some(value)) = scalar_values.get(&(row.variable_type, row.key)) else {
                    return Some(resolved);
                };
                reconcile_equation_value(resolved, Some(*value)).ok()
            };
            let mut radius_value = match scalar_value(radius)? {
                Some(value) if value.is_finite() && value >= 0.0 => Some(value),
                Some(_) => return None,
                None => None,
            };
            let mut angle_value = match scalar_value(angle)? {
                Some(value) if value.is_finite() => Some(value),
                Some(_) => return None,
                None => None,
            };
            let active = !section_solver_equation_is_disabled(definition, equation.equation_id);
            if active {
                let first_point = coordinates.get(&first_u.key).and_then(|point| {
                    Some([point[0]?, point[1]?])
                });
                let second_point = coordinates.get(&second_u.key).and_then(|point| {
                    Some([point[0]?, point[1]?])
                });
                if let (Some(first), Some(second)) = (first_point, second_point) {
                    if !first.into_iter().chain(second).all(f64::is_finite) {
                        return None;
                    }
                    let delta = [second[0] - first[0], second[1] - first[1]];
                    let distance = delta[0].hypot(delta[1]);
                    let scale = distance
                        .abs()
                        .max(radius_value.unwrap_or(0.0).abs())
                        .max(1.0);
                    if radius_value
                        .is_some_and(|value| (value - distance).abs() > EPS_RADIAL_VALUE * scale)
                    {
                        return None;
                    }
                    radius_value.get_or_insert(distance);
                    if distance > EPS_RADIAL_ZERO {
                        let derived_angle = delta[1].atan2(delta[0]);
                        if angle_value.is_some_and(|value| {
                            let difference =
                                (value - derived_angle).rem_euclid(std::f64::consts::TAU);
                            difference.min(std::f64::consts::TAU - difference)
                                > EPS_RADIAL_ANGLE
                        }) {
                            return None;
                        }
                        angle_value.get_or_insert(derived_angle);
                    }
                }
            }
            Some(SectionRadialConstraint {
                first: first_u.key,
                second: second_u.key,
                radius: (radius.variable_type, radius.key),
                angle: (angle.variable_type, angle.key),
                radius_value,
                angle_value,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active,
            })
        })
        .collect()
}

pub(crate) fn resolved_section_scalar_values(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<(u32, u32), f64> {
    let coordinates = resolved_section_coordinates(definition);
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .map_or_else(BTreeSet::new, |variables| variables.reconciled_points().1);
    let mut values = BTreeMap::<(u32, u32), Option<f64>>::new();
    for (variable, value) in section_equation_scalar_equalities(definition) {
        values.insert(variable, Some(value));
    }
    for (variable, value) in
        section_equation_scalar_values_from_coordinates(definition, &coordinates)
    {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for (variable, value) in section_equation_function_six_distance_values(
        definition,
        &coordinates,
        &ambiguous_point_ids,
    ) {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for (variable, value) in section_equation_function_forty_three_axis_distance_values(
        definition,
        &coordinates,
        &ambiguous_point_ids,
    ) {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for (variable, value) in section_equation_function_sixteen_angle_difference_values(definition) {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for constraint in
        section_equation_unsigned_coordinate_distances(definition, &ambiguous_point_ids)
    {
        merge_scalar_value_candidate(&mut values, constraint.scalar, constraint.value);
    }
    for constraint in section_equation_radius_dimensions(definition)
        .into_iter()
        .filter(|constraint| constraint.active)
    {
        merge_scalar_value_candidate(&mut values, constraint.radius_variable, constraint.value);
        merge_scalar_value_candidate(&mut values, constraint.scalar, constraint.value);
    }
    for (variable, value) in section_relation_radius_scalar_values(definition) {
        merge_scalar_value_candidate(&mut values, variable, value);
    }
    for constraint in
        section_equation_radial_constraints(definition, &coordinates, &ambiguous_point_ids)
    {
        for (variable, value) in [
            (constraint.radius, constraint.radius_value),
            (constraint.angle, constraint.angle_value),
        ] {
            let Some(value) = value else {
                continue;
            };
            merge_scalar_value_candidate(&mut values, variable, value);
        }
    }
    propagate_section_equation_scalar_equality_values(definition, &mut values);
    values
        .into_iter()
        .filter_map(|(variable, value)| Some((variable, value?)))
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct SectionFunctionFiveScalarEquality {
    pub(crate) first: SectionScalarVariable,
    pub(crate) second: SectionScalarVariable,
    pub(crate) equation_id: u32,
    pub(crate) offset: usize,
}

pub(crate) fn section_equation_function_five_scalar_equality_rows(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<SectionFunctionFiveScalarEquality> {
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
    let scalar_equality_values = section_equation_scalar_equality_values(definition);
    equations
        .rows
        .iter()
        .filter(|equation| {
            equation.function_id == 5
                && equation.arguments.len() == 3
                && !section_solver_equation_is_disabled(definition, equation.equation_id)
        })
        .filter_map(|equation| {
            let (first, second, selector) = direct_function_five_scalar_rows(
                equation.function_id,
                &equation.arguments,
                &variables.rows,
            )?;
            let selector_value = reconcile_equation_value(
                selector.value,
                scalar_equality_values
                    .get(&(selector.variable_type, selector.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?,
            )
            .ok()?;
            if selector_value != Some(0.0) {
                return None;
            }
            for scalar in [first, second] {
                reconcile_equation_value(
                    scalar.value,
                    scalar_equality_values
                        .get(&(scalar.variable_type, scalar.key))
                        .copied()
                        .unwrap_or(Ok(None))
                        .ok()?,
                )
                .ok()?;
            }
            Some(SectionFunctionFiveScalarEquality {
                first: (first.variable_type, first.key),
                second: (second.variable_type, second.key),
                equation_id: equation.equation_id,
                offset: equation.offset,
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct SectionFunctionSixteenAngleDifference {
    pub(crate) first: SectionScalarVariable,
    pub(crate) second: SectionScalarVariable,
    pub(crate) difference: SectionScalarVariable,
    pub(crate) value: f64,
    pub(crate) equation_id: u32,
    pub(crate) offset: usize,
    pub(crate) active: bool,
}

pub(crate) fn section_equation_function_sixteen_angle_difference_values(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<(SectionScalarVariable, f64)> {
    section_equation_function_sixteen_angle_difference_rows(definition)
        .into_iter()
        .filter(|constraint| constraint.active)
        .map(|constraint| (constraint.difference, constraint.value))
        .collect()
}

pub(crate) fn section_equation_function_sixteen_angle_difference_rows(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<SectionFunctionSixteenAngleDifference> {
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
    let scalar_equality_values = section_equation_scalar_equality_values(definition);
    let row = |ordinal: u32| {
        usize::try_from(ordinal)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    equations
        .rows
        .iter()
        .filter_map(|equation| {
            if equation.function_id != 16 || equation.arguments.len() != 4 {
                return None;
            }
            let [Some(first), Some(second), Some(difference), Some(selector)] =
                equation.arguments.as_slice()
            else {
                return None;
            };
            let (Some(first), Some(second), Some(difference), Some(selector)) =
                (row(*first), row(*second), row(*difference), row(*selector))
            else {
                return None;
            };
            let first_value = reconcile_equation_value(
                first.value,
                scalar_equality_values
                    .get(&(first.variable_type, first.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?,
            )
            .ok()?;
            let second_value = reconcile_equation_value(
                second.value,
                scalar_equality_values
                    .get(&(second.variable_type, second.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?,
            )
            .ok()?;
            let difference_value = reconcile_equation_value(
                difference.value,
                scalar_equality_values
                    .get(&(difference.variable_type, difference.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?,
            )
            .ok()?;
            let selector_value = reconcile_equation_value(
                selector.value,
                scalar_equality_values
                    .get(&(selector.variable_type, selector.key))
                    .copied()
                    .unwrap_or(Ok(None))
                    .ok()?,
            )
            .ok()?;
            if first.variable_type != 4
                || second.variable_type != 4
                || difference.variable_type != 0
                || selector.variable_type != 5
                || selector_value != Some(0.0)
            {
                return None;
            }
            let (Some(first_value), Some(second_value)) = (first_value, second_value) else {
                return None;
            };
            if !first_value.is_finite() || !second_value.is_finite() || first_value < second_value {
                return None;
            }
            let value = first_value - second_value;
            if !value.is_finite() || value > std::f64::consts::PI {
                return None;
            }
            if difference_value.is_some_and(|stored| {
                !stored.is_finite() || stored < 0.0 || !approximately_equal(stored, value)
            }) {
                return None;
            }
            Some(SectionFunctionSixteenAngleDifference {
                first: (first.variable_type, first.key),
                second: (second.variable_type, second.key),
                difference: (difference.variable_type, difference.key),
                value,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !section_solver_equation_is_disabled(definition, equation.equation_id),
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct SectionFunctionFortyThreeAxisDistance {
    pub(crate) first: u32,
    pub(crate) second: u32,
    pub(crate) coordinate: usize,
    pub(crate) scalar: SectionScalarVariable,
    pub(crate) value: f64,
    pub(crate) equation_id: u32,
    pub(crate) offset: usize,
    pub(crate) active: bool,
}

pub(crate) fn section_equation_function_forty_three_axis_distance_values(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(SectionScalarVariable, f64)> {
    section_equation_function_forty_three_axis_distance_rows(
        definition,
        coordinates,
        ambiguous_point_ids,
    )
    .into_iter()
    .filter(|constraint| constraint.active)
    .map(|constraint| (constraint.scalar, constraint.value))
    .collect()
}

pub(crate) fn section_equation_function_forty_three_axis_distance_rows(
    definition: &crate::feature::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<SectionFunctionFortyThreeAxisDistance> {
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
    let scalar_equality_values = section_equation_scalar_equality_values(definition);
    let row = |ordinal: u32| {
        usize::try_from(ordinal)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    equations
        .rows
        .iter()
        .filter_map(|equation| {
            if equation.function_id != 43 || equation.arguments.len() != 8 {
                return None;
            }
            let [
                Some(first_u),
                Some(first_v),
                Some(second_u),
                Some(second_v),
                Some(first_auxiliary),
                Some(second_auxiliary),
                Some(distance),
                Some(final_auxiliary),
            ] = equation.arguments.as_slice()
            else {
                return None;
            };
            let (Some(first_u), Some(first_v), Some(second_u), Some(second_v)) = (
                row(*first_u),
                row(*first_v),
                row(*second_u),
                row(*second_v),
            ) else {
                return None;
            };
            let (Some(first_auxiliary), Some(second_auxiliary), Some(distance), Some(final_auxiliary)) =
                (
                    row(*first_auxiliary),
                    row(*second_auxiliary),
                    row(*distance),
                    row(*final_auxiliary),
                )
            else {
                return None;
            };
            if first_u.variable_type != 1
                || first_v.variable_type != 2
                || first_u.key != first_v.key
                || second_u.variable_type != 1
                || second_v.variable_type != 2
                || second_u.key != second_v.key
                || first_u.key == second_u.key
                || !matches!(first_auxiliary.variable_type, 4 | 5)
                || !matches!(second_auxiliary.variable_type, 4 | 5)
                || distance.variable_type != 0
                || final_auxiliary.variable_type != 5
                || ambiguous_point_ids.contains(&first_u.key)
                || ambiguous_point_ids.contains(&second_u.key)
                || [first_auxiliary, second_auxiliary, final_auxiliary]
                    .into_iter()
                    .any(|row| {
                        row.value.is_some_and(|value| {
                            !value.is_finite()
                                || row.variable_type == 5 && value.abs() > EPS_AXIS_ZERO
                        })
                    })
            {
                return None;
            }
            let auxiliary_value = |row: &crate::feature::FeatureVariableRow| {
                reconcile_equation_value(
                    row.value,
                    scalar_equality_values
                        .get(&(row.variable_type, row.key))
                        .copied()
                        .unwrap_or(Ok(None))
                        .ok()?,
                )
                .ok()
            };
            let auxiliary_values = [
                (first_auxiliary, auxiliary_value(first_auxiliary)?),
                (second_auxiliary, auxiliary_value(second_auxiliary)?),
                (final_auxiliary, auxiliary_value(final_auxiliary)?),
            ];
            if auxiliary_values.into_iter().any(|(row, value)| {
                value.is_some_and(|value| row.variable_type == 5 && value.abs() > EPS_AXIS_ZERO)
            }) {
                return None;
            }
            let distance_equality = scalar_equality_values
                .get(&(distance.variable_type, distance.key))
                .copied()
                .unwrap_or(Ok(None))
                .ok()?;
            let distance_value =
                reconcile_equation_value(distance.value, distance_equality).ok()?;
            let first = coordinates
                .get(&first_u.key)
                .and_then(|point| Some([point[0]?, point[1]?]))?;
            let second = coordinates
                .get(&second_u.key)
                .and_then(|point| Some([point[0]?, point[1]?]))?;
            let deltas = [
                (second[0] - first[0]).abs(),
                (second[1] - first[1]).abs(),
            ];
            if !deltas.into_iter().all(f64::is_finite) {
                return None;
            }
            let matches_distance = |value: f64| {
                deltas.iter().enumerate().filter_map(move |(coordinate, delta)| {
                    let scale = value.abs().max(delta.abs()).max(1.0);
                    ((*delta - value).abs() <= EPS_AXIS_DISTANCE * scale)
                        .then_some((coordinate, *delta))
                })
            };
            let (coordinate, value) = if let Some(stored) = distance_value {
                if !stored.is_finite() || stored < 0.0 {
                    return None;
                }
                let mut matches = matches_distance(stored);
                let value = matches.next()?;
                matches.next().is_none().then_some(value)?
            } else {
                let mut nonzero = deltas
                    .iter()
                    .enumerate()
                    .filter_map(|(coordinate, delta)| {
                        (*delta > EPS_AXIS_ZERO).then_some((coordinate, *delta))
                    });
                let value = nonzero.next()?;
                nonzero.next().is_none().then_some(value)?
            };
            Some(SectionFunctionFortyThreeAxisDistance {
                first: first_u.key,
                second: second_u.key,
                coordinate,
                scalar: (distance.variable_type, distance.key),
                value,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !section_solver_equation_is_disabled(definition, equation.equation_id),
            })
        })
        .collect()
}
