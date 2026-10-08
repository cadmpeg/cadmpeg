// SPDX-License-Identifier: Apache-2.0
//! Skamp incidence, symmetry, and fixed-coordinate helpers.

use super::axis::SectionAxis;
use crate::feature::segment_rows::SegmentRow;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

use crate::decode::sketch_transfer::identity::{
    saved_section_entity_fallback_allowed, saved_section_line_witness_allowed,
};
use crate::decode::sketch_transfer::loci::{
    section_saved_entity, section_skamp_is_line, section_skamp_is_point,
    unique_bounded_curve_segment, unique_centered_line_segment, unique_circle_segment,
    unique_point_segment, unique_reference_line_segment, visit_section_skamps,
};

const EPS_SAVED_LINE_AXIS: f64 = 1.0e-9;
const EPS_SKAMP_AGREEMENT: f64 = 1.0e-9;

pub(in crate::decode) fn section_line_fixed_coordinate(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SectionAxis>, CodecError> {
    let Some(segment) = unique_section_skamp_segment(definition, segment.external_id) else {
        return Ok(None);
    };
    if !matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Line(_)
    ) {
        return Ok(None);
    }
    section_line_entity_fixed_coordinate(ctx, definition, segment.external_id)
}

pub(super) fn section_line_entity_fixed_coordinate(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
) -> Result<Option<SectionAxis>, CodecError> {
    section_line_entity_fixed_coordinate_with_mode(ctx, definition, entity_id, false)
}

pub(super) fn section_line_entity_fixed_coordinate_with_unique_rows(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
) -> Result<Option<SectionAxis>, CodecError> {
    section_line_entity_fixed_coordinate_with_mode(ctx, definition, entity_id, true)
}

fn section_line_entity_fixed_coordinate_with_mode(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
    include_unique_rows: bool,
) -> Result<Option<SectionAxis>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo section line entity fixed coordinate with mode scratch")?;
    let mut adjacency = BTreeMap::<u32, Vec<(u32, bool)>>::new();
    let mut skamp_coordinates = std::collections::HashMap::<u32, [bool; 2]>::new();
    let ControlFlow::Continue(()) =
        visit_section_skamps::<std::convert::Infallible>(ctx, definition, true, |skamp| {
            if let (1 | 2, [item]) = (skamp.kind, skamp.items.as_slice()) {
                if item.sense == 0 {
                    let coordinate = if skamp.kind == 1 { SectionAxis::V } else { SectionAxis::U };
                    scratch.with_storage(|| ctx.entry_hash_map(&mut skamp_coordinates, item.entity_id, "creo fixed-coordinate skamp index nodes"))?.or_insert([false; 2])[coordinate.index()] = true;
                }
            }
            let (parity, first, second) = match (skamp.kind, skamp.items.as_slice()) {
                (5 | 7, [first, second]) if first.sense == 0 && second.sense == 0 => {
                    (skamp.kind == 5, first, second)
                }
                _ => return Ok(ControlFlow::Continue(())),
            };
            if !section_skamp_is_line(ctx, definition, first)?
                || !section_skamp_is_line(ctx, definition, second)?
            {
                return Ok(ControlFlow::Continue(()));
            }
            for (entity_id, neighbor) in [
                (first.entity_id, second.entity_id),
                (second.entity_id, first.entity_id),
            ] {
                let neighbors = scratch.with_storage(|| ctx
                    .entry_btree_map(
                        &mut adjacency,
                        entity_id,
                        "creo fixed-coordinate adjacency nodes",
                    ))?
                    .or_default();
                scratch.with_storage(|| ctx.reserve_vec(neighbors, 1, "creo fixed-coordinate adjacency links"))?;
                neighbors.push((neighbor, parity));
            }
            Ok(ControlFlow::Continue(()))
        })?;
    let mut parities = BTreeMap::new();
    scratch.with_storage(|| ctx.insert_btree_map(&mut parities, entity_id, false, "creo fixed-coordinate parity seed"))?;
    let mut pending = std::collections::VecDeque::new();
    scratch.with_storage(|| ctx.push_back(
        &mut pending,
        entity_id,
        "creo fixed-coordinate pending seed",
    ))?;
    while let Some(entity_id) = pending.pop_front() {
        ctx.charge_work(1, "creo fixed-coordinate graph traversal")?;
        let Some(&parity) =
            ctx.get_btree_map(&parities, &entity_id, "creo fixed-coordinate parity lookup")?
        else {
            continue;
        };
        let Some(neighbors) = ctx.get_btree_map(
            &adjacency,
            &entity_id,
            "creo fixed-coordinate adjacency lookup",
        )?
        else {
            continue;
        };
        let mut links = neighbors.iter();
        while let Some(&(neighbor, edge_parity)) = ctx.next_charged(&mut links, "creo fixed-coordinate adjacency links")? {
            let neighbor_parity = parity ^ edge_parity;
            match ctx.get_btree_map(&parities, &neighbor, "creo fixed-coordinate parity lookup")? {
                Some(stored) if *stored != neighbor_parity => return Ok(None),
                Some(_) => {}
                None => {
                    scratch.with_storage(|| ctx.insert_btree_map(
                        &mut parities,
                        neighbor,
                        neighbor_parity,
                        "creo fixed-coordinate parity nodes",
                    ))?;
                    scratch.with_storage(|| ctx.push_back(
                        &mut pending,
                        neighbor,
                        "creo fixed-coordinate pending nodes",
                    ))?;
                }
            }
        }
    }
    let mut coordinates = BTreeSet::new();
    for (entity_id, parity) in ctx.admit_iter(&parities, "creo fixed-coordinate parity rows")? {
        let mut direct_storage = ctx.reserve_scoped(0, "creo direct fixed-coordinate scratch")?;
        let direct_coordinates = direct_storage.with_storage(|| section_line_direct_fixed_coordinates_with_mode(
            ctx,
            definition,
            *entity_id,
            include_unique_rows,
            skamp_coordinates.get(entity_id).copied().unwrap_or([false; 2]),
        ))?;
        for coordinate in
            &direct_coordinates
        {
            let coordinate = if *parity {
                coordinate.other()
            } else {
                *coordinate
            };
            scratch.with_storage(|| ctx.insert_btree_set(
                &mut coordinates,
                coordinate,
                "creo fixed-coordinate result nodes",
            ))?;
        }
    }
    Ok(coordinates
        .first()
        .copied()
        .filter(|_| coordinates.len() == 1))
}

fn section_line_direct_fixed_coordinates_with_mode(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
    include_unique_rows: bool,
    skamp_coordinates: [bool; 2],
) -> Result<BTreeSet<SectionAxis>, CodecError> {
    let segment = if include_unique_rows {
        unique_decoded_section_segment(definition, entity_id)
    } else {
        unique_section_skamp_segment(definition, entity_id)
    };
    let segment_coordinate = segment
        .filter(|segment| {
            matches!(
                segment.kind,
                crate::feature::definitions::FeatureSegmentKind::Line(_)
            )
        })
        .and_then(|segment| segment.vertical_horizontal)
        .and_then(|selector| match selector {
            0 => Some(SectionAxis::U),
            1 => Some(SectionAxis::V),
            _ => None,
        });
    let mut coordinates = BTreeSet::new();
    if let Some(coordinate) = segment_coordinate {
        ctx.insert_btree_set(
            &mut coordinates,
            coordinate,
            "creo direct fixed-coordinate nodes",
        )?;
    }
    if let Some(coordinate) = unique_reference_line_segment(definition, entity_id)
        .and_then(|segment| segment.vertical_horizontal)
        .and_then(|selector| match selector {
            0 => Some(SectionAxis::U),
            1 => Some(SectionAxis::V),
            _ => None,
        })
    {
        ctx.insert_btree_set(
            &mut coordinates,
            coordinate,
            "creo direct fixed-coordinate nodes",
        )?;
    }
    for (coordinate, present) in SectionAxis::ALL.into_iter().zip(skamp_coordinates) {
        if present { ctx.insert_btree_set(&mut coordinates, coordinate, "creo direct fixed-coordinate nodes")?; }
    }
    if saved_section_line_witness_allowed(definition, entity_id) {
        if let Some(crate::feature::definitions::FeatureSavedEntity::Line(line)) =
            section_saved_entity(ctx, definition, entity_id)?
        {
            let [[Some(x0), Some(y0), _], [Some(x1), Some(y1), _]] = line.endpoints else {
                return Ok(coordinates);
            };
            let scale = [x0, y0, x1, y1]
                .into_iter()
                .map(f64::abs)
                .fold(1.0, f64::max);
            let tolerance = EPS_SAVED_LINE_AXIS * scale;
            match [(x0 - x1).abs() <= tolerance, (y0 - y1).abs() <= tolerance] {
                [true, false] => {
                    ctx.insert_btree_set(
                        &mut coordinates,
                        SectionAxis::U,
                        "creo direct fixed-coordinate nodes",
                    )?;
                }
                [false, true] => {
                    ctx.insert_btree_set(
                        &mut coordinates,
                        SectionAxis::V,
                        "creo direct fixed-coordinate nodes",
                    )?;
                }
                _ => {}
            }
        }
    }
    Ok(coordinates)
}

pub(in crate::decode) fn section_skamp_point_on_line(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Result<Option<(u32, u32, SectionAxis)>, CodecError> {
    let [first, second] = skamp.items.as_slice() else {
        return Ok(None);
    };
    let selected_point_id = |item: &crate::feature::definitions::FeatureSkampItem| {
        section_skamp_selected_point_id(definition, item).or_else(|| {
            section_skamp_selected_point_id_with_ordinary_segment(
                definition,
                item,
                unique_decoded_section_segment(definition, item.entity_id),
            )
        })
    };
    let line_for_item = |item: &crate::feature::definitions::FeatureSkampItem| {
        unique_section_skamp_segment(definition, item.entity_id).or_else(|| {
            unique_decoded_section_segment(definition, item.entity_id).filter(|segment| {
                matches!(
                    segment.kind,
                    crate::feature::definitions::FeatureSegmentKind::Line(_)
                )
            })
        })
    };
    let mut pair = None;
    if matches!(skamp.kind, 3 | 9) {
        for (line_item, point_item) in [(first, second), (second, first)] {
            let Some(line) = line_for_item(line_item) else {
                continue;
            };
            if line_item.sense != 0
                || !matches!(
                    line.kind,
                    crate::feature::definitions::FeatureSegmentKind::Line(_)
                )
            {
                continue;
            }
            if skamp.kind == 9
                && (point_item.sense != 0 || !section_skamp_is_point(ctx, definition, point_item)?)
            {
                continue;
            }
            if let Some(point_id) = selected_point_id(point_item) {
                pair = Some((line, point_id));
                break;
            }
        }
    }
    let Some(pair) = pair else {
        return Ok(None);
    };
    let Some(segment_table) = definition.segments.as_ref() else {
        return Ok(None);
    };
    let coordinate = if segment_table.is_complete() {
        section_line_fixed_coordinate(ctx, definition, pair.0)?
    } else {
        section_line_entity_fixed_coordinate_with_unique_rows(ctx, definition, pair.0.external_id)?
    };
    Ok(coordinate.map(|coordinate| (pair.0.point_ids()[0], pair.1, coordinate)))
}

pub(in crate::decode) fn section_skamp_saved_point_on_line(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Result<Option<(u32, SectionAxis, f64)>, CodecError> {
    let [first, second] = skamp.items.as_slice() else {
        return Ok(None);
    };
    let selected = if matches!(skamp.kind, 3 | 9) {
        let mut selected = None;
        for (line_item, point_item) in [(first, second), (second, first)] {
            if line_item.sense != 0 || (skamp.kind == 9 && point_item.sense != 0) {
                continue;
            }
            if skamp.kind == 9 && !section_skamp_is_point(ctx, definition, point_item)? {
                continue;
            }
            let Some(point_id) = section_skamp_selected_point_id(definition, point_item) else {
                continue;
            };
            selected = Some((line_item, point_id));
            break;
        }
        selected
    } else {
        None
    };
    let Some((line_item, point_id)) = selected else {
        return Ok(None);
    };
    if !saved_section_entity_fallback_allowed(definition, line_item.entity_id) {
        return Ok(None);
    }
    let Some(crate::feature::definitions::FeatureSavedEntity::Line(line)) =
        section_saved_entity(ctx, definition, line_item.entity_id)?
    else {
        return Ok(None);
    };
    let coordinate = section_line_entity_fixed_coordinate(ctx, definition, line_item.entity_id)?;
    Ok(coordinate.and_then(|coordinate| {
        Some((
            point_id,
            coordinate,
            saved_line_fixed_coordinate_value(line, coordinate)?,
        ))
    }))
}

#[derive(Clone, Copy)]
pub(super) enum SectionSymmetryAxis {
    Point(u32),
    Value(f64),
}

pub(super) fn section_skamp_axis_symmetry(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Result<
    Option<(
        SectionSymmetryAxis,
        SectionPointSource,
        SectionPointSource,
        SectionAxis,
    )>,
    CodecError,
> {
    let (14, [axis_item, first_item, second_item]) = (skamp.kind, skamp.items.as_slice()) else {
        return Ok(None);
    };
    if axis_item.sense != 0 || !section_skamp_is_line(ctx, definition, axis_item)? {
        return Ok(None);
    }
    let unique_row = unique_decoded_section_segment(definition, axis_item.entity_id);
    let Some(coordinate) = section_line_entity_fixed_coordinate_with_unique_rows(
        ctx,
        definition,
        axis_item.entity_id,
    )?
    else {
        return Ok(None);
    };
    let axis = if let Some(segment) = unique_section_skamp_segment(definition, axis_item.entity_id)
    {
        SectionSymmetryAxis::Point(segment.point_ids()[0])
    } else if let Some(segment) = unique_row {
        SectionSymmetryAxis::Point(segment.point_ids()[0])
    } else {
        if !saved_section_line_witness_allowed(definition, axis_item.entity_id) {
            return Ok(None);
        }
        let Some(crate::feature::definitions::FeatureSavedEntity::Line(line)) =
            section_saved_entity(ctx, definition, axis_item.entity_id)?
        else {
            return Ok(None);
        };
        let Some(value) = saved_line_fixed_coordinate_value(line, coordinate) else {
            return Ok(None);
        };
        SectionSymmetryAxis::Value(value)
    };
    let Some(first_point) = section_skamp_incidence_point(ctx, definition, first_item)? else {
        return Ok(None);
    };
    let Some(second_point) = section_skamp_incidence_point(ctx, definition, second_item)? else {
        return Ok(None);
    };
    Ok(Some((axis, first_point, second_point, coordinate)))
}

pub(super) fn section_skamp_point_symmetry(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Result<Option<(u32, SectionPointSource, SectionPointSource)>, CodecError> {
    let (14, [center, first, second]) = (skamp.kind, skamp.items.as_slice()) else {
        return Ok(None);
    };
    let Some(center) = section_skamp_point_entity_id(definition, center) else {
        return Ok(None);
    };
    let Some(first_point) = section_skamp_incidence_point(ctx, definition, first)? else {
        return Ok(None);
    };
    let Some(second_point) = section_skamp_incidence_point(ctx, definition, second)? else {
        return Ok(None);
    };
    Ok(Some((center, first_point, second_point)))
}

fn saved_line_fixed_coordinate_value(
    line: &crate::feature::definitions::FeatureSavedLine,
    coordinate: SectionAxis,
) -> Option<f64> {
    let [Some(first), Some(second)] = [
        line.endpoints[0][coordinate.index()],
        line.endpoints[1][coordinate.index()],
    ] else {
        return None;
    };
    let scale = first.abs().max(second.abs()).max(1.0);
    ((first - second).abs() <= EPS_SKAMP_AGREEMENT * scale).then_some(first)
}

#[derive(Clone, Copy)]
pub(in crate::decode) enum SectionPointSource {
    Point(u32),
    Value(cadmpeg_ir::units::FiniteVector<2>),
}

pub(in crate::decode) fn unique_section_skamp_segment(
    definition: &crate::feature::definitions::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::definitions::FeatureSegment> {
    definition.segments.as_ref()?.segment(external_id)
}

pub(in crate::decode) fn unique_decoded_section_segment(
    definition: &crate::feature::definitions::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::definitions::FeatureSegment> {
    definition.segments.as_ref()?.unique_segment(external_id)
}

pub(in crate::decode) fn section_segment_rows<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'a crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<&'a crate::feature::definitions::FeatureSegment>, cadmpeg_core::CodecError> {
    match definition.segments.as_ref() {
        Some(table) => ordinary_segment_rows(ctx, table, "creo section segment rows"),
        None => Ok(Vec::new()),
    }
}

pub(in crate::decode) fn complete_section_segment_rows<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'a crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<&'a crate::feature::definitions::FeatureSegment>, cadmpeg_core::CodecError> {
    match definition
        .segments
        .as_ref()
        .filter(|table| table.is_complete())
    {
        Some(table) => ordinary_segment_rows(ctx, table, "creo complete section segment rows"),
        None => Ok(Vec::new()),
    }
}

/// The ordinary segment rows of `table`, in stored order, in one pass.
fn ordinary_segment_rows<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    table: &'a crate::feature::definitions::FeatureSegmentTable,
    operation: &'static str,
) -> Result<Vec<&'a crate::feature::definitions::FeatureSegment>, cadmpeg_core::CodecError> {
    let mut rows = Vec::new();
    for row in ctx.admit_iter(table.rows.as_slice(), operation)? {
        if let SegmentRow::Ordinary(segment) = row {
            ctx.push_vec(&mut rows, segment, operation)?;
        }
    }
    Ok(rows)
}

pub(super) fn section_skamp_point_entity_id(
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Option<u32> {
    if let Some(point) = unique_point_segment(definition, item.entity_id) {
        return (item.sense == 0).then_some(point.point_id);
    }
    let segment = unique_decoded_section_segment(definition, item.entity_id)?;
    (item.sense == 0
        && matches!(
            segment.kind,
            crate::feature::definitions::FeatureSegmentKind::Point(_)
        ))
    .then_some(segment.point_ids()[0])
}

pub(in crate::decode) fn section_skamp_selected_point_id(
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Option<u32> {
    let ordinary_segment = unique_section_skamp_segment(definition, item.entity_id).or_else(|| {
        unique_decoded_section_segment(definition, item.entity_id).filter(|segment| {
            matches!(
                segment.kind,
                crate::feature::definitions::FeatureSegmentKind::Point(_)
            )
        })
    });
    section_skamp_selected_point_id_with_ordinary_segment(definition, item, ordinary_segment)
}

pub(in crate::decode) fn section_skamp_selected_point_id_with_ordinary_segment(
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
    ordinary_segment: Option<&crate::feature::definitions::FeatureSegment>,
) -> Option<u32> {
    if let Some(segment) = unique_centered_line_segment(definition, item.entity_id) {
        return match item.sense {
            2 => Some(0),
            3 => Some(1),
            4 => Some(segment.center_id),
            _ => None,
        };
    }
    if let Some(segment) = unique_reference_line_segment(definition, item.entity_id) {
        return match item.sense {
            2 => segment.point_ids[0],
            3 => segment.point_ids[1],
            _ => None,
        };
    }
    if let Some(segment) = unique_bounded_curve_segment(definition, item.entity_id) {
        return match item.sense {
            2 => Some(segment.point_ids[0]),
            3 => Some(segment.point_ids[1]),
            _ => None,
        };
    }
    if let Some(point) = unique_point_segment(definition, item.entity_id) {
        return matches!(item.sense, 0 | 4).then_some(point.point_id);
    }
    if let Some(circle) = unique_circle_segment(definition, item.entity_id) {
        return (item.sense == 4).then_some(circle.center_id);
    }
    let segment = ordinary_segment?;
    if matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Point(_)
    ) {
        return matches!(item.sense, 0 | 4).then_some(segment.point_ids()[0]);
    }
    match item.sense {
        2 => Some(segment.point_ids()[0]),
        3 => Some(segment.point_ids()[1]),
        4 => segment.center_id,
        _ => None,
    }
}

fn section_skamp_selected_point(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SectionPointSource>, CodecError> {
    if let Some(point_id) = section_skamp_selected_point_id(definition, item) {
        return Ok(Some(SectionPointSource::Point(point_id)));
    }
    Ok(saved_section_point(ctx, definition, item)?.map(SectionPointSource::Value))
}

pub(in crate::decode) fn section_skamp_incidence_point(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SectionPointSource>, CodecError> {
    if let Some(point) = section_skamp_selected_point(ctx, definition, item)? {
        return Ok(Some(point));
    }
    Ok(section_skamp_selected_point_id_with_ordinary_segment(
        definition,
        item,
        unique_decoded_section_segment(definition, item.entity_id),
    )
    .map(SectionPointSource::Point))
}

fn saved_section_point(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<cadmpeg_ir::units::FiniteVector<2>>, CodecError> {
    if !saved_section_entity_fallback_allowed(definition, item.entity_id) {
        return Ok(None);
    }
    let Some(saved) = section_saved_entity(ctx, definition, item.entity_id)? else {
        return Ok(None);
    };
    let coordinates = match (saved, item.sense) {
        (crate::feature::definitions::FeatureSavedEntity::Line(line), 2) => line.endpoints[0],
        (crate::feature::definitions::FeatureSavedEntity::Line(line), 3) => line.endpoints[1],
        (crate::feature::definitions::FeatureSavedEntity::Arc(arc), 2) => arc.endpoints[0],
        (crate::feature::definitions::FeatureSavedEntity::Arc(arc), 3) => arc.endpoints[1],
        (crate::feature::definitions::FeatureSavedEntity::Arc(arc), 4) => arc.center,
        (crate::feature::definitions::FeatureSavedEntity::Circle(circle), 4) => circle.center,
        (crate::feature::definitions::FeatureSavedEntity::Conic(conic), 4) => {
            let Some(frame) = conic.local_system else {
                return Ok(None);
            };
            [Some(frame[9]), Some(frame[10]), Some(frame[11])]
        }
        _ => return Ok(None),
    };
    let [Some(u), Some(v), _] = coordinates else {
        return Ok(None);
    };
    Ok(cadmpeg_ir::units::FiniteVector::new([u, v]))
}

#[cfg(test)]
mod tests {
    use super::{
        section_line_entity_fixed_coordinate as section_line_entity_fixed_coordinate_admitted,
        section_line_entity_fixed_coordinate_with_unique_rows as section_line_entity_fixed_coordinate_with_unique_rows_admitted,
        section_skamp_axis_symmetry as section_skamp_axis_symmetry_admitted,
        section_skamp_point_entity_id,
        section_skamp_point_on_line as section_skamp_point_on_line_admitted,
        section_skamp_point_symmetry, section_skamp_selected_point_id, SectionPointSource,
    };

    fn section_line_entity_fixed_coordinate(
        definition: &crate::feature::definitions::FeatureDefinition,
        entity_id: u32,
    ) -> Option<super::SectionAxis> {
        crate::decode::with_test_decode_ctx(|ctx| {
            section_line_entity_fixed_coordinate_admitted(ctx, definition, entity_id)
        })
        .expect("test fixed-coordinate graph")
    }

    fn section_line_entity_fixed_coordinate_with_unique_rows(
        definition: &crate::feature::definitions::FeatureDefinition,
        entity_id: u32,
    ) -> Option<super::SectionAxis> {
        crate::decode::with_test_decode_ctx(|ctx| {
            section_line_entity_fixed_coordinate_with_unique_rows_admitted(
                ctx, definition, entity_id,
            )
        })
        .expect("test fixed-coordinate graph")
    }

    fn section_skamp_axis_symmetry(
        definition: &crate::feature::definitions::FeatureDefinition,
        skamp: &crate::feature::definitions::FeatureSkamp,
    ) -> Option<(
        super::SectionSymmetryAxis,
        super::SectionPointSource,
        super::SectionPointSource,
        super::SectionAxis,
    )> {
        crate::decode::with_test_decode_ctx(|ctx| {
            section_skamp_axis_symmetry_admitted(ctx, definition, skamp)
        })
        .expect("test axis symmetry")
    }

    fn section_skamp_point_on_line(
        definition: &crate::feature::definitions::FeatureDefinition,
        skamp: &crate::feature::definitions::FeatureSkamp,
    ) -> Option<(u32, u32, super::SectionAxis)> {
        crate::decode::with_test_decode_ctx(|ctx| {
            section_skamp_point_on_line_admitted(ctx, definition, skamp)
        })
        .expect("test point-on-line")
    }

    fn point_definition(
        declared_count: u32,
        rows: Vec<crate::feature::definitions::FeatureSegment>,
        point_rows: Vec<crate::feature::definitions::FeaturePointSegment>,
    ) -> crate::feature::definitions::FeatureDefinition {
        crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (rows)
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                    .chain(
                        (point_rows)
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
            relations: None,
            saved_section: None,
            offset: 0,
        }
    }

    fn fixed_coordinate_graph_fixture() -> crate::feature::definitions::FeatureDefinition {
        let line = |external_id, vertical_horizontal| crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line([
                external_id,
                external_id + 1,
            ]),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: usize::try_from(external_id).expect("fixture index fits usize"),
        };
        let mut definition =
            point_definition(2, vec![line(10, Some(0)), line(20, None)], Vec::new());
        definition.relations = Some(crate::feature::definitions::FeatureRelationTable {
            declared_count: 1,
            entity_ref: None,
            rows: Vec::new(),
            skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
                header: crate::feature::definitions::FeatureSolverTableHeader {
                    declared_count: 1,
                    entity_ref: 0,
                    offset: 0,
                },
                rows: vec![crate::feature::definitions::FeatureSkamp {
                    id: 1,
                    kind: 7,
                    flags: 0,
                    status: 1,
                    items: vec![
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 10,
                            sense: 0,
                        },
                        crate::feature::definitions::FeatureSkampItem {
                            entity_id: 20,
                            sense: 0,
                        },
                    ],
                    offset: 0,
                }],
            }),
            triples: None,
            offset: 0,
        });
        definition
    }

    fn assert_fixed_coordinate_graph_refusal(operations: &[&str]) {
        let definition = fixed_coordinate_graph_fixture();
        crate::test_support::assert_refusal_order(cadmpeg_core::decode::ResourceDimension::CollectionItems, operations, |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::default();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
            super::section_line_entity_fixed_coordinate(&ctx, &definition, 20)
        });
    }

    #[test]
    fn fixed_coordinate_adjacency_node_refuses_before_insertion() {
        assert_fixed_coordinate_graph_refusal(&["creo fixed-coordinate adjacency nodes", "creo fixed-coordinate adjacency nodes"]);
    }

    #[test]
    fn fixed_coordinate_adjacency_link_refuses_before_reservation() {
        assert_fixed_coordinate_graph_refusal(&["creo fixed-coordinate adjacency links", "creo fixed-coordinate adjacency links"]);
    }

    #[test]
    fn fixed_coordinate_parity_seed_refuses_before_insertion() {
        assert_fixed_coordinate_graph_refusal(&["creo fixed-coordinate parity seed"]);
    }

    #[test]
    fn fixed_coordinate_pending_seed_refuses_before_reservation() {
        assert_fixed_coordinate_graph_refusal(&["creo fixed-coordinate pending seed"]);
    }

    #[test]
    fn fixed_coordinate_parity_node_refuses_before_insertion() {
        assert_fixed_coordinate_graph_refusal(&["creo fixed-coordinate parity nodes"]);
    }

    #[test]
    fn fixed_coordinate_pending_node_refuses_before_reservation() {
        assert_fixed_coordinate_graph_refusal(&["creo fixed-coordinate pending nodes"]);
    }

    #[test]
    fn direct_fixed_coordinate_node_refuses_before_insertion() {
        assert_fixed_coordinate_graph_refusal(&["creo direct fixed-coordinate nodes"]);
    }

    #[test]
    fn fixed_coordinate_result_node_refuses_before_insertion() {
        assert_fixed_coordinate_graph_refusal(&["creo fixed-coordinate result nodes"]);
    }

    #[test]
    fn fixed_coordinate_graph_service_keeps_axis() {
        let definition = fixed_coordinate_graph_fixture();
        assert_eq!(
            section_line_entity_fixed_coordinate(&definition, 20),
            Some(super::SectionAxis::U)
        );
    }

    #[test]
    fn fixed_coordinate_graph_work_refuses_before_scan_and_traversal() {
        let definition = fixed_coordinate_graph_fixture();
        crate::test_support::assert_work_boundaries(
            &[
                "creo fixed-coordinate graph traversal",
                "creo fixed-coordinate parity lookup",
                "creo fixed-coordinate adjacency lookup",
            ],
            |ctx| super::section_line_entity_fixed_coordinate(ctx, &definition, 20),
        );
    }

    fn ordinary_point(
        external_id: u32,
        point_id: u32,
        offset: usize,
    ) -> crate::feature::definitions::FeatureSegment {
        crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Point(point_id),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset,
        }
    }

    #[test]
    fn section_segment_rows_refuse_before_vector_growth() {
        let definition = point_definition(2, vec![ordinary_point(7, 42, 1)], Vec::new());
        let error = crate::test_support::last_refusal_at(&[], cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo section segment rows", |ctx| super::section_segment_rows(ctx, &definition));
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "creo section segment rows"),
            "{error:?}"
        );
        let rows = crate::decode::with_test_decode_ctx(|ctx| {
            super::section_segment_rows(ctx, &definition)
        })
        .expect("service profile admits the ordinary row");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].external_id, 7);
    }

    #[test]
    fn complete_section_segment_rows_refuse_before_vector_growth() {
        let definition = point_definition(1, vec![ordinary_point(7, 42, 1)], Vec::new());
        let error = crate::test_support::last_refusal_at(&[], cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo complete section segment rows", |ctx| super::complete_section_segment_rows(ctx, &definition));
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "creo complete section segment rows"),
            "{error:?}"
        );
        let rows = crate::decode::with_test_decode_ctx(|ctx| {
            super::complete_section_segment_rows(ctx, &definition)
        })
        .expect("service profile admits the complete ordinary row");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].external_id, 7);
    }

    #[test]
    fn unique_ordinary_point_rows_supply_sense_zero_ids_in_incomplete_tables() {
        let incomplete = point_definition(2, vec![ordinary_point(7, 42, 1)], Vec::new());
        assert_eq!(
            section_skamp_point_entity_id(
                &incomplete,
                &crate::feature::definitions::FeatureSkampItem {
                    entity_id: 7,
                    sense: 0,
                },
            ),
            Some(42)
        );
        assert_eq!(
            section_skamp_selected_point_id(
                &incomplete,
                &crate::feature::definitions::FeatureSkampItem {
                    entity_id: 7,
                    sense: 4,
                },
            ),
            Some(42)
        );

        let duplicate = point_definition(
            2,
            vec![ordinary_point(7, 42, 1), ordinary_point(7, 43, 2)],
            Vec::new(),
        );
        assert_eq!(
            section_skamp_point_entity_id(
                &duplicate,
                &crate::feature::definitions::FeatureSkampItem {
                    entity_id: 7,
                    sense: 0,
                },
            ),
            None
        );
        assert_eq!(
            section_skamp_selected_point_id(
                &duplicate,
                &crate::feature::definitions::FeatureSkampItem {
                    entity_id: 7,
                    sense: 4,
                },
            ),
            None
        );

        let cross_family_duplicate = point_definition(
            1,
            vec![ordinary_point(7, 42, 1)],
            vec![crate::feature::definitions::FeaturePointSegment {
                point_id: 44,
                external_id: 7,
                offset: 2,
            }],
        );
        assert_eq!(
            section_skamp_point_entity_id(
                &cross_family_duplicate,
                &crate::feature::definitions::FeatureSkampItem {
                    entity_id: 7,
                    sense: 0,
                },
            ),
            None
        );
        assert_eq!(
            section_skamp_selected_point_id(
                &cross_family_duplicate,
                &crate::feature::definitions::FeatureSkampItem {
                    entity_id: 7,
                    sense: 4,
                },
            ),
            None
        );
    }

    #[test]
    fn incomplete_unique_rows_supply_point_symmetry_sources() {
        let line = |external_id, point_ids| crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line(point_ids),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: usize::try_from(external_id).expect("fixture index fits usize"),
        };
        let definition = point_definition(
            4,
            vec![ordinary_point(5, 9, 1), line(10, [1, 2]), line(11, [3, 4])],
            Vec::new(),
        );
        let skamp = crate::feature::definitions::FeatureSkamp {
            id: 14,
            kind: 14,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 5,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 10,
                    sense: 2,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 11,
                    sense: 3,
                },
            ],
            offset: 0,
        };
        let Some((center, first, second)) = crate::decode::with_test_decode_ctx(|ctx| {
            section_skamp_point_symmetry(ctx, &definition, &skamp)
        })
        .expect("admitted point symmetry rows") else {
            panic!("point-symmetry sources");
        };
        assert_eq!(center, 9);
        assert!(matches!(first, SectionPointSource::Point(1)));
        assert!(matches!(second, SectionPointSource::Point(4)));

        let mut duplicate = definition.clone();
        duplicate.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Ordinary(line(10, [6, 7])),
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_skamp_point_symmetry(
                ctx, &duplicate, &skamp
            ))
            .expect("admitted point symmetry rows")
            .is_none()
        );

        let mut cross_family = definition;
        cross_family
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Point(
                crate::feature::definitions::FeaturePointSegment {
                    point_id: 99,
                    external_id: 10,
                    offset: 99,
                },
            ));
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_skamp_point_symmetry(
                ctx,
                &cross_family,
                &skamp
            ))
            .expect("admitted point symmetry rows")
            .is_none()
        );
    }

    #[test]
    fn incomplete_unique_rows_supply_axis_symmetry_sources() {
        let line = |external_id, point_ids| crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line(point_ids),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: usize::try_from(external_id).expect("fixture index fits usize"),
        };
        let mut definition =
            point_definition(3, vec![line(10, [1, 2]), line(11, [3, 4])], Vec::new());
        definition.order_table = Some(crate::feature::definitions::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::definitions::FeatureOrderRow {
                external_id: 99,
                internal_id: 20,
                bitmask: 0,
                offset: 1,
            }]
            .into(),
            offset: 0,
        });
        definition.saved_section = Some(crate::feature::definitions::FeatureSavedSection {
            entities: vec![crate::feature::definitions::FeatureSavedEntity::Line(
                crate::feature::definitions::FeatureSavedLine {
                    entity_id: 20,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [
                        [Some(0.0), Some(0.0), Some(0.0)],
                        [Some(0.0), Some(2.0), Some(0.0)],
                    ],
                    body: Vec::new(),
                    offset: 2,
                },
            )],
            offset: 2,
        });
        let skamp = crate::feature::definitions::FeatureSkamp {
            id: 14,
            kind: 14,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 99,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 10,
                    sense: 2,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 11,
                    sense: 3,
                },
            ],
            offset: 0,
        };
        let Some((axis, first, second, coordinate)) =
            section_skamp_axis_symmetry(&definition, &skamp)
        else {
            panic!("axis-symmetry sources");
        };
        assert!(matches!(axis, super::SectionSymmetryAxis::Value(0.0)));
        assert!(matches!(first, SectionPointSource::Point(1)));
        assert!(matches!(second, SectionPointSource::Point(4)));
        assert_eq!(coordinate, crate::decode::sketch::axis::SectionAxis::U);

        let mut conflicting_saved_axis = definition.clone();
        conflicting_saved_axis
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::ReferenceLine(
                crate::feature::definitions::FeatureReferenceLineSegment {
                    directions: [None; 3],
                    point_ids: [None; 2],
                    vertical_horizontal: Some(0),
                    external_id: 99,
                    offset: 3,
                },
            ));
        assert!(section_skamp_axis_symmetry(&conflicting_saved_axis, &skamp).is_none());

        let mut duplicate = definition;
        duplicate.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Ordinary(line(10, [5, 6])),
        );
        assert!(section_skamp_axis_symmetry(&duplicate, &skamp).is_none());

        let mut incomplete_axis = point_definition(
            4,
            vec![line(99, [8, 9]), line(10, [1, 2]), line(11, [3, 4])],
            Vec::new(),
        );
        incomplete_axis
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .edit_ordinary(|rows| rows[0].vertical_horizontal = Some(0));
        assert_eq!(
            section_line_entity_fixed_coordinate(&incomplete_axis, 99),
            None
        );
        assert_eq!(
            section_line_entity_fixed_coordinate_with_unique_rows(&incomplete_axis, 99),
            Some(crate::decode::sketch::axis::SectionAxis::U)
        );
        let Some((axis, first, second, coordinate)) =
            section_skamp_axis_symmetry(&incomplete_axis, &skamp)
        else {
            panic!("incomplete axis-symmetry sources");
        };
        assert!(matches!(axis, super::SectionSymmetryAxis::Point(8)));
        assert!(matches!(first, SectionPointSource::Point(1)));
        assert!(matches!(second, SectionPointSource::Point(4)));
        assert_eq!(coordinate, crate::decode::sketch::axis::SectionAxis::U);

        let mut duplicate_axis = incomplete_axis.clone();
        duplicate_axis
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(line(
                99,
                [10, 11],
            )));
        let mut duplicate_axis_skamp = skamp.clone();
        duplicate_axis_skamp.items[0].entity_id = 99;
        assert!(section_skamp_axis_symmetry(&duplicate_axis, &duplicate_axis_skamp).is_none());

        let mut conflicting_orientation = incomplete_axis;
        conflicting_orientation.relations =
            Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: 1,
                entity_ref: None,
                rows: Vec::new(),
                skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
                    header: crate::feature::definitions::FeatureSolverTableHeader {
                        declared_count: 1,
                        entity_ref: 0,
                        offset: 0,
                    },
                    rows: vec![crate::feature::definitions::FeatureSkamp {
                        id: 1,
                        kind: 1,
                        flags: 0,
                        status: 1,
                        items: vec![crate::feature::definitions::FeatureSkampItem {
                            entity_id: 99,
                            sense: 0,
                        }],
                        offset: 0,
                    }],
                }),
                triples: None,
                offset: 0,
            });
        assert!(section_line_entity_fixed_coordinate_with_unique_rows(
            &conflicting_orientation,
            99
        )
        .is_none());
        assert!(section_skamp_axis_symmetry(&conflicting_orientation, &skamp).is_none());
    }

    #[test]
    fn incomplete_unique_rows_supply_point_on_line_sources() {
        let line = |external_id, point_ids, vertical_horizontal| {
            crate::feature::definitions::FeatureSegment {
                kind: crate::feature::definitions::FeatureSegmentKind::Line(point_ids),
                directions: [None; 3],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal,
                radius_ref: None,
                radius2_ref: None,
                external_id,
                body: Vec::new(),
                offset: usize::try_from(external_id).expect("fixture index fits usize"),
            }
        };
        let definition = point_definition(
            4,
            vec![line(10, [1, 2], Some(1)), line(20, [3, 4], None)],
            vec![crate::feature::definitions::FeaturePointSegment {
                point_id: 5,
                external_id: 30,
                offset: 30,
            }],
        );
        assert!(!definition
            .segments
            .as_ref()
            .expect("segments")
            .is_complete());

        let type_three = crate::feature::definitions::FeatureSkamp {
            id: 3,
            kind: 3,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 10,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 20,
                    sense: 2,
                },
            ],
            offset: 0,
        };
        assert_eq!(
            section_skamp_point_on_line(&definition, &type_three),
            Some((1, 3, crate::decode::sketch::axis::SectionAxis::V))
        );

        let mut unary_orientation = definition.clone();
        unary_orientation
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .edit_ordinary(|rows| rows[0].vertical_horizontal = None);
        unary_orientation.relations = Some(crate::feature::definitions::FeatureRelationTable {
            declared_count: 1,
            entity_ref: None,
            rows: Vec::new(),
            skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
                header: crate::feature::definitions::FeatureSolverTableHeader {
                    declared_count: 1,
                    entity_ref: 0,
                    offset: 0,
                },
                rows: vec![crate::feature::definitions::FeatureSkamp {
                    id: 1,
                    kind: 1,
                    flags: 0,
                    status: 1,
                    items: vec![crate::feature::definitions::FeatureSkampItem {
                        entity_id: 10,
                        sense: 0,
                    }],
                    offset: 0,
                }],
            }),
            triples: None,
            offset: 0,
        });
        assert_eq!(
            section_skamp_point_on_line(&unary_orientation, &type_three),
            Some((1, 3, crate::decode::sketch::axis::SectionAxis::V))
        );

        let type_nine = crate::feature::definitions::FeatureSkamp {
            id: 9,
            kind: 9,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 10,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 30,
                    sense: 0,
                },
            ],
            offset: 0,
        };
        assert_eq!(
            section_skamp_point_on_line(&definition, &type_nine),
            Some((1, 5, crate::decode::sketch::axis::SectionAxis::V))
        );

        let mut duplicate = definition.clone();
        duplicate.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Ordinary(line(20, [6, 7], None)),
        );
        assert!(section_skamp_point_on_line(&duplicate, &type_three).is_none());

        let mut cross_family = definition.clone();
        cross_family
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Point(
                crate::feature::definitions::FeaturePointSegment {
                    point_id: 8,
                    external_id: 20,
                    offset: 31,
                },
            ));
        assert!(section_skamp_point_on_line(&cross_family, &type_three).is_none());

        let mut missing_selector = definition;
        missing_selector
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .edit_ordinary(|rows| rows[0].vertical_horizontal = None);
        assert!(section_skamp_point_on_line(&missing_selector, &type_three).is_none());
    }

    #[test]
    fn saved_line_axis_witness_requires_an_ordinary_line_identity() {
        let line = |external_id| crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: usize::try_from(external_id).expect("fixture index fits usize"),
        };
        let saved_section = crate::feature::definitions::FeatureSavedSection {
            entities: vec![crate::feature::definitions::FeatureSavedEntity::Line(
                crate::feature::definitions::FeatureSavedLine {
                    entity_id: 7,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [
                        [Some(0.0), Some(0.0), Some(0.0)],
                        [Some(0.0), Some(2.0), Some(0.0)],
                    ],
                    body: Vec::new(),
                    offset: 1,
                },
            )],
            offset: 1,
        };
        let order_table = crate::feature::definitions::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::definitions::FeatureOrderRow {
                external_id: 7,
                internal_id: 7,
                bitmask: 0,
                offset: 1,
            }]
            .into(),
            offset: 1,
        };

        let mut special = point_definition(1, Vec::new(), Vec::new());
        special.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Circle(
                crate::feature::definitions::FeatureCircleSegment {
                    center_id: 1,
                    radius_ref: 2,
                    external_id: 7,
                    offset: 1,
                },
            ),
        );
        special.order_table = Some(order_table.clone());
        special.saved_section = Some(saved_section.clone());
        assert_eq!(
            section_line_entity_fixed_coordinate_with_unique_rows(&special, 7),
            None
        );

        let mut ordinary = special;
        let segments = ordinary.segments.as_mut().expect("segments");
        segments.rows.edit_circles(Vec::clear);
        segments
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(line(7)));
        ordinary.saved_section = Some(saved_section);
        assert_eq!(
            section_line_entity_fixed_coordinate_with_unique_rows(&ordinary, 7),
            Some(crate::decode::sketch::axis::SectionAxis::U)
        );
    }
}
