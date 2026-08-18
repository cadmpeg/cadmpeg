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
    section_equal_length_coordinate_values, section_equation_equal_length_constraints,
    section_equation_point_on_line_constraints, section_equation_unsigned_coordinate_distances,
    solve_section_coordinate_equations, solve_unsigned_dimension_coordinates,
    SectionCoordinateEquation,
};
use super::equations_scalar::{
    append_section_equation_auxiliary_coordinate_constraints, merge_scalar_value_candidate,
    section_equation_auxiliary_constraints, section_equation_coordinate_equalities,
    section_equation_radial_constraints, section_equation_scalar_seed_values,
    section_equation_scalar_values_from_coordinates,
};
use super::geometry::{saved_section_circle_values, saved_section_segment_point_coordinates};
use super::radii::section_relation_length_dimension;
use super::skamp::{
    section_line_fixed_coordinate, section_skamp_axis_symmetry, section_skamp_coincidence_point,
    section_skamp_point_entity_id, section_skamp_point_on_line, section_skamp_point_symmetry,
    section_skamp_saved_point_on_line, SectionPointSource, SectionSymmetryAxis,
};

const EPS_SECTION_COORDINATE: f64 = 1e-9;

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
                    section_skamp_coincidence_point(definition, first)?,
                    section_skamp_coincidence_point(definition, second)?,
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
                            section_skamp_coincidence_point(definition, second)?,
                        ]),
                        (None, Some(point)) => Some([
                            section_skamp_coincidence_point(definition, first)?,
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
    for (target, first, second) in
        section_equation_point_on_line_constraints(definition, &ambiguous_point_ids)
    {
        let (
            Some([Some(first_u), Some(first_v)]),
            Some([Some(second_u), Some(second_v)]),
            Some(target_coordinates),
        ) = (points.get(&first), points.get(&second), points.get(&target))
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
        if missing_coefficient > 1e-12 {
            equations.push(equation);
        }
    }
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
    let mut solved_coordinates =
        solve_section_coordinate_equations(&equations, &stored_coordinates);
    for _ in 0..equal_length_constraints.len() {
        let equal_length_values =
            section_equal_length_coordinate_values(&equal_length_constraints, &solved_coordinates);
        let mut added = false;
        for (variable, value) in equal_length_values {
            let Some(value) = value else {
                continue;
            };
            equations.push(SectionCoordinateEquation::point_value(
                variable.0, variable.1, value,
            ));
            added = true;
        }
        if !added {
            break;
        }
        solved_coordinates = solve_section_coordinate_equations(&equations, &stored_coordinates);
    }
    for constraint in
        section_equation_radial_constraints(definition, &solved_coordinates, &ambiguous_point_ids)
    {
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
    let solved_coordinates = solve_section_coordinate_equations(&equations, &stored_coordinates);
    for (variable, value) in
        section_equation_scalar_values_from_coordinates(definition, &solved_coordinates)
    {
        merge_scalar_value_candidate(&mut auxiliary_scalar_values, variable, value);
    }
    append_section_equation_auxiliary_coordinate_constraints(
        &auxiliary_constraints,
        &auxiliary_scalar_values,
        &stored_coordinates,
        &mut equations,
    );
    let solved_coordinates = solve_section_coordinate_equations(&equations, &stored_coordinates);
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
        if let Some(fixed_coordinate) = section_line_fixed_coordinate(definition, segment) {
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
        FeatureSolverTableHeader, FeatureVariableTable,
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
}
