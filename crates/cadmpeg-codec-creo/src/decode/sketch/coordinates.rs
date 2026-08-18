// SPDX-License-Identifier: Apache-2.0
//! Resolved section point coordinates from variables, dimensions, and equations.

use std::collections::{BTreeMap, BTreeSet};

use super::super::feature_history::feature_relation_table_complete;
use super::super::sketch_transfer::{
    active_complete_section_skamps, section_linear_distance_vectors,
    section_skamp_arc_midpoint_source, section_skamp_line_midpoint_sources,
    section_skamp_same_coordinate_sources, section_solver_relation_is_disabled,
};
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

const EPS_SECTION_COORDINATE: f64 = 1e-9;
const EPS_POINT_ON_LINE_COEFFICIENT: f64 = 1e-12;

pub(crate) fn saved_section_coordinate_witnesses(
    definition: &crate::feature::FeatureDefinition,
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Vec<(u32, [f64; 2])> {
    let mut witnesses = definition
        .segments
        .iter()
        .flat_map(|table| {
            table
                .rows
                .iter()
                .filter(|segment| table.external_id_count(segment.external_id) == 1)
        })
        .filter(|segment| {
            segment
                .point_ids
                .iter()
                .all(|point_id| !ambiguous_point_ids.contains(point_id))
        })
        .filter_map(|segment| saved_section_segment_point_coordinates(definition, segment))
        .flatten()
        .collect::<Vec<_>>();
    witnesses.extend(
        definition
            .segments
            .iter()
            .flat_map(|table| &table.circle_rows)
            .filter_map(|segment| {
                (!ambiguous_point_ids.contains(&segment.center_id)).then_some(())?;
                let (center, _) = saved_section_circle_values(definition, segment)?;
                Some((segment.center_id, center))
            }),
    );
    witnesses
}

fn append_point_on_line_equations(
    constraints: &[(u32, u32, u32)],
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    equations: &mut Vec<SectionCoordinateEquation>,
) -> bool {
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
        equation.add_point(target, 0, -delta_v);
        equation.add_point(target, 1, delta_u);
        equation.rhs = delta_u * first_v - delta_v * first_u;
        let missing_coefficient = if target_u.is_none() {
            delta_v.abs()
        } else {
            delta_u.abs()
        };
        if missing_coefficient > EPS_POINT_ON_LINE_COEFFICIENT
            && !equations.iter().any(|candidate| {
                candidate.terms == equation.terms
                    && approximately_equal(candidate.rhs, equation.rhs)
            })
        {
            equations.push(equation);
            appended = true;
        }
    }
    appended
}

fn append_equal_length_coordinate_values(
    constraints: &[SectionEqualLengthConstraint],
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    equations: &mut Vec<SectionCoordinateEquation>,
) -> bool {
    let mut appended = false;
    for (variable, value) in section_equal_length_coordinate_values(constraints, coordinates) {
        let Some(value) = value else {
            continue;
        };
        let equation = SectionCoordinateEquation::point_value(variable.0, variable.1, value);
        if equations.iter().any(|candidate| {
            candidate.terms == equation.terms && approximately_equal(candidate.rhs, equation.rhs)
        }) {
            continue;
        }
        equations.push(equation);
        appended = true;
    }
    appended
}

fn append_unique_auxiliary_coordinate_constraints(
    constraints: &SectionEquationAuxiliaryConstraints,
    scalar_values: &BTreeMap<SectionScalarVariable, Option<f64>>,
    stored_coordinates: &BTreeMap<(u32, usize), f64>,
    equations: &mut Vec<SectionCoordinateEquation>,
) -> bool {
    let previous_len = equations.len();
    append_section_equation_auxiliary_coordinate_constraints(
        constraints,
        scalar_values,
        stored_coordinates,
        equations,
    );
    let pending = equations.drain(previous_len..).collect::<Vec<_>>();
    let mut appended = false;
    for equation in pending {
        if equations.iter().any(|candidate| {
            candidate.terms == equation.terms && approximately_equal(candidate.rhs, equation.rhs)
        }) {
            continue;
        }
        equations.push(equation);
        appended = true;
    }
    appended
}

fn solve_section_coordinates_with_derived_constraints(
    definition: &crate::feature::FeatureDefinition,
    equations: &mut Vec<SectionCoordinateEquation>,
    stored_coordinates: &BTreeMap<(u32, usize), f64>,
    point_on_line_constraints: &[(u32, u32, u32)],
    equal_length_constraints: &[SectionEqualLengthConstraint],
    auxiliary_constraints: &SectionEquationAuxiliaryConstraints,
    auxiliary_scalar_values: &mut BTreeMap<SectionScalarVariable, Option<f64>>,
) -> BTreeMap<u32, [Option<f64>; 2]> {
    let mut solved_coordinates = solve_section_coordinate_equations(equations, stored_coordinates);
    let max_passes = point_on_line_constraints
        .len()
        .saturating_add(equal_length_constraints.len())
        .saturating_add(auxiliary_constraints.midpoints.len())
        .saturating_add(auxiliary_constraints.point_bindings.len().saturating_mul(2))
        .saturating_add(1);
    for _ in 0..max_passes {
        let mut appended = false;
        if append_point_on_line_equations(point_on_line_constraints, &solved_coordinates, equations)
        {
            appended = true;
            solved_coordinates = solve_section_coordinate_equations(equations, stored_coordinates);
        }
        if append_equal_length_coordinate_values(
            equal_length_constraints,
            &solved_coordinates,
            equations,
        ) {
            appended = true;
            solved_coordinates = solve_section_coordinate_equations(equations, stored_coordinates);
        }
        let previous_scalar_values = auxiliary_scalar_values.clone();
        for (variable, value) in
            section_equation_scalar_values_from_coordinates(definition, &solved_coordinates)
        {
            merge_scalar_value_candidate(auxiliary_scalar_values, variable, value);
        }
        propagate_section_equation_scalar_equality_values(definition, auxiliary_scalar_values);
        if *auxiliary_scalar_values != previous_scalar_values
            && append_unique_auxiliary_coordinate_constraints(
                auxiliary_constraints,
                auxiliary_scalar_values,
                stored_coordinates,
                equations,
            )
        {
            appended = true;
            solved_coordinates = solve_section_coordinate_equations(equations, stored_coordinates);
        }
        if !appended {
            break;
        }
    }
    solved_coordinates
}

pub(crate) fn resolved_section_coordinates(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, [Option<f64>; 2]> {
    let (points, ambiguous_point_ids) = match &definition.variables {
        Some(variables) if variables.is_complete() => variables.reconciled_points(),
        Some(_) => (BTreeMap::new(), BTreeSet::new()),
        None => (BTreeMap::new(), BTreeSet::new()),
    };
    let mut segment_counts = BTreeMap::new();
    for segment in definition.segments.iter().flat_map(|table| &table.rows) {
        *segment_counts.entry(segment.external_id).or_insert(0usize) += 1;
    }
    let saved_segment_points = saved_section_coordinate_witnesses(definition, &ambiguous_point_ids);
    let segments = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .filter(|segment| segment_counts[&segment.external_id] == 1)
        .filter(|segment| {
            segment
                .point_ids
                .iter()
                .all(|point_id| !ambiguous_point_ids.contains(point_id))
        })
        .collect::<Vec<_>>();
    let coincident_points = active_complete_section_skamps(definition)
        .filter_map(|skamp| {
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
        })
        .collect::<Vec<_>>();
    let same_coordinate_points = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_same_coordinate_sources(definition, skamp))
        .filter(|(pair, _)| {
            pair.iter()
                .any(|point| matches!(point, SectionPointSource::Point(_)))
                && pair.iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                })
        })
        .collect::<Vec<_>>();
    let point_on_line_coordinates = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_point_on_line(definition, skamp))
        .filter(|(first, second, _)| {
            !ambiguous_point_ids.contains(first) && !ambiguous_point_ids.contains(second)
        })
        .collect::<Vec<_>>();
    let saved_point_on_line_coordinates = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_saved_point_on_line(definition, skamp))
        .filter(|(point_id, _, _)| !ambiguous_point_ids.contains(point_id))
        .collect::<Vec<_>>();
    let line_midpoint_constraints = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_line_midpoint_sources(definition, skamp))
        .filter(|(point_sources, point)| {
            point_sources.iter().all(|source| match source {
                SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                SectionPointSource::Value(_) => true,
            }) && match point {
                SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                SectionPointSource::Value(_) => true,
            }
        })
        .collect::<Vec<_>>();
    let symmetric_point_constraints = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_axis_symmetry(definition, skamp))
        .filter(|(axis, first, second, _)| {
            [first, second]
                .into_iter()
                .any(|point| matches!(point, SectionPointSource::Point(_)))
                && [first, second].into_iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                })
                && match axis {
                    SectionSymmetryAxis::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionSymmetryAxis::Value(_) => true,
                }
        })
        .collect::<Vec<_>>();
    let point_symmetric_constraints = active_complete_section_skamps(definition)
        .filter_map(|skamp| section_skamp_point_symmetry(definition, skamp))
        .filter(|(center, first, second)| {
            !ambiguous_point_ids.contains(center)
                && [first, second].into_iter().all(|point| match point {
                    SectionPointSource::Point(point_id) => !ambiguous_point_ids.contains(point_id),
                    SectionPointSource::Value(_) => true,
                })
        })
        .collect::<Vec<_>>();
    let auxiliary_constraints =
        section_equation_auxiliary_constraints(definition, &ambiguous_point_ids);
    let mut auxiliary_scalar_values = section_equation_scalar_seed_values(definition);
    propagate_section_equation_scalar_equality_values(definition, &mut auxiliary_scalar_values);
    let linear_dimension_candidates = definition
        .relations
        .iter()
        .filter(|table| feature_relation_table_complete(table))
        .flat_map(|table| &table.rows)
        .filter_map(|relation| {
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
            let coordinate = section_linear_distance_coordinate(
                definition,
                &segments,
                first,
                second,
                &points,
                &saved_segment_points,
                &ambiguous_point_ids,
            )?;
            let magnitude = section_relation_length_dimension(definition, relation)?
                .value
                .filter(|value| value.is_finite() && *value >= 0.0)?;
            matches!(relation.sign, 0 | 1 | 0xf6).then_some((
                first,
                second,
                coordinate,
                magnitude,
                relation.sign,
            ))
        })
        .collect::<Vec<_>>();
    let signed_dimension_candidates = linear_dimension_candidates
        .iter()
        .filter_map(|&(first, second, coordinate, magnitude, sign)| {
            let delta = match sign {
                1 => magnitude,
                0xf6 => -magnitude,
                _ => return None,
            };
            Some((first, second, coordinate, delta))
        })
        .collect::<Vec<_>>();
    let mut unsigned_dimension_candidates = linear_dimension_candidates
        .iter()
        .filter_map(|&(first, second, coordinate, magnitude, sign)| {
            (sign == 0).then_some((first, second, coordinate, magnitude))
        })
        .collect::<Vec<_>>();
    unsigned_dimension_candidates.extend(
        section_equation_unsigned_coordinate_distances(definition, &ambiguous_point_ids)
            .into_iter()
            .map(|constraint| {
                (
                    constraint.first,
                    constraint.second,
                    constraint.coordinate,
                    constraint.value,
                )
            }),
    );
    let radial_constraints =
        section_equation_radial_constraints(definition, &points, &ambiguous_point_ids);
    let equal_length_constraints =
        section_equation_equal_length_constraints(definition, &ambiguous_point_ids);
    let mut signed_dimensions = BTreeMap::<(u32, u32, usize), Option<f64>>::new();
    for (first, second, coordinate, delta) in signed_dimension_candidates {
        let (key, canonical_delta) = if first <= second {
            ((first, second, coordinate), delta)
        } else {
            ((second, first, coordinate), -delta)
        };
        signed_dimensions
            .entry(key)
            .and_modify(|stored| {
                if stored.is_some_and(|stored| stored != canonical_delta) {
                    *stored = None;
                }
            })
            .or_insert(Some(canonical_delta));
    }
    let signed_dimensions = signed_dimensions
        .into_iter()
        .filter_map(|((first, second, coordinate), delta)| {
            Some((first, second, coordinate, delta?))
        })
        .collect::<Vec<_>>();
    let mut equations = Vec::new();
    for (&point_id, coordinates) in &points {
        for (coordinate, value) in coordinates.iter().copied().enumerate() {
            if let Some(value) = value {
                equations.push(SectionCoordinateEquation::point_value(
                    point_id, coordinate, value,
                ));
            }
        }
    }
    for &(point_id, coordinates) in &saved_segment_points {
        for (coordinate, value) in coordinates.into_iter().enumerate() {
            equations.push(SectionCoordinateEquation::point_value(
                point_id, coordinate, value,
            ));
        }
    }
    for segment in &segments {
        if let Some(coordinate) = section_line_fixed_coordinate(definition, segment) {
            equations.push(SectionCoordinateEquation::point_difference(
                segment.point_ids[0],
                segment.point_ids[1],
                coordinate,
                0.0,
            ));
        }
    }
    for &(first, second, coordinate, delta) in &signed_dimensions {
        equations.push(SectionCoordinateEquation::point_difference(
            first, second, coordinate, delta,
        ));
    }
    for &[first, second] in &coincident_points {
        for coordinate in 0..2 {
            equations.push(SectionCoordinateEquation::source_difference(
                first, second, coordinate, 0.0,
            ));
        }
    }
    for (first, second, coordinate) in
        section_equation_coordinate_equalities(definition, &ambiguous_point_ids)
    {
        equations.push(SectionCoordinateEquation::point_difference(
            first, second, coordinate, 0.0,
        ));
    }
    let point_on_line_constraints =
        section_equation_point_on_line_constraints(definition, &ambiguous_point_ids);
    for constraint in &radial_constraints {
        if let Some(offset) = constraint.offset() {
            equations.push(SectionCoordinateEquation::point_difference(
                constraint.first,
                constraint.second,
                0,
                offset[0],
            ));
            equations.push(SectionCoordinateEquation::point_difference(
                constraint.first,
                constraint.second,
                1,
                offset[1],
            ));
        }
    }
    for &([first, second], coordinate) in &same_coordinate_points {
        equations.push(SectionCoordinateEquation::source_difference(
            first, second, coordinate, 0.0,
        ));
    }
    for &(first, second, coordinate) in &point_on_line_coordinates {
        equations.push(SectionCoordinateEquation::point_difference(
            first, second, coordinate, 0.0,
        ));
    }
    for &(point, coordinate, value) in &saved_point_on_line_coordinates {
        equations.push(SectionCoordinateEquation::point_value(
            point, coordinate, value,
        ));
    }
    for &(point_sources, point) in &line_midpoint_constraints {
        for coordinate in 0..2 {
            let mut equation = SectionCoordinateEquation::default();
            equation.add_source(point_sources[0], coordinate, 1.0);
            equation.add_source(point_sources[1], coordinate, 1.0);
            equation.add_source(point, coordinate, -2.0);
            equations.push(equation);
        }
    }
    for &(axis, first, second, fixed_coordinate) in &symmetric_point_constraints {
        let parallel_coordinate = 1usize.saturating_sub(fixed_coordinate);
        equations.push(SectionCoordinateEquation::source_difference(
            first,
            second,
            parallel_coordinate,
            0.0,
        ));
        let mut equation = SectionCoordinateEquation::default();
        equation.add_source(first, fixed_coordinate, 1.0);
        equation.add_source(second, fixed_coordinate, 1.0);
        match axis {
            SectionSymmetryAxis::Point(point_id) => {
                equation.add_point(point_id, fixed_coordinate, -2.0);
            }
            SectionSymmetryAxis::Value(value) => equation.rhs += 2.0 * value,
        }
        equations.push(equation);
    }
    for &(center, first, second) in &point_symmetric_constraints {
        for coordinate in 0..2 {
            let mut equation = SectionCoordinateEquation::default();
            equation.add_source(first, coordinate, 1.0);
            equation.add_source(second, coordinate, 1.0);
            equation.add_point(center, coordinate, -2.0);
            equations.push(equation);
        }
    }
    let stored_coordinates = points
        .iter()
        .flat_map(|(&point, coordinates)| {
            coordinates
                .iter()
                .copied()
                .enumerate()
                .filter_map(move |(coordinate, value)| Some(((point, coordinate), value?)))
        })
        .collect();
    append_section_equation_auxiliary_coordinate_constraints(
        &auxiliary_constraints,
        &auxiliary_scalar_values,
        &stored_coordinates,
        &mut equations,
    );
    let unsigned_coordinates = solve_unsigned_dimension_coordinates(
        &equations,
        &stored_coordinates,
        &unsigned_dimension_candidates,
    );
    for ((point, coordinate), value) in unsigned_coordinates {
        equations.push(SectionCoordinateEquation::point_value(
            point, coordinate, value,
        ));
    }
    let solved_coordinates = solve_section_coordinates_with_derived_constraints(
        definition,
        &mut equations,
        &stored_coordinates,
        &point_on_line_constraints,
        &equal_length_constraints,
        &auxiliary_constraints,
        &mut auxiliary_scalar_values,
    );
    for constraint in section_equation_radial_constraints_with_scalar_values(
        definition,
        &solved_coordinates,
        &ambiguous_point_ids,
        &auxiliary_scalar_values,
    ) {
        if let Some(offset) = constraint.offset() {
            equations.push(SectionCoordinateEquation::point_difference(
                constraint.first,
                constraint.second,
                0,
                offset[0],
            ));
            equations.push(SectionCoordinateEquation::point_difference(
                constraint.first,
                constraint.second,
                1,
                offset[1],
            ));
        }
    }
    let second_unsigned_coordinates = solve_unsigned_dimension_coordinates(
        &equations,
        &stored_coordinates,
        &unsigned_dimension_candidates,
    );
    for ((point, coordinate), value) in second_unsigned_coordinates {
        equations.push(SectionCoordinateEquation::point_value(
            point, coordinate, value,
        ));
    }
    let solved_coordinates = solve_section_coordinates_with_derived_constraints(
        definition,
        &mut equations,
        &stored_coordinates,
        &point_on_line_constraints,
        &equal_length_constraints,
        &auxiliary_constraints,
        &mut auxiliary_scalar_values,
    );
    let arc_midpoint_constraints = active_complete_section_skamps(definition)
        .filter_map(|skamp| {
            section_skamp_arc_midpoint_source(definition, skamp, &solved_coordinates)
        })
        .filter_map(|(point, midpoint)| match point {
            SectionPointSource::Point(point_id) if !ambiguous_point_ids.contains(&point_id) => {
                Some((point_id, midpoint))
            }
            SectionPointSource::Point(_) | SectionPointSource::Value(_) => None,
        })
        .collect::<Vec<_>>();
    for &(point_id, midpoint) in &arc_midpoint_constraints {
        for (coordinate, value) in midpoint.into_iter().enumerate() {
            equations.push(SectionCoordinateEquation::point_value(
                point_id, coordinate, value,
            ));
        }
    }
    solve_section_coordinate_equations(&equations, &stored_coordinates)
}

pub(crate) fn section_linear_distance_coordinate(
    definition: &crate::feature::FeatureDefinition,
    segments: &[&crate::feature::FeatureSegment],
    first: u32,
    second: u32,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    saved_segment_points: &[(u32, [f64; 2])],
    ambiguous_point_ids: &BTreeSet<u32>,
) -> Option<usize> {
    let matching_segments = segments
        .iter()
        .copied()
        .filter(|segment| {
            segment.point_ids == [first, second] || segment.point_ids == [second, first]
        })
        .collect::<Vec<_>>();
    let point_coordinate = |point_id: u32, coordinate: usize| -> Result<Option<f64>, ()> {
        if ambiguous_point_ids.contains(&point_id) {
            return Err(());
        }
        let mut values = Vec::new();
        if let Some(value) = coordinates
            .get(&point_id)
            .and_then(|point| point[coordinate])
        {
            value.is_finite().then_some(()).ok_or(())?;
            values.push(value);
        }
        for &(_, point) in saved_segment_points
            .iter()
            .filter(|(saved_point_id, _)| *saved_point_id == point_id)
        {
            let value = point[coordinate];
            value.is_finite().then_some(()).ok_or(())?;
            values.push(value);
        }
        let Some(first) = values.first().copied() else {
            return Ok(None);
        };
        let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
        values
            .iter()
            .all(|value| (*value - first).abs() <= EPS_SECTION_COORDINATE * scale)
            .then_some(Some(first))
            .ok_or(())
    };
    if let [segment] = matching_segments.as_slice() {
        if let Some(fixed_coordinate) =
            section_line_entity_fixed_coordinate_with_unique_rows(definition, segment.external_id)
        {
            let Ok(first_coordinate) = point_coordinate(first, fixed_coordinate) else {
                return None;
            };
            let Ok(second_coordinate) = point_coordinate(second, fixed_coordinate) else {
                return None;
            };
            if let (Some(first), Some(second)) = (first_coordinate, second_coordinate) {
                let scale = first.abs().max(second.abs()).max(1.0);
                if (first - second).abs() > EPS_SECTION_COORDINATE * scale {
                    return None;
                }
            }
            return 1usize.checked_sub(fixed_coordinate);
        }
    }
    if matching_segments.len() > 1 {
        return None;
    }
    let table = definition.segments.as_ref()?;
    // Only decoded point-bearing families establish that a dimension operand
    // is a section endpoint. Opaque rows retain native identity but do not
    // prove an endpoint role.
    let has_unique_incident_entity = |point_id| {
        table.rows.iter().any(|segment| {
            segment.point_ids.contains(&point_id)
                && table.external_id_count(segment.external_id) == 1
        }) || table.point_rows.iter().any(|segment| {
            segment.point_id == point_id && table.external_id_count(segment.external_id) == 1
        }) || (matches!(point_id, 0 | 1)
            && table
                .centered_line_rows
                .iter()
                .any(|segment| table.external_id_count(segment.external_id) == 1))
            || table.reference_line_rows.iter().any(|segment| {
                segment.point_ids.contains(&Some(point_id))
                    && table.external_id_count(segment.external_id) == 1
            })
            || table.bounded_curve_rows.iter().any(|segment| {
                segment.point_ids.contains(&point_id)
                    && table.external_id_count(segment.external_id) == 1
            })
    };
    has_unique_incident_entity(first).then_some(())?;
    has_unique_incident_entity(second).then_some(())?;
    let equal_coordinate = |coordinate: usize| -> Option<bool> {
        let first = point_coordinate(first, coordinate).ok().flatten()?;
        let second = point_coordinate(second, coordinate).ok().flatten()?;
        let scale = first.abs().max(second.abs()).max(1.0);
        Some((first - second).abs() <= EPS_SECTION_COORDINATE * scale)
    };
    let equal_u = equal_coordinate(0);
    let equal_v = equal_coordinate(1);
    if equal_u == Some(true) && equal_v != Some(true) {
        return Some(1);
    }
    if equal_v == Some(true) && equal_u != Some(true) {
        return Some(0);
    }
    None
}

pub(crate) fn resolved_section_points(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, [f64; 2]> {
    resolved_section_coordinates(definition)
        .into_iter()
        .filter_map(|(point, [u, v])| Some((point, [u?, v?])))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::resolved_section_points;
    use crate::feature::{
        FeatureDefinition, FeaturePointSegment, FeatureRelationTable, FeatureSectionPoint,
        FeatureSegment, FeatureSegmentKind, FeatureSegmentTable, FeatureSkamp, FeatureSkampItem,
        FeatureSolverTableHeader, FeatureVariableRow, FeatureVariableTable,
    };

    fn incomplete_segment_definition() -> FeatureDefinition {
        FeatureDefinition {
            id: 1,
            owner_feature_id: None,
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                points: vec![
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
                offset: 0,
            }),
            segments: Some(FeatureSegmentTable {
                declared_count: 3,
                has_elided_prototype: false,
                entity_ref: None,
                rows: vec![FeatureSegment {
                    kind: FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [1, 2],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: None,
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 7,
                    body: Vec::new(),
                    offset: 0,
                }],
                circle_rows: Vec::new(),
                point_rows: vec![FeaturePointSegment {
                    point_id: 2,
                    external_id: 8,
                    offset: 1,
                }],
                centered_line_rows: Vec::new(),
                reference_line_rows: Vec::new(),
                bounded_curve_rows: Vec::new(),
                conic_rows: Vec::new(),
                opaque_rows: Vec::new(),
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
                skamps: vec![
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
                skamp_header: Some(FeatureSolverTableHeader {
                    declared_count: 2,
                    entity_ref: 0,
                    offset: 0,
                }),
                triples: Vec::new(),
                triples_header: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        }
    }

    #[test]
    fn incomplete_unique_ordinary_rows_supply_coincidence_point_ids() {
        let definition = incomplete_segment_definition();
        assert_eq!(
            resolved_section_points(&definition).get(&2),
            Some(&[2.0, 3.0])
        );

        let mut duplicate_ordinary = definition.clone();
        let duplicate = duplicate_ordinary.segments.as_ref().expect("segments").rows[0].clone();
        duplicate_ordinary
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(FeatureSegment {
                offset: 2,
                ..duplicate
            });
        assert!(!resolved_section_points(&duplicate_ordinary).contains_key(&2));

        let mut duplicate_family = definition;
        duplicate_family
            .segments
            .as_mut()
            .expect("segments")
            .point_rows
            .push(FeaturePointSegment {
                point_id: 1,
                external_id: 7,
                offset: 2,
            });
        assert!(!resolved_section_points(&duplicate_family).contains_key(&2));
    }

    #[test]
    fn incomplete_unique_spanning_line_selector_supplies_distance_axis() {
        let mut definition = incomplete_segment_definition();
        definition.segments.as_mut().expect("segments").rows[0].vertical_horizontal = Some(0);
        let segments = definition
            .segments
            .as_ref()
            .expect("segments")
            .rows
            .iter()
            .collect::<Vec<_>>();
        assert_eq!(
            super::section_linear_distance_coordinate(
                &definition,
                &segments,
                1,
                2,
                &std::collections::BTreeMap::new(),
                &[],
                &std::collections::BTreeSet::new(),
            ),
            Some(1)
        );

        let mut duplicate = definition;
        let duplicate_row = duplicate.segments.as_ref().expect("segments").rows[0].clone();
        duplicate
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(FeatureSegment {
                offset: 2,
                ..duplicate_row
            });
        let duplicate_segments = duplicate
            .segments
            .as_ref()
            .expect("segments")
            .rows
            .iter()
            .collect::<Vec<_>>();
        assert_eq!(
            super::section_linear_distance_coordinate(
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
    fn point_on_line_retries_after_auxiliary_reference_coordinates_resolve() {
        let row = |variable_type, key, value| FeatureVariableRow {
            variable_type,
            key,
            value,
            value_body: Vec::new(),
            guess: value,
            guess_body: Vec::new(),
            guess_dimension_driven: false,
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            dimension_driven: false,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        body.extend_from_slice(b"\x01\x23\xf8\x09\x00\x01\x02\x03\x04\x05\x06\x07\x08\xf6\xe2");
        body.extend_from_slice(b"\x02\x1f\xf8\x04\x02\x03\x09\x0a\xf6\xe2");
        body.extend_from_slice(b"\x03\x1f\xf8\x04\x04\x05\x0b\x0c\xf6\xe2");
        let definition = FeatureDefinition {
            id: 2,
            owner_feature_id: None,
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
                points: Vec::new(),
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
            resolved_section_points(&definition).get(&30),
            Some(&[5.0, 5.0])
        );
    }

    #[test]
    fn equal_length_retries_after_derived_auxiliary_reference_coordinates_resolve() {
        let row = |variable_type, key, value| FeatureVariableRow {
            variable_type,
            key,
            value,
            value_body: Vec::new(),
            guess: value,
            guess_body: Vec::new(),
            guess_dimension_driven: false,
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            dimension_driven: false,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x08\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        let mut equation = |id, function, arguments: &[u8]| {
            body.extend_from_slice(&[id, function, 0xf8, arguments.len() as u8]);
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
            id: 3,
            owner_feature_id: None,
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
                points: Vec::new(),
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
            resolved_section_points(&definition).get(&30),
            Some(&[0.0, 4.0])
        );
    }

    #[test]
    fn derived_auxiliary_values_retry_after_point_on_line_resolution() {
        let row = |variable_type, key, value| FeatureVariableRow {
            variable_type,
            key,
            value,
            value_body: Vec::new(),
            guess: value,
            guess_body: Vec::new(),
            guess_dimension_driven: false,
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            dimension_driven: false,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x07\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        let mut equation = |id, function, arguments: &[u8]| {
            body.extend_from_slice(&[id, function, 0xf8, arguments.len() as u8]);
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
            id: 4,
            owner_feature_id: None,
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
                points: Vec::new(),
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
            resolved_section_points(&definition).get(&40),
            Some(&[4.0, 0.0])
        );
    }

    #[test]
    fn derived_auxiliary_values_cross_scalar_equalities() {
        let row = |variable_type, key, value| FeatureVariableRow {
            variable_type,
            key,
            value,
            value_body: Vec::new(),
            guess: value,
            guess_body: Vec::new(),
            guess_dimension_driven: false,
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            dimension_driven: false,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        let mut equation = |id, function, arguments: &[u8]| {
            body.extend_from_slice(&[id, function, 0xf8, arguments.len() as u8]);
            body.extend_from_slice(arguments);
            body.extend_from_slice(b"\xf6\xe2");
        };
        equation(1, 0x2a, &[0, 1, 2]);
        equation(2, 0x02, &[2, 3]);
        equation(3, 0x1f, &[5, 6, 3, 4]);
        let definition = FeatureDefinition {
            id: 5,
            owner_feature_id: None,
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
                points: Vec::new(),
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
            resolved_section_points(&definition).get(&30),
            Some(&[2.0, 3.0])
        );
    }

    #[test]
    fn derived_axis_distance_feeds_equal_radius_polar_constraint() {
        let row = |variable_type, key, value| FeatureVariableRow {
            variable_type,
            key,
            value,
            value_body: Vec::new(),
            guess: value,
            guess_body: Vec::new(),
            guess_dimension_driven: false,
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            dimension_driven: false,
            offset: 0,
        };
        let mut body = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2"
            .to_vec();
        let mut equation = |id, function, arguments: &[u8]| {
            body.extend_from_slice(&[id, function, 0xf8, arguments.len() as u8]);
            body.extend_from_slice(arguments);
            body.extend_from_slice(b"\xf6\xe2");
        };
        equation(1, 0x2b, &[0, 1, 2, 3, 4, 5, 6, 7]);
        equation(2, 0x02, &[6, 8]);
        equation(3, 0x00, &[9, 10, 11, 12, 8, 13]);
        let definition = FeatureDefinition {
            id: 6,
            owner_feature_id: None,
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
                points: Vec::new(),
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
            resolved_section_points(&definition).get(&40),
            Some(&[5.0, 1.0])
        );
    }
}
