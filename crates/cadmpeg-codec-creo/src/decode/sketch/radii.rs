// SPDX-License-Identifier: Apache-2.0
//! Resolved section radii and intersection carriers.

use super::axis::SectionAxis;

use crate::feature::definitions::VariableType;
use crate::feature::segment_rows::SegmentRow;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

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
    section_degenerate_axis_line, section_saved_entity, unique_circle_segment, visit_section_skamps,
};

const EPS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;

fn append_radius_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &mut BTreeMap<u32, Vec<f64>>,
    radius_id: u32,
    value: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    let values = ctx
        .entry_btree_map(candidates, radius_id, "creo radius candidate nodes")?
        .or_default();
    ctx.reserve_vec(values, 1, "creo radius candidate values")?;
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
        let neighbors = ctx
            .entry_btree_map(adjacency, radius_id, "creo radius adjacency nodes")?
            .or_default();
        ctx.insert_btree_set(neighbors, neighbor, "creo radius adjacency links")?;
    }
    Ok(())
}

pub(in crate::decode) fn resolved_section_radii(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeMap<u32, f64>, cadmpeg_core::CodecError> {
    let mut candidates = BTreeMap::<u32, Vec<f64>>::new();
    if let Some(table) = definition.segments.as_ref() {
        for segment in ctx
            .admit_iter(table.rows.as_slice(), "creo resolved radius circle rows")?
            .filter_map(|row| match row {
                SegmentRow::Circle(segment) => Some(segment),
                _ => None,
            })
        {
            if let Some((_, radius)) = saved_section_circle_values(ctx, definition, segment)? {
                append_radius_candidate(ctx, &mut candidates, segment.radius_ref, radius)?;
            }
        }
    }
    if let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    {
        for row in ctx.admit_iter(&variables.rows, "creo radius variable rows")? {
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
    let radial_constraints = section_equation_radial_constraints(
        ctx,
        definition,
        &radial_coordinates,
        &ambiguous_point_ids,
    )?;
    for constraint in ctx
        .admit_iter(&radial_constraints, "creo radial equation constraints")?
        .copied()
    {
        if constraint.radius.0 == VariableType::Radius {
            if let Some(value) = constraint.radius_value.filter(|value| value.get() > 0.0) {
                append_radius_candidate(ctx, &mut candidates, constraint.radius.1, value.get())?;
            }
        }
    }
    let distance_values = section_equation_function_six_distance_values(
        ctx,
        definition,
        &radial_coordinates,
        &ambiguous_point_ids,
    )?;
    for (variable, value) in ctx
        .admit_iter(&distance_values, "creo radial distance values")?
        .copied()
    {
        if variable.0 == VariableType::Radius && value.is_finite() && value > 0.0 {
            append_radius_candidate(ctx, &mut candidates, variable.1, value)?;
        }
    }
    let radius_dimensions = section_equation_radius_dimensions(ctx, definition)?;
    for constraint in ctx
        .admit_iter(&radius_dimensions, "creo radius dimensions")?
        .copied()
        .filter(|constraint| constraint.active)
    {
        append_radius_candidate(
            ctx,
            &mut candidates,
            constraint.radius,
            constraint.value.get(),
        )?;
    }
    if let Some(relations) = definition
        .relations
        .as_ref()
        .filter(|table| feature_relation_table_complete(table))
    {
        for relation in ctx.admit_iter(&relations.rows, "creo radius relation rows")? {
            if section_solver_relation_is_disabled(ctx, definition, relation.relation_id)? {
                continue;
            }
            if matches!(relation.relation_type, 5 | 6) && relation.sign == 1 {
                if section_radius_relation_arc(ctx, definition, relation)?.is_none() {
                    continue;
                }
                let Some(dimension) = section_relation_length_dimension(definition, relation)
                else {
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
    }
    let relation_radius_values = section_relation_radius_scalar_values(ctx, definition)?;
    for ((_, radius_id), value) in ctx
        .admit_iter(&relation_radius_values, "creo scalar radius values")?
        .copied()
    {
        append_radius_candidate(ctx, &mut candidates, radius_id, value)?;
    }
    if let Some(dimensions) = definition
        .dimensions
        .as_ref()
        .filter(|dimensions| feature_dimension_table_complete(dimensions))
    {
        if let Some(table) = definition.segments.as_ref() {
            for circle in ctx
                .admit_iter(table.rows.as_slice(), "creo dimension circle rows")?
                .filter_map(|row| match row {
                    SegmentRow::Circle(segment) => Some(segment),
                    _ => None,
                })
                .filter(|segment| {
                    unique_circle_segment(definition, segment.external_id)
                        .is_some_and(|candidate| std::ptr::eq(candidate, *segment))
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
    }
    let points = resolved_section_points(ctx, definition)?;
    if let Some(table) = definition.segments.as_ref() {
        for segment in ctx
            .admit_iter(table.rows.as_slice(), "creo radius arc rows")?
            .filter_map(|row| match row {
                SegmentRow::Ordinary(segment) => Some(segment),
                _ => None,
            })
            .filter(|segment| {
                matches!(
                    segment.kind,
                    crate::feature::definitions::FeatureSegmentKind::Arc(_)
                )
            })
        {
            if unique_decoded_section_segment(definition, segment.external_id)
                .is_none_or(|unique| !std::ptr::eq(unique, segment))
            {
                continue;
            }
            let Some(radius_id) = segment.radius_ref else {
                continue;
            };
            let Some(center_id) = segment.center_id else {
                continue;
            };
            let Some(center) =
                ctx.get_btree_map(&points, &center_id, "creo radius point lookup")?
            else {
                continue;
            };
            // The two endpoint radii that resolve to a finite nonzero length.
            let mut endpoint_radii = [None; 2];
            for (slot, id) in endpoint_radii.iter_mut().zip(segment.point_ids()) {
                if let Some(point) = ctx.get_btree_map(&points, &id, "creo radius point lookup")? {
                    let radius = (point[0] - center[0]).hypot(point[1] - center[1]);
                    if radius.is_finite() && radius > EPS_RADIUS_NONZERO {
                        *slot = Some(radius);
                    }
                }
            }
            let Some(radius) = endpoint_radii.into_iter().flatten().next() else {
                continue;
            };
            let scale = endpoint_radii.into_iter().flatten().fold(radius, f64::max);
            if endpoint_radii
                .into_iter()
                .flatten()
                .all(|candidate| (candidate - radius).abs() <= EPS_RADIUS_AGREEMENT * scale)
            {
                append_radius_candidate(ctx, &mut candidates, radius_id, radius)?;
            }
        }
    }
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    let mut invalid_scalar_radius_ids = BTreeSet::new();
    if let Some(variables) = definition
        .variables
        .as_ref()
        .filter(|table| table.is_complete())
    {
        let components = section_equation_scalar_equality_components(ctx, definition)?;
        for component in ctx.admit_iter(&components, "creo scalar equality components")? {
            if ctx.any_by(
                component,
                |&(variable_type, _)| Ok(variable_type != VariableType::Radius),
                "creo scalar component variables",
            )? {
                continue;
            }
            let invalid = ctx.any_by(
                component,
                |&(variable_type, radius_id)| {
                    ctx.any_by(
                        &variables.rows,
                        |row| {
                            Ok(row.variable_type == variable_type
                                && row.key == radius_id
                                && row
                                    .value
                                    .value()
                                    .is_some_and(|value| !value.is_finite() || value <= 0.0))
                        },
                        "creo scalar radius variable rows",
                    )
                },
                "creo scalar radius validation",
            )?;
            if invalid {
                for &(_, radius_id) in
                    ctx.admit_iter(component, "creo invalid scalar radius IDs")?
                {
                    ctx.insert_btree_set(
                        &mut invalid_scalar_radius_ids,
                        radius_id,
                        "creo invalid radius nodes",
                    )?;
                }
                continue;
            }
            let mut previous = None;
            for &(_, radius_id) in ctx.admit_iter(component, "creo scalar radius links")? {
                if let Some(first) = previous {
                    link_radii(ctx, &mut adjacency, first, radius_id)?;
                }
                previous = Some(radius_id);
            }
        }
    }
    let ControlFlow::Continue(()) = visit_section_skamps::<std::convert::Infallible>(
        ctx,
        definition,
        true,
        |skamp| {
            let [first, second] = skamp.items.as_slice() else {
                return Ok(ControlFlow::Continue(()));
            };
            if skamp.kind != 6 || first.sense != 0 || second.sense != 0 {
                return Ok(ControlFlow::Continue(()));
            }
            let Some(first_radius) = section_skamp_radius_source(ctx, definition, first)? else {
                return Ok(ControlFlow::Continue(()));
            };
            let Some(second_radius) = section_skamp_radius_source(ctx, definition, second)? else {
                return Ok(ControlFlow::Continue(()));
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
            Ok(ControlFlow::Continue(()))
        },
    )?;
    // Every radius identifier in key order; each component starts from the
    // smallest identifier no earlier component reached.
    let mut seeds = BTreeSet::new();
    for radius_id in ctx
        .admit_iter(&candidates, "creo radius candidate keys")?
        .map(|(id, _)| id)
    {
        ctx.insert_btree_set(&mut seeds, *radius_id, "creo remaining radius nodes")?;
    }
    for radius_id in ctx
        .admit_iter(&adjacency, "creo radius adjacency keys")?
        .map(|(id, _)| id)
    {
        ctx.insert_btree_set(&mut seeds, *radius_id, "creo remaining radius nodes")?;
    }
    let mut reached = BTreeSet::new();
    let mut radii = BTreeMap::new();
    for &seed in ctx.admit_iter(&seeds, "creo radius components")? {
        if ctx.contains_btree_set(&reached, &seed, "creo reached radius lookup")? {
            continue;
        }
        let mut component = BTreeSet::new();
        ctx.insert_btree_set(&mut component, seed, "creo radius component nodes")?;
        let mut pending = std::collections::VecDeque::new();
        ctx.push_back(&mut pending, seed, "creo pending radius nodes")?;
        while let Some(radius_id) = pending.pop_front() {
            ctx.charge_work(1, "creo radius graph visits")?;
            if let Some(neighbors) =
                ctx.get_btree_map(&adjacency, &radius_id, "creo radius adjacency lookup")?
            {
                for neighbor in ctx.admit_iter(neighbors, "creo radius neighbors")? {
                    if ctx.insert_btree_set(
                        &mut component,
                        *neighbor,
                        "creo radius component nodes",
                    )? {
                        ctx.push_back(&mut pending, *neighbor, "creo pending radius nodes")?;
                    }
                }
            }
        }
        for &radius_id in ctx.admit_iter(&component, "creo reached radius IDs")? {
            ctx.insert_btree_set(&mut reached, radius_id, "creo reached radius nodes")?;
        }
        if ctx.any_by(
            &component,
            |radius_id| {
                ctx.contains_btree_set(
                    &invalid_scalar_radius_ids,
                    radius_id,
                    "creo invalid radius lookup",
                )
            },
            "creo radius component invalidity",
        )? {
            continue;
        }
        let first_value = ctx.find_map(
            &component,
            |radius_id| {
                Ok(ctx
                    .get_btree_map(&candidates, radius_id, "creo radius candidate lookup")?
                    .and_then(|values| values.first().copied()))
            },
            "creo radius component values",
        )?;
        let Some(value) = first_value else {
            continue;
        };
        let mut scale = value;
        for radius_id in ctx.admit_iter(&component, "creo radius agreement scale")? {
            if let Some(values) =
                ctx.get_btree_map(&candidates, radius_id, "creo radius candidate lookup")?
            {
                for candidate in ctx.admit_iter(values, "creo radius scale candidates")? {
                    scale = scale.max(*candidate);
                }
            }
        }
        let agrees = ctx.all_by(
            &component,
            |radius_id| {
                let Some(values) =
                    ctx.get_btree_map(&candidates, radius_id, "creo radius candidate lookup")?
                else {
                    return Ok(true);
                };
                ctx.all_by(
                    values,
                    |candidate| Ok((*candidate - value).abs() <= EPS_RADIUS_AGREEMENT * scale),
                    "creo radius agreement candidates",
                )
            },
            "creo radius agreement",
        )?;
        if !agrees {
            continue;
        }
        for radius_id in ctx.admit_iter(&component, "creo resolved radius IDs")? {
            ctx.insert_btree_map(&mut radii, *radius_id, value, "creo resolved radius nodes")?;
        }
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'a crate::feature::definitions::FeatureDefinition,
    relation: &crate::feature::definitions::FeatureRelation,
) -> Result<Option<&'a crate::feature::definitions::FeatureSegment>, cadmpeg_core::CodecError> {
    if relation.relation_type != 5
        || relation.sign != 1
        || section_relation_length_dimension(definition, relation).is_none()
    {
        return Ok(None);
    }
    let Some(vectors) = relation.operand_vectors else {
        return Ok(None);
    };
    let [Some(first_point), Some(0), Some(second_point), Some(0)] = vectors[0] else {
        return Ok(None);
    };
    let [Some(center), Some(10), Some(0), Some(1)] = vectors[1] else {
        return Ok(None);
    };
    if vectors[2] != [Some(16), Some(15), Some(0), Some(0)] {
        return Ok(None);
    }
    unique_section_radius_arc(
        ctx,
        definition,
        relation.dimension_id,
        first_point,
        second_point,
        center,
    )
}

fn section_type6_radius_arc<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'a crate::feature::definitions::FeatureDefinition,
    relation: &crate::feature::definitions::FeatureRelation,
) -> Result<Option<&'a crate::feature::definitions::FeatureSegment>, cadmpeg_core::CodecError> {
    if relation.relation_type != 6
        || relation.sign != 1
        || section_relation_length_dimension(definition, relation).is_none()
    {
        return Ok(None);
    }
    let Some(vectors) = relation.operand_vectors else {
        return Ok(None);
    };
    let [Some(first_point), Some(second_point), Some(0), Some(1)] = vectors[0] else {
        return Ok(None);
    };
    let [Some(center), Some(0), Some(0), Some(0)] = vectors[1] else {
        return Ok(None);
    };
    if !vectors[2].iter().all(Option::is_some) {
        return Ok(None);
    }
    unique_section_radius_arc(
        ctx,
        definition,
        relation.dimension_id,
        first_point,
        second_point,
        center,
    )
}

pub(in crate::decode) fn section_radius_relation_arc<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'a crate::feature::definitions::FeatureDefinition,
    relation: &crate::feature::definitions::FeatureRelation,
) -> Result<Option<&'a crate::feature::definitions::FeatureSegment>, cadmpeg_core::CodecError> {
    match relation.relation_type {
        5 => section_type5_radius_arc(ctx, definition, relation),
        6 => section_type6_radius_arc(ctx, definition, relation),
        _ => Ok(None),
    }
}

fn unique_section_radius_arc<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'a crate::feature::definitions::FeatureDefinition,
    dimension_id: u32,
    first_point: u32,
    second_point: u32,
    center: u32,
) -> Result<Option<&'a crate::feature::definitions::FeatureSegment>, cadmpeg_core::CodecError> {
    let Some(table) = definition.segments.as_ref() else {
        return Ok(None);
    };
    let row = crate::decode::uniqueness::exactly_one_by(
        ctx,
        table.rows.as_slice(),
        |row| {
            let SegmentRow::Ordinary(segment) = row else {
                return Ok(false);
            };
            Ok(matches!(
                segment.kind,
                crate::feature::definitions::FeatureSegmentKind::Arc(_)
            ) && segment.radius_ref == Some(dimension_id)
                && segment.center_id == Some(center)
                && (segment.point_ids() == [first_point, second_point]
                    || segment.point_ids() == [second_point, first_point])
                && table.rows.get(segment.external_id).is_some())
        },
        "creo section radius arc rows",
    )?;
    Ok(match row {
        Some(SegmentRow::Ordinary(segment)) => Some(segment),
        _ => None,
    })
}

#[derive(Clone, Copy)]
enum SectionRadiusSource {
    Reference(u32),
    Value(PositiveLength),
}

fn section_skamp_radius_source(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SectionRadiusSource>, cadmpeg_core::CodecError> {
    if let Some(circle) = unique_circle_segment(definition, item.entity_id) {
        return Ok(Some(SectionRadiusSource::Reference(circle.radius_ref)));
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return Ok(matches!(
            segment.kind,
            crate::feature::definitions::FeatureSegmentKind::Arc(_)
        )
        .then_some(segment.radius_ref)
        .flatten()
        .map(SectionRadiusSource::Reference));
    }
    if !saved_section_entity_fallback_allowed(definition, item.entity_id) {
        return Ok(None);
    }
    let Some(saved) = section_saved_entity(ctx, definition, item.entity_id)? else {
        return Ok(None);
    };
    let radius = match saved {
        crate::feature::definitions::FeatureSavedEntity::Arc(arc) => arc.radius,
        crate::feature::definitions::FeatureSavedEntity::Circle(circle) => circle.radius,
        _ => None,
    };
    Ok(radius
        .and_then(PositiveLength::new)
        .map(SectionRadiusSource::Value))
}

pub(super) fn section_arc_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    radii: &BTreeMap<u32, f64>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SectionArcCarrier>, cadmpeg_core::CodecError> {
    if !matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Arc(_)
    ) {
        return Ok(None);
    }
    let (Some(center_id), Some(radius_id)) = (segment.center_id, segment.radius_ref) else {
        return Ok(None);
    };
    let Some(center) = ctx.get_btree_map(points, &center_id, "creo arc carrier point lookup")?
    else {
        return Ok(None);
    };
    let Some(radius) = ctx.get_btree_map(radii, &radius_id, "creo arc carrier radius lookup")?
    else {
        return Ok(None);
    };
    Ok(SectionArcCarrier::new(*center, *radius))
}

/// The coordinate on `axis` of variable point `point`, when it is resolved.
fn fixed_point_coordinate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    point: u32,
    axis: SectionAxis,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    Ok(ctx
        .get_btree_map(
            variable_points,
            &point,
            "creo section variable point lookup",
        )?
        .and_then(|coordinates| coordinates[axis.index()]))
}

pub(in crate::decode) fn section_axis_line_carrier_with_points(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    if !matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Line(_)
    ) {
        return Ok(None);
    }
    let fixed_coordinate = match segment.directions {
        [Some(0), _, _] => SectionAxis::U,
        [_, Some(0), _] => SectionAxis::V,
        _ => return Ok(None),
    };
    section_fixed_coordinate_line_carrier(ctx, variable_points, segment, fixed_coordinate)
}

/// A reference line along `axis` through a value both endpoints agree on.
fn axis_reference_line(value: f64, fixed_coordinate: SectionAxis) -> Option<SketchGeometry> {
    let (origin, direction) = if fixed_coordinate == SectionAxis::U {
        (Point2::new(value, 0.0), Point2::new(0.0, 1.0))
    } else {
        (Point2::new(0.0, value), Point2::new(1.0, 0.0))
    };
    SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine { origin, direction }).ok()
}

fn section_fixed_coordinate_line_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
    fixed_coordinate: SectionAxis,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    if !matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Line(_)
    ) {
        return Ok(None);
    }
    let [first, second] = segment.point_ids();
    let (Some(first), Some(second)) = (
        fixed_point_coordinate(ctx, variable_points, first, fixed_coordinate)?,
        fixed_point_coordinate(ctx, variable_points, second, fixed_coordinate)?,
    ) else {
        return Ok(None);
    };
    let scale = first.abs().max(second.abs()).max(1.0);
    if (first - second).abs() > EPS_RADIUS_AGREEMENT * scale {
        return Ok(None);
    }
    Ok(axis_reference_line(first, fixed_coordinate))
}

fn section_proven_axis_line_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    if let Some(geometry) = section_axis_line_carrier_with_points(ctx, variable_points, segment)? {
        return Ok(Some(geometry));
    }
    let Some(coordinate) = section_line_entity_fixed_coordinate_with_unique_rows(
        ctx,
        definition,
        segment.external_id,
    )?
    else {
        return Ok(None);
    };
    section_fixed_coordinate_line_carrier(ctx, variable_points, segment, coordinate)
}

pub(in crate::decode) fn section_axis_reference_line_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    if !section_degenerate_axis_line(ctx, definition, segment)? {
        return section_proven_axis_line_carrier(ctx, definition, variable_points, segment);
    }
    let Some(fixed_coordinate) = segment
        .vertical_horizontal
        .and_then(SectionAxis::from_selector)
    else {
        return Ok(None);
    };
    // Two endpoint identifiers, possibly the same point.
    let point_ids = segment.point_ids();
    let expected_value_count = if point_ids[0] == point_ids[1] { 1 } else { 2 };
    let values = [
        fixed_point_coordinate(ctx, variable_points, point_ids[0], fixed_coordinate)?,
        fixed_point_coordinate(ctx, variable_points, point_ids[1], fixed_coordinate)?,
    ];
    if values.iter().flatten().count() != expected_value_count {
        return Ok(None);
    }
    let Some(value) = values.into_iter().flatten().next() else {
        return Ok(None);
    };
    let scale = values
        .into_iter()
        .flatten()
        .map(f64::abs)
        .fold(value.abs().max(1.0), f64::max);
    if !values
        .into_iter()
        .flatten()
        .all(|candidate| (candidate - value).abs() <= EPS_RADIUS_AGREEMENT * scale)
    {
        return Ok(None);
    }
    Ok(axis_reference_line(value, fixed_coordinate))
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
        ctx,
        definition,
        points,
        segment,
        missing_line,
    )? {
        return Ok(Some(geometry));
    }
    if let Some(geometry) =
        section_proven_axis_line_carrier(ctx, definition, variable_points, segment)?
    {
        return Ok(Some(geometry));
    }
    let carrier = match section_arc_carrier(ctx, radii, points, segment)? {
        Some(carrier) => Some(carrier),
        None => saved_section_arc_carrier(ctx, definition, segment)?,
    };
    let Some(carrier) = carrier else {
        return Ok(None);
    };
    Ok(SketchGeometry::from_parts(SketchGeometryDefinition::Arc {
        center: carrier.center,
        radius: carrier.radius,
        start_angle: Angle::ZERO,
        end_angle: Angle::FULL_TURN,
    })
    .ok())
}

/// The section segment identifier each trim entity row names, by row
/// position: the row's own identifier when one ordinary segment carries it,
/// or the one segment no trim row names when exactly one row names no
/// segment. The table checks and the unmatched search run once per table.
pub(in crate::decode) fn trim_segment_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<Option<u32>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "creo trim segment IDs";
    let Some(trim_table) = definition.trim_entities.as_ref() else {
        return Ok(Vec::new());
    };
    let trim_rows = &trim_table.rows;
    let mut ids = Vec::new();
    ctx.reserve_vec(&mut ids, trim_rows.len(), OPERATION)?;
    if !trim_table.has_complete_bucket_frame(ctx)? || !trim_table.has_unique_external_ids(ctx)? {
        ids.resize(trim_rows.len(), None);
        return Ok(ids);
    }
    let Some(segment_table) = &definition.segments else {
        for row in ctx.admit_iter(trim_rows, OPERATION)? {
            ids.push(Some(row.external_id));
        }
        return Ok(ids);
    };
    let complete = segment_table.is_complete();
    for row in ctx.admit_iter(trim_rows, OPERATION)? {
        ids.push(if segment_table.unique_segment(row.external_id).is_some() {
            Some(row.external_id)
        } else {
            None
        });
    }
    if !complete {
        return Ok(ids);
    }
    // Sorted scratch identifiers of the trim rows and the ordinary segments.
    let (mut trim_ids, _trim_storage) = ctx.temporary_vec(trim_rows.len(), OPERATION)?;
    for row in ctx.admit_iter(trim_rows, OPERATION)? {
        trim_ids.push(row.external_id);
    }
    ctx.sort_unstable_by(&mut trim_ids, |id| id, Ord::cmp, OPERATION)?;
    let (mut ordinary_ids, _ordinary_storage) =
        ctx.temporary_vec(segment_table.rows.len(), OPERATION)?;
    for segment_row in ctx.admit_iter(segment_table.rows.as_slice(), OPERATION)? {
        if let SegmentRow::Ordinary(segment) = segment_row {
            ordinary_ids.push(segment.external_id);
        }
    }
    // Exactly one ordinary segment and exactly one trim row lack a partner.
    let Some(&unmatched_segment) = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &ordinary_ids,
        |id| Ok(ctx.binary_search(&trim_ids, id, OPERATION)?.is_err()),
        OPERATION,
    )?
    else {
        return Ok(ids);
    };
    ctx.sort_unstable_by(&mut ordinary_ids, |id| id, Ord::cmp, OPERATION)?;
    let unmatched_row =
        |row: &crate::feature::definitions::FeatureTrimEntity| -> Result<bool, cadmpeg_core::CodecError> {
            Ok(ctx
                .binary_search(&ordinary_ids, &row.external_id, OPERATION)?
                .is_err())
        };
    let Some(index) = ctx.position_by(trim_rows, unmatched_row, OPERATION)? else {
        return Ok(ids);
    };
    if ctx.any_by(&trim_rows[index + 1..], unmatched_row, OPERATION)? {
        return Ok(ids);
    }
    // A row whose identifier names a segment of another family stays unmatched.
    if !segment_table.rows.contains_id(trim_rows[index].external_id) {
        ids[index] = Some(unmatched_segment);
    }
    Ok(ids)
}

/// The section segment identifier one trim entity row names.
#[cfg(test)]
pub(in crate::decode) fn trim_segment_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    row: &crate::feature::definitions::FeatureTrimEntity,
) -> Result<Option<u32>, cadmpeg_core::CodecError> {
    let ids = trim_segment_ids(ctx, definition)?;
    let rows = definition
        .trim_entities
        .as_ref()
        .map_or(&[][..], |table| table.rows.as_slice());
    Ok(rows
        .iter()
        .position(|candidate| std::ptr::eq(candidate, row))
        .or_else(|| rows.iter().position(|candidate| candidate == row))
        .and_then(|index| ids[index]))
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
    fn radius_component_scans_refuse_before_component_and_frontier_visits() {
        let definition = arc_radius_definition([3.0, 3.0]);
        let radii = crate::test_support::assert_work_boundaries(
            &["creo radius components", "creo radius graph visits"],
            |ctx| resolved_section_radii(ctx, &definition),
        );
        assert_eq!(radii.get(&42), Some(&3.0));
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
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_arc_carrier(
                ctx,
                &radii,
                &points,
                &arc_carrier_segment()
            ))
            .expect("admitted arc carrier")
            .is_none()
        );
    }

    #[test]
    fn resolved_arc_zero_radius_is_not_a_carrier() {
        let points = std::collections::BTreeMap::from([(3, [0.0, 0.0])]);
        let radii = std::collections::BTreeMap::from([(4, 0.0)]);
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_arc_carrier(
                ctx,
                &radii,
                &points,
                &arc_carrier_segment()
            ))
            .expect("admitted arc carrier")
            .is_none()
        );
    }

    #[test]
    fn resolved_arc_negative_radius_is_not_a_carrier() {
        let points = std::collections::BTreeMap::from([(3, [0.0, 0.0])]);
        let radii = std::collections::BTreeMap::from([(4, -2.0)]);
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_arc_carrier(
                ctx,
                &radii,
                &points,
                &arc_carrier_segment()
            ))
            .expect("admitted arc carrier")
            .is_none()
        );
    }

    #[test]
    fn resolved_arc_nonfinite_center_is_not_a_carrier() {
        let points = std::collections::BTreeMap::from([(3, [f64::NAN, 0.0])]);
        let radii = std::collections::BTreeMap::from([(4, 2.0)]);
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_arc_carrier(
                ctx,
                &radii,
                &points,
                &arc_carrier_segment()
            ))
            .expect("admitted arc carrier")
            .is_none()
        );
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
            crate::decode::with_test_decode_ctx(|ctx| {
                trim_segment_id(
                    ctx,
                    &definition,
                    &definition
                        .trim_entities
                        .as_ref()
                        .expect("trim entities")
                        .rows[0],
                )
            })
            .expect("test trim-segment resources"),
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
            crate::decode::with_test_decode_ctx(|ctx| {
                trim_segment_id(
                    ctx,
                    &duplicate,
                    &duplicate
                        .trim_entities
                        .as_ref()
                        .expect("trim entities")
                        .rows[0],
                )
            })
            .expect("test trim-segment resources"),
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
            crate::decode::with_test_decode_ctx(|ctx| {
                section_skamp_radius_source(
                    ctx,
                    &definition,
                    &crate::feature::definitions::FeatureSkampItem {
                        entity_id: 10,
                        sense: 0,
                    },
                )
            })
            .expect("test radius source"),
            Some(SectionRadiusSource::Reference(42))
        ));
    }
}
