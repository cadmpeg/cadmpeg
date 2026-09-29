// SPDX-License-Identifier: Apache-2.0
//! Resolved section radii and intersection carriers.

use super::axis::SectionAxis;

use crate::feature::definitions::VariableType;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::index_from_u32;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, PositiveLength};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};

use super::super::feature_history::dimensions::{
    feature_dimension_table_complete, feature_relation_table_complete,
};
use super::coordinates::{resolved_section_coordinates, resolved_section_points};
use super::equations_coordinate::{
    section_equation_function_six_distance_values, section_equation_radius_dimensions,
};
use super::equations_scalar::{
    section_equation_radial_constraints, section_equation_scalar_equality_components,
    section_relation_radius_scalar_values,
};
use super::geometry::{
    resolved_section_segment_geometry_with_missing_line, saved_section_arc_carrier,
    saved_section_circle_values, SectionArcCarrier,
};
use super::skamp::{
    section_line_entity_fixed_coordinate_with_unique_rows, unique_decoded_section_segment,
};
use crate::decode::sketch_transfer::constraints::section_solver_relation_is_disabled;
use crate::decode::sketch_transfer::identity::saved_section_entity_fallback_allowed;
use crate::decode::sketch_transfer::loci::{
    active_complete_section_skamps, section_degenerate_axis_line, section_saved_entity,
    unique_circle_segment,
};

const EPS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;

fn append_radius_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &mut BTreeMap<u32, Vec<f64>>,
    radius_id: u32,
    value: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    if !candidates.contains_key(&radius_id) {
        ctx.charge_collection_items(1, "creo radius candidate nodes")?;
    }
    let values = candidates.entry(radius_id).or_default();
    ctx.try_reserve_items(values, 1, "creo radius candidate values")?;
    values.push(value);
    Ok(())
}

fn link_radii(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    adjacency: &mut BTreeMap<u32, BTreeSet<u32>>,
    first: u32,
    second: u32,
) -> Result<(), cadmpeg_core::CodecError> {
    for (radius_id, neighbor) in [(first, second), (second, first)] {
        if !adjacency.contains_key(&radius_id) {
            ctx.charge_collection_items(1, "creo radius adjacency nodes")?;
        }
        let neighbors = adjacency.entry(radius_id).or_default();
        if !neighbors.contains(&neighbor) {
            ctx.charge_collection_items(1, "creo radius adjacency links")?;
            neighbors.insert(neighbor);
        }
    }
    Ok(())
}

pub(in crate::decode) fn resolved_section_radii(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeMap<u32, f64>, cadmpeg_core::CodecError> {
    let mut candidates = BTreeMap::<u32, Vec<f64>>::new();
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.circles())
    {
        if let Some((_, radius)) = saved_section_circle_values(definition, segment) {
            append_radius_candidate(ctx, &mut candidates, segment.radius_ref, radius)?;
        }
    }
    for row in definition
        .variables
        .iter()
        .filter(|table| table.is_complete())
        .flat_map(|table| &table.rows)
    {
        if row.variable_type == VariableType::Radius {
            if let Some(value) = row
                .value
                .value()
                .filter(|value| value.is_finite() && *value > 0.0)
            {
                append_radius_candidate(ctx, &mut candidates, row.key, value)?;
            }
        }
    }
    let radial_coordinates = resolved_section_coordinates(ctx, definition)?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .map(|variables| {
            variables
                .reconciled_points(ctx)
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    for constraint in section_equation_radial_constraints(
        ctx,
        definition,
        &radial_coordinates,
        &ambiguous_point_ids,
    )? {
        if constraint.radius.0 == VariableType::Radius {
            if let Some(value) = constraint.radius_value.filter(|value| value.get() > 0.0) {
                append_radius_candidate(ctx, &mut candidates, constraint.radius.1, value.get())?;
            }
        }
    }
    for (variable, value) in section_equation_function_six_distance_values(
        ctx,
        definition,
        &radial_coordinates,
        &ambiguous_point_ids,
    )? {
        if variable.0 == VariableType::Radius && value.is_finite() && value > 0.0 {
            append_radius_candidate(ctx, &mut candidates, variable.1, value)?;
        }
    }
    for constraint in section_equation_radius_dimensions(ctx, definition)?
        .into_iter()
        .filter(|constraint| constraint.active)
    {
        append_radius_candidate(
            ctx,
            &mut candidates,
            constraint.radius,
            constraint.value.get(),
        )?;
    }
    for relation in definition
        .relations
        .iter()
        .filter(|table| feature_relation_table_complete(table))
        .flat_map(|table| &table.rows)
    {
        if section_solver_relation_is_disabled(definition, relation.relation_id) {
            continue;
        }
        if matches!(relation.relation_type, 5 | 6) && relation.sign == 1 {
            let Some(_) = section_radius_relation_arc(definition, relation) else {
                continue;
            };
            let Some(dimension) = section_relation_length_dimension(definition, relation) else {
                continue;
            };
            let Some(value) = dimension
                .value
                .resolved()
                .filter(|value| value.is_finite() && *value > 0.0)
            else {
                continue;
            };
            let radius = match dimension.dimension_type {
                4 => value / 2.0,
                _ => value,
            };
            let Some(radius) = PositiveLength::new(radius) else {
                continue;
            };
            append_radius_candidate(ctx, &mut candidates, relation.dimension_id, radius.get())?;
        }
    }
    for ((_, radius_id), value) in section_relation_radius_scalar_values(ctx, definition)? {
        append_radius_candidate(ctx, &mut candidates, radius_id, value)?;
    }
    if let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|dimensions| feature_dimension_table_complete(dimensions))
    {
        for circle in definition
            .segments
            .iter()
            .flat_map(|segments| segments.rows.circles())
            .filter(|segment| {
                unique_circle_segment(definition, segment.external_id)
                    .is_some_and(|candidate| candidate == *segment)
            })
        {
            let radius_id = circle.radius_ref;
            let Some(dimension) = dimensions.rows.get(index_from_u32(radius_id)) else {
                continue;
            };
            let Some(value) = dimension
                .value
                .resolved()
                .filter(|value| value.is_finite() && *value > 0.0)
            else {
                continue;
            };
            let radius = match dimension.dimension_type {
                3 => value,
                4 => value / 2.0,
                _ => continue,
            };
            if let Some(radius) = PositiveLength::new(radius) {
                append_radius_candidate(ctx, &mut candidates, radius_id, radius.get())?;
            }
        }
    }
    let points = resolved_section_points(ctx, definition)?;
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.ordinary())
        .filter(|segment| {
            matches!(
                segment.kind,
                crate::feature::definitions::FeatureSegmentKind::Arc(_)
            )
        })
    {
        if unique_decoded_section_segment(definition, segment.external_id) != Some(segment) {
            continue;
        }
        let Some(radius_id) = segment.radius_ref else {
            continue;
        };
        let Some(center) = segment.center_id.and_then(|id| points.get(&id)) else {
            continue;
        };
        let endpoint_radii = || {
            segment
                .point_ids()
                .into_iter()
                .filter_map(|id| points.get(&id))
                .map(|point| (point[0] - center[0]).hypot(point[1] - center[1]))
                .filter(|radius| radius.is_finite() && *radius > EPS_RADIUS_NONZERO)
        };
        let Some(radius) = endpoint_radii().next() else {
            continue;
        };
        let scale = endpoint_radii().fold(radius, f64::max);
        if endpoint_radii()
            .all(|candidate| (candidate - radius).abs() <= EPS_RADIUS_AGREEMENT * scale)
        {
            append_radius_candidate(ctx, &mut candidates, radius_id, radius)?;
        }
    }
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    let mut invalid_scalar_radius_ids = BTreeSet::new();
    if let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    {
        for component in section_equation_scalar_equality_components(ctx, definition)? {
            if component
                .iter()
                .any(|&(variable_type, _)| variable_type != VariableType::Radius)
            {
                continue;
            }
            let invalid = component.iter().any(|&(variable_type, radius_id)| {
                variables.rows.iter().any(|row| {
                    row.variable_type == variable_type
                        && row.key == radius_id
                        && row
                            .value
                            .value()
                            .is_some_and(|value| !value.is_finite() || value <= 0.0)
                })
            });
            if invalid {
                for &(_, radius_id) in &component {
                    if !invalid_scalar_radius_ids.contains(&radius_id) {
                        ctx.charge_collection_items(1, "creo invalid radius nodes")?;
                        invalid_scalar_radius_ids.insert(radius_id);
                    }
                }
                continue;
            }
            let mut previous = None;
            for &(_, radius_id) in &component {
                if let Some(first) = previous {
                    link_radii(ctx, &mut adjacency, first, radius_id)?;
                }
                previous = Some(radius_id);
            }
        }
    }
    for skamp in active_complete_section_skamps(definition) {
        let [first, second] = skamp.items.as_slice() else {
            continue;
        };
        if skamp.kind != 6 || first.sense != 0 || second.sense != 0 {
            continue;
        }
        let Some(first_radius) = section_skamp_radius_source(definition, first) else {
            continue;
        };
        let Some(second_radius) = section_skamp_radius_source(definition, second) else {
            continue;
        };
        match (first_radius, second_radius) {
            (SectionRadiusSource::Reference(first), SectionRadiusSource::Reference(second)) => {
                link_radii(ctx, &mut adjacency, first, second)?;
            }
            (SectionRadiusSource::Reference(reference), SectionRadiusSource::Value(value))
            | (SectionRadiusSource::Value(value), SectionRadiusSource::Reference(reference)) => {
                append_radius_candidate(ctx, &mut candidates, reference, value.get())?;
            }
            (SectionRadiusSource::Value(_), SectionRadiusSource::Value(_)) => {}
        }
    }
    let mut remaining = BTreeSet::new();
    for radius_id in candidates.keys().chain(adjacency.keys()) {
        if !remaining.contains(radius_id) {
            ctx.charge_collection_items(1, "creo remaining radius nodes")?;
            remaining.insert(*radius_id);
        }
    }
    let mut radii = BTreeMap::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::new();
        ctx.charge_collection_items(1, "creo radius component nodes")?;
        component.insert(seed);
        let mut pending = std::collections::VecDeque::new();
        ctx.try_collection(1, "creo pending radius nodes", || pending.try_reserve(1))?;
        pending.push_back(seed);
        while let Some(radius_id) = pending.pop_front() {
            for neighbor in adjacency.get(&radius_id).into_iter().flatten() {
                if !component.contains(neighbor) {
                    ctx.charge_collection_items(1, "creo radius component nodes")?;
                    component.insert(*neighbor);
                    ctx.try_collection(1, "creo pending radius nodes", || pending.try_reserve(1))?;
                    pending.push_back(*neighbor);
                }
            }
        }
        if component
            .iter()
            .any(|radius_id| invalid_scalar_radius_ids.contains(radius_id))
        {
            remaining.retain(|radius_id| !component.contains(radius_id));
            continue;
        }
        let values = || {
            component
                .iter()
                .flat_map(|radius_id| candidates.get(radius_id).into_iter().flatten())
                .copied()
        };
        if let Some(value) = values().next() {
            let scale = values().fold(value, f64::max);
            if !values().all(|candidate| (candidate - value).abs() <= EPS_RADIUS_AGREEMENT * scale)
            {
                remaining.retain(|radius_id| !component.contains(radius_id));
                continue;
            }
            for radius_id in &component {
                if !radii.contains_key(radius_id) {
                    ctx.charge_collection_items(1, "creo resolved radius nodes")?;
                }
                radii.insert(*radius_id, value);
            }
        }
        remaining.retain(|radius_id| !component.contains(radius_id));
    }
    Ok(radii)
}

pub(super) fn section_relation_length_dimension<'a>(
    definition: &'a crate::feature::definitions::FeatureDefinition,
    relation: &crate::feature::definitions::FeatureRelation,
) -> Option<&'a crate::feature::definitions::FeatureDimension> {
    let dimension = definition
        .dimensions
        .as_ref()
        .filter(|table| feature_dimension_table_complete(table))?
        .rows
        .get(usize::try_from(relation.dimension_id).ok()?)?;
    matches!(dimension.dimension_type, 1..=5).then_some(dimension)
}

fn section_type5_radius_arc<'a>(
    definition: &'a crate::feature::definitions::FeatureDefinition,
    relation: &crate::feature::definitions::FeatureRelation,
) -> Option<&'a crate::feature::definitions::FeatureSegment> {
    (relation.relation_type == 5 && relation.sign == 1).then_some(())?;
    section_relation_length_dimension(definition, relation)?;
    let vectors = relation.operand_vectors?;
    let [Some(first_point), Some(0), Some(second_point), Some(0)] = vectors[0] else {
        return None;
    };
    let [Some(center), Some(10), Some(0), Some(1)] = vectors[1] else {
        return None;
    };
    if vectors[2] != [Some(16), Some(15), Some(0), Some(0)] {
        return None;
    }
    unique_section_radius_arc(
        definition,
        relation.dimension_id,
        first_point,
        second_point,
        center,
    )
}

fn section_type6_radius_arc<'a>(
    definition: &'a crate::feature::definitions::FeatureDefinition,
    relation: &crate::feature::definitions::FeatureRelation,
) -> Option<&'a crate::feature::definitions::FeatureSegment> {
    (relation.relation_type == 6 && relation.sign == 1).then_some(())?;
    section_relation_length_dimension(definition, relation)?;
    let vectors = relation.operand_vectors?;
    let [Some(first_point), Some(second_point), Some(0), Some(1)] = vectors[0] else {
        return None;
    };
    let [Some(center), Some(0), Some(0), Some(0)] = vectors[1] else {
        return None;
    };
    if !vectors[2].iter().all(Option::is_some) {
        return None;
    }
    unique_section_radius_arc(
        definition,
        relation.dimension_id,
        first_point,
        second_point,
        center,
    )
}

pub(in crate::decode) fn section_radius_relation_arc<'a>(
    definition: &'a crate::feature::definitions::FeatureDefinition,
    relation: &crate::feature::definitions::FeatureRelation,
) -> Option<&'a crate::feature::definitions::FeatureSegment> {
    match relation.relation_type {
        5 => section_type5_radius_arc(definition, relation),
        6 => section_type6_radius_arc(definition, relation),
        _ => None,
    }
}

fn unique_section_radius_arc(
    definition: &crate::feature::definitions::FeatureDefinition,
    dimension_id: u32,
    first_point: u32,
    second_point: u32,
    center: u32,
) -> Option<&crate::feature::definitions::FeatureSegment> {
    let table = definition.segments.as_ref()?;
    let mut matching = table.rows.ordinary().filter(|segment| {
        matches!(
            segment.kind,
            crate::feature::definitions::FeatureSegmentKind::Arc(_)
        ) && segment.radius_ref == Some(dimension_id)
            && segment.center_id == Some(center)
            && (segment.point_ids() == [first_point, second_point]
                || segment.point_ids() == [second_point, first_point])
            && table.rows.get(segment.external_id).is_some()
    });
    let segment = matching.next()?;
    matching.next().is_none().then_some(segment)
}

#[derive(Clone, Copy)]
enum SectionRadiusSource {
    Reference(u32),
    Value(PositiveLength),
}

fn section_skamp_radius_source(
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Option<SectionRadiusSource> {
    if let Some(circle) = unique_circle_segment(definition, item.entity_id) {
        return Some(SectionRadiusSource::Reference(circle.radius_ref));
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return matches!(
            segment.kind,
            crate::feature::definitions::FeatureSegmentKind::Arc(_)
        )
        .then_some(segment.radius_ref)
        .flatten()
        .map(SectionRadiusSource::Reference);
    }
    if !saved_section_entity_fallback_allowed(definition, item.entity_id) {
        return None;
    }
    let radius = match section_saved_entity(definition, item.entity_id)? {
        crate::feature::definitions::FeatureSavedEntity::Arc(arc) => arc.radius,
        crate::feature::definitions::FeatureSavedEntity::Circle(circle) => circle.radius,
        _ => None,
    }?;
    PositiveLength::new(radius).map(SectionRadiusSource::Value)
}

pub(super) fn section_arc_carrier(
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<SectionArcCarrier> {
    matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Arc(_)
    )
    .then_some(())?;
    let center = *points.get(&segment.center_id?)?;
    let radius = *radii.get(&segment.radius_ref?)?;
    SectionArcCarrier::new(center, radius)
}

pub(in crate::decode) fn section_axis_line_carrier_with_points(
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<SketchGeometry> {
    matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Line(_)
    )
    .then_some(())?;
    let fixed_coordinate = match segment.directions {
        [Some(0), _, _] => SectionAxis::U,
        [_, Some(0), _] => SectionAxis::V,
        _ => return None,
    };
    section_fixed_coordinate_line_carrier(variable_points, segment, fixed_coordinate)
}

fn section_fixed_coordinate_line_carrier(
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
    fixed_coordinate: SectionAxis,
) -> Option<SketchGeometry> {
    (matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Line(_)
    ))
    .then_some(())?;
    let endpoint = |id| variable_points.get(&id);
    let [first, second] = segment.point_ids().map(endpoint);
    let (Some(first), Some(second)) = (first, second) else {
        return None;
    };
    let (Some(first), Some(second)) = (
        first[fixed_coordinate.index()],
        second[fixed_coordinate.index()],
    ) else {
        return None;
    };
    let scale = first.abs().max(second.abs()).max(1.0);
    ((first - second).abs() <= EPS_RADIUS_AGREEMENT * scale)
        .then(|| {
            if fixed_coordinate == SectionAxis::U {
                SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                    origin: Point2::new(first, 0.0),
                    direction: Point2::new(0.0, 1.0),
                })
                .ok()
            } else {
                SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                    origin: Point2::new(0.0, first),
                    direction: Point2::new(1.0, 0.0),
                })
                .ok()
            }
        })
        .flatten()
}

fn section_proven_axis_line_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    if let Some(geometry) = section_axis_line_carrier_with_points(variable_points, segment) {
        Ok(Some(geometry))
    } else {
        let Some(coordinate) = section_line_entity_fixed_coordinate_with_unique_rows(
            ctx,
            definition,
            segment.external_id,
        )?
        else {
            return Ok(None);
        };
        Ok(section_fixed_coordinate_line_carrier(
            variable_points,
            segment,
            coordinate,
        ))
    }
}

pub(in crate::decode) fn section_axis_reference_line_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    if !section_degenerate_axis_line(definition, segment) {
        return section_proven_axis_line_carrier(ctx, definition, variable_points, segment);
    }
    Ok((|| {
        let fixed_coordinate = SectionAxis::from_selector(segment.vertical_horizontal?)?;
        let values = || {
            segment.point_ids().into_iter().filter_map(|point| {
                variable_points
                    .get(&point)?
                    .get(fixed_coordinate.index())
                    .copied()
                    .flatten()
            })
        };
        let expected_value_count = if segment.point_ids()[0] == segment.point_ids()[1] {
            1
        } else {
            2
        };
        (values().count() == expected_value_count).then_some(())?;
        let value = values().next()?;
        let scale = values().map(f64::abs).fold(value.abs().max(1.0), f64::max);
        values()
            .all(|candidate| (candidate - value).abs() <= EPS_RADIUS_AGREEMENT * scale)
            .then_some(())?;
        let (origin, direction) = if fixed_coordinate == SectionAxis::U {
            (Point2::new(value, 0.0), Point2::new(0.0, 1.0))
        } else {
            (Point2::new(0.0, value), Point2::new(1.0, 0.0))
        };
        SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine { origin, direction }).ok()
    })())
}

pub(in crate::decode) fn section_segment_intersection_carrier_with_missing_line(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    if let Some(geometry) = resolved_section_segment_geometry_with_missing_line(
        definition,
        points,
        segment,
        missing_line,
    ) {
        return Ok(Some(geometry));
    }
    if let Some(geometry) =
        section_proven_axis_line_carrier(ctx, definition, variable_points, segment)?
    {
        return Ok(Some(geometry));
    }
    Ok((|| {
        let carrier = section_arc_carrier(radii, points, segment)
            .or_else(|| saved_section_arc_carrier(definition, segment))?;
        SketchGeometry::from_parts(SketchGeometryDefinition::Arc {
            center: carrier.center,
            radius: carrier.radius,
            start_angle: Angle::ZERO,
            end_angle: Angle::FULL_TURN,
        })
        .ok()
    })())
}

pub(in crate::decode) fn trim_segment_id(
    definition: &crate::feature::definitions::FeatureDefinition,
    row: &crate::feature::definitions::FeatureTrimEntity,
) -> Option<u32> {
    let trim_table = definition.trim_entities.as_ref()?;
    (trim_table.has_complete_bucket_frame() && trim_table.has_unique_external_ids())
        .then_some(())?;
    let Some(segment_table) = &definition.segments else {
        return Some(row.external_id);
    };
    let trim_rows = &trim_table.rows;
    let matching_trim_count = trim_rows
        .iter()
        .filter(|trim| trim.external_id == row.external_id)
        .count();
    if segment_table.unique_segment(row.external_id).is_some() && matching_trim_count == 1 {
        return Some(row.external_id);
    }
    segment_table.is_complete().then_some(())?;
    if segment_table.rows.contains_id(row.external_id) || matching_trim_count != 1 {
        return None;
    }
    let unmatched_segment = crate::decode::uniqueness::exactly_one(
        segment_table
            .rows
            .ordinary()
            .filter(|segment| {
                !trim_rows
                    .iter()
                    .any(|trim| trim.external_id == segment.external_id)
            })
            .map(|segment| segment.external_id),
    );
    let unmatched_row = crate::decode::uniqueness::exactly_one(trim_rows.iter().filter(|trim| {
        !segment_table
            .rows
            .ordinary()
            .any(|segment| segment.external_id == trim.external_id)
    }));
    match (unmatched_segment, unmatched_row) {
        (Some(segment_id), Some(unmatched)) if std::ptr::eq(unmatched, row) => Some(segment_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn numerical_followup_arc_radius_evidence_requires_matching_endpoints() {
        for radius in [1e-6, 1.0] {
            let equal = arc_radius_definition([radius, radius]);
            assert_eq!(
                crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(ctx, &equal))
                    .expect("test section solve")
                    .get(&42),
                Some(&radius)
            );
            let unequal = arc_radius_definition([radius, 1.0005 * radius]);
            assert!(
                !crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(ctx, &unequal))
                    .expect("test section solve")
                    .contains_key(&42)
            );
        }
    }

    use super::{
        append_radius_candidate, link_radii, resolved_section_radii, section_arc_carrier,
        section_proven_axis_line_carrier as section_proven_axis_line_carrier_admitted,
        section_skamp_radius_source, trim_segment_id, SectionRadiusSource,
    };

    fn section_proven_axis_line_carrier(
        definition: &crate::feature::definitions::FeatureDefinition,
        variable_points: &std::collections::BTreeMap<u32, [Option<f64>; 2]>,
        segment: &crate::feature::definitions::FeatureSegment,
    ) -> Option<cadmpeg_ir::sketches::SketchGeometry> {
        crate::decode::with_test_decode_ctx(|ctx| {
            section_proven_axis_line_carrier_admitted(ctx, definition, variable_points, segment)
        })
        .expect("test axis line carrier")
    }

    fn with_collection_limit<T>(
        limit: u64,
        run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
    ) -> T {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("test input admitted");
        run(&ctx)
    }

    #[test]
    fn radius_candidate_refuses_before_node_and_nested_value() {
        let run = |limit| {
            with_collection_limit(limit, |ctx| {
                let mut candidates = std::collections::BTreeMap::new();
                append_radius_candidate(ctx, &mut candidates, 42, 3.0)?;
                Ok::<_, cadmpeg_core::CodecError>(candidates)
            })
        };
        assert!(
            matches!(run(0), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "creo radius candidate nodes")
        );
        assert!(
            matches!(run(1), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "creo radius candidate values")
        );
        assert_eq!(
            run(2).expect("candidate admitted").get(&42),
            Some(&vec![3.0])
        );
    }

    #[test]
    fn radius_adjacency_refuses_before_node_and_link() {
        let run = |limit| {
            with_collection_limit(limit, |ctx| {
                let mut adjacency = std::collections::BTreeMap::new();
                link_radii(ctx, &mut adjacency, 41, 42)?;
                Ok::<_, cadmpeg_core::CodecError>(adjacency)
            })
        };
        assert!(
            matches!(run(0), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "creo radius adjacency nodes")
        );
        assert!(
            matches!(run(1), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "creo radius adjacency links")
        );
        assert_eq!(
            run(4).expect("adjacency admitted").get(&41),
            Some(&std::collections::BTreeSet::from([42]))
        );
    }

    fn arc_carrier_segment() -> crate::feature::definitions::FeatureSegment {
        crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Arc([1, 2]),
            directions: [None; 3],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: Some(4),
            radius2_ref: None,
            external_id: 5,
            body: Vec::new(),
            offset: 9,
        }
    }

    #[test]
    fn diameter_underflow_does_not_resolve_circle_radius() {
        use crate::feature::definitions::{
            DimensionValue, FeatureCircleSegment, FeatureDimension, FeatureDimensionTable,
            FeatureSegmentTable,
        };
        let mut definition = arc_radius_definition([0.0, 0.0]);
        definition.segments = Some(FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::segment_rows::SegmentRow::Circle(
                FeatureCircleSegment {
                    center_id: 1,
                    radius_ref: 0,
                    external_id: 10,
                    offset: 0,
                },
            )]
            .into_iter()
            .collect(),
            offset: 0,
        });
        definition.dimensions = Some(FeatureDimensionTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![FeatureDimension {
                dimension_type: 4,
                value: DimensionValue::Resolved(f64::from_bits(1)),
                value_body: Vec::new(),
                direction_byte: 0,
                auxiliary_value: None,
                auxiliary_body: Vec::new(),
                external_id: 10,
                references: None,
                offset: 0,
            }],
            offset: 0,
        });
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(ctx, &definition))
                .expect("test section solve")
                .is_empty()
        );
    }

    #[test]
    fn relation_diameter_underflow_does_not_resolve_arc_radius() {
        use crate::feature::definitions::{
            DimensionValue, FeatureDimension, FeatureDimensionTable, FeatureRelation,
            FeatureRelationTable,
        };
        let mut definition = arc_radius_definition([0.0, 0.0]);
        let segment_table = definition.segments.as_mut().expect("arc segment table");
        segment_table.declared_count = 1;
        let mut segment = segment_table
            .rows
            .ordinary()
            .next()
            .expect("arc segment")
            .clone();
        segment.radius_ref = Some(0);
        segment_table.rows =
            std::iter::once(crate::feature::segment_rows::SegmentRow::Ordinary(segment)).collect();
        definition.dimensions = Some(FeatureDimensionTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![FeatureDimension {
                dimension_type: 4,
                value: DimensionValue::Resolved(f64::from_bits(1)),
                value_body: Vec::new(),
                direction_byte: 0,
                auxiliary_value: None,
                auxiliary_body: Vec::new(),
                external_id: 1,
                references: None,
                offset: 0,
            }],
            offset: 0,
        });
        definition.relations = Some(FeatureRelationTable {
            declared_count: 3,
            entity_ref: None,
            rows: vec![FeatureRelation {
                relation_id: 1,
                used: 0,
                operands: Vec::new(),
                operand_vectors: Some([
                    [Some(2), Some(0), Some(3), Some(0)],
                    [Some(1), Some(10), Some(0), Some(1)],
                    [Some(16), Some(15), Some(0), Some(0)],
                ]),
                sign: 1,
                dimension_id: 0,
                relation_type: 5,
                body: Vec::new(),
                offset: 0,
            }],
            skamps: None,
            triples: None,
            offset: 0,
        });
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(ctx, &definition))
                .expect("test section solve")
                .is_empty()
        );
    }

    #[test]
    fn resolved_arc_nonfinite_radius_is_not_a_carrier() {
        let points = std::collections::BTreeMap::from([(3, [0.0, 0.0])]);
        let radii = std::collections::BTreeMap::from([(4, f64::INFINITY)]);
        assert!(section_arc_carrier(&radii, &points, &arc_carrier_segment()).is_none());
    }

    #[test]
    fn resolved_arc_zero_radius_is_not_a_carrier() {
        let points = std::collections::BTreeMap::from([(3, [0.0, 0.0])]);
        let radii = std::collections::BTreeMap::from([(4, 0.0)]);
        assert!(section_arc_carrier(&radii, &points, &arc_carrier_segment()).is_none());
    }

    #[test]
    fn resolved_arc_negative_radius_is_not_a_carrier() {
        let points = std::collections::BTreeMap::from([(3, [0.0, 0.0])]);
        let radii = std::collections::BTreeMap::from([(4, -2.0)]);
        assert!(section_arc_carrier(&radii, &points, &arc_carrier_segment()).is_none());
    }

    #[test]
    fn resolved_arc_nonfinite_center_is_not_a_carrier() {
        let points = std::collections::BTreeMap::from([(3, [f64::NAN, 0.0])]);
        let radii = std::collections::BTreeMap::from([(4, 2.0)]);
        assert!(section_arc_carrier(&radii, &points, &arc_carrier_segment()).is_none());
    }

    #[test]
    fn unique_incomplete_axis_row_supplies_unbounded_carrier() {
        let line = crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: Some(0),
            radius_ref: None,
            radius2_ref: None,
            external_id: 10,
            body: Vec::new(),
            offset: 0,
        };
        let variable_points =
            std::collections::BTreeMap::from([(1, [Some(0.0), None]), (2, [Some(0.0), None])]);
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(916),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 2,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![line.clone()])
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                    .collect(),
                offset: 0,
            }),
            trim_entities: Some(crate::feature::definitions::FeatureTrimEntityTable {
                declared_count: None,
                entity_ref: None,
                entry_ref: None,
                buckets: Vec::new(),
                rows: vec![crate::feature::definitions::FeatureTrimEntity {
                    external_id: 10,
                    mode: None,
                    vertices: [1, 2],
                    kind: crate::feature::definitions::TrimEntityKind::Line,
                    offset: 1,
                }],
                solved_external_ids: vec![10],
                offset: 1,
            }),
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };

        assert_eq!(
            section_proven_axis_line_carrier(&definition, &variable_points, &line,),
            Some(
                cadmpeg_ir::sketches::SketchGeometry::try_from(
                    cadmpeg_ir::sketches::SketchGeometryDefinition::ReferenceLine {
                        origin: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                        direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                    }
                )
                .expect("valid test fixture")
            )
        );
        assert_eq!(
            trim_segment_id(
                &definition,
                &definition
                    .trim_entities
                    .as_ref()
                    .expect("trim entities")
                    .rows[0],
            ),
            Some(10)
        );

        let mut duplicate = definition;
        duplicate.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Ordinary(
                crate::feature::definitions::FeatureSegment { offset: 2, ..line },
            ),
        );
        assert!(section_proven_axis_line_carrier(
            &duplicate,
            &variable_points,
            &duplicate
                .segments
                .as_ref()
                .expect("segments")
                .rows
                .ordinary()
                .cloned()
                .collect::<Vec<_>>()[0],
        )
        .is_none());
        assert_eq!(
            trim_segment_id(
                &duplicate,
                &duplicate
                    .trim_entities
                    .as_ref()
                    .expect("trim entities")
                    .rows[0],
            ),
            None
        );
    }

    fn arc_radius_definition(radii: [f64; 2]) -> crate::feature::definitions::FeatureDefinition {
        crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::definitions::test_support::with_points(
                crate::feature::definitions::FeatureVariableTable {
                    declared_count: 0,
                    entity_ref: None,
                    rows: Vec::new(),
                    offset: 0,
                },
                vec![
                    crate::feature::definitions::FeatureSectionPoint {
                        point_id: 1,
                        u: Some(0.0),
                        v: Some(0.0),
                    },
                    crate::feature::definitions::FeatureSectionPoint {
                        point_id: 2,
                        u: Some(radii[0]),
                        v: Some(0.0),
                    },
                    crate::feature::definitions::FeatureSectionPoint {
                        point_id: 3,
                        u: Some(0.0),
                        v: Some(radii[1]),
                    },
                ],
            )),
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 2,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![crate::feature::definitions::FeatureSegment {
                    kind: crate::feature::definitions::FeatureSegmentKind::Arc([2, 3]),
                    directions: [None; 3],
                    center_id: Some(1),
                    arc_orientation: Some(1),
                    vertical_horizontal: None,
                    radius_ref: Some(42),
                    radius2_ref: None,
                    external_id: 10,
                    body: Vec::new(),
                    offset: 0,
                }])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .collect(),
                offset: 0,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        }
    }

    #[test]
    fn unique_arc_rows_remain_radius_sources_in_incomplete_segment_tables() {
        let definition = arc_radius_definition([3.0, 3.0]);
        assert!(!definition
            .segments
            .as_ref()
            .expect("segments")
            .is_complete());
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(ctx, &definition))
                .expect("test section solve"),
            std::collections::BTreeMap::from([(42, 3.0)])
        );
        assert!(matches!(
            section_skamp_radius_source(
                &definition,
                &crate::feature::definitions::FeatureSkampItem {
                    entity_id: 10,
                    sense: 0,
                },
            ),
            Some(SectionRadiusSource::Reference(42))
        ));
    }
}
