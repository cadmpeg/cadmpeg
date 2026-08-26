// SPDX-License-Identifier: Apache-2.0
//! Section point, segment, and SKAMP locus resolution.

use super::super::feature_history::feature_skamp_table_complete;
use super::super::sketch::{
    resolved_section_points, section_skamp_selected_point, section_skamp_selected_point_id,
    unique_decoded_section_segment, unique_section_skamp_segment, SectionPointSource,
};
use super::super::sketch_ids::sketch_entity_id;
use super::{
    semantic_saved_section_entities, solver_only_section_entities,
    solver_only_section_entity_family, unique_section_incidence_curve_family,
    unique_section_segment_external_ids, SectionEntityIncidenceFamily,
};
use cadmpeg_ir::sketches::{
    SketchCoordinateAxis, SketchEntityId, SketchGeometry, SketchId, SketchLocus,
};
use std::collections::BTreeMap;

const EPS_LOCUS_COORDINATE: f64 = 1.0e-9;
const EPS_LOCUS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_LOCUS_RADIUS_AGREEMENT: f64 = 1.0e-9;

pub(in super::super) fn section_point_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    point_id: u32,
) -> Option<SketchLocus> {
    let unique_entities = unique_section_segment_external_ids(definition);
    definition
        .segments
        .as_ref()?
        .rows
        .iter()
        .filter(|segment| unique_entities.contains(&segment.external_id))
        .filter_map(|segment| {
            let entity = sketch_entity_id(sketch, segment.external_id);
            let locus = match segment.kind {
                crate::feature::FeatureSegmentKind::Point
                    if segment.point_ids[0] == point_id && segment.point_ids[1] == point_id =>
                {
                    SketchLocus::Entity(entity)
                }
                crate::feature::FeatureSegmentKind::Line if segment.point_ids[0] == point_id => {
                    SketchLocus::Start(entity)
                }
                crate::feature::FeatureSegmentKind::Line if segment.point_ids[1] == point_id => {
                    SketchLocus::End(entity)
                }
                crate::feature::FeatureSegmentKind::Arc if segment.point_ids[0] == point_id => {
                    SketchLocus::End(entity)
                }
                crate::feature::FeatureSegmentKind::Arc if segment.point_ids[1] == point_id => {
                    SketchLocus::Start(entity)
                }
                _ => return None,
            };
            Some((segment.offset, locus))
        })
        .min_by_key(|(offset, _)| *offset)
        .map(|(_, locus)| locus)
}

pub(in super::super) fn unique_circle_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureCircleSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .circle_rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(in super::super) fn unique_point_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeaturePointSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .point_rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(in super::super) fn unique_centered_line_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureCenteredLineSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .centered_line_rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(in super::super) fn unique_reference_line_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureReferenceLineSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .reference_line_rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(in super::super) fn unique_bounded_curve_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureBoundedCurveSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .bounded_curve_rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(in super::super) fn section_skamp_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    let entity = sketch_entity_id(sketch, item.entity_id);
    if let Some(family) = solver_only_section_entity_family(definition, item.entity_id) {
        return section_entity_family_locus(family, entity, item.sense);
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return match (segment.kind, item.sense) {
            (_, 0) => Some(SketchLocus::Entity(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 2) => Some(SketchLocus::End(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 3) => Some(SketchLocus::Start(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 4) => Some(SketchLocus::Center(entity)),
            (_, 2) => Some(SketchLocus::Start(entity)),
            (_, 3) => Some(SketchLocus::End(entity)),
            _ => None,
        };
    }
    if let Some(segment) = unique_reference_line_segment(definition, item.entity_id) {
        return match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            2 if segment.point_ids[0].is_some() => Some(SketchLocus::Start(entity)),
            3 if segment.point_ids[1].is_some() => Some(SketchLocus::End(entity)),
            _ => None,
        };
    }
    if unique_bounded_curve_segment(definition, item.entity_id).is_some() {
        return match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            2 => Some(SketchLocus::Start(entity)),
            3 => Some(SketchLocus::End(entity)),
            _ => None,
        };
    }
    if unique_point_segment(definition, item.entity_id).is_some() {
        return matches!(item.sense, 0 | 4).then_some(SketchLocus::Entity(entity));
    }
    if unique_centered_line_segment(definition, item.entity_id).is_some() {
        return match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            2 => Some(SketchLocus::Start(entity)),
            3 => Some(SketchLocus::End(entity)),
            4 => Some(SketchLocus::Center(entity)),
            _ => None,
        };
    }
    if unique_circle_segment(definition, item.entity_id).is_some() {
        return match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            4 => Some(SketchLocus::Center(entity)),
            _ => None,
        };
    }
    if definition
        .segments
        .iter()
        .flat_map(|segments| {
            segments
                .rows
                .iter()
                .map(|segment| segment.external_id)
                .chain(
                    segments
                        .circle_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .point_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .centered_line_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .reference_line_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .bounded_curve_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .conic_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .opaque_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
        })
        .any(|external_id| external_id == item.entity_id)
    {
        return section_incidence_curve_locus(definition, entity, item);
    }
    let saved = section_saved_entity(definition, item.entity_id)?;
    match (saved, item.sense) {
        (_, 0) => Some(SketchLocus::Entity(entity)),
        (crate::feature::FeatureSavedEntity::Line(_), 2) => Some(SketchLocus::Start(entity)),
        (crate::feature::FeatureSavedEntity::Line(_), 3) => Some(SketchLocus::End(entity)),
        (crate::feature::FeatureSavedEntity::Arc(_), 2) => Some(SketchLocus::End(entity)),
        (crate::feature::FeatureSavedEntity::Arc(_), 3) => Some(SketchLocus::Start(entity)),
        (
            crate::feature::FeatureSavedEntity::Arc(_)
            | crate::feature::FeatureSavedEntity::Circle(_)
            | crate::feature::FeatureSavedEntity::Conic(_),
            4,
        ) => Some(SketchLocus::Center(entity)),
        _ => None,
    }
}

pub(in super::super) fn section_incidence_curve_locus(
    definition: &crate::feature::FeatureDefinition,
    entity: SketchEntityId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    section_entity_family_locus(
        unique_section_incidence_curve_family(definition, item.entity_id)?,
        entity,
        item.sense,
    )
}

pub(in super::super) fn section_entity_family_locus(
    family: SectionEntityIncidenceFamily,
    entity: SketchEntityId,
    sense: u32,
) -> Option<SketchLocus> {
    match (family, sense) {
        (SectionEntityIncidenceFamily::Point, 0) => Some(SketchLocus::Entity(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            0,
        ) => Some(SketchLocus::Entity(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            2,
        ) => Some(SketchLocus::Start(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
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
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    matches!(item.sense, 2 | 3)
        .then(|| section_skamp_locus(definition, sketch, item))
        .flatten()
}

pub(in super::super) fn section_skamp_shared_endpoint(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    entity: &crate::feature::FeatureSkampItem,
    selected: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    (entity.sense == 0).then_some(())?;
    let segment = unique_section_skamp_segment(definition, entity.entity_id)?;
    matches!(
        segment.kind,
        crate::feature::FeatureSegmentKind::Line | crate::feature::FeatureSegmentKind::Arc
    )
    .then_some(())?;
    let selected_point = section_skamp_selected_point_id(definition, selected)?;
    let mut endpoints = segment
        .point_ids
        .iter()
        .enumerate()
        .filter(|(_, point_id)| **point_id == selected_point);
    let (endpoint, _) = endpoints.next()?;
    endpoints.next().is_none().then_some(())?;
    section_skamp_locus(
        definition,
        sketch,
        &crate::feature::FeatureSkampItem {
            entity_id: entity.entity_id,
            sense: if endpoint == 0 { 2 } else { 3 },
        },
    )
}

pub(in super::super) fn section_skamp_tangent_loci(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    first: &crate::feature::FeatureSkampItem,
    second: &crate::feature::FeatureSkampItem,
    active: bool,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Option<[SketchLocus; 2]> {
    let selected_locus = |item| {
        if active {
            section_skamp_endpoint(definition, sketch, item)
        } else if matches!(item.sense, 2 | 3) {
            section_skamp_incidence_locus(definition, sketch, item, geometry)
        } else {
            None
        }
    };
    if let (Some(first), Some(second)) = (selected_locus(first), selected_locus(second)) {
        return Some([first, second]);
    }
    [(first, second), (second, first)]
        .into_iter()
        .find_map(|(entity, selected)| {
            Some([
                section_skamp_shared_endpoint(definition, sketch, entity, selected)?,
                selected_locus(selected)?,
            ])
        })
        .map(|loci| {
            if first.sense == 0 {
                loci
            } else {
                [loci[1].clone(), loci[0].clone()]
            }
        })
}

pub(in super::super) fn section_skamp_point_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    if item.sense == 0 && section_skamp_is_point(definition, item) {
        return section_skamp_locus(definition, sketch, item);
    }
    matches!(item.sense, 2..=4)
        .then(|| section_skamp_locus(definition, sketch, item))
        .flatten()
}

pub(in super::super) fn section_skamp_incidence_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Option<SketchLocus> {
    section_skamp_point_locus(definition, sketch, item).or_else(|| {
        let entity = sketch_entity_id(sketch, item.entity_id);
        let locus = match item.sense {
            2 => SketchLocus::Start(entity.clone()),
            3 => SketchLocus::End(entity.clone()),
            _ => return None,
        };
        geometry?
            .get(&entity)
            .is_some_and(|geometry| matches!(geometry, SketchGeometry::Native { .. }))
            .then_some(locus)
    })
}

pub(in super::super) fn section_skamp_line_pair(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    first: &crate::feature::FeatureSkampItem,
    second: &crate::feature::FeatureSkampItem,
) -> Option<[SketchEntityId; 2]> {
    if first.sense != 0
        || second.sense != 0
        || !section_skamp_is_line(definition, first)
        || !section_skamp_is_line(definition, second)
    {
        return None;
    }
    Some([first, second].map(|item| sketch_entity_id(sketch, item.entity_id)))
}

pub(in super::super) fn section_skamp_oriented_line(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Option<SketchEntityId> {
    (item.sense == 0).then_some(())?;
    let entity = sketch_entity_id(sketch, item.entity_id);
    if section_skamp_is_line(definition, item) {
        return Some(entity);
    }
    if section_skamp_is_point(definition, item)
        || section_skamp_is_circular(definition, item)
        || section_saved_entity(definition, item.entity_id)
            .is_some_and(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Spline(_)))
    {
        return None;
    }
    if solver_only_section_entities(definition).contains_key(&item.entity_id) {
        return Some(entity);
    }
    let line_role_evidence = complete_section_skamps(definition).any(|skamp| {
        skamp.items.iter().any(|candidate| {
            candidate.entity_id == item.entity_id && matches!(candidate.sense, 2 | 3)
        }) || match (skamp.kind, skamp.items.as_slice()) {
            (35, [first, second]) => {
                (first.entity_id == item.entity_id
                    && first.sense == 0
                    && section_skamp_point_locus(definition, sketch, second).is_some())
                    || (second.entity_id == item.entity_id
                        && second.sense == 0
                        && section_skamp_point_locus(definition, sketch, first).is_some())
            }
            _ => false,
        }
    });
    (line_role_evidence
        && geometry?
            .get(&entity)
            .is_some_and(|geometry| matches!(geometry, SketchGeometry::Native { .. })))
    .then_some(entity)
}

pub(in super::super) fn section_skamp_same_coordinate(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    skamp: &crate::feature::FeatureSkamp,
    require_satisfied: bool,
) -> Option<(SketchLocus, SketchLocus, SketchCoordinateAxis)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let first_locus = section_skamp_point_locus(definition, sketch, first)?;
    let second_locus = section_skamp_point_locus(definition, sketch, second)?;
    let coordinate = section_skamp_same_coordinate_axis(skamp)?;
    let axis = [SketchCoordinateAxis::U, SketchCoordinateAxis::V][coordinate];
    if require_satisfied {
        let ([first_source, second_source], _) =
            section_skamp_same_coordinate_sources(definition, skamp)?;
        let points = resolved_section_points(definition);
        let point = |source| {
            Some(match source {
                SectionPointSource::Point(point_id) => *points.get(&point_id)?,
                SectionPointSource::Value(point) => point,
            })
        };
        if let (Some(first_point), Some(second_point)) = (point(first_source), point(second_source))
        {
            let scale = first_point
                .iter()
                .chain(&second_point)
                .map(|coordinate| coordinate.abs())
                .fold(1.0, f64::max);
            ((first_point[coordinate] - second_point[coordinate]).abs()
                <= EPS_LOCUS_COORDINATE * scale)
                .then_some(())?;
        }
    }
    Some((first_locus, second_locus, axis))
}

pub(in super::super) fn section_skamp_same_coordinate_sources(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<([SectionPointSource; 2], usize)> {
    if matches!(skamp.kind, 12 | 13) {
        let [item] = skamp.items.as_slice() else {
            return None;
        };
        (item.sense == 0 && section_skamp_is_arc(definition, item)).then_some(())?;
        let endpoint = |sense| {
            section_skamp_selected_point(
                definition,
                &crate::feature::FeatureSkampItem {
                    entity_id: item.entity_id,
                    sense,
                },
            )
        };
        return Some((
            [endpoint(2)?, endpoint(3)?],
            section_skamp_same_coordinate_axis(skamp)?,
        ));
    }
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let coordinate = section_skamp_same_coordinate_axis(skamp)?;
    Some((
        [
            section_skamp_selected_point(definition, first)?,
            section_skamp_selected_point(definition, second)?,
        ],
        coordinate,
    ))
}

pub(in super::super) fn section_skamp_same_coordinate_axis(
    skamp: &crate::feature::FeatureSkamp,
) -> Option<usize> {
    Some(match (skamp.kind, skamp.flags) {
        (12, _) => 1,
        (13, _) => 0,
        (15 | 17, 1) => 0,
        (15 | 17, 2) => 1,
        (30, _) => 1,
        (31, _) => 0,
        _ => return None,
    })
}

pub(in super::super) fn section_skamp_is_line(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    if solver_only_section_entity_family(definition, item.entity_id)
        == Some(SectionEntityIncidenceFamily::Line)
    {
        return true;
    }
    if unique_centered_line_segment(definition, item.entity_id).is_some() {
        return true;
    }
    if unique_reference_line_segment(definition, item.entity_id).is_some() {
        return true;
    }
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id);
    if has_segment {
        return unique_decoded_section_segment(definition, item.entity_id).is_some_and(|segment| {
            segment.kind == crate::feature::FeatureSegmentKind::Line
                || section_degenerate_axis_line(definition, segment)
        });
    }
    section_saved_entity(definition, item.entity_id)
        .is_some_and(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Line(_)))
}

pub(in super::super) fn section_degenerate_axis_line(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> bool {
    if segment.kind != crate::feature::FeatureSegmentKind::Point
        || segment.point_ids[0] != segment.point_ids[1]
    {
        return false;
    }
    let expected_kind = match segment.vertical_horizontal {
        Some(0) => 2,
        Some(1) => 1,
        _ => return false,
    };
    let unary_orientation = complete_section_skamps(definition).any(|skamp| {
        matches!(
            (skamp.kind, skamp.items.as_slice()),
            (kind, [item])
                if kind == expected_kind && item.entity_id == segment.external_id && item.sense == 0
        )
    });
    let symmetry_axis = complete_section_skamps(definition).any(|skamp| {
        matches!(
            (skamp.kind, skamp.items.as_slice()),
            (14, [axis, _, _]) if axis.entity_id == segment.external_id && axis.sense == 0
        )
    });
    unary_orientation && symmetry_axis
}

pub(in super::super) fn section_skamp_is_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    solver_only_section_entity_family(definition, item.entity_id)
        == Some(SectionEntityIncidenceFamily::Point)
        || unique_point_segment(definition, item.entity_id).is_some()
        || unique_decoded_section_segment(definition, item.entity_id).is_some_and(|segment| {
            segment.kind == crate::feature::FeatureSegmentKind::Point
                && !section_degenerate_axis_line(definition, segment)
        })
}

pub(in super::super) fn section_skamp_is_arc(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id);
    if has_segment {
        return unique_decoded_section_segment(definition, item.entity_id)
            .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc);
    }
    section_saved_entity(definition, item.entity_id)
        .is_some_and(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Arc(_)))
}

pub(in super::super) fn section_skamp_curve_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    if item.sense != 0 {
        return None;
    }
    let is_curve = section_skamp_is_line(definition, item)
        || unique_bounded_curve_segment(definition, item.entity_id).is_some()
        || section_skamp_is_circular(definition, item)
        || matches!(
            solver_only_section_entity_family(definition, item.entity_id),
            Some(
                SectionEntityIncidenceFamily::BoundedCurve
                    | SectionEntityIncidenceFamily::Arc
                    | SectionEntityIncidenceFamily::Circular
            )
        )
        || section_saved_entity(definition, item.entity_id).is_some_and(|entity| {
            matches!(
                entity,
                crate::feature::FeatureSavedEntity::Conic(_)
                    | crate::feature::FeatureSavedEntity::Spline(_)
            )
        });
    is_curve.then(|| sketch_entity_id(sketch, item.entity_id))
}

pub(in super::super) fn section_skamp_midpoint(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    first: &crate::feature::FeatureSkampItem,
    second: &crate::feature::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Option<(SketchLocus, SketchEntityId)> {
    let target = |item: &crate::feature::FeatureSkampItem| {
        if item.sense == 4 && unique_centered_line_segment(definition, item.entity_id).is_some() {
            return Some(sketch_entity_id(sketch, item.entity_id));
        }
        (item.sense == 0).then_some(())?;
        if section_skamp_is_arc(definition, item) {
            return Some(sketch_entity_id(sketch, item.entity_id));
        }
        section_skamp_oriented_line(definition, sketch, item, geometry)
    };
    let point = |item: &crate::feature::FeatureSkampItem| {
        section_skamp_point_locus(definition, sketch, item).or_else(|| {
            (item.sense == 0 && section_skamp_is_circular(definition, item))
                .then(|| SketchLocus::Center(sketch_entity_id(sketch, item.entity_id)))
        })
    };
    let candidate = |target, point| Some((point?, target?));
    match (
        candidate(target(first), point(second)),
        candidate(target(second), point(first)),
    ) {
        (Some(candidate), None) | (None, Some(candidate)) => Some(candidate),
        _ => None,
    }
}

pub(in super::super) fn section_saved_entity(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSavedEntity> {
    let internal_id = definition.order_table.as_ref()?.internal_id(external_id)?;
    let mut matches = semantic_saved_section_entities(definition).filter(|entity| match entity {
        crate::feature::FeatureSavedEntity::Line(line) => line.entity_id == internal_id,
        crate::feature::FeatureSavedEntity::Arc(arc) => arc.entity_id == internal_id,
        crate::feature::FeatureSavedEntity::Circle(circle) => circle.entity_id == internal_id,
        crate::feature::FeatureSavedEntity::Conic(conic) => conic.entity_id == internal_id,
        crate::feature::FeatureSavedEntity::Spline(spline) => spline.entity_id == Some(internal_id),
        crate::feature::FeatureSavedEntity::Dummy(dummy) => dummy.entity_id == Some(internal_id),
    });
    let entity = matches.next()?;
    matches.next().is_none().then_some(entity)
}

pub(in super::super) fn section_skamp_circular_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    if item.sense != 0 {
        return None;
    }
    section_skamp_is_circular(definition, item).then(|| sketch_entity_id(sketch, item.entity_id))
}

pub(in super::super) fn section_skamp_center_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    (item.sense == 4 && section_skamp_is_circular(definition, item))
        .then(|| sketch_entity_id(sketch, item.entity_id))
}

pub(in super::super) fn section_skamp_is_circular(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    if solver_only_section_entities(definition).contains_key(&item.entity_id) {
        return solver_only_section_entity_family(definition, item.entity_id).is_some_and(
            |family| {
                matches!(
                    family,
                    SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular
                )
            },
        );
    }
    if matches!(
        unique_section_incidence_curve_family(definition, item.entity_id),
        Some(SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular)
    ) {
        return true;
    }
    if unique_circle_segment(definition, item.entity_id).is_some() {
        return true;
    }
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .any(|segment| segment.external_id == item.entity_id);
    if has_segment {
        unique_decoded_section_segment(definition, item.entity_id)
            .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
    } else {
        section_saved_entity(definition, item.entity_id).is_some_and(|entity| {
            matches!(
                entity,
                crate::feature::FeatureSavedEntity::Arc(_)
                    | crate::feature::FeatureSavedEntity::Circle(_)
            )
        })
    }
}

pub(in super::super) fn section_skamp_line_midpoint_sources(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<([u32; 2], SectionPointSource)> {
    let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    let target = |item: &crate::feature::FeatureSkampItem| {
        if item.sense == 4 {
            return unique_centered_line_segment(definition, item.entity_id)
                .map(|line| [line.center_id, line.center_id]);
        }
        if item.sense != 0 {
            return None;
        }
        let segment = unique_decoded_section_segment(definition, item.entity_id)?;
        (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(segment.point_ids)
    };
    let point =
        |item: &crate::feature::FeatureSkampItem| section_skamp_selected_point(definition, item);
    let candidates = [(first, second), (second, first)]
        .into_iter()
        .filter_map(|(target_item, point_item)| Some((target(target_item)?, point(point_item)?)))
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(in super::super) fn section_skamp_arc_midpoint_source(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Option<(SectionPointSource, [f64; 2])> {
    let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    let candidates = [(first, second), (second, first)]
        .into_iter()
        .filter_map(|(target, point)| {
            (target.sense == 0 && section_skamp_is_arc(definition, target)).then_some(())?;
            Some((
                section_skamp_selected_point(definition, point)?,
                section_skamp_arc_midpoint(definition, target, coordinates)?,
            ))
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(in super::super) fn section_skamp_arc_midpoint(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Option<[f64; 2]> {
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        if segment.kind != crate::feature::FeatureSegmentKind::Arc
            || segment.arc_orientation != Some(0)
        {
            return None;
        }
        let center = complete_section_coordinate(coordinates, segment.center_id?)?;
        let first = complete_section_coordinate(coordinates, segment.point_ids[0])?;
        let second = complete_section_coordinate(coordinates, segment.point_ids[1])?;
        return oriented_arc_midpoint(center, first, second, None);
    }
    let crate::feature::FeatureSavedEntity::Arc(arc) =
        section_saved_entity(definition, item.entity_id)?
    else {
        return None;
    };
    saved_arc_midpoint(arc)
}

pub(in super::super) fn complete_section_coordinate(
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    point_id: u32,
) -> Option<[f64; 2]> {
    let [Some(u), Some(v)] = coordinates.get(&point_id).copied()? else {
        return None;
    };
    Some([u, v])
}

pub(in super::super) fn saved_arc_midpoint(
    arc: &crate::feature::FeatureSavedArc,
) -> Option<[f64; 2]> {
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

pub(in super::super) fn oriented_arc_midpoint(
    center: [f64; 2],
    first: [f64; 2],
    second: [f64; 2],
    stored_radius: Option<f64>,
) -> Option<[f64; 2]> {
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
    while end <= start {
        end += std::f64::consts::TAU;
    }
    let angle = f64::midpoint(start, end);
    Some([
        center[0] + radius * angle.cos(),
        center[1] + radius * angle.sin(),
    ])
}

pub(in super::super) fn section_skamp_active(status: u32) -> bool {
    status & 1 != 0
}

pub(in super::super) fn complete_section_skamps(
    definition: &crate::feature::FeatureDefinition,
) -> impl Iterator<Item = &crate::feature::FeatureSkamp> {
    definition
        .relations
        .iter()
        .filter(|relations| feature_skamp_table_complete(relations))
        .flat_map(|relations| &relations.skamps)
}

pub(in super::super) fn active_complete_section_skamps(
    definition: &crate::feature::FeatureDefinition,
) -> impl Iterator<Item = &crate::feature::FeatureSkamp> {
    complete_section_skamps(definition).filter(|skamp| section_skamp_active(skamp.status))
}
