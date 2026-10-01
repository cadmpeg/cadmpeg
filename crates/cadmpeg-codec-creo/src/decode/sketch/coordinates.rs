// SPDX-License-Identifier: Apache-2.0
//! Resolved section point coordinates from variables, dimensions, and equations.

use super::axis::SectionAxis;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

use std::collections::{BTreeMap, BTreeSet};

use super::super::feature_history::dimensions::feature_relation_table_complete;
use super::equations_coordinate::{
    approximately_equal, section_equal_length_coordinate_values,
    section_equation_equal_length_constraints, section_equation_point_on_line_constraints,
    section_equation_unsigned_coordinate_distances, solve_section_coordinate_equations,
    solve_unsigned_dimension_coordinates, SectionCoordinateEquation, SectionEqualLengthConstraint,
};
use super::equations_scalar::{
    append_section_equation_auxiliary_coordinate_constraints, merge_scalar_value_candidate,
    propagate_section_equation_scalar_equality_values, section_equation_auxiliary_constraints,
    section_equation_coordinate_equalities, section_equation_radial_constraints,
    section_equation_radial_constraints_with_scalar_values, section_equation_scalar_seed_values,
    section_equation_scalar_values_from_coordinates, SectionEquationAuxiliaryConstraints,
    SectionScalarVariable,
};
use super::geometry::{saved_section_circle_values, saved_section_segment_point_coordinates};
use super::radii::section_relation_length_dimension;
use super::skamp::{
    section_line_entity_fixed_coordinate_with_unique_rows, section_line_fixed_coordinate,
    section_skamp_axis_symmetry, section_skamp_incidence_point, section_skamp_point_entity_id,
    section_skamp_point_on_line, section_skamp_point_symmetry, section_skamp_saved_point_on_line,
    SectionPointSource, SectionSymmetryAxis,
};
use crate::decode::sketch_transfer::constraints::{
    section_linear_distance_vectors, section_solver_relation_is_disabled,
};
use crate::decode::sketch_transfer::loci::{
    active_complete_section_skamps, section_skamp_arc_midpoint_source,
    section_skamp_line_midpoint_sources, section_skamp_same_coordinate_sources,
};

const EPS_SECTION_COORDINATE: f64 = 1.0e-9;
const EPS_POINT_ON_LINE_COEFFICIENT: f64 = 1.0e-12;

fn push_coordinate_equation(
    ctx: &DecodeContext<'_>,
    equations: &mut Vec<SectionCoordinateEquation>,
    equation: SectionCoordinateEquation,
) -> Result<(), CodecError> {
    ctx.reserve_vec(equations, 1, "creo section coordinate equations")?;
    equations.push(equation);
    Ok(())
}

pub(in crate::decode) fn saved_section_coordinate_witnesses(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Vec<(u32, [f64; 2])>, CodecError> {
    let ordinary = definition
        .segments
        .iter()
        .flat_map(|table| {
            table
                .rows
                .ordinary()
                .filter(|segment| table.rows.get(segment.external_id).is_some())
        })
        .filter(|segment| {
            segment
                .point_ids()
                .iter()
                .all(|point_id| !ambiguous_point_ids.contains(point_id))
        })
        .filter_map(|segment| saved_section_segment_point_coordinates(definition, segment))
        .flatten();
    let circles = definition
        .segments
        .iter()
        .flat_map(|table| table.rows.circles())
        .filter_map(|segment| {
            (!ambiguous_point_ids.contains(&segment.center_id)).then_some(())?;
            let (center, _) = saved_section_circle_values(definition, segment)?;
            Some((segment.center_id, center))
        });
    crate::decode::collect_items(
        ctx,
        ordinary.chain(circles),
        "creo saved section coordinate witnesses",
    )
}

fn append_point_on_line_equations(
    ctx: &DecodeContext<'_>,
    constraints: &[(u32, u32, u32)],
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    equations: &mut Vec<SectionCoordinateEquation>,
) -> Result<bool, CodecError> {
    let mut appended = false;
    for &(target, first, second) in constraints {
        let (
            Some([Some(first_u), Some(first_v)]),
            Some([Some(second_u), Some(second_v)]),
            Some(target_coordinates),
        ) = (
            coordinates.get(&first),
            coordinates.get(&second),
            coordinates.get(&target),
        )
        else {
            continue;
        };
        let [target_u, target_v] = *target_coordinates;
        if target_u.is_some() == target_v.is_some() {
            continue;
        }
        let delta_u = second_u - first_u;
        let delta_v = second_v - first_v;
        let mut equation = SectionCoordinateEquation::default();
        equation.add_point(ctx, target, SectionAxis::U, -delta_v)?;
        equation.add_point(ctx, target, SectionAxis::V, delta_u)?;
        equation.rhs = delta_u * first_v - delta_v * first_u;
        let Some(rhs) = FiniteReal::new(equation.rhs) else {
            continue;
        };
        let missing_coefficient = if target_u.is_none() {
            delta_v.abs()
        } else {
            delta_u.abs()
        };
        if missing_coefficient > EPS_POINT_ON_LINE_COEFFICIENT
            && !equations.iter().any(|candidate| {
                candidate.terms == equation.terms
                    && (FiniteReal::new(candidate.rhs))
                        .zip(Some(rhs))
                        .is_some_and(|(first, second)| approximately_equal(first, second))
            })
        {
            push_coordinate_equation(ctx, equations, equation)?;
            appended = true;
        }
    }
    Ok(appended)
}

fn append_equal_length_coordinate_values(
    ctx: &DecodeContext<'_>,
    constraints: &[SectionEqualLengthConstraint],
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    equations: &mut Vec<SectionCoordinateEquation>,
) -> Result<bool, CodecError> {
    let mut appended = false;
    for (variable, value) in section_equal_length_coordinate_values(ctx, constraints, coordinates)?
    {
        let Some(value) = value else {
            continue;
        };
        let equation = SectionCoordinateEquation::point_value(ctx, variable.0, variable.1, value)?;
        if equations.iter().any(|candidate| {
            candidate.terms == equation.terms
                && (FiniteReal::new(candidate.rhs))
                    .zip(FiniteReal::new(equation.rhs))
                    .is_some_and(|(first, second)| approximately_equal(first, second))
        }) {
            continue;
        }
        push_coordinate_equation(ctx, equations, equation)?;
        appended = true;
    }
    Ok(appended)
}

fn append_unique_auxiliary_coordinate_constraints(
    ctx: &DecodeContext<'_>,
    constraints: &SectionEquationAuxiliaryConstraints,
    scalar_values: &BTreeMap<SectionScalarVariable, Option<f64>>,
    stored_coordinates: &BTreeMap<(u32, SectionAxis), f64>,
    equations: &mut Vec<SectionCoordinateEquation>,
) -> Result<bool, CodecError> {
    let previous_len = equations.len();
    append_section_equation_auxiliary_coordinate_constraints(
        ctx,
        constraints,
        scalar_values,
        stored_coordinates,
        equations,
    )?;
    let mut appended = false;
    let mut index = previous_len;
    while index < equations.len() {
        if equations[..index].iter().any(|candidate| {
            candidate.terms == equations[index].terms
                && (FiniteReal::new(candidate.rhs))
                    .zip(FiniteReal::new(equations[index].rhs))
                    .is_some_and(|(first, second)| approximately_equal(first, second))
        }) {
            equations.remove(index);
            continue;
        }
        appended = true;
        index += 1;
    }
    Ok(appended)
}

fn solve_section_coordinates_with_derived_constraints(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    equations: &mut Vec<SectionCoordinateEquation>,
    stored_coordinates: &BTreeMap<(u32, SectionAxis), f64>,
    geometric_constraints: (&[(u32, u32, u32)], &[SectionEqualLengthConstraint]),
    auxiliary_constraints: &SectionEquationAuxiliaryConstraints,
    auxiliary_scalar_values: &mut BTreeMap<SectionScalarVariable, Option<f64>>,
) -> Result<BTreeMap<u32, [Option<f64>; 2]>, CodecError> {
    let (point_on_line_constraints, equal_length_constraints) = geometric_constraints;
    let mut solved_coordinates =
        solve_section_coordinate_equations(ctx, equations, stored_coordinates)?;
    let max_passes = point_on_line_constraints
        .len()
        .checked_add(equal_length_constraints.len())
        .and_then(|count| count.checked_add(auxiliary_constraints.midpoints.len()))
        .and_then(|count| {
            auxiliary_constraints
                .point_bindings
                .len()
                .checked_mul(2)
                .and_then(|bindings| count.checked_add(bindings))
        })
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("creo section solver pass count", u64::MAX, u64::MAX)
        })?;
    for _ in 0..max_passes {
        let mut appended = false;
        if append_point_on_line_equations(
            ctx,
            point_on_line_constraints,
            &solved_coordinates,
            equations,
        )? {
            appended = true;
            solved_coordinates =
                solve_section_coordinate_equations(ctx, equations, stored_coordinates)?;
        }
        if append_equal_length_coordinate_values(
            ctx,
            equal_length_constraints,
            &solved_coordinates,
            equations,
        )? {
            appended = true;
            solved_coordinates =
                solve_section_coordinate_equations(ctx, equations, stored_coordinates)?;
        }
        let mut previous_scalar_values = BTreeMap::new();
        for (key, value) in auxiliary_scalar_values.iter() {
            ctx.insert_btree_map(
                &mut previous_scalar_values,
                *key,
                *value,
                "creo previous scalar value nodes",
            )?;
        }
        for (variable, value) in
            section_equation_scalar_values_from_coordinates(ctx, definition, &solved_coordinates)?
        {
            merge_scalar_value_candidate(ctx, auxiliary_scalar_values, variable, value)?;
        }
        propagate_section_equation_scalar_equality_values(
            ctx,
            definition,
            auxiliary_scalar_values,
        )?;
        if *auxiliary_scalar_values != previous_scalar_values
            && append_unique_auxiliary_coordinate_constraints(
                ctx,
                auxiliary_constraints,
                auxiliary_scalar_values,
                stored_coordinates,
                equations,
            )?
        {
            appended = true;
            solved_coordinates =
                solve_section_coordinate_equations(ctx, equations, stored_coordinates)?;
        }
        if !appended {
            break;
        }
    }
    Ok(solved_coordinates)
}

pub(in crate::decode) fn resolved_section_coordinates(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeMap<u32, [Option<f64>; 2]>, CodecError> {
    let crate::feature::definitions::ReconciledPoints {
        points,
        ambiguous: ambiguous_point_ids,
    } = match &definition.variables {
        Some(variables) if variables.is_complete() => variables.reconciled_points(ctx)?,
        Some(_) | None => crate::feature::definitions::ReconciledPoints {
            points: BTreeMap::new(),
            ambiguous: BTreeSet::new(),
        },
    };
    let mut segment_counts = BTreeMap::new();
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.ordinary())
    {
        ctx.admit_btree_entry(
            &segment_counts,
            &segment.external_id,
            "creo section segment count nodes",
        )?;
        *segment_counts.entry(segment.external_id).or_insert(0usize) += 1;
    }
    let saved_segment_points =
        saved_section_coordinate_witnesses(ctx, definition, &ambiguous_point_ids)?;
    let segments = crate::decode::collect_items(
        ctx,
        definition
            .segments
            .iter()
            .flat_map(|table| table.rows.ordinary())
            .filter(|segment| {
                matches!(
                    segment.kind,
                    crate::feature::definitions::FeatureSegmentKind::Line(_)
                )
            })
            .filter(|segment| segment_counts[&segment.external_id] == 1)
            .filter(|segment| {
                segment
                    .point_ids()
                    .iter()
                    .all(|point_id| !ambiguous_point_ids.contains(point_id))
            }),
        "creo section line segments",
    )?;
    let coincident_points = crate::decode::collect_items(
        ctx,
        active_complete_section_skamps(definition).filter_map(|skamp| {
            let [first, second] = skamp.items.as_slice() else {
                return None;
            };
            let pair = match skamp.kind {
                0 => Some([
                    section_skamp_incidence_point(definition, first)?,
                    section_skamp_incidence_point(definition, second)?,
                ]),
                3 => {
                    let first_point = section_skamp_point_entity_id(definition, first);
                    let second_point = section_skamp_point_entity_id(definition, second);
                    match (first_point, second_point) {
                        (Some(first), Some(second)) => Some([
                            SectionPointSource::Point(first),
                            SectionPointSource::Point(second),
                        ]),
                        (Some(point), None) => Some([
                            SectionPointSource::Point(point),
                            section_skamp_incidence_point(definition, second)?,
                        ]),
                        (None, Some(point)) => Some([
                            section_skamp_incidence_point(definition, first)?,
                            SectionPointSource::Point(point),
                        ]),
                        _ => None,
                    }
                }
                _ => None,
            }?;
            (pair
                .iter()
                .any(|point| matches!(point, SectionPointSource::Point(_)))
                && pair.iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                }))
            .then_some(pair)
        }),
        "creo section coincident point pairs",
    )?;
    let same_coordinate_points = crate::decode::collect_items(
        ctx,
        active_complete_section_skamps(definition)
            .filter_map(|skamp| section_skamp_same_coordinate_sources(definition, skamp))
            .filter(|(pair, _)| {
                pair.iter()
                    .any(|point| matches!(point, SectionPointSource::Point(_)))
                    && pair.iter().all(|point| match point {
                        SectionPointSource::Point(point_id) => {
                            !ambiguous_point_ids.contains(point_id)
                        }
                        SectionPointSource::Value(_) => true,
                    })
            }),
        "creo section same-coordinate pairs",
    )?;
    let mut point_on_line_coordinates = Vec::new();
    let mut saved_point_on_line_coordinates = Vec::new();
    for skamp in active_complete_section_skamps(definition) {
        if let Some((first, second, coordinate)) =
            section_skamp_point_on_line(ctx, definition, skamp)?
        {
            if !ambiguous_point_ids.contains(&first) && !ambiguous_point_ids.contains(&second) {
                ctx.reserve_vec(
                    &mut point_on_line_coordinates,
                    1,
                    "creo section point-on-line coordinates",
                )?;
                point_on_line_coordinates.push((first, second, coordinate));
            }
        }
        if let Some((point_id, coordinate, value)) =
            section_skamp_saved_point_on_line(ctx, definition, skamp)?
        {
            if !ambiguous_point_ids.contains(&point_id) {
                ctx.reserve_vec(
                    &mut saved_point_on_line_coordinates,
                    1,
                    "creo section saved point-on-line coordinates",
                )?;
                saved_point_on_line_coordinates.push((point_id, coordinate, value));
            }
        }
    }
    let line_midpoint_constraints = crate::decode::collect_items(
        ctx,
        active_complete_section_skamps(definition)
            .filter_map(|skamp| section_skamp_line_midpoint_sources(definition, skamp))
            .filter(|(point_sources, point)| {
                point_sources.iter().all(|source| match source {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                }) && match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                }
            }),
        "creo section line midpoint constraints",
    )?;
    let mut symmetric_point_constraints = Vec::new();
    for skamp in active_complete_section_skamps(definition) {
        let Some((axis, first, second, coordinate)) =
            section_skamp_axis_symmetry(ctx, definition, skamp)?
        else {
            continue;
        };
        if [first, second]
            .into_iter()
            .any(|point| matches!(point, SectionPointSource::Point(_)))
            && [first, second].into_iter().all(|point| match point {
                SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(&point_id),
                SectionPointSource::Value(_) => true,
            })
            && match axis {
                SectionSymmetryAxis::Point(point_id) => !ambiguous_point_ids.contains(&point_id),
                SectionSymmetryAxis::Value(_) => true,
            }
        {
            ctx.reserve_vec(
                &mut symmetric_point_constraints,
                1,
                "creo section axis symmetry constraints",
            )?;
            symmetric_point_constraints.push((axis, first, second, coordinate));
        }
    }
    let point_symmetric_constraints = crate::decode::collect_items(
        ctx,
        active_complete_section_skamps(definition)
            .filter_map(|skamp| section_skamp_point_symmetry(definition, skamp))
            .filter(|(center, first, second)| {
                !ambiguous_point_ids.contains(center)
                    && [first, second].into_iter().all(|point| match point {
                        SectionPointSource::Point(point_id) => {
                            !ambiguous_point_ids.contains(point_id)
                        }
                        SectionPointSource::Value(_) => true,
                    })
            }),
        "creo section point symmetry constraints",
    )?;
    let auxiliary_constraints =
        section_equation_auxiliary_constraints(ctx, definition, &ambiguous_point_ids)?;
    let mut auxiliary_scalar_values = section_equation_scalar_seed_values(ctx, definition)?;
    propagate_section_equation_scalar_equality_values(
        ctx,
        definition,
        &mut auxiliary_scalar_values,
    )?;
    let mut linear_dimension_candidates = Vec::new();
    for relation in definition
        .relations
        .iter()
        .filter(|table| feature_relation_table_complete(table))
        .flat_map(|table| &table.rows)
    {
        let Some((first, second)) = (|| {
            if section_solver_relation_is_disabled(definition, relation.relation_id) {
                return None;
            }
            if relation.relation_type != 0 {
                return None;
            }
            let vectors = relation.operand_vectors?;
            if !section_linear_distance_vectors(vectors) {
                return None;
            }
            let [Some(first), Some(second), _, _] = vectors[0] else {
                return None;
            };
            Some((first, second))
        })() else {
            continue;
        };
        let Some(coordinate) = section_linear_distance_coordinate(
            ctx,
            definition,
            &segments,
            [first, second],
            &points,
            &saved_segment_points,
            &ambiguous_point_ids,
        )?
        else {
            continue;
        };
        let Some(magnitude) =
            section_relation_length_dimension(definition, relation).and_then(|dimension| {
                dimension
                    .value
                    .resolved()
                    .filter(|value| value.is_finite() && *value >= 0.0)
            })
        else {
            continue;
        };
        if matches!(relation.sign, 0 | 1 | 0xf6) {
            ctx.reserve_vec(
                &mut linear_dimension_candidates,
                1,
                "creo section linear dimension candidates",
            )?;
            linear_dimension_candidates.push((first, second, coordinate, magnitude, relation.sign));
        }
    }
    let signed_dimension_candidates = crate::decode::collect_items(
        ctx,
        linear_dimension_candidates.iter().filter_map(
            |&(first, second, coordinate, magnitude, sign)| {
                let delta = match sign {
                    1 => magnitude,
                    0xf6 => -magnitude,
                    _ => return None,
                };
                Some((first, second, coordinate, delta))
            },
        ),
        "creo section signed dimension candidates",
    )?;
    let mut unsigned_dimension_candidates = crate::decode::collect_items(
        ctx,
        linear_dimension_candidates.iter().filter_map(
            |&(first, second, coordinate, magnitude, sign)| {
                (sign == 0).then_some((first, second, coordinate, magnitude))
            },
        ),
        "creo section unsigned dimension candidates",
    )?;
    let unsigned_equation_distances =
        section_equation_unsigned_coordinate_distances(ctx, definition, &ambiguous_point_ids)?;
    ctx.reserve_vec(
        &mut unsigned_dimension_candidates,
        unsigned_equation_distances.len(),
        "creo section unsigned dimension candidates",
    )?;
    unsigned_dimension_candidates.extend(unsigned_equation_distances.into_iter().map(
        |constraint| {
            (
                constraint.first,
                constraint.second,
                constraint.coordinate,
                constraint.value,
            )
        },
    ));
    let radial_constraints =
        section_equation_radial_constraints(ctx, definition, &points, &ambiguous_point_ids)?;
    let equal_length_constraints =
        section_equation_equal_length_constraints(ctx, definition, &ambiguous_point_ids)?;
    let mut signed_dimensions = BTreeMap::<(u32, u32, SectionAxis), Option<f64>>::new();
    for (first, second, coordinate, delta) in signed_dimension_candidates {
        let (key, canonical_delta) = if first <= second {
            ((first, second, coordinate), delta)
        } else {
            ((second, first, coordinate), -delta)
        };
        ctx.admit_btree_entry(&signed_dimensions, &key, "creo section signed dimension nodes")?;
        signed_dimensions
            .entry(key)
            .and_modify(|stored| {
                if stored.is_some_and(|stored| stored != canonical_delta) {
                    *stored = None;
                }
            })
            .or_insert(Some(canonical_delta));
    }
    let signed_dimensions = crate::decode::collect_items(
        ctx,
        signed_dimensions
            .into_iter()
            .filter_map(|((first, second, coordinate), delta)| {
                Some((first, second, coordinate, delta?))
            }),
        "creo section canonical signed dimensions",
    )?;
    let mut equations = Vec::new();
    for (&point_id, coordinates) in &points {
        for (coordinate, value) in SectionAxis::ALL
            .into_iter()
            .zip(coordinates.iter().copied())
        {
            if let Some(value) = value {
                push_coordinate_equation(
                    ctx,
                    &mut equations,
                    SectionCoordinateEquation::point_value(ctx, point_id, coordinate, value)?,
                )?;
            }
        }
    }
    for &(point_id, coordinates) in &saved_segment_points {
        for (coordinate, value) in SectionAxis::ALL.into_iter().zip(coordinates) {
            push_coordinate_equation(
                ctx,
                &mut equations,
                SectionCoordinateEquation::point_value(ctx, point_id, coordinate, value)?,
            )?;
        }
    }
    for segment in &segments {
        if let Some(coordinate) = section_line_fixed_coordinate(ctx, definition, segment)? {
            push_coordinate_equation(
                ctx,
                &mut equations,
                SectionCoordinateEquation::point_difference(
                    ctx,
                    segment.point_ids()[0],
                    segment.point_ids()[1],
                    coordinate,
                    0.0,
                )?,
            )?;
        }
    }
    for &(first, second, coordinate, delta) in &signed_dimensions {
        push_coordinate_equation(
            ctx,
            &mut equations,
            SectionCoordinateEquation::point_difference(ctx, first, second, coordinate, delta)?,
        )?;
    }
    for &[first, second] in &coincident_points {
        for coordinate in SectionAxis::ALL {
            push_coordinate_equation(
                ctx,
                &mut equations,
                SectionCoordinateEquation::source_difference(ctx, first, second, coordinate, 0.0)?,
            )?;
        }
    }
    for (first, second, coordinate) in
        section_equation_coordinate_equalities(ctx, definition, &ambiguous_point_ids)?
    {
        push_coordinate_equation(
            ctx,
            &mut equations,
            SectionCoordinateEquation::point_difference(ctx, first, second, coordinate, 0.0)?,
        )?;
    }
    let point_on_line_constraints =
        section_equation_point_on_line_constraints(ctx, definition, &ambiguous_point_ids)?;
    for constraint in &radial_constraints {
        if let Some(offset) = constraint.offset() {
            push_coordinate_equation(
                ctx,
                &mut equations,
                SectionCoordinateEquation::point_difference(
                    ctx,
                    constraint.first,
                    constraint.second,
                    SectionAxis::U,
                    offset[0],
                )?,
            )?;
            push_coordinate_equation(
                ctx,
                &mut equations,
                SectionCoordinateEquation::point_difference(
                    ctx,
                    constraint.first,
                    constraint.second,
                    SectionAxis::V,
                    offset[1],
                )?,
            )?;
        }
    }
    for &([first, second], coordinate) in &same_coordinate_points {
        push_coordinate_equation(
            ctx,
            &mut equations,
            SectionCoordinateEquation::source_difference(ctx, first, second, coordinate, 0.0)?,
        )?;
    }
    for &(first, second, coordinate) in &point_on_line_coordinates {
        push_coordinate_equation(
            ctx,
            &mut equations,
            SectionCoordinateEquation::point_difference(ctx, first, second, coordinate, 0.0)?,
        )?;
    }
    for &(point, coordinate, value) in &saved_point_on_line_coordinates {
        push_coordinate_equation(
            ctx,
            &mut equations,
            SectionCoordinateEquation::point_value(ctx, point, coordinate, value)?,
        )?;
    }
    for &(point_sources, point) in &line_midpoint_constraints {
        for coordinate in SectionAxis::ALL {
            let mut equation = SectionCoordinateEquation::default();
            equation.add_source(ctx, point_sources[0], coordinate, 1.0)?;
            equation.add_source(ctx, point_sources[1], coordinate, 1.0)?;
            equation.add_source(ctx, point, coordinate, -2.0)?;
            push_coordinate_equation(ctx, &mut equations, equation)?;
        }
    }
    for &(axis, first, second, fixed_coordinate) in &symmetric_point_constraints {
        let parallel_coordinate = fixed_coordinate.other();
        push_coordinate_equation(
            ctx,
            &mut equations,
            SectionCoordinateEquation::source_difference(
                ctx,
                first,
                second,
                parallel_coordinate,
                0.0,
            )?,
        )?;
        let mut equation = SectionCoordinateEquation::default();
        equation.add_source(ctx, first, fixed_coordinate, 1.0)?;
        equation.add_source(ctx, second, fixed_coordinate, 1.0)?;
        match axis {
            SectionSymmetryAxis::Point(point_id) => {
                equation.add_point(ctx, point_id, fixed_coordinate, -2.0)?;
            }
            SectionSymmetryAxis::Value(value) => equation.rhs += 2.0 * value,
        }
        push_coordinate_equation(ctx, &mut equations, equation)?;
    }
    for &(center, first, second) in &point_symmetric_constraints {
        for coordinate in SectionAxis::ALL {
            let mut equation = SectionCoordinateEquation::default();
            equation.add_source(ctx, first, coordinate, 1.0)?;
            equation.add_source(ctx, second, coordinate, 1.0)?;
            equation.add_point(ctx, center, coordinate, -2.0)?;
            push_coordinate_equation(ctx, &mut equations, equation)?;
        }
    }
    let mut stored_coordinates = BTreeMap::new();
    for (&point, coordinates) in &points {
        for (coordinate, value) in SectionAxis::ALL
            .into_iter()
            .zip(coordinates.iter().copied())
        {
            if let Some(value) = value {
                ctx.insert_btree_map(
                    &mut stored_coordinates,
                    (point, coordinate),
                    value,
                    "creo section stored coordinate nodes",
                )?;
            }
        }
    }
    append_section_equation_auxiliary_coordinate_constraints(
        ctx,
        &auxiliary_constraints,
        &auxiliary_scalar_values,
        &stored_coordinates,
        &mut equations,
    )?;
    let unsigned_coordinates = solve_unsigned_dimension_coordinates(
        ctx,
        &equations,
        &stored_coordinates,
        &unsigned_dimension_candidates,
    )?;
    for ((point, coordinate), value) in unsigned_coordinates {
        push_coordinate_equation(
            ctx,
            &mut equations,
            SectionCoordinateEquation::point_value(ctx, point, coordinate, value)?,
        )?;
    }
    let solved_coordinates = solve_section_coordinates_with_derived_constraints(
        ctx,
        definition,
        &mut equations,
        &stored_coordinates,
        (&point_on_line_constraints, &equal_length_constraints),
        &auxiliary_constraints,
        &mut auxiliary_scalar_values,
    )?;
    for constraint in section_equation_radial_constraints_with_scalar_values(
        ctx,
        definition,
        &solved_coordinates,
        &ambiguous_point_ids,
        &auxiliary_scalar_values,
    )? {
        if let Some(offset) = constraint.offset() {
            push_coordinate_equation(
                ctx,
                &mut equations,
                SectionCoordinateEquation::point_difference(
                    ctx,
                    constraint.first,
                    constraint.second,
                    SectionAxis::U,
                    offset[0],
                )?,
            )?;
            push_coordinate_equation(
                ctx,
                &mut equations,
                SectionCoordinateEquation::point_difference(
                    ctx,
                    constraint.first,
                    constraint.second,
                    SectionAxis::V,
                    offset[1],
                )?,
            )?;
        }
    }
    let second_unsigned_coordinates = solve_unsigned_dimension_coordinates(
        ctx,
        &equations,
        &stored_coordinates,
        &unsigned_dimension_candidates,
    )?;
    for ((point, coordinate), value) in second_unsigned_coordinates {
        push_coordinate_equation(
            ctx,
            &mut equations,
            SectionCoordinateEquation::point_value(ctx, point, coordinate, value)?,
        )?;
    }
    let solved_coordinates = solve_section_coordinates_with_derived_constraints(
        ctx,
        definition,
        &mut equations,
        &stored_coordinates,
        (&point_on_line_constraints, &equal_length_constraints),
        &auxiliary_constraints,
        &mut auxiliary_scalar_values,
    )?;
    let arc_midpoint_constraints = crate::decode::collect_items(
        ctx,
        active_complete_section_skamps(definition)
            .filter_map(|skamp| {
                section_skamp_arc_midpoint_source(definition, skamp, &solved_coordinates)
            })
            .filter_map(|(point, midpoint)| match point {
                SectionPointSource::Point(point_id) if !ambiguous_point_ids.contains(&point_id) => {
                    Some((point_id, midpoint))
                }
                SectionPointSource::Point(_) | SectionPointSource::Value(_) => None,
            }),
        "creo section arc midpoint constraints",
    )?;
    for &(point_id, midpoint) in &arc_midpoint_constraints {
        for (coordinate, value) in SectionAxis::ALL.into_iter().zip(midpoint) {
            push_coordinate_equation(
                ctx,
                &mut equations,
                SectionCoordinateEquation::point_value(ctx, point_id, coordinate, value)?,
            )?;
        }
    }
    solve_section_coordinate_equations(ctx, &equations, &stored_coordinates)
}

pub(in crate::decode) fn section_linear_distance_coordinate(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segments: &[&crate::feature::definitions::FeatureSegment],
    point_ids: [u32; 2],
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    saved_segment_points: &[(u32, [f64; 2])],
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Result<Option<SectionAxis>, CodecError> {
    let [first, second] = point_ids;

    let mut matching_segments = segments.iter().copied().filter(|segment| {
        segment.point_ids() == [first, second] || segment.point_ids() == [second, first]
    });
    let unique_segment = matching_segments.next();
    let duplicate_segment = matching_segments.next().is_some();
    let point_coordinate = |point_id: u32, coordinate: SectionAxis| -> Result<Option<f64>, ()> {
        if ambiguous_point_ids.contains(&point_id) {
            return Err(());
        }
        let values = || {
            coordinates
                .get(&point_id)
                .and_then(|point| point[coordinate.index()])
                .into_iter()
                .chain(
                    saved_segment_points
                        .iter()
                        .filter(move |(saved_point_id, _)| *saved_point_id == point_id)
                        .map(move |(_, point)| point[coordinate.index()]),
                )
        };
        let mut first = None;
        let mut scale = 1.0_f64;
        for value in values() {
            value.is_finite().then_some(()).ok_or(())?;
            if first.is_none() {
                first = Some(value);
            }
            scale = scale.max(value.abs());
        }
        let Some(first) = first else {
            return Ok(None);
        };
        values()
            .all(|value| (value - first).abs() <= EPS_SECTION_COORDINATE * scale)
            .then_some(Some(first))
            .ok_or(())
    };
    if let Some(segment) = unique_segment.filter(|_| !duplicate_segment) {
        if let Some(fixed_coordinate) = section_line_entity_fixed_coordinate_with_unique_rows(
            ctx,
            definition,
            segment.external_id,
        )? {
            let Ok(first_coordinate) = point_coordinate(first, fixed_coordinate) else {
                return Ok(None);
            };
            let Ok(second_coordinate) = point_coordinate(second, fixed_coordinate) else {
                return Ok(None);
            };
            if let (Some(first), Some(second)) = (first_coordinate, second_coordinate) {
                let scale = first.abs().max(second.abs()).max(1.0);
                if (first - second).abs() > EPS_SECTION_COORDINATE * scale {
                    return Ok(None);
                }
            }
            return Ok(Some(fixed_coordinate.other()));
        }
    }
    if duplicate_segment {
        return Ok(None);
    }
    Ok((|| {
        let table = definition.segments.as_ref()?;
        // Only decoded point-bearing families establish that a dimension operand
        // is a section endpoint. Opaque rows retain native identity but do not
        // prove an endpoint role.
        let has_unique_incident_entity = |point_id| {
            table.rows.ordinary().any(|segment| {
                segment.point_ids().contains(&point_id)
                    && table.rows.get(segment.external_id).is_some()
            }) || table.rows.points().any(|segment| {
                segment.point_id == point_id && table.rows.get(segment.external_id).is_some()
            }) || table.rows.ordinary().any(|segment| {
                matches!(
                    segment.kind,
                    crate::feature::definitions::FeatureSegmentKind::Arc(_)
                ) && segment.center_id == Some(point_id)
                    && table.rows.get(segment.external_id).is_some()
            }) || table.rows.circles().any(|segment| {
                segment.center_id == point_id && table.rows.get(segment.external_id).is_some()
            }) || (matches!(point_id, 0 | 1)
                && table
                    .rows
                    .centered_lines()
                    .any(|segment| table.rows.get(segment.external_id).is_some()))
                || table.rows.reference_lines().any(|segment| {
                    segment.point_ids.contains(&Some(point_id))
                        && table.rows.get(segment.external_id).is_some()
                })
                || table.rows.bounded_curves().any(|segment| {
                    segment.point_ids.contains(&point_id)
                        && table.rows.get(segment.external_id).is_some()
                })
        };
        has_unique_incident_entity(first).then_some(())?;
        has_unique_incident_entity(second).then_some(())?;
        let equal_coordinate = |coordinate: SectionAxis| -> Option<bool> {
            let first = point_coordinate(first, coordinate).ok().flatten()?;
            let second = point_coordinate(second, coordinate).ok().flatten()?;
            let scale = first.abs().max(second.abs()).max(1.0);
            Some((first - second).abs() <= EPS_SECTION_COORDINATE * scale)
        };
        let equal_u = equal_coordinate(SectionAxis::U);
        let equal_v = equal_coordinate(SectionAxis::V);
        if equal_u == Some(true) && equal_v != Some(true) {
            return Some(SectionAxis::V);
        }
        if equal_v == Some(true) && equal_u != Some(true) {
            return Some(SectionAxis::U);
        }
        None
    })())
}

pub(in crate::decode) fn resolved_section_points(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeMap<u32, [f64; 2]>, CodecError> {
    let mut resolved = BTreeMap::new();
    for (point, [u, v]) in resolved_section_coordinates(ctx, definition)? {
        if let (Some(u), Some(v)) = (u, v) {
            ctx.insert_btree_map(
                &mut resolved,
                point,
                [u, v],
                "creo resolved section point nodes",
            )?;
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::super::equations_scalar::resolved_section_scalar_values;
    use super::resolved_section_points;
    use super::{
        append_point_on_line_equations as append_point_on_line_equations_checked, SectionAxis,
        SectionCoordinateEquation,
    };
    use crate::feature::definitions::FeatureSolverTableHeader;
    use crate::feature::definitions::FeatureVariableTable;
    use crate::feature::definitions::{
        FeatureCircleSegment, FeatureDefinition, FeatureDimension, FeatureDimensionTable,
        FeaturePointSegment, FeatureRelation, FeatureRelationTable, FeatureSectionPoint,
        FeatureSegment, FeatureSegmentKind, FeatureSegmentTable, FeatureSkamp, FeatureSkampItem,
        FeatureVariableRow,
    };

    fn section_linear_distance_coordinate(
        definition: &FeatureDefinition,
        segments: &[&FeatureSegment],
        first: u32,
        second: u32,
        coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
        saved_segment_points: &[(u32, [f64; 2])],
        ambiguous_point_ids: &BTreeSet<u32>,
    ) -> Option<SectionAxis> {
        crate::decode::with_test_decode_ctx(|ctx| {
            super::section_linear_distance_coordinate(
                ctx,
                definition,
                segments,
                [first, second],
                coordinates,
                saved_segment_points,
                ambiguous_point_ids,
            )
        })
        .expect("test linear distance coordinate")
    }

    fn append_point_on_line_equations(
        constraints: &[(u32, u32, u32)],
        coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
        equations: &mut Vec<SectionCoordinateEquation>,
    ) -> bool {
        crate::decode::with_test_decode_ctx(|ctx| {
            append_point_on_line_equations_checked(ctx, constraints, coordinates, equations)
        })
        .expect("test coordinate equations")
    }

    #[test]
    fn section_coordinate_equation_vec_refuses_before_append() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("test input admitted");
        let mut equations = Vec::new();
        assert!(
            matches!(super::push_coordinate_equation(&ctx, &mut equations,
            SectionCoordinateEquation::default()),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "creo section coordinate equations")
        );
        assert!(equations.is_empty());
        crate::decode::with_test_decode_ctx(|ctx| {
            super::push_coordinate_equation(
                ctx,
                &mut equations,
                SectionCoordinateEquation::default(),
            )
        })
        .expect("test equation append");
        assert_eq!(equations.len(), 1);
    }

    #[test]
    fn infinite_point_on_line_rhs_is_not_admitted() {
        let coordinates = BTreeMap::from([
            (1, [Some(0.0), Some(1.0e308)]),
            (2, [Some(1.0e308), Some(1.0e308)]),
            (3, [Some(0.0), None]),
        ]);
        let mut equations = Vec::new();
        assert!(!append_point_on_line_equations(
            &[(3, 1, 2)],
            &coordinates,
            &mut equations,
        ));
        assert!(equations.is_empty());
    }

    #[test]
    fn finite_point_on_line_rhs_is_distinct_from_infinite_existing_rhs() {
        let coordinates = BTreeMap::from([
            (1, [Some(0.0), Some(0.0)]),
            (2, [Some(1.0), Some(0.0)]),
            (3, [Some(0.0), None]),
        ]);
        let mut existing = SectionCoordinateEquation::default();
        crate::decode::with_test_decode_ctx(|ctx| {
            existing.add_point(ctx, 3, SectionAxis::U, 0.0)?;
            existing.add_point(ctx, 3, SectionAxis::V, 1.0)
        })
        .expect("test equation terms");
        existing.rhs = f64::INFINITY;
        let mut equations = vec![existing];
        assert!(append_point_on_line_equations(
            &[(3, 1, 2)],
            &coordinates,
            &mut equations,
        ));
        assert_eq!(equations.len(), 2);
        assert_eq!(equations[1].rhs, 0.0);
    }

    fn incomplete_segment_definition() -> FeatureDefinition {
        FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::definitions::test_support::with_points(
                FeatureVariableTable {
                    declared_count: 0,
                    entity_ref: None,
                    rows: Vec::new(),
                    offset: 0,
                },
                vec![
                    FeatureSectionPoint {
                        point_id: 1,
                        u: Some(2.0),
                        v: Some(3.0),
                    },
                    FeatureSectionPoint {
                        point_id: 2,
                        u: None,
                        v: None,
                    },
                ],
            )),
            segments: Some(FeatureSegmentTable {
                declared_count: 3,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![FeatureSegment {
                    kind: FeatureSegmentKind::Line([1, 2]),
                    directions: [None; 3],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: None,
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 7,
                    body: Vec::new(),
                    offset: 0,
                }])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .chain(
                    (vec![FeaturePointSegment {
                        point_id: 2,
                        external_id: 8,
                        offset: 1,
                    }])
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::Point),
                )
                .collect(),
                offset: 0,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(FeatureRelationTable {
                declared_count: 1,
                entity_ref: None,
                rows: Vec::new(),
                skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
                    header: FeatureSolverTableHeader {
                        declared_count: 2,
                        entity_ref: 0,
                        offset: 0,
                    },
                    rows: vec![
                        FeatureSkamp {
                            id: 1,
                            kind: 0,
                            flags: 0,
                            status: 1,
                            items: vec![
                                FeatureSkampItem {
                                    entity_id: 7,
                                    sense: 2,
                                },
                                FeatureSkampItem {
                                    entity_id: 7,
                                    sense: 3,
                                },
                            ],
                            offset: 0,
                        },
                        FeatureSkamp {
                            id: 2,
                            kind: 3,
                            flags: 0,
                            status: 1,
                            items: vec![
                                FeatureSkampItem {
                                    entity_id: 8,
                                    sense: 0,
                                },
                                FeatureSkampItem {
                                    entity_id: 7,
                                    sense: 2,
                                },
                            ],
                            offset: 1,
                        },
                    ],
                }),
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        }
    }

    #[test]
    fn section_segment_counts_refuse_before_tree_node() {
        let definition = incomplete_segment_definition();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        // Two reconciled point IDs and two point nodes precede the segment count.
        policy.limits.max_collection_items = 4;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("test input admitted");
        assert!(
            matches!(super::resolved_section_coordinates(&ctx, &definition),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "creo section segment count nodes")
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &definition))
                .expect("test section solve")
                .get(&2),
            Some(&[2.0, 3.0])
        );
    }

    #[test]
    fn incomplete_unique_ordinary_rows_supply_coincidence_point_ids() {
        let definition = incomplete_segment_definition();
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &definition))
                .expect("test section solve")
                .get(&2),
            Some(&[2.0, 3.0])
        );

        let mut duplicate_ordinary = definition.clone();
        let duplicate = duplicate_ordinary
            .segments
            .as_ref()
            .expect("segments")
            .rows
            .ordinary()
            .cloned()
            .collect::<Vec<_>>()[0]
            .clone();
        duplicate_ordinary
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
                FeatureSegment {
                    offset: 2,
                    ..duplicate
                },
            ));
        assert!(
            !crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
                ctx,
                &duplicate_ordinary
            ))
            .expect("test section solve")
            .contains_key(&2)
        );

        let mut duplicate_family = definition;
        duplicate_family
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Point(
                FeaturePointSegment {
                    point_id: 1,
                    external_id: 7,
                    offset: 2,
                },
            ));
        assert!(
            !crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
                ctx,
                &duplicate_family
            ))
            .expect("test section solve")
            .contains_key(&2)
        );
    }

    #[test]
    fn incomplete_unique_spanning_line_selector_supplies_distance_axis() {
        let mut definition = incomplete_segment_definition();
        definition
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .edit_ordinary(|rows| rows[0].vertical_horizontal = Some(0));
        let segments = definition
            .segments
            .as_ref()
            .expect("segments")
            .rows
            .ordinary()
            .collect::<Vec<_>>();
        assert_eq!(
            section_linear_distance_coordinate(
                &definition,
                &segments,
                1,
                2,
                &std::collections::BTreeMap::new(),
                &[],
                &std::collections::BTreeSet::new(),
            ),
            Some(crate::decode::sketch::axis::SectionAxis::V)
        );

        let mut duplicate = definition;
        let duplicate_row = duplicate
            .segments
            .as_ref()
            .expect("segments")
            .rows
            .ordinary()
            .cloned()
            .collect::<Vec<_>>()[0]
            .clone();
        duplicate.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Ordinary(FeatureSegment {
                offset: 2,
                ..duplicate_row
            }),
        );
        let duplicate_segments = duplicate
            .segments
            .as_ref()
            .expect("segments")
            .rows
            .ordinary()
            .collect::<Vec<_>>();
        assert_eq!(
            section_linear_distance_coordinate(
                &duplicate,
                &duplicate_segments,
                1,
                2,
                &std::collections::BTreeMap::new(),
                &[],
                &std::collections::BTreeSet::new(),
            ),
            None
        );
    }

    #[test]
    fn unique_arc_center_supplies_distance_axis() {
        let mut definition = incomplete_segment_definition();
        {
            let table = definition.segments.as_mut().expect("segments");
            let mut arc = table.rows.ordinary().cloned().collect::<Vec<_>>()[0].clone();
            arc.kind = FeatureSegmentKind::Arc([10, 11]);
            arc.center_id = Some(3);
            arc.external_id = 7;
            arc.vertical_horizontal = None;
            let mut line = table.rows.ordinary().cloned().collect::<Vec<_>>()[0].clone();
            line.kind = FeatureSegmentKind::Line([4, 5]);
            line.center_id = None;
            line.external_id = 8;
            line.vertical_horizontal = None;
            table.rows.edit_ordinary(|rows| *rows = vec![arc, line]);
            table.rows.edit_circles(Vec::clear);
            table.rows.edit_points(Vec::clear);
            table.rows.edit_centered_lines(Vec::clear);
            table.rows.edit_reference_lines(Vec::clear);
            table.rows.edit_bounded_curves(Vec::clear);
            table.rows.edit_conics(Vec::clear);
            table.rows.edit_opaque(Vec::clear);
        }
        let table = definition.segments.as_ref().expect("segments");
        let segments = table.rows.ordinary().collect::<Vec<_>>();
        let coordinates =
            BTreeMap::from([(3, [Some(1.0), Some(4.0)]), (4, [Some(1.0), Some(9.0)])]);

        assert_eq!(
            section_linear_distance_coordinate(
                &definition,
                &segments,
                3,
                4,
                &coordinates,
                &[],
                &BTreeSet::new(),
            ),
            Some(crate::decode::sketch::axis::SectionAxis::V)
        );

        let mut circle_definition = definition;
        {
            let table = circle_definition.segments.as_mut().expect("segments");
            let line = table.rows.ordinary().cloned().collect::<Vec<_>>()[1].clone();
            table.rows.edit_ordinary(|rows| *rows = vec![line]);
            table.rows.edit_circles(|rows| {
                *rows = vec![FeatureCircleSegment {
                    center_id: 3,
                    radius_ref: 0,
                    external_id: 9,
                    offset: 2,
                }];
            });
        }
        let table = circle_definition.segments.as_ref().expect("segments");
        let segments = table.rows.ordinary().collect::<Vec<_>>();
        assert_eq!(
            section_linear_distance_coordinate(
                &circle_definition,
                &segments,
                3,
                4,
                &coordinates,
                &[],
                &BTreeSet::new(),
            ),
            Some(crate::decode::sketch::axis::SectionAxis::V)
        );
    }

    #[test]
    fn point_on_line_retries_after_auxiliary_reference_coordinates_resolve() {
        let row = |variable_type, key, value: Option<f64>| FeatureVariableRow {
            variable_type: crate::feature::definitions::VariableType::from(variable_type),
            key,
            value: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            value_body: Vec::new(),
            guess: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            guess_body: Vec::new(),
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        body.extend_from_slice(b"\x01\x23\xf8\x09\x00\x01\x02\x03\x04\x05\x06\x07\x08\xf6\xe2");
        body.extend_from_slice(b"\x02\x1f\xf8\x04\x02\x03\x09\x0a\xf6\xe2");
        body.extend_from_slice(b"\x03\x1f\xf8\x04\x04\x05\x0b\x0c\xf6\xe2");
        let definition = FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(2),
                owner_feature_id: None,
            },
            body,
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                declared_count: 13,
                entity_ref: None,
                rows: vec![
                    row(1, 30, None),
                    row(2, 30, Some(5.0)),
                    row(1, 10, None),
                    row(2, 10, None),
                    row(1, 11, None),
                    row(2, 11, None),
                    row(4, 2, None),
                    row(5, 3, Some(0.0)),
                    row(5, 4, Some(0.0)),
                    row(6, 100, Some(0.0)),
                    row(6, 101, Some(0.0)),
                    row(6, 102, Some(10.0)),
                    row(6, 103, Some(10.0)),
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &definition))
                .expect("test section solve")
                .get(&30),
            Some(&[5.0, 5.0])
        );
    }

    #[test]
    fn equal_length_retries_after_derived_auxiliary_reference_coordinates_resolve() {
        let row = |variable_type, key, value: Option<f64>| FeatureVariableRow {
            variable_type: crate::feature::definitions::VariableType::from(variable_type),
            key,
            value: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            value_body: Vec::new(),
            guess: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            guess_body: Vec::new(),
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x08\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        let mut equation = |id, function, arguments: &[u8]| {
            body.extend_from_slice(&[
                id,
                function,
                0xf8,
                u8::try_from(arguments.len()).expect("fixture value fits u8"),
            ]);
            body.extend_from_slice(arguments);
            body.extend_from_slice(b"\xf6\xe2");
        };
        equation(1, 0x21, &[0, 1, 2, 3, 4, 5, 6, 7, 8]);
        equation(2, 0x2a, &[9, 10, 11]);
        equation(3, 0x2a, &[12, 13, 14]);
        equation(4, 0x2a, &[15, 16, 17]);
        equation(5, 0x2a, &[18, 19, 20]);
        equation(6, 0x1f, &[4, 5, 11, 14]);
        equation(7, 0x1f, &[6, 7, 17, 20]);
        let definition = FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(3),
                owner_feature_id: None,
            },
            body,
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                declared_count: 21,
                entity_ref: None,
                rows: vec![
                    row(1, 30, None),
                    row(2, 30, Some(4.0)),
                    row(1, 31, Some(0.0)),
                    row(2, 31, Some(0.0)),
                    row(1, 10, None),
                    row(2, 10, None),
                    row(1, 11, None),
                    row(2, 11, None),
                    row(7, 5, Some(0.0)),
                    row(1, 20, Some(0.0)),
                    row(1, 21, Some(2.0)),
                    row(6, 100, None),
                    row(2, 20, Some(0.0)),
                    row(2, 21, Some(2.0)),
                    row(6, 101, None),
                    row(1, 22, Some(0.0)),
                    row(1, 23, Some(2.0)),
                    row(6, 102, None),
                    row(2, 22, Some(4.0)),
                    row(2, 23, Some(6.0)),
                    row(6, 103, None),
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &definition))
                .expect("test section solve")
                .get(&30),
            Some(&[0.0, 4.0])
        );
    }

    #[test]
    fn derived_auxiliary_values_retry_after_point_on_line_resolution() {
        let row = |variable_type, key, value: Option<f64>| FeatureVariableRow {
            variable_type: crate::feature::definitions::VariableType::from(variable_type),
            key,
            value: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            value_body: Vec::new(),
            guess: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            guess_body: Vec::new(),
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x07\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        let mut equation = |id, function, arguments: &[u8]| {
            body.extend_from_slice(&[
                id,
                function,
                0xf8,
                u8::try_from(arguments.len()).expect("fixture value fits u8"),
            ]);
            body.extend_from_slice(arguments);
            body.extend_from_slice(b"\xf6\xe2");
        };
        equation(1, 0x2a, &[9, 11, 13]);
        equation(2, 0x1f, &[2, 3, 13, 14]);
        equation(3, 0x1f, &[4, 5, 15, 16]);
        equation(4, 0x23, &[0, 1, 2, 3, 4, 5, 6, 7, 8]);
        equation(5, 0x2a, &[0, 17, 21]);
        equation(6, 0x1f, &[19, 20, 21, 22]);
        let definition = FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(4),
                owner_feature_id: None,
            },
            body,
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                declared_count: 23,
                entity_ref: None,
                rows: vec![
                    row(1, 30, None),
                    row(2, 30, Some(4.0)),
                    row(1, 10, None),
                    row(2, 10, None),
                    row(1, 11, None),
                    row(2, 11, None),
                    row(4, 0, None),
                    row(5, 0, Some(0.0)),
                    row(5, 1, Some(0.0)),
                    row(1, 20, Some(0.0)),
                    row(2, 20, Some(0.0)),
                    row(1, 21, Some(2.0)),
                    row(2, 21, Some(2.0)),
                    row(6, 100, None),
                    row(6, 101, Some(0.0)),
                    row(6, 102, Some(2.0)),
                    row(6, 103, Some(2.0)),
                    row(1, 31, Some(5.0)),
                    row(2, 31, Some(0.0)),
                    row(1, 40, None),
                    row(2, 40, Some(0.0)),
                    row(6, 104, None),
                    row(6, 105, Some(0.0)),
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &definition))
                .expect("test section solve")
                .get(&40),
            Some(&[4.0, 0.0])
        );
    }

    #[test]
    fn derived_auxiliary_values_cross_scalar_equalities() {
        let row = |variable_type, key, value: Option<f64>| FeatureVariableRow {
            variable_type: crate::feature::definitions::VariableType::from(variable_type),
            key,
            value: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            value_body: Vec::new(),
            guess: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            guess_body: Vec::new(),
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        let mut equation = |id, function, arguments: &[u8]| {
            body.extend_from_slice(&[
                id,
                function,
                0xf8,
                u8::try_from(arguments.len()).expect("fixture value fits u8"),
            ]);
            body.extend_from_slice(arguments);
            body.extend_from_slice(b"\xf6\xe2");
        };
        equation(1, 0x2a, &[0, 1, 2]);
        equation(2, 0x02, &[2, 3]);
        equation(3, 0x1f, &[5, 6, 3, 4]);
        let definition = FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(5),
                owner_feature_id: None,
            },
            body,
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                declared_count: 7,
                entity_ref: None,
                rows: vec![
                    row(1, 10, Some(0.0)),
                    row(1, 11, Some(4.0)),
                    row(6, 100, None),
                    row(6, 101, None),
                    row(6, 102, Some(3.0)),
                    row(1, 30, None),
                    row(2, 30, None),
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &definition))
                .expect("test section solve")
                .get(&30),
            Some(&[2.0, 3.0])
        );
    }

    #[test]
    fn derived_axis_distance_feeds_equal_radius_polar_constraint() {
        let row = |variable_type, key, value: Option<f64>| FeatureVariableRow {
            variable_type: crate::feature::definitions::VariableType::from(variable_type),
            key,
            value: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            value_body: Vec::new(),
            guess: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            guess_body: Vec::new(),
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        let mut equation = |id, function, arguments: &[u8]| {
            body.extend_from_slice(&[
                id,
                function,
                0xf8,
                u8::try_from(arguments.len()).expect("fixture value fits u8"),
            ]);
            body.extend_from_slice(arguments);
            body.extend_from_slice(b"\xf6\xe2");
        };
        equation(1, 0x2b, &[0, 1, 2, 3, 4, 5, 6, 7]);
        equation(2, 0x02, &[6, 8]);
        equation(3, 0x00, &[9, 10, 11, 12, 8, 13]);
        let definition = FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(6),
                owner_feature_id: None,
            },
            body,
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                declared_count: 14,
                entity_ref: None,
                rows: vec![
                    row(1, 10, Some(0.0)),
                    row(2, 10, Some(0.0)),
                    row(1, 11, Some(4.0)),
                    row(2, 11, Some(0.0)),
                    row(4, 2, Some(0.0)),
                    row(5, 0, Some(0.0)),
                    row(0, 20, None),
                    row(5, 1, Some(0.0)),
                    row(0, 21, None),
                    row(1, 30, Some(1.0)),
                    row(2, 30, Some(1.0)),
                    row(1, 40, None),
                    row(2, 40, None),
                    row(4, 3, Some(0.0)),
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &definition))
                .expect("test section solve")
                .get(&40),
            Some(&[5.0, 1.0])
        );
    }

    #[test]
    fn dimension_driven_radius_feeds_polar_constraint() {
        let row = |variable_type, key, value: Option<f64>| FeatureVariableRow {
            variable_type: crate::feature::definitions::VariableType::from(variable_type),
            key,
            value: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            value_body: Vec::new(),
            guess: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            guess_body: Vec::new(),
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x03\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        let mut equation = |id, function, arguments: &[u8]| {
            body.extend_from_slice(&[
                id,
                function,
                0xf8,
                u8::try_from(arguments.len()).expect("fixture value fits u8"),
            ]);
            body.extend_from_slice(arguments);
            body.extend_from_slice(b"\xf6\xe2");
        };
        equation(1, 0x02, &[0, 1]);
        equation(2, 0x00, &[2, 3, 4, 5, 1, 6]);
        let mut definition = FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: None,
            },
            body,
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                declared_count: 7,
                entity_ref: None,
                rows: vec![
                    row(3, 42, None),
                    row(0, 0, None),
                    row(1, 30, Some(1.0)),
                    row(2, 30, Some(1.0)),
                    row(1, 40, None),
                    row(2, 40, None),
                    row(4, 3, Some(0.0)),
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(FeatureDimensionTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![FeatureDimension {
                    dimension_type: 3,
                    value: crate::feature::definitions::DimensionValue::Resolved(2.0),
                    value_body: Vec::new(),
                    direction_byte: 0,
                    auxiliary_value: None,
                    auxiliary_body: Vec::new(),
                    external_id: 1,
                    references: None,
                    offset: 0,
                }],
                offset: 0,
            }),
            relations: None,
            saved_section: None,
            offset: 0,
        };

        definition.variables.as_mut().expect("variables").rows[1].value =
            crate::feature::definitions::ScalarLane::DimensionDriven;

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &definition))
                .expect("test section solve")
                .get(&40),
            Some(&[3.0, 1.0])
        );
    }

    #[test]
    fn relation_dimension_radius_feeds_polar_constraint() {
        let row = |variable_type, key, value: Option<f64>| FeatureVariableRow {
            variable_type: crate::feature::definitions::VariableType::from(variable_type),
            key,
            value: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            value_body: Vec::new(),
            guess: value.map_or(
                crate::feature::definitions::ScalarLane::Undefined,
                crate::feature::definitions::ScalarLane::Value,
            ),
            guess_body: Vec::new(),
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        body.extend_from_slice(b"\x01\x00\xf8\x06\x01\x02\x03\x04\x00\x05\xf6\xe2");
        let definition = FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(8),
                owner_feature_id: None,
            },
            body,
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                declared_count: 6,
                entity_ref: None,
                rows: vec![
                    row(3, 42, None),
                    row(1, 30, Some(1.0)),
                    row(2, 30, Some(1.0)),
                    row(1, 40, None),
                    row(2, 40, None),
                    row(4, 3, Some(0.0)),
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(FeatureDimensionTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![FeatureDimension {
                    dimension_type: 3,
                    value: crate::feature::definitions::DimensionValue::Resolved(2.0),
                    value_body: Vec::new(),
                    direction_byte: 0,
                    auxiliary_value: None,
                    auxiliary_body: Vec::new(),
                    external_id: 1,
                    references: None,
                    offset: 0,
                }],
                offset: 0,
            }),
            relations: Some(FeatureRelationTable {
                declared_count: 3,
                entity_ref: None,
                rows: vec![FeatureRelation {
                    relation_id: 1,
                    used: 1,
                    operands: Vec::new(),
                    operand_vectors: Some([
                        [Some(42), Some(0), Some(0), Some(0)],
                        [Some(0); 4],
                        [Some(15), Some(0), Some(0), Some(0)],
                    ]),
                    sign: 1,
                    dimension_id: 0,
                    relation_type: 14,
                    body: Vec::new(),
                    offset: 0,
                }],
                skamps: None,
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        };

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &definition))
                .expect("test section solve")
                .get(&40),
            Some(&[3.0, 1.0])
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_scalar_values(
                ctx,
                &definition
            ))
            .expect("test section solve")
            .get(&(crate::feature::definitions::VariableType::Radius, 42)),
            Some(&2.0)
        );
    }
}
