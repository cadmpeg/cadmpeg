// SPDX-License-Identifier: Apache-2.0
//! Section-equation scalar constraints, seeds, and resolved scalar values.

use super::axis::SectionAxis;

use crate::feature::definitions::{FeatureVariableRow, VariableType};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::scalar::{Angle, NonNegativeLength, PositiveLength};
use std::collections::{BTreeMap, BTreeSet};

use super::super::feature_history::dimensions::{
    feature_dimension_table_complete, feature_relation_table_complete,
};
use super::coordinates::resolved_section_coordinates;
use super::equations_coordinate::{
    approximately_equal, section_equation_function_six_distance_values,
    section_equation_radius_dimensions, section_equation_unsigned_coordinate_distances,
    SectionCoordinateEquation, SectionCoordinateVariable,
};
use crate::decode::sketch_transfer::solver_links::{EquationIncidences, RelationIncidences};

const EPS_RADIAL_VALUE: f64 = 1.0e-9;
const EPS_RADIAL_ANGLE: f64 = 1.0e-9;
const EPS_RADIAL_ZERO: f64 = 1.0e-12;
const EPS_AXIS_DISTANCE: f64 = 1.0e-9;
const EPS_AXIS_ZERO: f64 = 1.0e-12;
const EPS_SCALAR_EQUALITY: f64 = 1.0e-9;

pub(super) fn section_equation_coordinate_equalities(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<(u32, u32, SectionAxis)>, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "creo section equation coordinate equalities scratch")?;
    let source_rows = scratch.with_storage(|| {
        section_equation_coordinate_equality_rows(ctx, definition, ambiguous_point_ids)
    })?;
    ctx.collect_vec(
        ctx.admit_iter(&source_rows, "creo section projected equation rows")?
            .copied()
            .filter(|constraint| constraint.active)
            .map(|constraint| (constraint.first, constraint.second, constraint.axis)),
        "creo section coordinate equalities",
    )
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionEquationCoordinateEquality {
    pub(in crate::decode) first: u32,
    pub(in crate::decode) second: u32,
    pub(in crate::decode) axis: SectionAxis,
    pub(in crate::decode) function_id: u32,
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
    pub(in crate::decode) active: bool,
}

fn reconciled_scalar_row(
    ctx: &DecodeContext<'_>,
    row: &crate::feature::definitions::FeatureVariableRow,
    equalities: &BTreeMap<SectionScalarVariable, Result<Option<f64>, ()>>,
) -> Result<Result<Option<f64>, ()>, CodecError> {
    let equality = ctx
        .get_btree_map(
            equalities,
            &(row.variable_type, row.key),
            "creo section scalar equality lookup",
        )?
        .copied()
        .unwrap_or(Ok(None));
    Ok(equality.and_then(|value| reconcile_equation_value(row.value.value(), value)))
}

fn section_equation_function_ten_axis_alignment(
    ctx: &DecodeContext<'_>,
    equation: &crate::feature::definitions::FeatureEquation,
    variables: &crate::feature::definitions::FeatureVariableTable,
    ambiguous_point_ids: &BTreeSet<u32>,
    scalar_equality_values: &BTreeMap<SectionScalarVariable, Result<Option<f64>, ()>>,
    points: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Result<Option<(u32, u32, SectionAxis)>, CodecError> {
    if equation.function_id != 10 || equation.arguments.len() != 7 {
        return Ok(None);
    }
    let [
        // Coordinate and scalar argument lanes.
        Some(first_axis),
        Some(second_axis),
        Some(target_axis),
        Some(first_auxiliary),
        Some(first_ordinate),
        Some(second_ordinate),
        Some(second_auxiliary),
    ] =
        equation.arguments.as_slice()
    else {
        return Ok(None);
    };
    let row = |ordinal: u32| {
        usize::try_from(ordinal)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    let (
        Some(first_axis),
        Some(second_axis),
        Some(target_axis),
        Some(first_auxiliary),
        Some(first_ordinate),
        Some(second_ordinate),
        Some(second_auxiliary),
    ) = (
        row(*first_axis),
        row(*second_axis),
        row(*target_axis),
        row(*first_auxiliary),
        row(*first_ordinate),
        row(*second_ordinate),
        row(*second_auxiliary),
    )
    else {
        return Ok(None);
    };
    if !matches!(first_axis.variable_type, VariableType::U | VariableType::V)
        || first_axis.variable_type != second_axis.variable_type
        || first_axis.variable_type != target_axis.variable_type
        || !matches!(
            first_ordinate.variable_type,
            VariableType::U | VariableType::V
        )
        || first_ordinate.variable_type == first_axis.variable_type
        || first_ordinate.variable_type != second_ordinate.variable_type
        || first_axis.key != first_ordinate.key
        || second_axis.key != second_ordinate.key
        || first_axis.key == second_axis.key
        || target_axis.key == first_axis.key
        || target_axis.key == second_axis.key
        || first_auxiliary.variable_type != VariableType::Auxiliary
        || second_auxiliary.variable_type != VariableType::Auxiliary
        || ctx.contains_btree_set(
            ambiguous_point_ids,
            &first_axis.key,
            "creo section ambiguous point lookup",
        )?
        || ctx.contains_btree_set(
            ambiguous_point_ids,
            &second_axis.key,
            "creo section ambiguous point lookup",
        )?
        || ctx.contains_btree_set(
            ambiguous_point_ids,
            &target_axis.key,
            "creo section ambiguous point lookup",
        )?
    {
        return Ok(None);
    }

    let auxiliary_is_zero =
        |row: &FeatureVariableRow| -> Result<bool, CodecError> {
            Ok(reconciled_scalar_row(ctx, row, scalar_equality_values)? == Ok(Some(0.0)))
        };
    if !auxiliary_is_zero(first_auxiliary)? || !auxiliary_is_zero(second_auxiliary)? {
        return Ok(None);
    }

    let Some(first_point) = ctx
        .get_btree_map(points, &first_axis.key, "creo function ten point lookup")?
        .copied()
    else {
        return Ok(None);
    };
    let Some(second_point) = ctx
        .get_btree_map(points, &second_axis.key, "creo function ten point lookup")?
        .copied()
    else {
        return Ok(None);
    };
    let Some(target_point) = ctx
        .get_btree_map(points, &target_axis.key, "creo function ten point lookup")?
        .copied()
    else {
        return Ok(None);
    };
    let Some(axis) = SectionAxis::from_variable(first_axis.variable_type) else {
        return Ok(None);
    };
    let constant_axis = axis.other();
    let first_varying = first_point[axis.index()];
    let second_varying = second_point[axis.index()];
    let first_constant = first_point[constant_axis.index()];
    let second_constant = second_point[constant_axis.index()];
    let (Some(first_varying), Some(second_varying), Some(first_constant), Some(second_constant)) = (
        first_varying,
        second_varying,
        first_constant,
        second_constant,
    ) else {
        return Ok(None);
    };
    if ![
        first_varying,
        second_varying,
        first_constant,
        second_constant,
    ]
    .into_iter()
    .all(f64::is_finite)
        || (FiniteReal::new(first_varying))
            .zip(FiniteReal::new(second_varying))
            .is_some_and(|(first, second)| approximately_equal(first, second))
        || !(FiniteReal::new(first_constant))
            .zip(FiniteReal::new(second_constant))
            .is_some_and(|(first, second)| approximately_equal(first, second))
        || target_point[axis.index()].is_none()
        || target_point[constant_axis.index()].is_some()
    {
        return Ok(None);
    }
    Ok(Some((target_axis.key, first_axis.key, constant_axis)))
}

pub(in crate::decode) fn section_equation_coordinate_equality_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionEquationCoordinateEquality>, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "creo section equation coordinate equality rows scratch")?;
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
    let mut function_ten_points = None;
    let mut rows = Vec::new();
    let equation_solver_rows = ctx.admit_iter(&equations.rows, "creo section source equation rows")?;
    let equation_solver = EquationIncidences::new(ctx, definition)?;
    for equation in equation_solver_rows {
        if equation.function_id == 10 {
            if function_ten_points.is_none() {
                function_ten_points = Some(
                    scratch
                        .with_storage(|| variables.reconciled_points(ctx))?
                        .points,
                );
            }
            let Some(points) = function_ten_points.as_ref() else {
                continue;
            };
            let Some((first, second, axis)) = section_equation_function_ten_axis_alignment(
                ctx,
                equation,
                variables,
                ambiguous_point_ids,
                &scalar_equality_values,
                points,
            )?
            else {
                continue;
            };
            ctx.push_vec(
                &mut rows,
                SectionEquationCoordinateEquality {
                    first,
                    second,
                    axis,
                    function_id: equation.function_id,
                    equation_id: equation.equation_id,
                    offset: equation.offset,
                    active: !equation_solver.is_disabled(equation.equation_id),
                },
                "creo section equation coordinate equality rows",
            )?;
            continue;
        }
        let (first, second, auxiliary) = match equation.function_id {
            2 if equation.arguments.len() == 2 => {
                let [Some(first), Some(second)] = equation.arguments.as_slice() else {
                    continue;
                };
                (*first, *second, None)
            }
            13 if equation.arguments.len() == 3 => {
                let [Some(first), Some(second), Some(auxiliary)] = equation.arguments.as_slice()
                else {
                    continue;
                };
                (*first, *second, Some(*auxiliary))
            }
            _ => continue,
        };
        let Some(first) = usize::try_from(first)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(second) = usize::try_from(second)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        if let Some(auxiliary) = auxiliary {
            let Some(auxiliary) = usize::try_from(auxiliary)
                .ok()
                .and_then(|ordinal| variables.rows.get(ordinal))
            else {
                continue;
            };
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
            if auxiliary.variable_type != VariableType::Auxiliary
                || reconcile_equation_value(auxiliary.value.value(), equality_value)
                    != Ok(Some(0.0))
            {
                continue;
            }
        }
        if first.variable_type != second.variable_type
            || !matches!(first.variable_type, VariableType::U | VariableType::V)
            || auxiliary.is_some() && first.variable_type != VariableType::V
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
        let Some(axis) = SectionAxis::from_variable(first.variable_type) else {
            continue;
        };
        ctx.push_vec(
            &mut rows,
            SectionEquationCoordinateEquality {
                first: first.key,
                second: second.key,
                axis,
                function_id: equation.function_id,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !equation_solver.is_disabled(equation.equation_id),
            },
            "creo section equation coordinate equality rows",
        )?;
    }
    Ok(rows)
}

pub(in crate::decode) type SectionScalarVariable = (VariableType, u32);

#[derive(Clone, Copy)]
pub(in crate::decode::sketch) struct SectionEquationMidpointConstraint {
    first: SectionCoordinateVariable,
    second: SectionCoordinateVariable,
    result: SectionScalarVariable,
}

#[cfg(test)]
impl SectionEquationMidpointConstraint {
    pub(in crate::decode::sketch) fn new_for_test(
        first: SectionCoordinateVariable,
        second: SectionCoordinateVariable,
        result: SectionScalarVariable,
    ) -> Self {
        Self {
            first,
            second,
            result,
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::decode::sketch) struct SectionEquationPointBinding {
    point: u32,
    coordinates: [SectionScalarVariable; 2],
}

#[derive(Default)]
pub(super) struct SectionEquationAuxiliaryConstraints {
    pub(super) midpoints: Vec<SectionEquationMidpointConstraint>,
    pub(super) point_bindings: Vec<SectionEquationPointBinding>,
}

pub(super) fn section_equation_auxiliary_constraints(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<SectionEquationAuxiliaryConstraints, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "creo section equation auxiliary constraints scratch")?;
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(SectionEquationAuxiliaryConstraints::default());
    };
    let Some(equations) = scratch.with_storage(|| {
        crate::feature::definitions::equation_table(ctx, &definition.body, 0, definition.body.len())
    })?
    else {
        return Ok(SectionEquationAuxiliaryConstraints::default());
    };
    let Some(declared_count) = usize::try_from(equations.declared_count).ok() else {
        return Ok(SectionEquationAuxiliaryConstraints::default());
    };
    if declared_count != equations.rows.len() + 1 {
        return Ok(SectionEquationAuxiliaryConstraints::default());
    }

    let row = |ordinal: Option<u32>| {
        usize::try_from(ordinal?)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    let mut constraints = SectionEquationAuxiliaryConstraints::default();
    let equation_solver_rows = ctx
        .admit_iter(&equations.rows, "creo auxiliary constraint equations")?;
    let equation_solver = EquationIncidences::new(ctx, definition)?;
    for equation in equation_solver_rows
        .filter(|equation| !equation_solver.is_disabled(equation.equation_id))
    {
        match (equation.function_id, equation.arguments.as_slice()) {
            (42, [Some(first), Some(second), Some(result)]) => {
                let (Some(first), Some(second), Some(result)) =
                    (row(Some(*first)), row(Some(*second)), row(Some(*result)))
                else {
                    continue;
                };
                if first.variable_type != second.variable_type
                    || !matches!(first.variable_type, VariableType::U | VariableType::V)
                    || result.variable_type != VariableType::Result
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
                {
                    continue;
                }
                let Some(coordinate) = SectionAxis::from_variable(first.variable_type) else {
                    continue;
                };
                ctx.reserve_vec(
                    &mut constraints.midpoints,
                    1,
                    "creo section equation midpoint constraints",
                )?;
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
                if first_u.variable_type != VariableType::U
                    || first_v.variable_type != VariableType::V
                    || first_u.key != first_v.key
                    || second_u.variable_type != VariableType::Result
                    || second_v.variable_type != VariableType::Result
                    || second_u.key == second_v.key
                    || ctx.contains_btree_set(
                        ambiguous_point_ids,
                        &first_u.key,
                        "creo section ambiguous point ids contains",
                    )?
                {
                    continue;
                }
                ctx.reserve_vec(
                    &mut constraints.point_bindings,
                    1,
                    "creo section equation point bindings",
                )?;
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
    Ok(constraints)
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionFunctionFortyTwoMidpointCoordinate {
    pub(in crate::decode) first: u32,
    pub(in crate::decode) second: u32,
    pub(in crate::decode) coordinate: SectionAxis,
    pub(in crate::decode) value: Option<f64>,
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
    pub(in crate::decode) active: bool,
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionFunctionThirtyOnePointCoordinates {
    pub(in crate::decode) point: u32,
    pub(in crate::decode) values: [Option<f64>; 2],
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
    pub(in crate::decode) active: bool,
}

pub(super) fn reconcile_equation_value(
    stored: Option<f64>,
    solved: Option<f64>,
) -> Result<Option<f64>, ()> {
    if stored.is_some_and(|value| !value.is_finite())
        || solved.is_some_and(|value| !value.is_finite())
    {
        return Err(());
    }
    match (stored, solved) {
        (Some(stored), Some(solved))
            if !(FiniteReal::new(stored))
                .zip(FiniteReal::new(solved))
                .is_some_and(|(first, second)| approximately_equal(first, second)) =>
        {
            Err(())
        }
        (Some(stored), _) => Ok(Some(stored)),
        (_, Some(solved)) => Ok(Some(solved)),
        (None, None) => Ok(None),
    }
}

pub(in crate::decode) fn section_equation_function_forty_two_midpoint_coordinate_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionFunctionFortyTwoMidpointCoordinate>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation function forty two midpoint coordinate rows scratch",
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
        let [Some(first), Some(second), Some(result)] = equation.arguments.as_slice() else {
            continue;
        };
        if equation.function_id != 42 {
            continue;
        }
        let (Some(first), Some(second), Some(result)) =
            (row(Some(*first)), row(Some(*second)), row(Some(*result)))
        else {
            continue;
        };
        if first.variable_type != second.variable_type
            || !matches!(first.variable_type, VariableType::U | VariableType::V)
            || result.variable_type != VariableType::Result
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
        {
            continue;
        }
        let Some(coordinate) = SectionAxis::from_variable(first.variable_type) else {
            continue;
        };
        let solved = ctx
            .get_btree_map(coordinates, &first.key, "creo section coordinates get")?
            .and_then(|point| point[coordinate.index()])
            .zip(
                ctx.get_btree_map(coordinates, &second.key, "creo section coordinates get")?
                    .and_then(|point| point[coordinate.index()]),
            )
            .map(|(first, second)| f64::midpoint(first, second));
        let result_variable = (result.variable_type, result.key);
        let Ok(equality_value) = ctx
            .get_btree_map(
                &scalar_equality_values,
                &result_variable,
                "creo section scalar equality values get",
            )?
            .copied()
            .unwrap_or(Ok(None))
        else {
            continue;
        };
        let Ok(stored) = reconcile_equation_value(result.value.value(), equality_value) else {
            continue;
        };
        let Ok(value) = reconcile_equation_value(stored, solved) else {
            continue;
        };
        ctx.push_vec(
            &mut rows,
            SectionFunctionFortyTwoMidpointCoordinate {
                first: first.key,
                second: second.key,
                coordinate,
                value,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !equation_solver.is_disabled(equation.equation_id),
            },
            "creo section equation function forty two midpoint coordinate rows",
        )?;
    }
    Ok(rows)
}

pub(in crate::decode) fn section_equation_function_thirty_one_point_coordinate_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionFunctionThirtyOnePointCoordinates>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation function thirty one point coordinate rows scratch",
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
        let [Some(first_u), Some(first_v), Some(second_u), Some(second_v)] =
            equation.arguments.as_slice()
        else {
            continue;
        };
        if equation.function_id != 31 {
            continue;
        }
        let (Some(first_u), Some(first_v), Some(second_u), Some(second_v)) = (
            row(Some(*first_u)),
            row(Some(*first_v)),
            row(Some(*second_u)),
            row(Some(*second_v)),
        ) else {
            continue;
        };
        if first_u.variable_type != VariableType::U
            || first_v.variable_type != VariableType::V
            || first_u.key != first_v.key
            || second_u.variable_type != VariableType::Result
            || second_v.variable_type != VariableType::Result
            || second_u.key == second_v.key
            || ctx.contains_btree_set(
                ambiguous_point_ids,
                &first_u.key,
                "creo section ambiguous point ids contains",
            )?
        {
            continue;
        }
        let point = ctx
            .get_btree_map(coordinates, &first_u.key, "creo section coordinates get")?
            .copied()
            .unwrap_or([None; 2]);
        let Ok(u_equality) = ctx
            .get_btree_map(
                &scalar_equality_values,
                &(second_u.variable_type, second_u.key),
                "creo section scalar equality values get",
            )?
            .copied()
            .unwrap_or(Ok(None))
        else {
            continue;
        };
        let Ok(v_equality) = ctx
            .get_btree_map(
                &scalar_equality_values,
                &(second_v.variable_type, second_v.key),
                "creo section scalar equality values get",
            )?
            .copied()
            .unwrap_or(Ok(None))
        else {
            continue;
        };
        let Ok(u_stored) = reconcile_equation_value(second_u.value.value(), u_equality) else {
            continue;
        };
        let Ok(v_stored) = reconcile_equation_value(second_v.value.value(), v_equality) else {
            continue;
        };
        let Ok(u) = reconcile_equation_value(u_stored, point[0]) else {
            continue;
        };
        let Ok(v) = reconcile_equation_value(v_stored, point[1]) else {
            continue;
        };
        let values = [u, v];
        ctx.push_vec(
            &mut rows,
            SectionFunctionThirtyOnePointCoordinates {
                point: first_u.key,
                values,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !equation_solver.is_disabled(equation.equation_id),
            },
            "creo section equation function thirty one point coordinate rows",
        )?;
    }
    Ok(rows)
}

pub(super) fn merge_scalar_value_candidate(
    ctx: &DecodeContext<'_>,
    values: &mut BTreeMap<SectionScalarVariable, Option<f64>>,
    variable: SectionScalarVariable,
    value: f64,
) -> Result<bool, CodecError> {
    if !value.is_finite() {
        return Ok(false);
    }
    match ctx.entry_btree_map(values, variable, "creo section scalar value nodes")? {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(Some(value));
            Ok(true)
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            let Some(stored) = *entry.get() else {
                return Ok(false);
            };
            if FiniteReal::new(stored)
                .zip(FiniteReal::new(value))
                .is_some_and(|(first, second)| approximately_equal(first, second))
            {
                Ok(false)
            } else {
                *entry.get_mut() = None;
                Ok(true)
            }
        }
    }
}

pub(super) fn section_relation_radius_scalar_values(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<(SectionScalarVariable, f64)>, CodecError> {
    let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|table| feature_dimension_table_complete(table))
    else {
        return Ok(Vec::new());
    };
    let Some(table) = definition
        .relations
        .as_ref()
        .filter(|table| feature_relation_table_complete(table))
    else {
        return Ok(Vec::new());
    };
    let mut values = Vec::new();
    let relation_solver_rows = ctx.admit_iter(&table.rows, "creo scalar radius relation rows")?;
    let relation_solver = RelationIncidences::new(ctx, definition)?;
    for relation in relation_solver_rows {
        if relation_solver.is_disabled(relation.relation_id)
            || relation.relation_type != 14
            || relation.sign != 1
        {
            continue;
        }
        let Some(vectors) = relation.operand_vectors else {
            continue;
        };
        let [Some(radius), Some(0), Some(0), Some(0)] = vectors[0] else {
            continue;
        };
        if vectors[1] != [Some(0); 4] || vectors[2] != [Some(15), Some(0), Some(0), Some(0)] {
            continue;
        }
        let Some(ordinal) = usize::try_from(relation.dimension_id).ok() else {
            continue;
        };
        let Some(dimension) = dimensions.rows.get(ordinal) else {
            continue;
        };
        if !matches!(dimension.dimension_type, 1..=5) {
            continue;
        }
        let Some(value) = dimension
            .value
            .resolved()
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            continue;
        };
        let value = if dimension.dimension_type == 4 {
            value / 2.0
        } else {
            value
        };
        if let Some(value) = PositiveLength::new(value) {
            ctx.push_vec(
                &mut values,
                ((VariableType::Radius, radius), value.get()),
                "creo section relation radius values",
            )?;
        }
    }
    Ok(values)
}

pub(super) fn section_equation_scalar_seed_values(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeMap<SectionScalarVariable, Option<f64>>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo section equation scalar seed values scratch")?;
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(BTreeMap::new());
    };
    let ambiguous_point_ids = scratch
        .with_storage(|| variables.reconciled_points(ctx))?
        .ambiguous;
    let mut values = BTreeMap::new();
    for row in ctx.admit_iter(&variables.rows, "creo scalar seed variable rows")? {
        if matches!(row.variable_type, VariableType::U | VariableType::V) {
            continue;
        }
        let variable = (row.variable_type, row.key);
        match row.value.value() {
            Some(value) if value.is_finite() => {
                merge_scalar_value_candidate(ctx, &mut values, variable, value)?;
            }
            Some(_) => {
                ctx.insert_btree_map(
                    &mut values,
                    variable,
                    None,
                    "creo section scalar seed nodes",
                )?;
            }
            None => {}
        }
    }
    let equalities =
        scratch.with_storage(|| section_equation_scalar_equalities(ctx, definition))?;
    for (&variable, &value) in ctx.admit_iter(&equalities, "creo scalar seed equalities")? {
        merge_scalar_value_candidate(ctx, &mut values, variable, value)?;
    }
    let coordinate_distances = scratch.with_storage(|| {
        section_equation_unsigned_coordinate_distances(ctx, definition, &ambiguous_point_ids)
    })?;
    for constraint in ctx.admit_iter(
        &coordinate_distances,
        "creo scalar seed coordinate distances",
    )? {
        merge_scalar_value_candidate(ctx, &mut values, constraint.scalar, constraint.value)?;
    }
    let radius_dimensions =
        scratch.with_storage(|| section_equation_radius_dimensions(ctx, definition))?;
    for constraint in ctx
        .admit_iter(&radius_dimensions, "creo scalar seed radius dimensions")?
        .filter(|constraint| constraint.active)
    {
        merge_scalar_value_candidate(
            ctx,
            &mut values,
            constraint.radius_variable,
            constraint.value.get(),
        )?;
        merge_scalar_value_candidate(ctx, &mut values, constraint.scalar, constraint.value.get())?;
    }
    let relation_radius_values =
        scratch.with_storage(|| section_relation_radius_scalar_values(ctx, definition))?;
    for &(variable, value) in
        ctx.admit_iter(&relation_radius_values, "creo scalar seed relation radii")?
    {
        merge_scalar_value_candidate(ctx, &mut values, variable, value)?;
    }
    let angle_differences = scratch.with_storage(|| {
        section_equation_function_sixteen_angle_difference_values(ctx, definition)
    })?;
    for &(variable, value) in
        ctx.admit_iter(&angle_differences, "creo scalar seed angle differences")?
    {
        merge_scalar_value_candidate(ctx, &mut values, variable, value)?;
    }
    Ok(values)
}

pub(super) fn propagate_section_equation_scalar_equality_values(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    values: &mut BTreeMap<SectionScalarVariable, Option<f64>>,
) -> Result<bool, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo propagate section equation scalar equality values scratch",
    )?;
    let Some(_variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(false);
    };
    let scalar_equality_values =
        scratch.with_storage(|| section_equation_scalar_equality_values(ctx, definition))?;
    let components =
        scratch.with_storage(|| section_equation_scalar_equality_components(ctx, definition))?;
    let mut changed = false;
    for component in ctx.admit_iter(&components, "creo scalar equality components")? {
        let mut component_value = None;
        let mut conflicting = false;
        let mut members = component.iter();
        while members.len() != 0 {
            let Some(variable) =
                ctx.next_charged(&mut members, "creo scalar equality component variables")?
            else {
                break;
            };
            let mut variable_value = match ctx
                .get_btree_map(
                    &scalar_equality_values,
                    variable,
                    "creo section scalar equality values get",
                )?
                .copied()
            {
                Some(Err(())) => {
                    conflicting = true;
                    break;
                }
                Some(Ok(value)) => value,
                None => None,
            };
            match ctx.get_btree_map(values, variable, "creo section values get")? {
                Some(Some(value)) if value.is_finite() => {
                    if variable_value.is_some_and(|stored| {
                        !(FiniteReal::new(stored))
                            .zip(FiniteReal::new(*value))
                            .is_some_and(|(first, second)| approximately_equal(first, second))
                    }) {
                        conflicting = true;
                        break;
                    }
                    variable_value = Some(*value);
                }
                Some(Some(_) | None) => {
                    conflicting = true;
                    break;
                }
                None => {}
            }
            if let Some(value) = variable_value {
                if component_value.is_some_and(|stored| {
                    !(FiniteReal::new(stored))
                        .zip(FiniteReal::new(value))
                        .is_some_and(|(first, second)| approximately_equal(first, second))
                }) {
                    conflicting = true;
                    break;
                }
                component_value = Some(value);
            }
        }
        if conflicting {
            for &variable in ctx.admit_iter(component, "creo conflicting scalar component nodes")? {
                if ctx.get_btree_map(values, &variable, "creo section values get")? != Some(&None) {
                    ctx.insert_btree_map(values, variable, None, "creo propagated scalar nodes")?;
                    changed = true;
                }
            }
        } else if let Some(value) = component_value {
            for &variable in ctx.admit_iter(component, "creo propagated scalar component nodes")? {
                if ctx.get_btree_map(values, &variable, "creo section values get")?
                    != Some(&Some(value))
                {
                    ctx.insert_btree_map(
                        values,
                        variable,
                        Some(value),
                        "creo propagated scalar nodes",
                    )?;
                    changed = true;
                }
            }
        }
    }
    Ok(changed)
}

pub(super) fn append_section_equation_auxiliary_coordinate_constraints(
    ctx: &DecodeContext<'_>,
    constraints: &SectionEquationAuxiliaryConstraints,
    scalar_values: &BTreeMap<SectionScalarVariable, Option<f64>>,
    stored_coordinates: &BTreeMap<SectionCoordinateVariable, f64>,
    equations: &mut Vec<SectionCoordinateEquation>,
) -> Result<(), CodecError> {
    for constraint in ctx.admit_iter(
        &constraints.midpoints,
        "creo auxiliary midpoint constraints",
    )? {
        let Some(Some(value)) = ctx.get_btree_map(
            scalar_values,
            &constraint.result,
            "creo section scalar values get",
        )?
        else {
            continue;
        };
        if ctx
            .get_btree_map(
                stored_coordinates,
                &constraint.first,
                "creo section stored coordinates get",
            )?
            .zip(ctx.get_btree_map(
                stored_coordinates,
                &constraint.second,
                "creo section stored coordinates get",
            )?)
            .is_some_and(|(first, second)| {
                !(FiniteReal::new(f64::midpoint(*first, *second)))
                    .zip(FiniteReal::new(*value))
                    .is_some_and(|(first, second)| approximately_equal(first, second))
            })
        {
            continue;
        }
        let Some(rhs) = FiniteReal::new(2.0 * value) else {
            continue;
        };
        let mut equation = SectionCoordinateEquation::default();
        equation.add_point(ctx, constraint.first.0, constraint.first.1, 1.0)?;
        equation.add_point(ctx, constraint.second.0, constraint.second.1, 1.0)?;
        equation.rhs = rhs.get();
        ctx.reserve_vec(equations, 1, "creo auxiliary coordinate equations")?;
        equations.push(equation);
    }
    for constraint in
        ctx.admit_iter(&constraints.point_bindings, "creo auxiliary point bindings")?
    {
        let mut values = [None; 2];
        let mut underdetermined = false;
        let mut invalid = false;
        for (coordinate, variable) in SectionAxis::ALL.into_iter().zip(constraint.coordinates) {
            match ctx.get_btree_map(scalar_values, &variable, "creo section scalar values get")? {
                Some(Some(value)) => {
                    if ctx
                        .get_btree_map(
                            stored_coordinates,
                            &(constraint.point, coordinate),
                            "creo section stored coordinates get",
                        )?
                        .is_some_and(|stored| {
                            !(FiniteReal::new(*stored))
                                .zip(FiniteReal::new(*value))
                                .is_some_and(|(first, second)| approximately_equal(first, second))
                        })
                    {
                        invalid = true;
                        break;
                    }
                    values[coordinate.index()] = Some(*value);
                }
                Some(None) => {
                    invalid = true;
                    break;
                }
                None => {
                    underdetermined |= !ctx.contains_key_btree_map(
                        stored_coordinates,
                        &(constraint.point, coordinate),
                        "creo section stored coordinates contains_key",
                    )?;
                }
            }
        }
        if invalid || underdetermined {
            continue;
        }
        for (coordinate, value) in SectionAxis::ALL
            .into_iter()
            .zip(values)
            .filter_map(|(coordinate, value)| Some((coordinate, value?)))
        {
            ctx.reserve_vec(equations, 1, "creo auxiliary coordinate equations")?;
            equations.push(SectionCoordinateEquation::point_value(
                ctx,
                constraint.point,
                coordinate,
                value,
            )?);
        }
    }
    Ok(())
}

pub(super) fn section_equation_scalar_values_from_coordinates(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Result<BTreeMap<SectionScalarVariable, f64>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation scalar values from coordinates scratch",
    )?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .map(|variables| {
            scratch
                .with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let constraints = scratch.with_storage(|| {
        section_equation_auxiliary_constraints(ctx, definition, &ambiguous_point_ids)
    })?;
    let seed_values =
        scratch.with_storage(|| section_equation_scalar_seed_values(ctx, definition))?;
    let mut derived = BTreeMap::<SectionScalarVariable, Option<f64>>::new();
    let compatible = |variable: SectionScalarVariable, value: f64| -> Result<bool, CodecError> {
        Ok(
            match ctx.get_btree_map(&seed_values, &variable, "creo section seed value lookup")? {
                None => true,
                Some(stored) => stored.is_some_and(|stored| {
                    FiniteReal::new(stored)
                        .zip(FiniteReal::new(value))
                        .is_some_and(|(first, second)| approximately_equal(first, second))
                }),
            },
        )
    };
    for constraint in ctx.admit_iter(&constraints.midpoints, "creo derived midpoint constraints")? {
        let (Some(Some(first)), Some(Some(second))) = (
            ctx.get_btree_map(
                coordinates,
                &constraint.first.0,
                "creo section coordinates get",
            )?
            .map(|point| point[constraint.first.1.index()]),
            ctx.get_btree_map(
                coordinates,
                &constraint.second.0,
                "creo section coordinates get",
            )?
            .map(|point| point[constraint.second.1.index()]),
        ) else {
            continue;
        };
        let value = f64::midpoint(first, second);
        if compatible(constraint.result, value)? {
            scratch.with_storage(|| {
                merge_scalar_value_candidate(ctx, &mut derived, constraint.result, value)
            })?;
        }
    }
    for constraint in ctx.admit_iter(&constraints.point_bindings, "creo derived point bindings")? {
        let Some(point) = ctx.get_btree_map(
            coordinates,
            &constraint.point,
            "creo section coordinates get",
        )?
        else {
            continue;
        };
        let mut invalid = false;
        let mut candidates = [None; 2];
        let mut candidate_count = 0;
        for (coordinate, variable) in SectionAxis::ALL.into_iter().zip(constraint.coordinates) {
            let Some(value) = point[coordinate.index()] else {
                continue;
            };
            if !compatible(variable, value)? {
                invalid = true;
                break;
            }
            if !ctx.contains_key_btree_map(
                &seed_values,
                &variable,
                "creo section seed values contains_key",
            )? {
                candidates[candidate_count] = Some((variable, value));
                candidate_count += 1;
            }
        }
        if !invalid {
            for candidate in &candidates[..candidate_count] {
                let Some(candidate) = candidate else {
                    continue;
                };
                let (variable, value) = *candidate;
                scratch.with_storage(|| {
                    merge_scalar_value_candidate(ctx, &mut derived, variable, value)
                })?;
            }
        }
    }
    let function_six_values = scratch.with_storage(|| {
        section_equation_function_six_distance_values(
            ctx,
            definition,
            coordinates,
            &ambiguous_point_ids,
        )
    })?;
    for &(variable, value) in
        ctx.admit_iter(&function_six_values, "creo derived function six distances")?
    {
        scratch
            .with_storage(|| merge_scalar_value_candidate(ctx, &mut derived, variable, value))?;
    }
    let function_forty_three_values = scratch.with_storage(|| {
        section_equation_function_forty_three_axis_distance_values(
            ctx,
            definition,
            coordinates,
            &ambiguous_point_ids,
        )
    })?;
    for &(variable, value) in ctx.admit_iter(
        &function_forty_three_values,
        "creo derived function forty three distances",
    )? {
        scratch
            .with_storage(|| merge_scalar_value_candidate(ctx, &mut derived, variable, value))?;
    }
    let radial_constraints = scratch.with_storage(|| {
        section_equation_radial_constraints(ctx, definition, coordinates, &ambiguous_point_ids)
    })?;
    for constraint in ctx.admit_iter(&radial_constraints, "creo derived radial constraints")? {
        for (variable, value) in [
            (
                constraint.radius,
                constraint.radius_value.map(NonNegativeLength::get),
            ),
            (constraint.angle, constraint.angle_value.map(Angle::get)),
        ] {
            let Some(value) = value else {
                continue;
            };
            scratch.with_storage(|| {
                merge_scalar_value_candidate(ctx, &mut derived, variable, value)
            })?;
        }
    }
    let mut resolved = BTreeMap::new();
    for (&variable, &value) in ctx.admit_iter(&derived, "creo derived scalar candidates")? {
        if let Some(value) = value {
            ctx.insert_btree_map(
                &mut resolved,
                variable,
                value,
                "creo section resolved scalar nodes",
            )?;
        }
    }
    Ok(resolved)
}

fn direct_function_five_scalar_rows<'a>(
    function_id: u32,
    arguments: &[Option<u32>],
    variables: &'a [crate::feature::definitions::FeatureVariableRow],
) -> Option<(
    &'a crate::feature::definitions::FeatureVariableRow,
    &'a crate::feature::definitions::FeatureVariableRow,
    &'a crate::feature::definitions::FeatureVariableRow,
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
    (first.variable_type == VariableType::Result
        && second.variable_type == VariableType::Result
        && selector.variable_type == VariableType::Selector
        && first.key != second.key)
        .then_some((first, second, selector))
}

pub(super) fn section_equation_scalar_equality_components(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<BTreeSet<SectionScalarVariable>>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation scalar equality components scratch",
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

    let mut adjacency = BTreeMap::<SectionScalarVariable, BTreeSet<SectionScalarVariable>>::new();
    let mut deferred_function_five = Vec::<(
        SectionScalarVariable,
        SectionScalarVariable,
        SectionScalarVariable,
        Option<f64>,
    )>::new();
    let equation_solver_rows = ctx
        .admit_iter(&equations.rows, "creo scalar equality source equations")?;
    let equation_solver = EquationIncidences::new(ctx, definition)?;
    for equation in equation_solver_rows
        .filter(|equation| !equation_solver.is_disabled(equation.equation_id))
    {
        if let Some((first, second, selector)) = direct_function_five_scalar_rows(
            equation.function_id,
            &equation.arguments,
            &variables.rows,
        ) {
            scratch.with_storage(|| {
                ctx.reserve_vec(
                    &mut deferred_function_five,
                    1,
                    "creo section deferred scalar equations",
                )
            })?;
            deferred_function_five.push((
                (first.variable_type, first.key),
                (second.variable_type, second.key),
                (selector.variable_type, selector.key),
                selector.value.value(),
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
            || matches!(first.variable_type, VariableType::U | VariableType::V)
            || first.key == second.key
        {
            continue;
        }
        let first = (first.variable_type, first.key);
        let second = (second.variable_type, second.key);
        scratch.with_storage(|| insert_scalar_adjacency(ctx, &mut adjacency, first, second))?;
        scratch.with_storage(|| insert_scalar_adjacency(ctx, &mut adjacency, second, first))?;
    }

    let base_components = scratch.with_storage(|| scalar_equality_components(ctx, &adjacency))?;
    let base_values = scratch.with_storage(|| {
        scalar_equality_values_for_components(ctx, &variables.rows, &base_components)
    })?;
    for &(first, second, selector, stored_selector) in ctx.admit_iter(
        &deferred_function_five,
        "creo deferred scalar equality equations",
    )? {
        let equality_value = ctx
            .get_btree_map(&base_values, &selector, "creo section base values get")?
            .copied()
            .unwrap_or(Ok(None));
        if !matches!(
            equality_value
                .ok()
                .and_then(|value| reconcile_equation_value(stored_selector, value).ok()),
            Some(Some(value)) if value == 0.0
        ) {
            continue;
        }
        scratch.with_storage(|| insert_scalar_adjacency(ctx, &mut adjacency, first, second))?;
        scratch.with_storage(|| insert_scalar_adjacency(ctx, &mut adjacency, second, first))?;
    }
    scalar_equality_components(ctx, &adjacency)
}

fn insert_scalar_adjacency(
    ctx: &DecodeContext<'_>,
    adjacency: &mut BTreeMap<SectionScalarVariable, BTreeSet<SectionScalarVariable>>,
    first: SectionScalarVariable,
    second: SectionScalarVariable,
) -> Result<(), CodecError> {
    let neighbors = ctx
        .entry_btree_map(adjacency, first, "creo section scalar adjacency nodes")?
        .or_default();
    ctx.insert_btree_set(neighbors, second, "creo section scalar adjacency edges")?;
    Ok(())
}

fn scalar_equality_components(
    ctx: &DecodeContext<'_>,
    adjacency: &BTreeMap<SectionScalarVariable, BTreeSet<SectionScalarVariable>>,
) -> Result<Vec<BTreeSet<SectionScalarVariable>>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo scalar equality components scratch")?;
    let mut reached = BTreeSet::new();
    let mut components = Vec::new();
    for (&seed, _) in ctx.admit_iter(adjacency, "creo scalar adjacency nodes")? {
        if !scratch.with_storage(|| {
            ctx.insert_btree_set(&mut reached, seed, "creo section scalar remaining nodes")
        })? {
            continue;
        }
        ctx.charge_work(1, "creo scalar equality components")?;
        let mut component = BTreeSet::new();
        ctx.insert_btree_set(&mut component, seed, "creo section scalar component nodes")?;
        let mut queue_storage = ctx.reserve_scoped(0, "creo scalar pending scratch")?;
        let mut pending = std::collections::VecDeque::new();
        queue_storage.with_storage(|| {
            ctx.push_back(&mut pending, seed, "creo section scalar pending nodes")
        })?;
        while !pending.is_empty() {
            let Some(variable) = ctx
                .next_charged(&mut pending.iter(), "creo scalar equality graph visits")?
                .copied()
            else {
                break;
            };
            pending.pop_front();
            if let Some(neighbors) =
                ctx.get_btree_map(adjacency, &variable, "creo section adjacency get")?
            {
                for &neighbor in ctx.admit_iter(neighbors, "creo scalar adjacency neighbors")? {
                    if ctx.insert_btree_set(
                        &mut component,
                        neighbor,
                        "creo section scalar component nodes",
                    )? {
                        scratch.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut reached,
                                neighbor,
                                "creo section scalar remaining nodes",
                            )
                        })?;
                        queue_storage.with_storage(|| {
                            ctx.push_back(
                                &mut pending,
                                neighbor,
                                "creo section scalar pending nodes",
                            )
                        })?;
                    }
                }
            }
        }
        ctx.reserve_vec(&mut components, 1, "creo section scalar components")?;
        components.push(component);
    }
    Ok(components)
}

/// The first source sample and extrema for scaled agreement.
struct ScalarSamples {
    first: f64,
    minimum: f64,
    maximum: f64,
}

fn scalar_equality_values_for_components(
    ctx: &DecodeContext<'_>,
    rows: &[crate::feature::definitions::FeatureVariableRow],
    components: &[BTreeSet<SectionScalarVariable>],
) -> Result<BTreeMap<SectionScalarVariable, Result<Option<f64>, ()>>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo scalar sample scratch")?;
    let mut samples = BTreeMap::<SectionScalarVariable, Option<ScalarSamples>>::new();
    for row in ctx.admit_iter(rows, "creo scalar equality variable rows")? {
        if matches!(row.variable_type, VariableType::U | VariableType::V) {
            continue;
        }
        let Some(value) = row.value.value() else {
            continue;
        };
        let entry = storage.with_storage(|| {
            ctx.entry_btree_map(
                &mut samples,
                (row.variable_type, row.key),
                "creo section scalar sample nodes",
            )
        })?;
        match entry {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(value.is_finite().then_some(ScalarSamples {
                    first: value,
                    minimum: value,
                    maximum: value,
                }));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if !value.is_finite() {
                    *entry.get_mut() = None;
                } else if let Some(samples) = entry.get_mut() {
                    samples.minimum = samples.minimum.min(value);
                    samples.maximum = samples.maximum.max(value);
                }
            }
        }
    }
    let mut resolved = BTreeMap::new();
    for component in ctx.admit_iter(components, "creo scalar equality value components")? {
        let mut first = None;
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        let mut invalid = false;
        let mut members = component.iter();
        while members.len() != 0 {
            let Some(variable) =
                ctx.next_charged(&mut members, "creo scalar component samples")?
            else {
                break;
            };
            match ctx.get_btree_map(&samples, variable, "creo scalar sample lookup")? {
                Some(Some(samples)) => {
                    first.get_or_insert(samples.first);
                    minimum = minimum.min(samples.minimum);
                    maximum = maximum.max(samples.maximum);
                }
                Some(None) => {
                    invalid = true;
                    break;
                }
                None => {}
            }
        }
        let value = if invalid {
            Err(())
        } else if let Some(first) = first {
            let scale = minimum.abs().max(maximum.abs()).max(1.0);
            let difference = (minimum - first).abs().max((maximum - first).abs());
            crate::vecmath::within(difference, EPS_SCALAR_EQUALITY * scale)
                .then_some(Some(first))
                .ok_or(())
        } else {
            Ok(None)
        };
        for &variable in ctx.admit_iter(component, "creo resolved scalar component members")? {
            ctx.insert_btree_map(
                &mut resolved,
                variable,
                value,
                "creo section reconciled scalar nodes",
            )?;
        }
    }
    Ok(resolved)
}

fn section_equation_scalar_equalities(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeMap<SectionScalarVariable, f64>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo section equation scalar equalities scratch")?;
    let mut equalities = BTreeMap::new();
    let scalar_values =
        scratch.with_storage(|| section_equation_scalar_equality_values(ctx, definition))?;
    for (&variable, &value) in ctx.admit_iter(&scalar_values, "creo scalar equality values")? {
        if let Ok(Some(value)) = value {
            ctx.insert_btree_map(
                &mut equalities,
                variable,
                value,
                "creo section scalar equality nodes",
            )?;
        }
    }
    Ok(equalities)
}

pub(super) fn section_equation_scalar_equality_values(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeMap<SectionScalarVariable, Result<Option<f64>, ()>>, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "creo section equation scalar equality values scratch")?;
    let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(BTreeMap::new());
    };
    let components =
        scratch.with_storage(|| section_equation_scalar_equality_components(ctx, definition))?;
    scalar_equality_values_for_components(ctx, &variables.rows, &components)
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionRadialConstraint {
    pub(in crate::decode) first: u32,
    pub(in crate::decode) second: u32,
    pub(in crate::decode) radius: SectionScalarVariable,
    angle: SectionScalarVariable,
    pub(in crate::decode) radius_value: Option<NonNegativeLength>,
    pub(in crate::decode) angle_value: Option<Angle>,
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
    pub(in crate::decode) active: bool,
}

impl SectionRadialConstraint {
    pub(super) fn offset(self) -> Option<[f64; 2]> {
        let radius = self.radius_value?.get();
        if radius <= EPS_RADIAL_ZERO {
            return Some([0.0; 2]);
        }
        let angle = self.angle_value?.get();
        Some([radius * angle.cos(), radius * angle.sin()])
    }
}

pub(super) fn section_equation_radial_constraints(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionRadialConstraint>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo section equation radial constraints scratch")?;
    let source_rows = scratch.with_storage(|| {
        section_equation_radial_constraint_rows(ctx, definition, coordinates, ambiguous_point_ids)
    })?;
    ctx.collect_vec(
        ctx.admit_iter(&source_rows, "creo section projected equation rows")?
            .copied()
            .filter(|constraint| constraint.active),
        "creo section radial constraints",
    )
}

pub(super) fn section_equation_radial_constraints_with_scalar_values(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
    scalar_values: &BTreeMap<SectionScalarVariable, Option<f64>>,
) -> Result<Vec<SectionRadialConstraint>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation radial constraints with scalar values scratch",
    )?;
    let source_rows = scratch.with_storage(|| {
        section_equation_radial_constraint_rows_with_scalar_values(
            ctx,
            definition,
            coordinates,
            ambiguous_point_ids,
            Some(scalar_values),
        )
    })?;
    ctx.collect_vec(
        ctx.admit_iter(&source_rows, "creo section projected equation rows")?
            .copied()
            .filter(|constraint| constraint.active),
        "creo section radial constraints",
    )
}

pub(in crate::decode) fn section_equation_radial_constraint_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionRadialConstraint>, CodecError> {
    section_equation_radial_constraint_rows_with_scalar_values(
        ctx,
        definition,
        coordinates,
        ambiguous_point_ids,
        None,
    )
}

fn section_equation_radial_constraint_rows_with_scalar_values(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
    scalar_values: Option<&BTreeMap<SectionScalarVariable, Option<f64>>>,
) -> Result<Vec<SectionRadialConstraint>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo radial constraint source scratch")?;
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
        .filter(|equation| equation.function_id == 0 && equation.arguments.len() == 6)
    {
        let [Some(first_u), Some(first_v), Some(second_u), Some(second_v), Some(radius), Some(angle)] =
            equation.arguments.as_slice()
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
        let Some(radius) = usize::try_from(*radius)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        let Some(angle) = usize::try_from(*angle)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
        else {
            continue;
        };
        if first_u.variable_type != VariableType::U
            || first_v.variable_type != VariableType::V
            || second_u.variable_type != VariableType::U
            || second_v.variable_type != VariableType::V
            || first_u.key != first_v.key
            || second_u.key != second_v.key
            || first_u.key == second_u.key
            || !matches!(
                radius.variable_type,
                VariableType::Dimension | VariableType::Radius
            )
            || !matches!(
                angle.variable_type,
                VariableType::Parameter | VariableType::Result
            )
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
        let scalar_value = |row: &FeatureVariableRow| -> Result<Option<Option<f64>>, CodecError> {
                let equality = ctx.get_btree_map(&scalar_equality_values, &(row.variable_type, row.key), "creo section scalar equality lookup")?.copied().unwrap_or(Ok(None));
                let Ok(equality) = equality else { return Ok(None); };
                let Ok(resolved) = reconcile_equation_value(row.value.value(), equality) else { return Ok(None); };
                let Some(scalar_values) = scalar_values else { return Ok(Some(resolved)); };
                let Some(Some(value)) = ctx.get_btree_map(scalar_values, &(row.variable_type, row.key), "creo section scalar lookup")? else { return Ok(Some(resolved)); };
                Ok(reconcile_equation_value(resolved, Some(*value)).ok())
            };
        let Some(radius_value) = scalar_value(radius)? else {
            continue;
        };
        let mut radius_value = match radius_value {
            Some(value) => {
                let Some(value) = NonNegativeLength::new(value) else {
                    continue;
                };
                Some(value)
            }
            None => None,
        };
        let Some(angle_value) = scalar_value(angle)? else {
            continue;
        };
        let mut angle_value = match angle_value {
            Some(value) => {
                let Some(value) = Angle::new(value) else {
                    continue;
                };
                Some(value)
            }
            None => None,
        };
        let active = !equation_solver.is_disabled(equation.equation_id);
        if active {
            let first_point = ctx
                .get_btree_map(coordinates, &first_u.key, "creo section coordinates get")?
                .and_then(|point| Some([point[0]?, point[1]?]));
            let second_point = ctx
                .get_btree_map(coordinates, &second_u.key, "creo section coordinates get")?
                .and_then(|point| Some([point[0]?, point[1]?]));
            if let (Some(first), Some(second)) = (first_point, second_point) {
                if !first.into_iter().chain(second).all(f64::is_finite) {
                    continue;
                }
                let delta = [second[0] - first[0], second[1] - first[1]];
                let Some(distance) = NonNegativeLength::new(delta[0].hypot(delta[1])) else {
                    continue;
                };
                let scale = distance
                    .get()
                    .max(radius_value.map_or(0.0, NonNegativeLength::get))
                    .max(1.0);
                if radius_value.is_some_and(|value| {
                    (value.get() - distance.get()).abs() > EPS_RADIAL_VALUE * scale
                }) {
                    continue;
                }
                radius_value.get_or_insert(distance);
                if distance.get() > EPS_RADIAL_ZERO {
                    let derived_angle = delta[1].atan2(delta[0]);
                    if angle_value.is_some_and(|value| {
                        let difference =
                            (value.get() - derived_angle).rem_euclid(std::f64::consts::TAU);
                        difference.min(std::f64::consts::TAU - difference) > EPS_RADIAL_ANGLE
                    }) {
                        continue;
                    }
                    let Some(angle) = Angle::new(derived_angle) else {
                        continue;
                    };
                    angle_value.get_or_insert(angle);
                }
            }
        }
        ctx.push_vec(
            &mut rows,
            SectionRadialConstraint {
                first: first_u.key,
                second: second_u.key,
                radius: (radius.variable_type, radius.key),
                angle: (angle.variable_type, angle.key),
                radius_value,
                angle_value,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active,
            },
            "creo section equation radial constraint rows with scalar values",
        )?;
    }
    Ok(rows)
}

pub(in crate::decode) fn resolved_section_scalar_values(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeMap<SectionScalarVariable, f64>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo resolved section scalar values scratch")?;
    let coordinates = scratch.with_storage(|| resolved_section_coordinates(ctx, definition))?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .map(|variables| {
            scratch
                .with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let mut values = BTreeMap::<SectionScalarVariable, Option<f64>>::new();
    let scalar_equalities =
        scratch.with_storage(|| section_equation_scalar_equalities(ctx, definition))?;
    for (&variable, &value) in
        ctx.admit_iter(&scalar_equalities, "creo resolved scalar equalities")?
    {
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut values,
                variable,
                Some(value),
                "creo resolved scalar candidate nodes",
            )
        })?;
    }
    let coordinate_values = scratch.with_storage(|| {
        section_equation_scalar_values_from_coordinates(ctx, definition, &coordinates)
    })?;
    for (variable, value) in
        ctx.admit_iter(&coordinate_values, "creo resolved coordinate scalar values")?
    {
        scratch
            .with_storage(|| merge_scalar_value_candidate(ctx, &mut values, *variable, *value))?;
    }
    let function_six_values = scratch.with_storage(|| {
        section_equation_function_six_distance_values(
            ctx,
            definition,
            &coordinates,
            &ambiguous_point_ids,
        )
    })?;
    for &(variable, value) in
        ctx.admit_iter(&function_six_values, "creo resolved function six distances")?
    {
        scratch.with_storage(|| merge_scalar_value_candidate(ctx, &mut values, variable, value))?;
    }
    let function_forty_three_values = scratch.with_storage(|| {
        section_equation_function_forty_three_axis_distance_values(
            ctx,
            definition,
            &coordinates,
            &ambiguous_point_ids,
        )
    })?;
    for &(variable, value) in ctx.admit_iter(
        &function_forty_three_values,
        "creo resolved function forty three distances",
    )? {
        scratch.with_storage(|| merge_scalar_value_candidate(ctx, &mut values, variable, value))?;
    }
    let angle_differences = scratch.with_storage(|| {
        section_equation_function_sixteen_angle_difference_values(ctx, definition)
    })?;
    for &(variable, value) in
        ctx.admit_iter(&angle_differences, "creo resolved angle differences")?
    {
        scratch.with_storage(|| merge_scalar_value_candidate(ctx, &mut values, variable, value))?;
    }
    let coordinate_distances = scratch.with_storage(|| {
        section_equation_unsigned_coordinate_distances(ctx, definition, &ambiguous_point_ids)
    })?;
    for constraint in ctx.admit_iter(&coordinate_distances, "creo resolved coordinate distances")? {
        scratch.with_storage(|| {
            merge_scalar_value_candidate(ctx, &mut values, constraint.scalar, constraint.value)
        })?;
    }
    let radius_dimensions =
        scratch.with_storage(|| section_equation_radius_dimensions(ctx, definition))?;
    for constraint in ctx
        .admit_iter(&radius_dimensions, "creo resolved radius dimensions")?
        .filter(|constraint| constraint.active)
    {
        scratch.with_storage(|| {
            merge_scalar_value_candidate(
                ctx,
                &mut values,
                constraint.radius_variable,
                constraint.value.get(),
            )
        })?;
        scratch.with_storage(|| {
            merge_scalar_value_candidate(
                ctx,
                &mut values,
                constraint.scalar,
                constraint.value.get(),
            )
        })?;
    }
    let relation_radius_values =
        scratch.with_storage(|| section_relation_radius_scalar_values(ctx, definition))?;
    for &(variable, value) in
        ctx.admit_iter(&relation_radius_values, "creo resolved relation radii")?
    {
        scratch.with_storage(|| merge_scalar_value_candidate(ctx, &mut values, variable, value))?;
    }
    let radial_constraints = scratch.with_storage(|| {
        section_equation_radial_constraints(ctx, definition, &coordinates, &ambiguous_point_ids)
    })?;
    for constraint in ctx.admit_iter(&radial_constraints, "creo resolved radial constraints")? {
        for (variable, value) in [
            (
                constraint.radius,
                constraint.radius_value.map(NonNegativeLength::get),
            ),
            (constraint.angle, constraint.angle_value.map(Angle::get)),
        ] {
            let Some(value) = value else {
                continue;
            };
            scratch
                .with_storage(|| merge_scalar_value_candidate(ctx, &mut values, variable, value))?;
        }
    }
    scratch.with_storage(|| {
        propagate_section_equation_scalar_equality_values(ctx, definition, &mut values)
    })?;
    let mut resolved = BTreeMap::new();
    for (&variable, &value) in ctx.admit_iter(&values, "creo resolved scalar values")? {
        if let Some(value) = value {
            ctx.insert_btree_map(
                &mut resolved,
                variable,
                value,
                "creo resolved scalar output nodes",
            )?;
        }
    }
    Ok(resolved)
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionFunctionFiveScalarEquality {
    pub(in crate::decode) first: SectionScalarVariable,
    pub(in crate::decode) second: SectionScalarVariable,
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
}

pub(in crate::decode) fn section_equation_function_five_scalar_equality_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<SectionFunctionFiveScalarEquality>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation function five scalar equality rows scratch",
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
        .filter(|equation| {
            equation.function_id == 5
                && equation.arguments.len() == 3
                && !equation_solver.is_disabled(equation.equation_id)
        })
    {
        let Some((first, second, selector)) = direct_function_five_scalar_rows(
            equation.function_id,
            &equation.arguments,
            &variables.rows,
        ) else {
            continue;
        };
        let Ok(selector_value) = reconciled_scalar_row(ctx, selector, &scalar_equality_values)?
        else {
            continue;
        };
        if selector_value != Some(0.0) {
            continue;
        }
        if reconciled_scalar_row(ctx, first, &scalar_equality_values)?.is_err()
            || reconciled_scalar_row(ctx, second, &scalar_equality_values)?.is_err()
        {
            continue;
        }
        ctx.push_vec(
            &mut rows,
            SectionFunctionFiveScalarEquality {
                first: (first.variable_type, first.key),
                second: (second.variable_type, second.key),
                equation_id: equation.equation_id,
                offset: equation.offset,
            },
            "creo section equation function five scalar equality rows",
        )?;
    }
    Ok(rows)
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionFunctionSixteenAngleDifference {
    pub(in crate::decode) first: SectionScalarVariable,
    pub(in crate::decode) second: SectionScalarVariable,
    pub(in crate::decode) difference: SectionScalarVariable,
    pub(in crate::decode) value: f64,
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
    pub(in crate::decode) active: bool,
}

fn section_equation_function_sixteen_angle_difference_values(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<(SectionScalarVariable, f64)>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation function sixteen angle difference values scratch",
    )?;
    let source_rows = scratch.with_storage(|| {
        section_equation_function_sixteen_angle_difference_rows(ctx, definition)
    })?;
    ctx.collect_vec(
        ctx.admit_iter(&source_rows, "creo section projected equation rows")?
            .copied()
            .filter(|constraint| constraint.active)
            .map(|constraint| (constraint.difference, constraint.value)),
        "creo section angle difference values",
    )
}

pub(in crate::decode) fn section_equation_function_sixteen_angle_difference_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<SectionFunctionSixteenAngleDifference>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation function sixteen angle difference rows scratch",
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
    let row = |ordinal: u32| {
        usize::try_from(ordinal)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    let mut rows = Vec::new();
    let equation_solver_rows = ctx.admit_iter(&equations.rows, "creo section source equation rows")?;
    let equation_solver = EquationIncidences::new(ctx, definition)?;
    for equation in equation_solver_rows {
        if equation.function_id != 16 || equation.arguments.len() != 4 {
            continue;
        }
        let [Some(first), Some(second), Some(difference), Some(selector)] =
            equation.arguments.as_slice()
        else {
            continue;
        };
        let (Some(first), Some(second), Some(difference), Some(selector)) =
            (row(*first), row(*second), row(*difference), row(*selector))
        else {
            continue;
        };
        let Ok(first_value) = reconciled_scalar_row(ctx, first, &scalar_equality_values)? else {
            continue;
        };
        let Ok(second_value) = reconciled_scalar_row(ctx, second, &scalar_equality_values)? else {
            continue;
        };
        let Ok(difference_value) = reconciled_scalar_row(ctx, difference, &scalar_equality_values)?
        else {
            continue;
        };
        let Ok(selector_value) = reconciled_scalar_row(ctx, selector, &scalar_equality_values)?
        else {
            continue;
        };
        if first.variable_type != VariableType::Parameter
            || second.variable_type != VariableType::Parameter
            || difference.variable_type != VariableType::Dimension
            || selector.variable_type != VariableType::Selector
            || selector_value != Some(0.0)
        {
            continue;
        }
        let (Some(first_value), Some(second_value)) = (first_value, second_value) else {
            continue;
        };
        if !first_value.is_finite() || !second_value.is_finite() || first_value < second_value {
            continue;
        }
        let value = first_value - second_value;
        if !value.is_finite() || value > std::f64::consts::PI {
            continue;
        }
        if difference_value.is_some_and(|stored| {
            !stored.is_finite()
                || stored < 0.0
                || !(FiniteReal::new(stored))
                    .zip(FiniteReal::new(value))
                    .is_some_and(|(first, second)| approximately_equal(first, second))
        }) {
            continue;
        }
        ctx.push_vec(
            &mut rows,
            SectionFunctionSixteenAngleDifference {
                first: (first.variable_type, first.key),
                second: (second.variable_type, second.key),
                difference: (difference.variable_type, difference.key),
                value,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !equation_solver.is_disabled(equation.equation_id),
            },
            "creo section equation function sixteen angle difference rows",
        )?;
    }
    Ok(rows)
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct SectionFunctionFortyThreeAxisDistance {
    pub(in crate::decode) first: u32,
    pub(in crate::decode) second: u32,
    pub(in crate::decode) coordinate: SectionAxis,
    pub(in crate::decode) scalar: SectionScalarVariable,
    pub(in crate::decode) value: f64,
    pub(in crate::decode) equation_id: u32,
    pub(in crate::decode) offset: usize,
    pub(in crate::decode) active: bool,
}

fn section_equation_function_forty_three_axis_distance_values(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<(SectionScalarVariable, f64)>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation function forty three axis distance values scratch",
    )?;
    let source_rows = scratch.with_storage(|| {
        section_equation_function_forty_three_axis_distance_rows(
            ctx,
            definition,
            coordinates,
            ambiguous_point_ids,
        )
    })?;
    ctx.collect_vec(
        ctx.admit_iter(&source_rows, "creo section projected equation rows")?
            .copied()
            .filter(|constraint| constraint.active)
            .map(|constraint| (constraint.scalar, constraint.value)),
        "creo section axis distance values",
    )
}

pub(in crate::decode) fn section_equation_function_forty_three_axis_distance_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<SectionFunctionFortyThreeAxisDistance>, CodecError> {
    let mut scratch = ctx.reserve_scoped(
        0,
        "creo section equation function forty three axis distance rows scratch",
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
    let row = |ordinal: u32| {
        usize::try_from(ordinal)
            .ok()
            .and_then(|ordinal| variables.rows.get(ordinal))
    };
    let mut rows = Vec::new();
    let equation_solver_rows = ctx.admit_iter(&equations.rows, "creo section source equation rows")?;
    let equation_solver = EquationIncidences::new(ctx, definition)?;
    for equation in equation_solver_rows {
        if equation.function_id != 43 || equation.arguments.len() != 8 {
            continue;
        }
        let [
            // Coordinate and scalar argument lanes.
            Some(first_u),
            Some(first_v),
            Some(second_u),
            Some(second_v),
            Some(first_auxiliary),
            Some(second_auxiliary),
            Some(distance),
            Some(final_auxiliary),
        ] =
            equation.arguments.as_slice()
        else {
            continue;
        };
        let (Some(first_u), Some(first_v), Some(second_u), Some(second_v)) =
            (row(*first_u), row(*first_v), row(*second_u), row(*second_v))
        else {
            continue;
        };
        let (Some(first_auxiliary), Some(second_auxiliary), Some(distance), Some(final_auxiliary)) = (
            row(*first_auxiliary),
            row(*second_auxiliary),
            row(*distance),
            row(*final_auxiliary),
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
            || !matches!(
                first_auxiliary.variable_type,
                VariableType::Parameter | VariableType::Selector
            )
            || !matches!(
                second_auxiliary.variable_type,
                VariableType::Parameter | VariableType::Selector
            )
            || distance.variable_type != VariableType::Dimension
            || final_auxiliary.variable_type != VariableType::Selector
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
            || [first_auxiliary, second_auxiliary, final_auxiliary]
                .into_iter()
                .any(|row| {
                    row.value.value().is_some_and(|value| {
                        !value.is_finite()
                            || row.variable_type == VariableType::Selector
                                && value.abs() > EPS_AXIS_ZERO
                    })
                })
        {
            continue;
        }
        let auxiliary_value = |row: &FeatureVariableRow| -> Result<Option<Option<f64>>, CodecError> {
                let equality = ctx.get_btree_map(&scalar_equality_values, &(row.variable_type, row.key), "creo section scalar equality lookup")?.copied().unwrap_or(Ok(None));
                Ok(equality.ok().and_then(|value| reconcile_equation_value(row.value.value(), value).ok()))
            };
        let Some(first_value) = auxiliary_value(first_auxiliary)? else {
            continue;
        };
        let Some(second_value) = auxiliary_value(second_auxiliary)? else {
            continue;
        };
        let Some(final_value) = auxiliary_value(final_auxiliary)? else {
            continue;
        };
        let auxiliary_values = [
            (first_auxiliary, first_value),
            (second_auxiliary, second_value),
            (final_auxiliary, final_value),
        ];
        if auxiliary_values.into_iter().any(|(row, value)| {
            value.is_some_and(|value| {
                row.variable_type == VariableType::Selector && value.abs() > EPS_AXIS_ZERO
            })
        }) {
            continue;
        }
        let Ok(distance_equality) = ctx
            .get_btree_map(
                &scalar_equality_values,
                &(distance.variable_type, distance.key),
                "creo section scalar equality values get",
            )?
            .copied()
            .unwrap_or(Ok(None))
        else {
            continue;
        };
        let Ok(distance_value) =
            reconcile_equation_value(distance.value.value(), distance_equality)
        else {
            continue;
        };
        let Some(first) = ctx
            .get_btree_map(coordinates, &first_u.key, "creo section coordinates get")?
            .and_then(|point| Some([point[0]?, point[1]?]))
        else {
            continue;
        };
        let Some(second) = ctx
            .get_btree_map(coordinates, &second_u.key, "creo section coordinates get")?
            .and_then(|point| Some([point[0]?, point[1]?]))
        else {
            continue;
        };
        let deltas = [(second[0] - first[0]).abs(), (second[1] - first[1]).abs()];
        if !deltas.into_iter().all(f64::is_finite) {
            continue;
        }
        let matches_distance = |value: f64| {
            SectionAxis::ALL.into_iter().zip(deltas.iter()).filter_map(
                move |(coordinate, delta)| {
                    let scale = value.abs().max(delta.abs()).max(1.0);
                    ((*delta - value).abs() <= EPS_AXIS_DISTANCE * scale)
                        .then_some((coordinate, *delta))
                },
            )
        };
        let (coordinate, value) = if let Some(stored) = distance_value {
            if !stored.is_finite() || stored < 0.0 {
                continue;
            }
            let mut matches = matches_distance(stored);
            let Some(value) = matches.next() else {
                continue;
            };
            if matches.next().is_some() {
                continue;
            }
            value
        } else {
            let mut nonzero = SectionAxis::ALL.into_iter().zip(deltas.iter()).filter_map(
                |(coordinate, delta)| (*delta > EPS_AXIS_ZERO).then_some((coordinate, *delta)),
            );
            let Some(value) = nonzero.next() else {
                continue;
            };
            if nonzero.next().is_some() {
                continue;
            }
            value
        };
        ctx.push_vec(
            &mut rows,
            SectionFunctionFortyThreeAxisDistance {
                first: first_u.key,
                second: second_u.key,
                coordinate,
                scalar: (distance.variable_type, distance.key),
                value,
                equation_id: equation.equation_id,
                offset: equation.offset,
                active: !equation_solver.is_disabled(equation.equation_id),
            },
            "creo section equation function forty three axis distance rows",
        )?;
    }
    Ok(rows)
}

#[cfg(test)]
mod propagation_tests;
