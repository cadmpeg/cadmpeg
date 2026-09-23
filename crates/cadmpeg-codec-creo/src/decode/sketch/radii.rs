// SPDX-License-Identifier: Apache-2.0
//! Resolved section radii and intersection carriers.

use super::axis::SectionAxis;

use crate::feature::definitions::VariableType;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::index_from_u32;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, Length};
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
    saved_section_circle_values,
};
use super::skamp::{
    section_line_entity_fixed_coordinate_with_unique_rows, section_segment_rows,
    unique_decoded_section_segment,
};
use crate::decode::sketch_transfer::constraints::section_solver_relation_is_disabled;
use crate::decode::sketch_transfer::identity::{
    saved_section_entity_fallback_allowed, unique_section_segment_external_ids,
};
use crate::decode::sketch_transfer::loci::{
    active_complete_section_skamps, section_degenerate_axis_line, section_saved_entity,
    unique_circle_segment,
};

const EPS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;

pub(in crate::decode) fn resolved_section_radii(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> BTreeMap<u32, f64> {
    let mut candidates = BTreeMap::<u32, Vec<f64>>::new();
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.circles())
    {
        if let Some((_, radius)) = saved_section_circle_values(definition, segment) {
            candidates
                .entry(segment.radius_ref)
                .or_default()
                .push(radius);
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
                candidates.entry(row.key).or_default().push(value);
            }
        }
    }
    let radial_coordinates = resolved_section_coordinates(definition);
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .map_or_else(BTreeSet::new, |variables| variables.reconciled_points().1);
    for constraint in
        section_equation_radial_constraints(definition, &radial_coordinates, &ambiguous_point_ids)
    {
        if constraint.radius.0 == VariableType::Radius {
            if let Some(value) = constraint
                .radius_value
                .filter(|value| value.is_finite() && *value > 0.0)
            {
                candidates
                    .entry(constraint.radius.1)
                    .or_default()
                    .push(value);
            }
        }
    }
    for (variable, value) in section_equation_function_six_distance_values(
        definition,
        &radial_coordinates,
        &ambiguous_point_ids,
    ) {
        if variable.0 == VariableType::Radius && value.is_finite() && value > 0.0 {
            candidates.entry(variable.1).or_default().push(value);
        }
    }
    for constraint in section_equation_radius_dimensions(definition)
        .into_iter()
        .filter(|constraint| constraint.active)
    {
        candidates
            .entry(constraint.radius)
            .or_default()
            .push(constraint.value);
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
            candidates
                .entry(relation.dimension_id)
                .or_default()
                .push(radius);
        }
    }
    for ((_, radius_id), value) in section_relation_radius_scalar_values(definition) {
        candidates.entry(radius_id).or_default().push(value);
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
            candidates.entry(radius_id).or_default().push(radius);
        }
    }
    let points = resolved_section_points(definition);
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
        let endpoint_radii = segment
            .point_ids()
            .iter()
            .filter_map(|id| points.get(id))
            .map(|point| (point[0] - center[0]).hypot(point[1] - center[1]))
            .filter(|radius| radius.is_finite() && *radius > EPS_RADIUS_NONZERO)
            .collect::<Vec<_>>();
        let Some(radius) = endpoint_radii.first().copied() else {
            continue;
        };
        let scale = endpoint_radii.iter().copied().fold(radius, f64::max);
        if endpoint_radii
            .iter()
            .all(|candidate| (*candidate - radius).abs() <= EPS_RADIUS_AGREEMENT * scale)
        {
            candidates.entry(radius_id).or_default().push(radius);
        }
    }
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    let mut invalid_scalar_radius_ids = BTreeSet::new();
    if let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    {
        for component in section_equation_scalar_equality_components(definition) {
            let radius_ids = component
                .iter()
                .filter_map(|&(variable_type, radius_id)| {
                    (variable_type == VariableType::Radius).then_some(radius_id)
                })
                .collect::<Vec<_>>();
            if radius_ids.len() != component.len() {
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
                invalid_scalar_radius_ids.extend(radius_ids);
                continue;
            }
            for (first, second) in radius_ids.iter().zip(radius_ids.iter().skip(1)) {
                adjacency.entry(*first).or_default().insert(*second);
                adjacency.entry(*second).or_default().insert(*first);
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
                adjacency.entry(first).or_default().insert(second);
                adjacency.entry(second).or_default().insert(first);
            }
            (SectionRadiusSource::Reference(reference), SectionRadiusSource::Value(value))
            | (SectionRadiusSource::Value(value), SectionRadiusSource::Reference(reference)) => {
                candidates.entry(reference).or_default().push(value);
            }
            (SectionRadiusSource::Value(_), SectionRadiusSource::Value(_)) => {}
        }
    }
    let mut remaining = candidates
        .keys()
        .chain(adjacency.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut radii = BTreeMap::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(radius_id) = pending.pop_front() {
            for neighbor in adjacency.get(&radius_id).into_iter().flatten() {
                if component.insert(*neighbor) {
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
        let values = component
            .iter()
            .flat_map(|radius_id| candidates.get(radius_id).into_iter().flatten())
            .copied()
            .collect::<Vec<_>>();
        if let Some(value) = values.first().copied() {
            let scale = values.iter().copied().fold(value, f64::max);
            if !values
                .iter()
                .all(|candidate| (*candidate - value).abs() <= EPS_RADIUS_AGREEMENT * scale)
            {
                remaining.retain(|radius_id| !component.contains(radius_id));
                continue;
            }
            radii.extend(component.iter().map(|radius_id| (*radius_id, value)));
        }
        remaining.retain(|radius_id| !component.contains(radius_id));
    }
    radii
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
    let unique_entities = unique_section_segment_external_ids(definition);
    let matching = section_segment_rows(definition)
        .into_iter()
        .filter(|segment| {
            matches!(
                segment.kind,
                crate::feature::definitions::FeatureSegmentKind::Arc(_)
            ) && segment.radius_ref == Some(dimension_id)
                && segment.center_id == Some(center)
                && (segment.point_ids() == [first_point, second_point]
                    || segment.point_ids() == [second_point, first_point])
                && unique_entities.contains(&segment.external_id)
        })
        .collect::<Vec<_>>();
    let [segment] = matching.as_slice() else {
        return None;
    };
    Some(segment)
}

#[derive(Clone, Copy)]
enum SectionRadiusSource {
    Reference(u32),
    Value(f64),
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
    (radius.is_finite() && radius > 0.0).then_some(SectionRadiusSource::Value(radius))
}

pub(super) fn section_arc_carrier(
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<([f64; 2], f64)> {
    matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Arc(_)
    )
    .then_some(())?;
    let center = *points.get(&segment.center_id?)?;
    let radius = *radii.get(&segment.radius_ref?)?;
    Some((center, radius))
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
    definition: &crate::feature::definitions::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<SketchGeometry> {
    if let Some(geometry) = section_axis_line_carrier_with_points(variable_points, segment) {
        Some(geometry)
    } else {
        section_fixed_coordinate_line_carrier(
            variable_points,
            segment,
            section_line_entity_fixed_coordinate_with_unique_rows(definition, segment.external_id)?,
        )
    }
}

pub(in crate::decode) fn section_axis_reference_line_geometry(
    definition: &crate::feature::definitions::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<SketchGeometry> {
    if !section_degenerate_axis_line(definition, segment) {
        return section_proven_axis_line_carrier(definition, variable_points, segment);
    }
    let fixed_coordinate = SectionAxis::from_selector(segment.vertical_horizontal?)?;
    let values = segment
        .point_ids()
        .iter()
        .filter_map(|point| {
            variable_points
                .get(point)?
                .get(fixed_coordinate.index())
                .copied()
                .flatten()
        })
        .collect::<Vec<_>>();
    let expected_value_count = if segment.point_ids()[0] == segment.point_ids()[1] {
        1
    } else {
        2
    };
    (values.len() == expected_value_count).then_some(())?;
    let value = *values.first()?;
    let scale = values
        .iter()
        .copied()
        .map(f64::abs)
        .fold(value.abs().max(1.0), f64::max);
    values
        .iter()
        .all(|candidate| (*candidate - value).abs() <= EPS_RADIUS_AGREEMENT * scale)
        .then_some(())?;
    let (origin, direction) = if fixed_coordinate == SectionAxis::U {
        (Point2::new(value, 0.0), Point2::new(0.0, 1.0))
    } else {
        (Point2::new(0.0, value), Point2::new(1.0, 0.0))
    };
    SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine { origin, direction }).ok()
}

pub(in crate::decode) fn section_segment_intersection_carrier_with_missing_line(
    definition: &crate::feature::definitions::FeatureDefinition,
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Option<SketchGeometry> {
    if let Some(geometry) = resolved_section_segment_geometry_with_missing_line(
        definition,
        points,
        segment,
        missing_line,
    ) {
        return Some(geometry);
    }
    if let Some(geometry) = section_proven_axis_line_carrier(definition, variable_points, segment) {
        return Some(geometry);
    }
    let ([center_u, center_v], radius) = section_arc_carrier(radii, points, segment)
        .or_else(|| saved_section_arc_carrier(definition, segment))?;
    SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: cadmpeg_ir::math::Point2::new(center_u, center_v),
        radius: Length::new(radius)?,
        start_angle: Angle::ZERO,
        end_angle: Angle::FULL_TURN,
    })
    .ok()
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
    let segments = segment_table.rows.ordinary().collect::<Vec<_>>();
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
    let unmatched_segments = segments
        .iter()
        .filter(|segment| {
            !trim_rows
                .iter()
                .any(|trim| trim.external_id == segment.external_id)
        })
        .map(|segment| segment.external_id)
        .collect::<Vec<_>>();
    let unmatched_rows = trim_rows
        .iter()
        .filter(|trim| {
            !segments
                .iter()
                .any(|segment| segment.external_id == trim.external_id)
        })
        .collect::<Vec<_>>();
    match (unmatched_segments.as_slice(), unmatched_rows.as_slice()) {
        ([segment_id], [unmatched]) if std::ptr::eq(*unmatched, row) => Some(*segment_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn numerical_followup_arc_radius_evidence_requires_matching_endpoints() {
        for radius in [1e-6, 1.0] {
            let equal = arc_radius_definition([radius, radius]);
            assert_eq!(resolved_section_radii(&equal).get(&42), Some(&radius));
            let unequal = arc_radius_definition([radius, 1.0005 * radius]);
            assert!(!resolved_section_radii(&unequal).contains_key(&42));
        }
    }

    use super::{
        resolved_section_radii, section_proven_axis_line_carrier, section_skamp_radius_source,
        trim_segment_id, SectionRadiusSource,
    };

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
            resolved_section_radii(&definition),
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
