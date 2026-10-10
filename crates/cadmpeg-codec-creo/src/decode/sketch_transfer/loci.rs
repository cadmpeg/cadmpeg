// SPDX-License-Identifier: Apache-2.0
//! Section point, segment, and SKAMP locus resolution.

use crate::decode::sketch::axis::SectionAxis;

use super::super::feature_history::dimensions::feature_skamp_table_complete;
use super::super::sketch::skamp::{
    section_skamp_incidence_point, section_skamp_selected_point_id_with_ordinary_segment,
    unique_decoded_section_segment, SectionPointSource,
};
use super::super::sketch_ids::sketch_entity_id_admitted;
use crate::decode::sketch_transfer::identity::{
    saved_section_entity_by_internal_id, saved_section_entity_fallback_allowed,
};
use crate::decode::sketch_transfer::profiles::{
    solver_only_section_entity_family, solver_only_section_entity_offset,
    unique_section_incidence_curve_family,
    unique_section_incidence_curve_family_without_type35_target, SectionEntityIncidenceFamily,
};
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_ir::sketches::{
    SketchCoordinateAxis, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchLocus,
};
use cadmpeg_ir::units::FiniteVector;
use std::collections::BTreeMap;
use std::ops::ControlFlow;

fn admitted_entity(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    sketch: &SketchId,
    external_id: u32,
) -> Option<SketchEntityId> {
    match sketch_entity_id_admitted(ctx, sketch, external_id) {
        Ok(id) => id,
        Err(error) => {
            refusal.set(Some(refusal.take().unwrap_or(error)));
            None
        }
    }
}

const EPS_NONDEGENERATE_LINE: f64 = 0.000_000_000_001_f64;
const EPS_LOCUS_COORDINATE: f64 = 1.0e-9;
const EPS_LOCUS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_LOCUS_RADIUS_AGREEMENT: f64 = 1.0e-9;

#[derive(Clone, Copy)]
enum PointLocusKind {
    Entity,
    Start,
    End,
    Center,
}

pub(super) fn section_point_locus(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    point_id: u32,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(None);
    };
    let mut candidate = None;
    let mut rows = segments.rows.as_slice().iter();
    while rows.len() != 0 {
        let Some(row) = ctx.next_charged(&mut rows, "creo point locus rows")? else {
            break;
        };
        let (external_id, kinds) = match row {
            SegmentRow::Ordinary(segment) => {
                use crate::feature::definitions::FeatureSegmentKind;
                let endpoint = match segment.kind {
                    FeatureSegmentKind::Point(id) if id == point_id => Some(PointLocusKind::Entity),
                    FeatureSegmentKind::Line([start, _]) if start == point_id => {
                        Some(PointLocusKind::Start)
                    }
                    FeatureSegmentKind::Line([_, end]) if end == point_id => {
                        Some(PointLocusKind::End)
                    }
                    FeatureSegmentKind::Arc([end, _]) if end == point_id => {
                        Some(PointLocusKind::End)
                    }
                    FeatureSegmentKind::Arc([_, start]) if start == point_id => {
                        Some(PointLocusKind::Start)
                    }
                    _ => None,
                };
                let center = (matches!(segment.kind, FeatureSegmentKind::Arc(_))
                    && segment.center_id == Some(point_id))
                .then_some(PointLocusKind::Center);
                (segment.external_id, [endpoint, center])
            }
            SegmentRow::Circle(segment) => (
                segment.external_id,
                [
                    (segment.center_id == point_id).then_some(PointLocusKind::Center),
                    None,
                ],
            ),
            SegmentRow::Point(segment) => (
                segment.external_id,
                [
                    (segment.point_id == point_id).then_some(PointLocusKind::Entity),
                    None,
                ],
            ),
            SegmentRow::CenteredLine(segment) => (
                segment.external_id,
                [
                    (point_id == 0).then_some(PointLocusKind::Start),
                    (point_id == 1).then_some(PointLocusKind::End),
                ],
            ),
            SegmentRow::ReferenceLine(segment) => (
                segment.external_id,
                [
                    (segment.point_ids[0] == Some(point_id)).then_some(PointLocusKind::Start),
                    (segment.point_ids[1] == Some(point_id)).then_some(PointLocusKind::End),
                ],
            ),
            SegmentRow::BoundedCurve(segment) => (
                segment.external_id,
                [
                    (segment.point_ids[0] == point_id).then_some(PointLocusKind::Start),
                    (segment.point_ids[1] == point_id).then_some(PointLocusKind::End),
                ],
            ),
            SegmentRow::Conic(_) | SegmentRow::Opaque(_) => continue,
        };
        if segments.rows.get(external_id).is_none() {
            continue;
        }
        for kind in kinds.into_iter().flatten() {
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some((external_id, kind));
        }
    }
    let Some((external_id, kind)) = candidate else {
        return Ok(None);
    };
    let Some(entity) =
        super::super::sketch_ids::sketch_entity_id_admitted(ctx, sketch, external_id)?
    else {
        return Ok(None);
    };
    Ok(Some(match kind {
        PointLocusKind::Entity => SketchLocus::Entity(entity),
        PointLocusKind::Start => SketchLocus::Start(entity),
        PointLocusKind::End => SketchLocus::End(entity),
        PointLocusKind::Center => SketchLocus::Center(entity),
    }))
}

pub(in super::super) fn unique_circle_segment(
    definition: &crate::feature::definitions::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::definitions::FeatureCircleSegment> {
    match definition.segments.as_ref()?.rows.get(external_id)? {
        SegmentRow::Circle(row) => Some(row),
        _ => None,
    }
}

pub(in super::super) fn unique_point_segment(
    definition: &crate::feature::definitions::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::definitions::FeaturePointSegment> {
    match definition.segments.as_ref()?.rows.get(external_id)? {
        SegmentRow::Point(row) => Some(row),
        _ => None,
    }
}

pub(in super::super) fn unique_centered_line_segment(
    definition: &crate::feature::definitions::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::definitions::FeatureCenteredLineSegment> {
    match definition.segments.as_ref()?.rows.get(external_id)? {
        SegmentRow::CenteredLine(row) => Some(row),
        _ => None,
    }
}

pub(in super::super) fn unique_reference_line_segment(
    definition: &crate::feature::definitions::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::definitions::FeatureReferenceLineSegment> {
    match definition.segments.as_ref()?.rows.get(external_id)? {
        SegmentRow::ReferenceLine(row) => Some(row),
        _ => None,
    }
}

pub(in super::super) fn unique_bounded_curve_segment(
    definition: &crate::feature::definitions::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::definitions::FeatureBoundedCurveSegment> {
    match definition.segments.as_ref()?.rows.get(external_id)? {
        SegmentRow::BoundedCurve(row) => Some(row),
        _ => None,
    }
}

pub(in super::super) fn section_skamp_locus(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    let Some(entity) = admitted_entity(ctx, refusal, sketch, item.entity_id) else {
        return Ok(None);
    };
    if let Some(family) = solver_only_section_entity_family(ctx, definition, item.entity_id)? {
        return Ok(section_entity_family_locus(family, entity, item.sense));
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return Ok(match (segment.kind, item.sense) {
            (_, 0) => Some(SketchLocus::Entity(entity)),
            (crate::feature::definitions::FeatureSegmentKind::Arc(_), 2) => {
                Some(SketchLocus::End(entity))
            }
            (crate::feature::definitions::FeatureSegmentKind::Arc(_), 3) => {
                Some(SketchLocus::Start(entity))
            }
            (crate::feature::definitions::FeatureSegmentKind::Arc(_), 4) => {
                Some(SketchLocus::Center(entity))
            }
            (_, 2) => Some(SketchLocus::Start(entity)),
            (_, 3) => Some(SketchLocus::End(entity)),
            _ => None,
        });
    }
    if let Some(segment) = unique_reference_line_segment(definition, item.entity_id) {
        return Ok(match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            2 if segment.point_ids[0].is_some() => Some(SketchLocus::Start(entity)),
            3 if segment.point_ids[1].is_some() => Some(SketchLocus::End(entity)),
            _ => None,
        });
    }
    if unique_bounded_curve_segment(definition, item.entity_id).is_some() {
        return Ok(match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            2 => Some(SketchLocus::Start(entity)),
            3 => Some(SketchLocus::End(entity)),
            _ => None,
        });
    }
    if unique_point_segment(definition, item.entity_id).is_some() {
        return Ok(matches!(item.sense, 0 | 4).then_some(SketchLocus::Entity(entity)));
    }
    if unique_centered_line_segment(definition, item.entity_id).is_some() {
        return Ok(match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            2 => Some(SketchLocus::Start(entity)),
            3 => Some(SketchLocus::End(entity)),
            4 => Some(SketchLocus::Center(entity)),
            _ => None,
        });
    }
    if unique_circle_segment(definition, item.entity_id).is_some() {
        return Ok(match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            4 => Some(SketchLocus::Center(entity)),
            _ => None,
        });
    }
    if definition
        .segments
        .as_ref()
        .is_some_and(|segments| segments.rows.contains_id(item.entity_id))
    {
        return section_incidence_curve_locus(ctx, definition, entity, item);
    }
    let Some(saved) = section_saved_entity(ctx, definition, item.entity_id)? else {
        return Ok(None);
    };
    Ok(match (saved, item.sense) {
        (_, 0) => Some(SketchLocus::Entity(entity)),
        (crate::feature::definitions::FeatureSavedEntity::Line(_), 2) => {
            Some(SketchLocus::Start(entity))
        }
        (crate::feature::definitions::FeatureSavedEntity::Line(_), 3) => {
            Some(SketchLocus::End(entity))
        }
        (crate::feature::definitions::FeatureSavedEntity::Arc(_), 2) => {
            Some(SketchLocus::End(entity))
        }
        (crate::feature::definitions::FeatureSavedEntity::Arc(_), 3) => {
            Some(SketchLocus::Start(entity))
        }
        (
            crate::feature::definitions::FeatureSavedEntity::Arc(_)
            | crate::feature::definitions::FeatureSavedEntity::Circle(_)
            | crate::feature::definitions::FeatureSavedEntity::Conic(_),
            4,
        ) => Some(SketchLocus::Center(entity)),
        _ => None,
    })
}

fn section_incidence_curve_locus(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity: SketchEntityId,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    let Some(family) = unique_section_incidence_curve_family(ctx, definition, item.entity_id)?
    else {
        return Ok(None);
    };
    Ok(section_entity_family_locus(family, entity, item.sense))
}

fn section_entity_family_locus(
    family: SectionEntityIncidenceFamily,
    entity: SketchEntityId,
    sense: u32,
) -> Option<SketchLocus> {
    match (family, sense) {
        (SectionEntityIncidenceFamily::Point, 0) => Some(SketchLocus::Entity(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::LineOrArc
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            0,
        ) => Some(SketchLocus::Entity(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::LineOrArc
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            2,
        ) => Some(SketchLocus::Start(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::LineOrArc
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            3,
        ) => Some(SketchLocus::End(entity)),
        (SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular, 4) => {
            Some(SketchLocus::Center(entity))
        }
        (SectionEntityIncidenceFamily::Circular, 0) => Some(SketchLocus::Entity(entity)),
        _ => None,
    }
}

pub(in super::super) fn section_skamp_endpoint(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    if !matches!(item.sense, 2 | 3) {
        return Ok(None);
    }
    section_skamp_locus(ctx, refusal, definition, sketch, item)
}

fn section_skamp_shared_endpoint(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    entity: &crate::feature::definitions::FeatureSkampItem,
    selected: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    if entity.sense != 0 {
        return Ok(None);
    }
    let Some(segment) = unique_decoded_section_segment(definition, entity.entity_id) else {
        return Ok(None);
    };
    if !matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Line(_)
            | crate::feature::definitions::FeatureSegmentKind::Arc(_)
    ) {
        return Ok(None);
    }
    let selected_point = section_skamp_selected_point_id_with_ordinary_segment(
        definition,
        selected,
        unique_decoded_section_segment(definition, selected.entity_id),
    );
    let Some(selected_point) = selected_point else {
        return Ok(None);
    };
    let mut endpoints = segment
        .point_ids()
        .into_iter()
        .enumerate()
        .filter(|(_, point_id)| *point_id == selected_point);
    let Some((endpoint, _)) = endpoints.next() else {
        return Ok(None);
    };
    if endpoints.next().is_some() {
        return Ok(None);
    }
    section_skamp_locus(
        ctx,
        refusal,
        definition,
        sketch,
        &crate::feature::definitions::FeatureSkampItem {
            entity_id: entity.entity_id,
            sense: if endpoint == 0 { 2 } else { 3 },
        },
    )
}

pub(super) fn section_skamp_tangent_loci(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    items: (
        &crate::feature::definitions::FeatureSkampItem,
        &crate::feature::definitions::FeatureSkampItem,
    ),
    active: bool,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Result<Option<[SketchLocus; 2]>, cadmpeg_core::CodecError> {
    let (first, second) = items;

    let selected_locus = |item| -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
        if active {
            section_skamp_endpoint(ctx, refusal, definition, sketch, item)
        } else if matches!(item.sense, 2 | 3) {
            section_skamp_incidence_locus(ctx, refusal, definition, sketch, item, geometry)
        } else {
            Ok(None)
        }
    };
    if let (Some(first), Some(second)) = (selected_locus(first)?, selected_locus(second)?) {
        return Ok(Some([first, second]));
    }
    for (entity, selected) in [(first, second), (second, first)] {
        let Some(shared) =
            section_skamp_shared_endpoint(ctx, refusal, definition, sketch, entity, selected)?
        else {
            continue;
        };
        let Some(selected) = selected_locus(selected)? else {
            continue;
        };
        return Ok(Some(if first.sense == 0 {
            [shared, selected]
        } else {
            [selected, shared]
        }));
    }
    Ok(None)
}

pub(in super::super) fn section_skamp_point_locus(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    if item.sense == 0 && section_skamp_is_point(ctx, definition, item)? {
        return section_skamp_locus(ctx, refusal, definition, sketch, item);
    }
    if matches!(item.sense, 2..=4) {
        return section_skamp_locus(ctx, refusal, definition, sketch, item);
    }
    Ok(None)
}

pub(in super::super) fn section_skamp_incidence_locus(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::definitions::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Result<Option<SketchLocus>, cadmpeg_core::CodecError> {
    if let Some(locus) = section_skamp_point_locus(ctx, refusal, definition, sketch, item)? {
        return Ok(Some(locus));
    }
    let Some(entity) = admitted_entity(ctx, refusal, sketch, item.entity_id) else {
        return Ok(None);
    };
    let Some(geometry) = geometry else {
        return Ok(None);
    };
    let native = ctx
        .get_btree_map(geometry, &entity, "creo SKAMP locus geometry lookup")?
        .is_some_and(|geometry| {
            matches!(
                geometry.definition(),
                SketchGeometryDefinition::Native { .. }
            )
        });
    if !native {
        return Ok(None);
    }
    Ok(match item.sense {
        2 => Some(SketchLocus::Start(entity)),
        3 => Some(SketchLocus::End(entity)),
        _ => None,
    })
}

pub(super) fn section_skamp_line_pair(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    first: &crate::feature::definitions::FeatureSkampItem,
    second: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<[SketchEntityId; 2]>, cadmpeg_core::CodecError> {
    if first.sense != 0
        || second.sense != 0
        || !section_skamp_is_line(ctx, definition, first)?
        || !section_skamp_is_line(ctx, definition, second)?
    {
        return Ok(None);
    }
    let Some(first_entity) = admitted_entity(ctx, refusal, sketch, first.entity_id) else {
        return Ok(None);
    };
    let Some(second_entity) = admitted_entity(ctx, refusal, sketch, second.entity_id) else {
        return Ok(None);
    };
    Ok(Some([first_entity, second_entity]))
}

pub(super) fn section_skamp_oriented_line(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::definitions::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    if item.sense != 0 {
        return Ok(None);
    }
    let Some(entity) = admitted_entity(ctx, refusal, sketch, item.entity_id) else {
        return Ok(None);
    };
    if section_skamp_is_line(ctx, definition, item)? {
        return Ok(Some(entity));
    }
    if section_skamp_is_point(ctx, definition, item)?
        || section_skamp_is_circular(ctx, definition, item)?
        || section_saved_entity(ctx, definition, item.entity_id)?.is_some_and(|entity| {
            matches!(
                entity,
                crate::feature::definitions::FeatureSavedEntity::Spline(_)
            )
        })
    {
        return Ok(None);
    }
    if solver_only_section_entity_offset(ctx, definition, item.entity_id)?.is_some() {
        return Ok(Some(entity));
    }
    let line_role_evidence = matches!(
        visit_section_skamps(ctx, definition, false, |skamp| {
            let endpoint_role = ctx.any_by(
                &skamp.items,
                |candidate| {
                    Ok(candidate.entity_id == item.entity_id && matches!(candidate.sense, 2 | 3))
                },
                "creo type-35 line-role SKAMP items",
            )?;
            let skamp_evidence = endpoint_role
                || match (skamp.kind, skamp.items.as_slice()) {
                    (35, [first, second]) => {
                        (first.entity_id == item.entity_id
                            && first.sense == 0
                            && section_skamp_point_locus(ctx, refusal, definition, sketch, second)?
                                .is_some())
                            || (second.entity_id == item.entity_id
                                && second.sense == 0
                                && section_skamp_point_locus(
                                    ctx, refusal, definition, sketch, first,
                                )?
                                .is_some())
                    }
                    _ => false,
                };
            Ok(if skamp_evidence {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            })
        })?,
        ControlFlow::Break(())
    );
    if !line_role_evidence {
        return Ok(None);
    }
    let Some(geometry) = geometry else {
        return Ok(None);
    };
    let Some(geometry) =
        ctx.get_btree_map(geometry, &entity, "creo SKAMP locus geometry lookup")?
    else {
        return Ok(None);
    };
    if !matches!(
        geometry.definition(),
        SketchGeometryDefinition::Native { .. }
    ) {
        return Ok(None);
    }
    Ok(Some(entity))
}

pub(super) fn section_skamp_same_coordinate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    skamp: &crate::feature::definitions::FeatureSkamp,
    require_satisfied: bool,
    resolved_points: Option<&BTreeMap<u32, [f64; 2]>>,
) -> Result<Option<(SketchLocus, SketchLocus, SketchCoordinateAxis)>, cadmpeg_core::CodecError> {
    let [first, second] = skamp.items.as_slice() else {
        return Ok(None);
    };
    let Some(first_locus) = section_skamp_point_locus(ctx, refusal, definition, sketch, first)?
    else {
        return Ok(None);
    };
    let Some(second_locus) = section_skamp_point_locus(ctx, refusal, definition, sketch, second)?
    else {
        return Ok(None);
    };
    let Some(coordinate) = section_skamp_same_coordinate_axis(skamp) else {
        return Ok(None);
    };
    let axis = [SketchCoordinateAxis::U, SketchCoordinateAxis::V][coordinate.index()];
    if require_satisfied {
        let Some(([first_source, second_source], _)) =
            section_skamp_same_coordinate_sources(ctx, definition, skamp)?
        else {
            return Ok(None);
        };
        let Some(points) = resolved_points else {
            return Ok(None);
        };
        let point = |source| -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
            match source {
                SectionPointSource::Point(point_id) => Ok(ctx
                    .get_btree_map(points, &point_id, "creo SKAMP resolved point lookup")?
                    .copied()),
                SectionPointSource::Value(point) => Ok(Some(point.get())),
            }
        };
        if let (Some(first_point), Some(second_point)) =
            (point(first_source)?, point(second_source)?)
        {
            let scale = first_point
                .iter()
                .chain(&second_point)
                .map(|coordinate| coordinate.abs())
                .fold(1.0, f64::max);
            if !crate::vecmath::within(
                (first_point[coordinate.index()] - second_point[coordinate.index()]).abs(),
                EPS_LOCUS_COORDINATE * scale,
            ) {
                return Ok(None);
            }
        }
    }
    Ok(Some((first_locus, second_locus, axis)))
}

pub(in super::super) fn section_skamp_same_coordinate_sources(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Result<Option<([SectionPointSource; 2], SectionAxis)>, cadmpeg_core::CodecError> {
    if matches!(skamp.kind, 12 | 13) {
        let [item] = skamp.items.as_slice() else {
            return Ok(None);
        };
        if item.sense != 0 || !section_skamp_is_arc(ctx, definition, item)? {
            return Ok(None);
        }
        let endpoint = |sense| {
            section_skamp_incidence_point(
                ctx,
                definition,
                &crate::feature::definitions::FeatureSkampItem {
                    entity_id: item.entity_id,
                    sense,
                },
            )
        };
        let Some(first) = endpoint(2)? else {
            return Ok(None);
        };
        let Some(second) = endpoint(3)? else {
            return Ok(None);
        };
        let axis = section_skamp_same_coordinate_axis(skamp);
        let Some(axis) = axis else {
            return Ok(None);
        };
        return Ok(Some(([first, second], axis)));
    }
    let [first, second] = skamp.items.as_slice() else {
        return Ok(None);
    };
    let Some(coordinate) = section_skamp_same_coordinate_axis(skamp) else {
        return Ok(None);
    };
    let Some(first) = section_skamp_incidence_point(ctx, definition, first)? else {
        return Ok(None);
    };
    let Some(second) = section_skamp_incidence_point(ctx, definition, second)? else {
        return Ok(None);
    };
    Ok(Some(([first, second], coordinate)))
}

pub(super) fn section_skamp_same_coordinate_axis(
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Option<SectionAxis> {
    Some(match (skamp.kind, skamp.flags) {
        (12, _) => SectionAxis::V,
        (13, _) => SectionAxis::U,
        (15 | 17, 1) => SectionAxis::U,
        (15 | 17, 2) => SectionAxis::V,
        (30, _) => SectionAxis::V,
        (31, _) => SectionAxis::U,
        _ => return None,
    })
}

pub(in super::super) fn section_skamp_is_line(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<bool, cadmpeg_core::CodecError> {
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return Ok(matches!(
            segment.kind,
            crate::feature::definitions::FeatureSegmentKind::Line(_)
        ) || section_degenerate_axis_line(ctx, definition, segment)?);
    }
    if solver_only_section_entity_family(ctx, definition, item.entity_id)?
        == Some(SectionEntityIncidenceFamily::Line)
        || unique_section_incidence_curve_family(ctx, definition, item.entity_id)?
            == Some(SectionEntityIncidenceFamily::Line)
    {
        return Ok(true);
    }
    if unique_centered_line_segment(definition, item.entity_id).is_some() {
        return Ok(true);
    }
    if unique_reference_line_segment(definition, item.entity_id).is_some() {
        return Ok(true);
    }
    if !saved_section_entity_fallback_allowed(definition, item.entity_id) {
        return Ok(false);
    }
    Ok(
        section_saved_entity(ctx, definition, item.entity_id)?.is_some_and(|entity| {
            matches!(
                entity,
                crate::feature::definitions::FeatureSavedEntity::Line(_)
            )
        }),
    )
}

pub(in super::super) fn section_degenerate_axis_line(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<bool, cadmpeg_core::CodecError> {
    if !matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Point(_)
    ) {
        return Ok(false);
    }
    let expected_kind = match segment.vertical_horizontal {
        Some(0) => 2,
        Some(1) => 1,
        _ => return Ok(false),
    };
    let mut unary_orientation = false;
    let mut symmetry_axis = false;
    let evidence = visit_section_skamps(ctx, definition, false, |skamp| {
        unary_orientation |= matches!((skamp.kind, skamp.items.as_slice()), (kind, [item])
            if kind == expected_kind && item.entity_id == segment.external_id && item.sense == 0);
        symmetry_axis |= matches!((skamp.kind, skamp.items.as_slice()), (14, [axis, _, _])
            if axis.entity_id == segment.external_id && axis.sense == 0);
        Ok(if unary_orientation && symmetry_axis {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        })
    })?;
    Ok(evidence.is_break())
}

pub(in super::super) fn section_skamp_is_point(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<bool, cadmpeg_core::CodecError> {
    if solver_only_section_entity_family(ctx, definition, item.entity_id)?
        == Some(SectionEntityIncidenceFamily::Point)
        || unique_section_incidence_curve_family(ctx, definition, item.entity_id)?
            == Some(SectionEntityIncidenceFamily::Point)
        || unique_point_segment(definition, item.entity_id).is_some()
    {
        return Ok(true);
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return Ok(matches!(
            segment.kind,
            crate::feature::definitions::FeatureSegmentKind::Point(_)
        ) && !section_degenerate_axis_line(ctx, definition, segment)?);
    }
    Ok(false)
}

pub(super) fn section_skamp_is_arc(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<bool, cadmpeg_core::CodecError> {
    if solver_only_section_entity_family(ctx, definition, item.entity_id)?
        == Some(SectionEntityIncidenceFamily::Arc)
        || unique_section_incidence_curve_family(ctx, definition, item.entity_id)?
            == Some(SectionEntityIncidenceFamily::Arc)
    {
        return Ok(true);
    }
    if !saved_section_entity_fallback_allowed(definition, item.entity_id) {
        return Ok(
            unique_decoded_section_segment(definition, item.entity_id).is_some_and(|segment| {
                matches!(
                    segment.kind,
                    crate::feature::definitions::FeatureSegmentKind::Arc(_)
                )
            }),
        );
    }
    Ok(
        section_saved_entity(ctx, definition, item.entity_id)?.is_some_and(|entity| {
            matches!(
                entity,
                crate::feature::definitions::FeatureSavedEntity::Arc(_)
            )
        }),
    )
}

pub(super) fn section_skamp_curve_entity(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    if item.sense != 0 {
        return Ok(None);
    }
    let is_curve = section_skamp_is_line(ctx, definition, item)?
        || unique_bounded_curve_segment(definition, item.entity_id).is_some()
        || matches!(
            unique_section_incidence_curve_family(ctx, definition, item.entity_id)?,
            Some(
                SectionEntityIncidenceFamily::BoundedCurve
                    | SectionEntityIncidenceFamily::LineOrArc
            )
        )
        || section_skamp_is_circular(ctx, definition, item)?
        || matches!(
            solver_only_section_entity_family(ctx, definition, item.entity_id)?,
            Some(
                SectionEntityIncidenceFamily::BoundedCurve
                    | SectionEntityIncidenceFamily::LineOrArc
                    | SectionEntityIncidenceFamily::Arc
                    | SectionEntityIncidenceFamily::Circular
            )
        )
        || (saved_section_entity_fallback_allowed(definition, item.entity_id)
            && section_saved_entity(ctx, definition, item.entity_id)?.is_some_and(|entity| {
                matches!(
                    entity,
                    crate::feature::definitions::FeatureSavedEntity::Conic(_)
                        | crate::feature::definitions::FeatureSavedEntity::Spline(_)
                )
            }));
    if !is_curve {
        return Ok(None);
    }
    Ok(admitted_entity(ctx, refusal, sketch, item.entity_id))
}

pub(in super::super) fn section_skamp_midpoint(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    first: &crate::feature::definitions::FeatureSkampItem,
    second: &crate::feature::definitions::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Result<Option<(SketchLocus, SketchEntityId)>, cadmpeg_core::CodecError> {
    let target = |item: &crate::feature::definitions::FeatureSkampItem| -> Result<
        Option<SketchEntityId>,
        cadmpeg_core::CodecError,
    > {
        if item.sense == 4 && unique_centered_line_segment(definition, item.entity_id).is_some() {
            return Ok(admitted_entity(ctx, refusal, sketch, item.entity_id));
        }
        if item.sense != 0 {
            return Ok(None);
        }
        if section_skamp_is_arc(ctx, definition, item)? {
            return Ok(admitted_entity(ctx, refusal, sketch, item.entity_id));
        }
        // A type-25 section-reference line and an axis line anchored at one
        // section point are unbounded, so neither is a midpoint target.
        if unique_reference_line_segment(definition, item.entity_id).is_some()
            || if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
                section_degenerate_axis_line(ctx, definition, segment)?
            } else {
                false
            }
        {
            return Ok(None);
        }
        let Some(line) = section_skamp_oriented_line(ctx, refusal, definition, sketch, item, geometry)?
        else {
            return Ok(None);
        };
        Ok(section_skamp_line_without_type35_target(ctx, definition, item)?.then_some(line))
    };
    let point = |item: &crate::feature::definitions::FeatureSkampItem| -> Result<
        Option<SketchLocus>,
        cadmpeg_core::CodecError,
    > {
        if let Some(locus) = section_skamp_point_locus(ctx, refusal, definition, sketch, item)? {
            return Ok(Some(locus));
        }
        if item.sense == 0 && section_skamp_is_circular(ctx, definition, item)? {
            return Ok(admitted_entity(ctx, refusal, sketch, item.entity_id).map(SketchLocus::Center));
        }
        Ok(None)
    };
    let centered_target = |item: &crate::feature::definitions::FeatureSkampItem| {
        item.sense == 4 && unique_centered_line_segment(definition, item.entity_id).is_some()
    };
    if centered_target(first) || centered_target(second) {
        let mut candidate = None;
        for (target_item, point_item) in [(first, second), (second, first)] {
            if !centered_target(target_item) || point_item.sense != 0 {
                continue;
            }
            let Some(point) = point(point_item)? else {
                continue;
            };
            let Some(target) = target(target_item)? else {
                continue;
            };
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some((point, target));
        }
        return Ok(candidate);
    }
    let first_target = target(first)?;
    let second_point = point(second)?;
    let first_candidate = second_point.zip(first_target);
    let second_target = target(second)?;
    let first_point = point(first)?;
    let second_candidate = first_point.zip(second_target);
    match (first_candidate, second_candidate) {
        (Some(candidate), None) | (None, Some(candidate)) => Ok(Some(candidate)),
        _ => Ok(None),
    }
}

/// Whether the entity is a line by evidence other than a sense-zero type-35
/// target role: its decoded or centered line row, its saved line, or a unique
/// line family from its other incidence roles. An endpoint role alone
/// establishes a bounded curve, not a line.
fn section_skamp_line_without_type35_target(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(
        unique_decoded_section_segment(definition, item.entity_id).is_some_and(|segment| {
            matches!(
                segment.kind,
                crate::feature::definitions::FeatureSegmentKind::Line(_)
            )
        }) || unique_centered_line_segment(definition, item.entity_id).is_some()
            || unique_section_incidence_curve_family_without_type35_target(
                ctx,
                definition,
                item.entity_id,
            )? == Some(SectionEntityIncidenceFamily::Line)
            || (saved_section_entity_fallback_allowed(definition, item.entity_id)
                && section_saved_entity(ctx, definition, item.entity_id)?.is_some_and(|entity| {
                    matches!(
                        entity,
                        crate::feature::definitions::FeatureSavedEntity::Line(_)
                    )
                })),
    )
}

pub(in super::super) fn section_saved_entity<'definition>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'definition crate::feature::definitions::FeatureDefinition,
    external_id: u32,
) -> Result<
    Option<&'definition crate::feature::definitions::FeatureSavedEntity>,
    cadmpeg_core::CodecError,
> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(table) = definition.order_table.as_ref() else {
        return Ok(None);
    };
    let Some(internal_id) = table.internal_id(external_id) else {
        return Ok(None);
    };
    saved_section_entity_by_internal_id(ctx, definition, internal_id)
}

pub(super) fn section_skamp_circular_entity(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    if item.sense != 0 {
        return Ok(None);
    }
    if !section_skamp_is_circular(ctx, definition, item)? {
        return Ok(None);
    }
    Ok(admitted_entity(ctx, refusal, sketch, item.entity_id))
}

pub(super) fn section_skamp_center_entity(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<Option<SketchEntityId>, cadmpeg_core::CodecError> {
    if item.sense != 4 || !section_skamp_is_circular(ctx, definition, item)? {
        return Ok(None);
    }
    Ok(admitted_entity(ctx, refusal, sketch, item.entity_id))
}

pub(in super::super) fn section_skamp_is_circular(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<bool, cadmpeg_core::CodecError> {
    if solver_only_section_entity_offset(ctx, definition, item.entity_id)?.is_some() {
        return Ok(
            solver_only_section_entity_family(ctx, definition, item.entity_id)?.is_some_and(
                |family| {
                    matches!(
                        family,
                        SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular
                    )
                },
            ),
        );
    }
    if matches!(
        unique_section_incidence_curve_family(ctx, definition, item.entity_id)?,
        Some(SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular)
    ) {
        return Ok(true);
    }
    if unique_circle_segment(definition, item.entity_id).is_some() {
        return Ok(true);
    }
    if saved_section_entity_fallback_allowed(definition, item.entity_id) {
        Ok(
            section_saved_entity(ctx, definition, item.entity_id)?.is_some_and(|entity| {
                matches!(
                    entity,
                    crate::feature::definitions::FeatureSavedEntity::Arc(_)
                        | crate::feature::definitions::FeatureSavedEntity::Circle(_)
                )
            }),
        )
    } else {
        Ok(
            unique_decoded_section_segment(definition, item.entity_id).is_some_and(|segment| {
                matches!(
                    segment.kind,
                    crate::feature::definitions::FeatureSegmentKind::Arc(_)
                )
            }),
        )
    }
}

pub(in super::super) fn section_skamp_line_midpoint_sources(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
) -> Result<Option<([SectionPointSource; 2], SectionPointSource)>, cadmpeg_core::CodecError> {
    let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
        return Ok(None);
    };
    let target = |item: &crate::feature::definitions::FeatureSkampItem| -> Result<
        Option<[SectionPointSource; 2]>,
        cadmpeg_core::CodecError,
    > {
        if item.sense == 4 {
            return Ok(unique_centered_line_segment(definition, item.entity_id).map(|line| {
                [
                    SectionPointSource::Point(line.center_id),
                    SectionPointSource::Point(line.center_id),
                ]
            }));
        }
        if item.sense != 0 {
            return Ok(None);
        }
        if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
            return Ok(matches!(
                segment.kind,
                crate::feature::definitions::FeatureSegmentKind::Line(_)
            )
            .then_some(segment.point_ids().map(SectionPointSource::Point)));
        }
        if !saved_section_entity_fallback_allowed(definition, item.entity_id) {
            return Ok(None);
        }
        let Some(crate::feature::definitions::FeatureSavedEntity::Line(line)) =
            section_saved_entity(ctx, definition, item.entity_id)?
        else {
            return Ok(None);
        };
        let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] =
            line.endpoints
        else {
            return Ok(None);
        };
        let scale = [first_u, first_v, second_u, second_v]
            .into_iter()
            .map(f64::abs)
            .fold(1.0, f64::max);
        let Some(midpoint) = FiniteVector::new([
            f64::midpoint(first_u, second_u),
            f64::midpoint(first_v, second_v),
        ]) else {
            return Ok(None);
        };
        Ok(((second_u - first_u).hypot(second_v - first_v) > EPS_NONDEGENERATE_LINE * scale)
            .then_some([
                SectionPointSource::Value(midpoint),
                SectionPointSource::Value(midpoint),
            ]))
    };
    let mut candidate = None;
    for (target_item, point_item) in [(first, second), (second, first)] {
        let Some(target) = target(target_item)? else {
            continue;
        };
        let Some(point) = section_skamp_incidence_point(ctx, definition, point_item)? else {
            continue;
        };
        if candidate.is_some() {
            return Ok(None);
        }
        candidate = Some((target, point));
    }
    Ok(candidate)
}

pub(in super::super) fn section_skamp_arc_midpoint_source(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    skamp: &crate::feature::definitions::FeatureSkamp,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Result<Option<(SectionPointSource, FiniteVector<2>)>, cadmpeg_core::CodecError> {
    let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
        return Ok(None);
    };
    let mut candidate = None;
    for (target, point) in [(first, second), (second, first)] {
        if target.sense != 0 || !section_skamp_is_arc(ctx, definition, target)? {
            continue;
        }
        let Some(point) = section_skamp_incidence_point(ctx, definition, point)? else {
            continue;
        };
        let Some(midpoint) = section_skamp_arc_midpoint(ctx, definition, target, coordinates)?
        else {
            continue;
        };
        if candidate.is_some() {
            return Ok(None);
        }
        candidate = Some((point, midpoint));
    }
    Ok(candidate)
}

fn section_skamp_arc_midpoint(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Result<Option<FiniteVector<2>>, cadmpeg_core::CodecError> {
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        if !matches!(
            segment.kind,
            crate::feature::definitions::FeatureSegmentKind::Arc(_)
        ) || segment.arc_orientation != Some(0)
        {
            return Ok(None);
        }
        let Some(center_id) = segment.center_id else {
            return Ok(None);
        };
        let Some(center) = complete_section_coordinate(ctx, coordinates, center_id)? else {
            return Ok(None);
        };
        let Some(first) = complete_section_coordinate(ctx, coordinates, segment.point_ids()[0])?
        else {
            return Ok(None);
        };
        let Some(second) = complete_section_coordinate(ctx, coordinates, segment.point_ids()[1])?
        else {
            return Ok(None);
        };
        return Ok(oriented_arc_midpoint(center, first, second, None));
    }
    if !saved_section_entity_fallback_allowed(definition, item.entity_id) {
        return Ok(None);
    }
    let Some(crate::feature::definitions::FeatureSavedEntity::Arc(arc)) =
        section_saved_entity(ctx, definition, item.entity_id)?
    else {
        return Ok(None);
    };
    Ok(saved_arc_midpoint(arc))
}

fn complete_section_coordinate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    point_id: u32,
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    Ok(
        match ctx.get_btree_map(coordinates, &point_id, "creo section coordinate lookup")? {
            Some([Some(u), Some(v)]) => Some([*u, *v]),
            _ => None,
        },
    )
}

fn saved_arc_midpoint(
    arc: &crate::feature::definitions::FeatureSavedArc,
) -> Option<FiniteVector<2>> {
    let [Some(center_u), Some(center_v), _] = arc.center else {
        return None;
    };
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return None;
    };
    oriented_arc_midpoint(
        [center_u, center_v],
        [first_u, first_v],
        [second_u, second_v],
        arc.radius,
    )
}

fn oriented_arc_midpoint(
    center: [f64; 2],
    first: [f64; 2],
    second: [f64; 2],
    stored_radius: Option<f64>,
) -> Option<FiniteVector<2>> {
    let first_offset = [first[0] - center[0], first[1] - center[1]];
    let second_offset = [second[0] - center[0], second[1] - center[1]];
    let first_radius = first_offset[0].hypot(first_offset[1]);
    let second_radius = second_offset[0].hypot(second_offset[1]);
    let radius = stored_radius.unwrap_or(first_radius);
    let scale = radius.max(first_radius).max(second_radius).max(1.0);
    if !center
        .into_iter()
        .chain(first)
        .chain(second)
        .chain([first_radius, second_radius, radius])
        .all(f64::is_finite)
        || radius <= EPS_LOCUS_RADIUS_NONZERO
        || (first_radius - second_radius).abs() > EPS_LOCUS_RADIUS_AGREEMENT * scale
        || (radius - first_radius).abs() > EPS_LOCUS_RADIUS_AGREEMENT * scale
    {
        return None;
    }
    let start = second_offset[1].atan2(second_offset[0]);
    let mut end = first_offset[1].atan2(first_offset[0]);
    // `atan2` lies in [-pi, pi], so at most two turns bring the end past the start.
    while end <= start {
        end += std::f64::consts::TAU;
    }
    let angle = f64::midpoint(start, end);
    FiniteVector::new([
        center[0] + radius * angle.cos(),
        center[1] + radius * angle.sin(),
    ])
}

pub(in super::super) fn section_skamp_active(status: u32) -> bool {
    status & 1 != 0
}

pub(in super::super) fn visit_section_skamps<B>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    active_only: bool,
    mut visit: impl FnMut(
        &crate::feature::definitions::FeatureSkamp,
    ) -> Result<ControlFlow<B>, cadmpeg_core::CodecError>,
) -> Result<ControlFlow<B>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(relation) = definition.relations.as_ref() else {
        return Ok(ControlFlow::Continue(()));
    };
    if !feature_skamp_table_complete(relation) {
        return Ok(ControlFlow::Continue(()));
    }
    let mut skamps = relation.skamps().iter();
    while skamps.len() != 0 {
        let Some(skamp) = ctx.next_charged(&mut skamps, "creo relation skamp rows")? else {
            break;
        };
        if active_only && !section_skamp_active(skamp.status) {
            continue;
        }
        if let ControlFlow::Break(value) = visit(skamp)? {
            return Ok(ControlFlow::Break(value));
        }
    }
    Ok(ControlFlow::Continue(()))
}

pub(in super::super) fn visit_all_section_skamps<B>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    mut visit: impl FnMut(
        &crate::feature::definitions::FeatureSkamp,
    ) -> Result<ControlFlow<B>, cadmpeg_core::CodecError>,
) -> Result<ControlFlow<B>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(relation) = definition.relations.as_ref() else {
        return Ok(ControlFlow::Continue(()));
    };
    let mut skamps = relation.skamps().iter();
    while skamps.len() != 0 {
        let Some(skamp) = ctx.next_charged(&mut skamps, "creo relation skamp rows")? else {
            break;
        };
        if let ControlFlow::Break(value) = visit(skamp)? {
            return Ok(ControlFlow::Break(value));
        }
    }
    Ok(ControlFlow::Continue(()))
}

#[cfg(test)]
pub(in super::super) fn with_test_locus<T>(
    run: impl FnOnce(
        &cadmpeg_core::decode::DecodeContext<'_>,
        &std::cell::Cell<Option<cadmpeg_core::CodecError>>,
    ) -> T,
) -> T {
    crate::decode::with_test_decode_ctx(|ctx| {
        let refusal = std::cell::Cell::new(None);
        let result = run(ctx, &refusal);
        assert!(
            refusal.into_inner().is_none(),
            "test locus resource refusal"
        );
        result
    })
}

#[cfg(test)]
mod tests {
    mod admission_visits;
    mod saved_entity;

    use super::{
        oriented_arc_midpoint, section_point_locus, section_skamp_arc_midpoint_source,
        section_skamp_curve_entity, section_skamp_is_arc, section_skamp_is_line,
        section_skamp_is_point, section_skamp_line_midpoint_sources, section_skamp_locus,
        section_skamp_point_locus, section_skamp_same_coordinate_sources,
        section_skamp_tangent_loci,
    };
    use crate::decode::sketch::skamp::SectionPointSource;
    use crate::decode::sketch_transfer::profiles::{
        unique_section_incidence_curve_family, SectionEntityIncidenceFamily,
    };
    use cadmpeg_ir::sketches::SketchEntityId;
    use cadmpeg_ir::sketches::{SketchId, SketchLocus};
    use std::collections::BTreeMap;

    const EPS_ARC_ANGLE: f64 = 1.0e-12;

    #[test]
    fn relation_skamp_scan_refuses_before_callback() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: 0,
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
                        kind: 0,
                        flags: 0,
                        status: 0,
                        items: Vec::new(),
                        offset: 0,
                    }],
                }),
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut called = false;
        let error = super::visit_section_skamps::<()>(&ctx, &definition, false, |_| {
            called = true;
            Ok(std::ops::ControlFlow::Continue(()))
        })
        .expect_err("the SKAMP scan exceeds zero work units");
        assert!(!called);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo relation skamp rows")
        );
    }

    #[test]
    fn unresolved_skamp_locus_refuses_below_entity_identity_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
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
        let sketch = SketchId::mint("creo:model:sketch#917").expect("sketch identity");
        let item = crate::feature::definitions::FeatureSkampItem {
            entity_id: 7,
            sense: 0,
        };
        let need = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                trial_policy.limits.max_retained_bytes = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                let refusal = std::cell::Cell::new(None);
                let result = section_skamp_locus(&ctx, &refusal, &definition, &sketch, &item)?;
                match refusal.into_inner() {
                    Some(error) => Err(error),
                    None => Ok(result),
                }
            },
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = need - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let refusal = std::cell::Cell::new(None);
        assert!(
            section_skamp_locus(&ctx, &refusal, &definition, &sketch, &item)
                .expect("test locus evaluation succeeds")
                .is_none()
        );
        assert!(
            matches!(refusal.into_inner(), Some(cadmpeg_core::CodecError::ResourceLimit(resource))
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo sketch entity identity")
        );
        policy.limits.max_retained_bytes = need;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let refusal = std::cell::Cell::new(None);
        assert!(
            section_skamp_locus(&ctx, &refusal, &definition, &sketch, &item)
                .expect("test locus evaluation succeeds")
                .is_none()
        );
        assert!(refusal.into_inner().is_none());
    }

    #[test]
    fn arc_midpoint_overflow_is_not_a_coordinate_source() {
        assert!(oriented_arc_midpoint(
            [f64::MAX, 0.0],
            [f64::MAX, f64::MAX],
            [f64::MAX, -f64::MAX],
            None,
        )
        .is_none());
    }

    #[test]
    fn arc_midpoint_normalizes_the_end_angle_past_the_start() {
        let Some(midpoint) = oriented_arc_midpoint([0.0, 0.0], [-1.0, -0.0], [-1.0, 0.0], None)
        else {
            panic!("arc midpoint across the branch cut");
        };
        assert!((midpoint[0] - 1.0).abs() < EPS_ARC_ANGLE);
        assert!(midpoint[1].abs() < EPS_ARC_ANGLE);

        let no_normalization = oriented_arc_midpoint([0.0, 0.0], [0.0, 1.0], [1.0, 0.0], None)
            .expect("valid zero-iteration midpoint");
        assert!((no_normalization[0] - std::f64::consts::FRAC_1_SQRT_2).abs() < EPS_ARC_ANGLE);
        assert!((no_normalization[1] - std::f64::consts::FRAC_1_SQRT_2).abs() < EPS_ARC_ANGLE);
        assert!(
            oriented_arc_midpoint([0.0, 0.0], [f64::INFINITY, 0.0], [0.0, 1.0], None).is_none()
        );
        let absent = crate::feature::definitions::FeatureSavedArc {
            entity_id: 1,
            center: [None; 3],
            radius: None,
            endpoints: [[None; 3]; 2],
            parameters: [None; 2],
            body: Vec::new(),
            offset: 0,
        };
        assert!(super::saved_arc_midpoint(&absent).is_none());
    }

    #[test]
    fn standalone_point_rows_supply_point_loci() {
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 1,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![crate::feature::definitions::FeaturePointSegment {
                    point_id: 7,
                    external_id: 12,
                    offset: 20,
                }])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Point)
                .collect(),
                offset: 10,
            }),
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
            crate::decode::with_test_decode_ctx(|ctx| section_point_locus(
                ctx,
                &definition,
                &SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture"),
                7,
            ))
            .expect("admitted point locus"),
            Some(SketchLocus::Entity(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string(),)
                    .expect("valid test fixture")
            ))
        );
    }

    #[test]
    fn point_locus_identity_refuses_below_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 1,
                has_elided_prototype: false,
                entity_ref: None,
                rows: [crate::feature::segment_rows::SegmentRow::Point(
                    crate::feature::definitions::FeaturePointSegment {
                        point_id: 7,
                        external_id: 12,
                        offset: 20,
                    },
                )]
                .into_iter()
                .collect(),
                offset: 10,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        let sketch = SketchId::mint("creo:model:sketch#917").expect("valid test fixture");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let need = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                trial_policy.limits.max_retained_bytes = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                section_point_locus(&ctx, &definition, &sketch, 7)
            },
        );
        policy.limits.max_retained_bytes = need - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = section_point_locus(&ctx, &definition, &sketch, 7)
            .expect_err("entity ID exceeds retained limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo sketch entity identity")
        );
        policy.limits.max_retained_bytes = need;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert_eq!(
            section_point_locus(&ctx, &definition, &sketch, 7)
                .expect("exact retained limit admits entity ID"),
            Some(SketchLocus::Entity(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12")
                    .expect("valid entity ID")
            ))
        );
    }

    #[test]
    fn point_locus_admits_source_rows() {
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 3,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![crate::feature::definitions::FeatureCenteredLineSegment {
                    center_id: 20,
                    external_id: 30,
                    offset: 30,
                }])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::CenteredLine)
                .chain(
                    (vec![crate::feature::definitions::FeatureReferenceLineSegment {
                        directions: [None; 3],
                        point_ids: [Some(7), Some(8)],
                        vertical_horizontal: None,
                        external_id: 31,
                        offset: 10,
                    }])
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::ReferenceLine),
                )
                .chain(
                    (vec![crate::feature::definitions::FeatureBoundedCurveSegment {
                        directions: [None; 3],
                        point_ids: [9, 10],
                        center_id: None,
                        arc_orientation: None,
                        vertical_horizontal: None,
                        radius_ref: None,
                        radius2_ref: None,
                        external_id: 32,
                        offset: 20,
                    }])
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::BoundedCurve),
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
        };
        let sketch =
            SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture");
        let result =
            crate::test_support::assert_work_boundaries(&["creo point locus rows"], |ctx| {
                section_point_locus(ctx, &definition, &sketch, 0)
            });
        assert_eq!(
            result,
            Some(SketchLocus::Start(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:30".to_string())
                    .expect("valid test fixture")
            ))
        );
    }

    #[test]
    fn endpoint_carriers_supply_ordered_point_loci() {
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 3,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![crate::feature::definitions::FeatureCenteredLineSegment {
                    center_id: 20,
                    external_id: 30,
                    offset: 30,
                }])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::CenteredLine)
                .chain(
                    (vec![crate::feature::definitions::FeatureReferenceLineSegment {
                        directions: [None; 3],
                        point_ids: [Some(7), Some(8)],
                        vertical_horizontal: None,
                        external_id: 31,
                        offset: 10,
                    }])
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::ReferenceLine),
                )
                .chain(
                    (vec![crate::feature::definitions::FeatureBoundedCurveSegment {
                        directions: [None; 3],
                        point_ids: [9, 10],
                        center_id: None,
                        arc_orientation: None,
                        vertical_horizontal: None,
                        radius_ref: None,
                        radius2_ref: None,
                        external_id: 32,
                        offset: 20,
                    }])
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::BoundedCurve),
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
        };
        let sketch =
            SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture");
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| section_point_locus(
                ctx,
                &definition,
                &sketch,
                0
            ))
            .expect("admitted point locus"),
            Some(SketchLocus::Start(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:30".to_string(),)
                    .expect("valid test fixture")
            ))
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| section_point_locus(
                ctx,
                &definition,
                &sketch,
                1
            ))
            .expect("admitted point locus"),
            Some(SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:30".to_string(),)
                    .expect("valid test fixture")
            ))
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| section_point_locus(
                ctx,
                &definition,
                &sketch,
                7
            ))
            .expect("admitted point locus"),
            Some(SketchLocus::Start(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:31".to_string(),)
                    .expect("valid test fixture")
            ))
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| section_point_locus(
                ctx,
                &definition,
                &sketch,
                8
            ))
            .expect("admitted point locus"),
            Some(SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:31".to_string(),)
                    .expect("valid test fixture")
            ))
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| section_point_locus(
                ctx,
                &definition,
                &sketch,
                9
            ))
            .expect("admitted point locus"),
            Some(SketchLocus::Start(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:32".to_string(),)
                    .expect("valid test fixture")
            ))
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| section_point_locus(
                ctx,
                &definition,
                &sketch,
                10
            ))
            .expect("admitted point locus"),
            Some(SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:32".to_string(),)
                    .expect("valid test fixture")
            ))
        );

        let mut ambiguous = definition;
        ambiguous.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::ReferenceLine(
                crate::feature::definitions::FeatureReferenceLineSegment {
                    directions: [None; 3],
                    point_ids: [Some(7), Some(8)],
                    vertical_horizontal: None,
                    external_id: 31,
                    offset: 40,
                },
            ),
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| section_point_locus(
                ctx, &ambiguous, &sketch, 7
            ))
            .expect("admitted point locus"),
            None
        );

        ambiguous.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Point(
                crate::feature::definitions::FeaturePointSegment {
                    point_id: 0,
                    external_id: 33,
                    offset: 50,
                },
            ),
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| section_point_locus(
                ctx, &ambiguous, &sketch, 0
            ))
            .expect("admitted point locus"),
            None
        );
    }

    #[test]
    fn circular_segment_centers_supply_center_loci() {
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 1,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![crate::feature::definitions::FeatureSegment {
                    kind: crate::feature::definitions::FeatureSegmentKind::Arc([1, 2]),
                    directions: [None; 3],
                    center_id: Some(3),
                    arc_orientation: Some(0),
                    vertical_horizontal: None,
                    radius_ref: Some(4),
                    radius2_ref: None,
                    external_id: 30,
                    body: Vec::new(),
                    offset: 10,
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
        };
        let sketch =
            SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture");
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| section_point_locus(
                ctx,
                &definition,
                &sketch,
                3
            ))
            .expect("admitted point locus"),
            Some(SketchLocus::Center(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:30".to_string(),)
                    .expect("valid test fixture")
            ))
        );
    }

    #[test]
    fn inferred_declared_families_gate_curve_predicates() {
        let opaque = |external_id| crate::feature::definitions::FeatureOpaqueSegment {
            kind: 25,
            directions: [None; 3],
            point_ids: [None; 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: usize::try_from(external_id).expect("fixture index fits usize"),
        };
        let skamp = |id, kind, items| crate::feature::definitions::FeatureSkamp {
            id,
            kind,
            flags: 0,
            status: 0,
            items,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 3,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![opaque(101), opaque(102), opaque(103)])
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::Opaque)
                    .collect(),
                offset: 0,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
                    header: crate::feature::definitions::FeatureSolverTableHeader {
                        declared_count: 4,
                        entity_ref: 1,
                        offset: 0,
                    },
                    rows: vec![
                        skamp(
                            1,
                            5,
                            vec![
                                crate::feature::definitions::FeatureSkampItem {
                                    entity_id: 101,
                                    sense: 0,
                                },
                                crate::feature::definitions::FeatureSkampItem {
                                    entity_id: 201,
                                    sense: 0,
                                },
                            ],
                        ),
                        skamp(
                            2,
                            0,
                            vec![
                                crate::feature::definitions::FeatureSkampItem {
                                    entity_id: 102,
                                    sense: 2,
                                },
                                crate::feature::definitions::FeatureSkampItem {
                                    entity_id: 202,
                                    sense: 0,
                                },
                            ],
                        ),
                        skamp(
                            3,
                            0,
                            vec![
                                crate::feature::definitions::FeatureSkampItem {
                                    entity_id: 102,
                                    sense: 4,
                                },
                                crate::feature::definitions::FeatureSkampItem {
                                    entity_id: 203,
                                    sense: 0,
                                },
                            ],
                        ),
                        skamp(
                            4,
                            0,
                            vec![
                                crate::feature::definitions::FeatureSkampItem {
                                    entity_id: 103,
                                    sense: 2,
                                },
                                crate::feature::definitions::FeatureSkampItem {
                                    entity_id: 204,
                                    sense: 0,
                                },
                            ],
                        ),
                    ],
                }),
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        };
        let sketch =
            SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture");
        let line = crate::feature::definitions::FeatureSkampItem {
            entity_id: 101,
            sense: 0,
        };
        let arc = crate::feature::definitions::FeatureSkampItem {
            entity_id: 102,
            sense: 0,
        };
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| unique_section_incidence_curve_family(
                ctx,
                &definition,
                101
            ))
            .expect("admitted unique section incidence curve family rows"),
            None
        );
        assert!(
            !crate::decode::with_test_decode_ctx(|ctx| section_skamp_is_line(
                ctx,
                &definition,
                &line
            ))
            .expect("admitted is line rows")
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| unique_section_incidence_curve_family(
                ctx,
                &definition,
                102
            ))
            .expect("admitted unique section incidence curve family rows"),
            Some(SectionEntityIncidenceFamily::Arc)
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_skamp_is_arc(ctx, &definition, &arc))
                .expect("admitted is arc rows")
        );
        let bounded_curve = crate::feature::definitions::FeatureSkampItem {
            entity_id: 103,
            sense: 0,
        };
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| unique_section_incidence_curve_family(
                ctx,
                &definition,
                103
            ))
            .expect("admitted unique section incidence curve family rows"),
            Some(SectionEntityIncidenceFamily::BoundedCurve)
        );
        assert_eq!(
            super::with_test_locus(|ctx, refusal| section_skamp_curve_entity(
                ctx,
                refusal,
                &definition,
                &sketch,
                &bounded_curve
            ))
            .expect("test curve entity resources"),
            Some(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:103".to_string(),)
                    .expect("valid test fixture")
            )
        );
        assert_eq!(
            super::with_test_locus(|ctx, refusal| section_skamp_locus(
                ctx,
                refusal,
                &definition,
                &sketch,
                &crate::feature::definitions::FeatureSkampItem {
                    entity_id: 102,
                    sense: 2,
                },
            ))
            .expect("test section locus resources"),
            Some(SketchLocus::Start(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:102".to_string(),)
                    .expect("valid test fixture")
            ))
        );

        let mut decoded_arc = definition.clone();
        decoded_arc
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
                crate::feature::definitions::FeatureSegment {
                    kind: crate::feature::definitions::FeatureSegmentKind::Arc([1, 2]),
                    directions: [None; 3],
                    center_id: Some(3),
                    arc_orientation: Some(0),
                    vertical_horizontal: None,
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 104,
                    body: Vec::new(),
                    offset: 104,
                },
            ));
        let relations = decoded_arc.relations.as_mut().expect("relations");
        crate::decode::tests::declared_solver_rows(&mut relations.skamps).push(
            crate::feature::definitions::FeatureSkamp {
                id: 5,
                kind: 5,
                flags: 0,
                status: 1,
                items: vec![
                    crate::feature::definitions::FeatureSkampItem {
                        entity_id: 104,
                        sense: 0,
                    },
                    crate::feature::definitions::FeatureSkampItem {
                        entity_id: 205,
                        sense: 0,
                    },
                ],
                offset: 105,
            },
        );
        relations
            .skamps
            .as_mut()
            .expect("skamp table")
            .header_mut()
            .expect("skamp header")
            .declared_count += 1;
        let decoded_arc_item = crate::feature::definitions::FeatureSkampItem {
            entity_id: 104,
            sense: 0,
        };
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| unique_section_incidence_curve_family(
                ctx,
                &decoded_arc,
                104
            ))
            .expect("admitted unique section incidence curve family rows"),
            None
        );
        assert!(
            !crate::decode::with_test_decode_ctx(|ctx| section_skamp_is_line(
                ctx,
                &decoded_arc,
                &decoded_arc_item
            ))
            .expect("admitted is line rows")
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_skamp_is_arc(
                ctx,
                &decoded_arc,
                &decoded_arc_item
            ))
            .expect("admitted is arc rows")
        );
    }

    #[test]
    fn inferred_declared_point_family_gates_point_predicates() {
        let opaque = crate::feature::definitions::FeatureOpaqueSegment {
            kind: 25,
            directions: [None; 3],
            point_ids: [None; 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 99,
            body: Vec::new(),
            offset: 99,
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
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
                rows: (vec![crate::feature::definitions::FeatureSegment {
                    kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
                    directions: [None; 3],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: None,
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 12,
                    body: Vec::new(),
                    offset: 12,
                }])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .chain(
                    (vec![opaque])
                        .into_iter()
                        .map(crate::feature::segment_rows::SegmentRow::Opaque),
                )
                .collect(),
                offset: 0,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: 0,
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
                        kind: 0,
                        flags: 0,
                        status: 0,
                        items: vec![
                            crate::feature::definitions::FeatureSkampItem {
                                entity_id: 99,
                                sense: 0,
                            },
                            crate::feature::definitions::FeatureSkampItem {
                                entity_id: 12,
                                sense: 2,
                            },
                        ],
                        offset: 1,
                    }],
                }),
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        };
        let item = crate::feature::definitions::FeatureSkampItem {
            entity_id: 99,
            sense: 0,
        };
        let sketch =
            SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture");
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_skamp_is_point(
                ctx,
                &definition,
                &item
            ))
            .expect("admitted is point rows")
        );
        assert_eq!(
            super::with_test_locus(|ctx, refusal| section_skamp_point_locus(
                ctx,
                refusal,
                &definition,
                &sketch,
                &item
            ))
            .expect("test point locus resources"),
            Some(SketchLocus::Entity(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:99".to_string(),)
                    .expect("valid test fixture")
            ))
        );
    }

    #[test]
    fn incomplete_unique_rows_supply_shared_endpoint_tangent_loci() {
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
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 3,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![line(10, [1, 2]), line(11, [1, 3])])
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
        };
        assert!(!definition
            .segments
            .as_ref()
            .expect("segments")
            .is_complete());
        let sketch =
            SketchId::mint("creo:model:sketch#917".to_string()).expect("valid test fixture");
        assert_eq!(
            super::with_test_locus(|ctx, refusal| section_skamp_tangent_loci(
                ctx,
                refusal,
                &definition,
                &sketch,
                (
                    &crate::feature::definitions::FeatureSkampItem {
                        entity_id: 10,
                        sense: 0,
                    },
                    &crate::feature::definitions::FeatureSkampItem {
                        entity_id: 11,
                        sense: 2,
                    }
                ),
                true,
                None,
            ))
            .expect("test tangent locus resources"),
            Some([
                SketchLocus::Start(
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:10".to_string(),)
                        .expect("valid test fixture")
                ),
                SketchLocus::Start(
                    SketchEntityId::mint("creo:featdefs:sketch_entity#917:11".to_string(),)
                        .expect("valid test fixture")
                ),
            ])
        );
    }

    #[test]
    fn incomplete_unique_rows_supply_solver_incidence_point_sources() {
        let segment = |kind: crate::feature::definitions::FeatureSegmentKind,
                       external_id: u32,
                       center_id: Option<u32>,
                       arc_orientation: Option<u32>| {
            crate::feature::definitions::FeatureSegment {
                kind,
                directions: [None; 3],
                center_id,
                arc_orientation,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id,
                body: Vec::new(),
                offset: usize::try_from(external_id).expect("fixture index fits usize"),
            }
        };
        let item =
            |entity_id, sense| crate::feature::definitions::FeatureSkampItem { entity_id, sense };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 4,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (vec![
                    segment(
                        crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
                        10,
                        None,
                        None,
                    ),
                    segment(
                        crate::feature::definitions::FeatureSegmentKind::Line([3, 4]),
                        20,
                        None,
                        None,
                    ),
                    segment(
                        crate::feature::definitions::FeatureSegmentKind::Arc([5, 6]),
                        30,
                        Some(7),
                        Some(0),
                    ),
                ])
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
        };
        assert!(!definition
            .segments
            .as_ref()
            .expect("segments")
            .is_complete());

        let same_coordinate = crate::feature::definitions::FeatureSkamp {
            id: 15,
            kind: 15,
            flags: 1,
            status: 1,
            items: vec![item(10, 2), item(20, 3)],
            offset: 0,
        };
        let sketch = SketchId::mint("creo:model:sketch#917").expect("sketch identity");
        let Some(([first, second], coordinate)) = crate::decode::with_test_decode_ctx(|ctx| {
            section_skamp_same_coordinate_sources(ctx, &definition, &same_coordinate)
        })
        .expect("admitted same coordinate sources rows") else {
            panic!("same-coordinate sources");
        };
        assert!(matches!(first, SectionPointSource::Point(1)));
        assert!(matches!(second, SectionPointSource::Point(4)));
        assert_eq!(coordinate, crate::decode::sketch::axis::SectionAxis::U);

        let resolved_points = BTreeMap::from([(1, [f64::NAN, 0.0]), (4, [0.0, 0.0])]);
        assert_eq!(
            super::with_test_locus(|ctx, refusal| {
                super::section_skamp_same_coordinate(
                    ctx,
                    refusal,
                    &definition,
                    &sketch,
                    &same_coordinate,
                    true,
                    Some(&resolved_points),
                )
            })
            .expect("admitted NaN coordinate comparison"),
            None
        );

        let arc_alignment = crate::feature::definitions::FeatureSkamp {
            id: 12,
            kind: 12,
            flags: 0,
            status: 1,
            items: vec![item(30, 0)],
            offset: 0,
        };
        let Some(([first, second], coordinate)) = crate::decode::with_test_decode_ctx(|ctx| {
            section_skamp_same_coordinate_sources(ctx, &definition, &arc_alignment)
        })
        .expect("admitted same coordinate sources rows") else {
            panic!("arc alignment sources");
        };
        assert!(matches!(first, SectionPointSource::Point(5)));
        assert!(matches!(second, SectionPointSource::Point(6)));
        assert_eq!(coordinate, crate::decode::sketch::axis::SectionAxis::V);

        let line_midpoint = crate::feature::definitions::FeatureSkamp {
            id: 35,
            kind: 35,
            flags: 0,
            status: 1,
            items: vec![item(10, 0), item(20, 2)],
            offset: 0,
        };
        let Some(([first, second], point)) = crate::decode::with_test_decode_ctx(|ctx| {
            section_skamp_line_midpoint_sources(ctx, &definition, &line_midpoint)
        })
        .expect("admitted line midpoint sources rows") else {
            panic!("line midpoint sources");
        };
        assert!(matches!(first, SectionPointSource::Point(1)));
        assert!(matches!(second, SectionPointSource::Point(2)));
        assert!(matches!(point, SectionPointSource::Point(3)));

        let arc_midpoint = crate::feature::definitions::FeatureSkamp {
            id: 35,
            kind: 35,
            flags: 0,
            status: 1,
            items: vec![item(30, 0), item(20, 2)],
            offset: 0,
        };
        let coordinates = BTreeMap::from([
            (5, [Some(1.0), Some(0.0)]),
            (6, [Some(0.0), Some(1.0)]),
            (7, [Some(0.0), Some(0.0)]),
        ]);
        let (point, midpoint) = crate::decode::with_test_decode_ctx(|ctx| {
            section_skamp_arc_midpoint_source(ctx, &definition, &arc_midpoint, &coordinates)
        })
        .expect("admitted arc midpoint source rows")
        .expect("arc midpoint");
        assert!(matches!(point, SectionPointSource::Point(3)));
        assert!(midpoint[0] < 0.0 && midpoint[1] < 0.0);

        let mut duplicate = definition.clone();
        duplicate.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Ordinary(segment(
                crate::feature::definitions::FeatureSegmentKind::Line([8, 9]),
                20,
                None,
                None,
            )),
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_skamp_same_coordinate_sources(
                ctx,
                &duplicate,
                &same_coordinate
            ))
            .expect("admitted same coordinate sources rows")
            .is_none()
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_skamp_line_midpoint_sources(
                ctx,
                &duplicate,
                &line_midpoint
            ))
            .expect("admitted line midpoint sources rows")
            .is_none()
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_skamp_arc_midpoint_source(
                ctx,
                &duplicate,
                &arc_midpoint,
                &coordinates
            ))
            .expect("admitted arc midpoint source rows")
            .is_none()
        );

        let mut cross_family = definition.clone();
        cross_family
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Point(
                crate::feature::definitions::FeaturePointSegment {
                    point_id: 99,
                    external_id: 20,
                    offset: 99,
                },
            ));
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_skamp_same_coordinate_sources(
                ctx,
                &cross_family,
                &same_coordinate
            ))
            .expect("admitted same coordinate sources rows")
            .is_none()
        );
    }

    #[test]
    fn saved_line_fallback_rejects_special_segment_identity_collision() {
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
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
                rows: (vec![crate::feature::definitions::FeatureCircleSegment {
                    center_id: 1,
                    radius_ref: 2,
                    external_id: 10,
                    offset: 10,
                }])
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Circle)
                .chain(
                    (vec![crate::feature::definitions::FeaturePointSegment {
                        point_id: 7,
                        external_id: 20,
                        offset: 20,
                    }])
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::Point),
                )
                .collect(),
                offset: 0,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: Some(crate::feature::definitions::FeatureOrderTable {
                declared_count: 1,
                has_prototype: false,
                entity_ref: None,
                rows: vec![crate::feature::definitions::FeatureOrderRow {
                    external_id: 10,
                    internal_id: 30,
                    bitmask: 0,
                    offset: 30,
                }]
                .into(),
                offset: 0,
            }),
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(crate::feature::definitions::FeatureSavedSection {
                entities: vec![crate::feature::definitions::FeatureSavedEntity::Line(
                    crate::feature::definitions::FeatureSavedLine {
                        entity_id: 30,
                        references: Vec::new(),
                        attributes: Vec::new(),
                        endpoints: [
                            [Some(0.0), Some(0.0), Some(0.0)],
                            [Some(0.0), Some(2.0), Some(0.0)],
                        ],
                        body: Vec::new(),
                        offset: 40,
                    },
                )],
                offset: 40,
            }),
            offset: 0,
        };
        let special_item = crate::feature::definitions::FeatureSkampItem {
            entity_id: 10,
            sense: 0,
        };
        assert!(
            !crate::decode::with_test_decode_ctx(|ctx| section_skamp_is_line(
                ctx,
                &definition,
                &special_item
            ))
            .expect("admitted is line rows")
        );
        assert!(
            !crate::decode::with_test_decode_ctx(|ctx| section_skamp_is_arc(
                ctx,
                &definition,
                &special_item
            ))
            .expect("admitted is arc rows")
        );

        let midpoint = crate::feature::definitions::FeatureSkamp {
            id: 35,
            kind: 35,
            flags: 0,
            status: 1,
            items: vec![
                special_item,
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 20,
                    sense: 0,
                },
            ],
            offset: 0,
        };
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| section_skamp_line_midpoint_sources(
                ctx,
                &definition,
                &midpoint
            ))
            .expect("admitted line midpoint sources rows")
            .is_none()
        );
    }
}
