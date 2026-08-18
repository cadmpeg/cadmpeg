// SPDX-License-Identifier: Apache-2.0
//! Resolved profile chains and solver-only section entities.

use super::super::feature_history::feature_skamp_table_complete;
use super::super::sketch::{trim_segment_id, unique_decoded_section_segment};
use super::super::sketch_ids::sketch_entity_id;
use super::super::uniqueness::exactly_one;
use super::{
    complete_section_skamps, saved_section_entity_fallback_allowed, section_degenerate_axis_line,
    section_saved_entity, section_skamp_active, unique_bounded_curve_segment,
    unique_centered_line_segment, unique_circle_segment, unique_point_segment,
    unique_reference_line_segment,
};
use cadmpeg_ir::sketches::{SketchEntityUse, SketchId};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn resolved_profile_chains(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = &definition.trim_entities else {
        return resolved_segment_profile_chains(definition, sketch, emitted);
    };
    if !table.has_complete_bucket_frame() || !table.has_unique_external_ids() {
        // A present trim table is authoritative; its failure cannot authorize
        // the point-incidence fallback reserved for an absent table.
        return Vec::new();
    }
    let rows = table
        .rows
        .iter()
        .filter_map(|row| Some((row, trim_segment_id(definition, row)?)))
        .collect::<Vec<_>>();
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, row) in rows.iter().enumerate() {
        for vertex in row.0.vertices {
            incident.entry(vertex).or_default().push(index);
        }
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut frontier = vec![seed];
        while let Some(index) = frontier.pop() {
            for vertex in rows[index].0.vertices {
                for adjacent in &incident[&vertex] {
                    if component.insert(*adjacent) {
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        remaining.retain(|index| !component.contains(index));
        if incident
            .values()
            .any(|rows| rows.iter().filter(|row| component.contains(row)).count() > 2)
        {
            continue;
        }
        if component
            .iter()
            .any(|index| !emitted.contains(&rows[*index].1))
        {
            continue;
        }
        let endpoints = incident
            .iter()
            .filter(|(_, rows)| rows.iter().filter(|row| component.contains(row)).count() == 1)
            .map(|(vertex, _)| *vertex)
            .collect::<Vec<_>>();
        if !matches!(endpoints.len(), 0 | 2) {
            continue;
        }
        let first_row = component
            .iter()
            .min_by_key(|index| rows[**index].1)
            .copied()
            .expect("component contains seed");
        let mut vertex = endpoints
            .iter()
            .min()
            .copied()
            .unwrap_or(rows[first_row].0.vertices[0]);
        let start_vertex = vertex;
        let mut unused = component;
        let mut profile = Vec::new();
        while !unused.is_empty() {
            let candidates = incident[&vertex]
                .iter()
                .filter(|index| unused.contains(index))
                .copied()
                .collect::<Vec<_>>();
            let index = if profile.is_empty() && endpoints.is_empty() {
                if candidates.contains(&first_row) {
                    first_row
                } else {
                    break;
                }
            } else if candidates.len() == 1 {
                candidates[0]
            } else {
                break;
            };
            let (row, external_id) = rows[index];
            let row_reversed = row.vertices[1] == vertex;
            if !row_reversed && row.vertices[0] != vertex {
                break;
            }
            let arc_orientation_reversed = definition
                .segments
                .as_ref()
                .and_then(|table| table.segment(external_id))
                .is_some_and(|segment| {
                    segment.kind == crate::feature::FeatureSegmentKind::Arc
                        && segment.arc_orientation == Some(0)
                });
            profile.push(SketchEntityUse {
                entity: sketch_entity_id(sketch, external_id),
                reversed: row_reversed ^ arc_orientation_reversed,
            });
            vertex = if row_reversed {
                row.vertices[0]
            } else {
                row.vertices[1]
            };
            unused.remove(&index);
        }
        let terminal_ok = if endpoints.is_empty() {
            vertex == start_vertex
        } else {
            endpoints.contains(&vertex) && vertex != start_vertex
        };
        if unused.is_empty() && terminal_ok {
            profiles.push(profile);
        }
    }
    profiles
}

pub(in super::super) fn resolved_segment_profile_chains(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = definition
        .segments
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let rows = table
        .rows
        .iter()
        .filter(|segment| {
            emitted.contains(&segment.external_id)
                && matches!(
                    segment.kind,
                    crate::feature::FeatureSegmentKind::Line
                        | crate::feature::FeatureSegmentKind::Arc
                )
        })
        .collect::<Vec<_>>();
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, segment) in rows.iter().enumerate() {
        for point in segment.point_ids {
            incident.entry(point).or_default().push(index);
        }
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut frontier = vec![seed];
        while let Some(index) = frontier.pop() {
            for point in rows[index].point_ids {
                for adjacent in &incident[&point] {
                    if component.insert(*adjacent) {
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        remaining.retain(|index| !component.contains(index));
        if component.iter().any(|index| {
            rows[*index].point_ids.into_iter().any(|point| {
                incident[&point]
                    .iter()
                    .filter(|row| component.contains(row))
                    .count()
                    != 2
            })
        }) {
            continue;
        }
        let first = component
            .iter()
            .min_by_key(|index| rows[**index].external_id)
            .copied()
            .expect("component contains seed");
        let mut point = rows[first].point_ids[0].min(rows[first].point_ids[1]);
        let start = point;
        let mut unused = component;
        let mut profile = Vec::new();
        while !unused.is_empty() {
            let candidates = incident[&point]
                .iter()
                .filter(|index| unused.contains(index))
                .copied()
                .collect::<BTreeSet<_>>();
            let index = if profile.is_empty() && candidates.contains(&first) {
                first
            } else if candidates.len() == 1 {
                *candidates.first().expect("one candidate")
            } else {
                break;
            };
            let segment = rows[index];
            let traversal_reversed = segment.point_ids[1] == point;
            if !traversal_reversed && segment.point_ids[0] != point {
                break;
            }
            let analytic_reversed = segment.kind == crate::feature::FeatureSegmentKind::Arc
                && segment.arc_orientation == Some(0);
            profile.push(SketchEntityUse {
                entity: sketch_entity_id(sketch, segment.external_id),
                reversed: traversal_reversed ^ analytic_reversed,
            });
            point = if traversal_reversed {
                segment.point_ids[0]
            } else {
                segment.point_ids[1]
            };
            unused.remove(&index);
        }
        if unused.is_empty() && point == start {
            profiles.push(profile);
        }
    }
    profiles
}

pub(in super::super) fn solver_only_section_entities(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, usize> {
    let declared_segment_ids = definition
        .segments
        .iter()
        .flat_map(|table| {
            table
                .rows
                .iter()
                .map(|segment| segment.external_id)
                .chain(table.circle_rows.iter().map(|segment| segment.external_id))
                .chain(table.point_rows.iter().map(|segment| segment.external_id))
                .chain(
                    table
                        .centered_line_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    table
                        .reference_line_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    table
                        .bounded_curve_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(table.conic_rows.iter().map(|segment| segment.external_id))
                .chain(table.opaque_rows.iter().map(|segment| segment.external_id))
        })
        .collect::<BTreeSet<_>>();
    definition
        .relations
        .iter()
        .flat_map(|relations| &relations.skamps)
        .flat_map(|skamp| {
            skamp
                .items
                .iter()
                .map(move |item| (item.entity_id, skamp.offset))
        })
        .filter(|(entity_id, _)| !declared_segment_ids.contains(entity_id))
        .fold(
            BTreeMap::<u32, usize>::new(),
            |mut entities, (id, offset)| {
                entities
                    .entry(id)
                    .and_modify(|first_offset| *first_offset = (*first_offset).min(offset))
                    .or_insert(offset);
                entities
            },
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in super::super) enum SectionEntityIncidenceFamily {
    Point,
    BoundedCurve,
    Line,
    Arc,
    Circular,
}

pub(in super::super) fn section_skamp_has_proven_point_locus(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    if item.sense == 0 {
        return unique_point_segment(definition, item.entity_id).is_some()
            || unique_decoded_section_segment(definition, item.entity_id).is_some_and(|segment| {
                segment.kind == crate::feature::FeatureSegmentKind::Point
                    && !section_degenerate_axis_line(definition, segment)
            });
    }
    let solver_family =
        section_incidence_curve_family_evidence_without_type35(definition, item.entity_id);
    if solver_family.len() == 1
        && ((solver_family.contains(&SectionEntityIncidenceFamily::BoundedCurve)
            || solver_family.contains(&SectionEntityIncidenceFamily::Line)
            || solver_family.contains(&SectionEntityIncidenceFamily::Arc))
            && matches!(item.sense, 2 | 3)
            || (solver_family.contains(&SectionEntityIncidenceFamily::Arc)
                || solver_family.contains(&SectionEntityIncidenceFamily::Circular))
                && matches!(item.sense, 2..=4))
    {
        return true;
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return matches!(
            (segment.kind, item.sense),
            (crate::feature::FeatureSegmentKind::Line, 2 | 3)
                | (crate::feature::FeatureSegmentKind::Arc, 2..=4)
        );
    }
    if unique_centered_line_segment(definition, item.entity_id).is_some() {
        return matches!(item.sense, 2..=4);
    }
    if let Some(segment) = unique_reference_line_segment(definition, item.entity_id) {
        return match item.sense {
            2 => segment.point_ids[0].is_some(),
            3 => segment.point_ids[1].is_some(),
            _ => false,
        };
    }
    if unique_bounded_curve_segment(definition, item.entity_id).is_some() {
        return matches!(item.sense, 2 | 3);
    }
    if unique_circle_segment(definition, item.entity_id).is_some() {
        return item.sense == 4;
    }
    if !saved_section_entity_fallback_allowed(definition, item.entity_id) {
        return false;
    }
    matches!(
        (section_saved_entity(definition, item.entity_id), item.sense),
        (Some(crate::feature::FeatureSavedEntity::Line(_)), 2 | 3)
            | (Some(crate::feature::FeatureSavedEntity::Arc(_)), 2..=4)
            | (
                Some(
                    crate::feature::FeatureSavedEntity::Circle(_)
                        | crate::feature::FeatureSavedEntity::Conic(_),
                ),
                4,
            )
    )
}

pub(in super::super) fn section_incidence_curve_family_evidence(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> BTreeSet<SectionEntityIncidenceFamily> {
    section_incidence_curve_family_evidence_with_solver_roles(definition, entity_id, true, true)
}

fn section_incidence_curve_family_evidence_without_type35(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> BTreeSet<SectionEntityIncidenceFamily> {
    section_incidence_curve_family_evidence_with_solver_roles(definition, entity_id, false, false)
}

fn section_incidence_curve_family_evidence_with_solver_roles(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
    include_type35: bool,
    include_type_zero: bool,
) -> BTreeSet<SectionEntityIncidenceFamily> {
    let mut evidence = BTreeSet::new();
    if complete_section_skamps(definition).any(|skamp| {
        matches!(
            (skamp.kind, skamp.items.as_slice()),
            (1 | 2, [item]) if item.sense == 0 && item.entity_id == entity_id
        )
    }) {
        evidence.insert(SectionEntityIncidenceFamily::Line);
    }
    for skamp in definition
        .relations
        .iter()
        .filter(|relations| feature_skamp_table_complete(relations))
        .flat_map(|relations| &relations.skamps)
    {
        for item in &skamp.items {
            if item.entity_id == entity_id && matches!(item.sense, 2 | 3) {
                evidence.insert(SectionEntityIncidenceFamily::BoundedCurve);
            }
            if item.entity_id == entity_id && item.sense == 4 {
                evidence.insert(SectionEntityIncidenceFamily::Circular);
            }
        }
        if include_type35 {
            let type35_line_role =
                |target: &crate::feature::FeatureSkampItem,
                 point: &crate::feature::FeatureSkampItem| {
                    if target.sense != 0
                        || (point.sense == 4
                            && unique_centered_line_segment(definition, point.entity_id).is_some())
                        || !section_skamp_has_proven_point_locus(definition, point)
                    {
                        return false;
                    }
                    unique_opaque_section_entity(definition, target.entity_id)
                        || solver_only_section_entities(definition).contains_key(&target.entity_id)
                };
            if let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) {
                if (first.entity_id == entity_id && type35_line_role(first, second))
                    || (second.entity_id == entity_id && type35_line_role(second, first))
                {
                    evidence.insert(SectionEntityIncidenceFamily::Line);
                }
            }
        }
        if include_type_zero {
            let type_zero_point_role =
                |target: &crate::feature::FeatureSkampItem,
                 point: &crate::feature::FeatureSkampItem| {
                    target.sense == 0
                        && section_skamp_has_proven_point_locus(definition, point)
                        && (unique_opaque_section_entity(definition, target.entity_id)
                            || solver_only_section_entities(definition)
                                .contains_key(&target.entity_id))
                };
            if let (0, [first, second]) = (skamp.kind, skamp.items.as_slice()) {
                if (first.entity_id == entity_id && type_zero_point_role(first, second))
                    || (second.entity_id == entity_id && type_zero_point_role(second, first))
                {
                    evidence.insert(SectionEntityIncidenceFamily::Point);
                }
            }
        }
        // Line-family roles are structural; type-six circular evidence is
        // activity-dependent, like its radius-equality constraint.
        match (skamp.kind, skamp.items.as_slice()) {
            (5 | 7 | 8, [first, second])
                if first.sense == 0
                    && second.sense == 0
                    && (first.entity_id == entity_id || second.entity_id == entity_id) =>
            {
                evidence.insert(SectionEntityIncidenceFamily::Line);
            }
            _ => {}
        }
        if section_skamp_active(skamp.status)
            && matches!((skamp.kind, skamp.items.as_slice()), (6, [first, second])
                if first.sense == 0
                    && second.sense == 0
                    && (first.entity_id == entity_id || second.entity_id == entity_id))
        {
            evidence.insert(SectionEntityIncidenceFamily::Circular);
        }
    }
    normalize_section_incidence_curve_family_evidence(&mut evidence);
    evidence
}

fn unique_opaque_section_entity(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> bool {
    definition.segments.as_ref().is_some_and(|segments| {
        segments
            .opaque_rows
            .iter()
            .any(|segment| segment.external_id == entity_id)
            && segments.external_id_count(entity_id) == 1
    })
}

pub(in super::super) fn unique_section_incidence_curve_family(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> Option<SectionEntityIncidenceFamily> {
    exactly_one(section_incidence_curve_family_evidence(definition, entity_id).into_iter())
}

pub(in super::super) fn normalize_section_incidence_curve_family_evidence(
    evidence: &mut BTreeSet<SectionEntityIncidenceFamily>,
) {
    if evidence.contains(&SectionEntityIncidenceFamily::Line) {
        evidence.remove(&SectionEntityIncidenceFamily::BoundedCurve);
    } else if evidence.contains(&SectionEntityIncidenceFamily::Circular)
        && evidence.remove(&SectionEntityIncidenceFamily::BoundedCurve)
    {
        evidence.remove(&SectionEntityIncidenceFamily::Circular);
        evidence.insert(SectionEntityIncidenceFamily::Arc);
    }
}

pub(in super::super) fn solver_only_section_entity_family(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> Option<SectionEntityIncidenceFamily> {
    solver_only_section_entities(definition)
        .contains_key(&entity_id)
        .then_some(())?;
    let mut evidence = section_incidence_curve_family_evidence(definition, entity_id);
    if complete_section_skamps(definition).any(|skamp| {
        skamp
            .items
            .iter()
            .any(|item| item.entity_id == entity_id && item.sense == 4)
    }) {
        evidence.insert(SectionEntityIncidenceFamily::Circular);
    }
    if !evidence.contains(&SectionEntityIncidenceFamily::Line)
        && complete_section_skamps(definition).any(|skamp| {
            let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
                return false;
            };
            [(first, second), (second, first)]
                .into_iter()
                .any(|(point, target)| {
                    point.entity_id == entity_id
                        && point.sense == 0
                        && target.sense == 4
                        && unique_centered_line_segment(definition, target.entity_id).is_some()
                })
        })
    {
        evidence.insert(SectionEntityIncidenceFamily::Point);
    }
    if !evidence.contains(&SectionEntityIncidenceFamily::Point) {
        let solver_only_point_from_midpoint = complete_section_skamps(definition).any(|skamp| {
            let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
                return false;
            };
            [(first, second), (second, first)]
                .into_iter()
                .filter(|(point, target)| {
                    point.sense == 0
                        && point.entity_id == entity_id
                        && target.sense == 0
                        && (unique_decoded_section_segment(definition, target.entity_id)
                            .is_some_and(|segment| {
                                matches!(
                                    segment.kind,
                                    crate::feature::FeatureSegmentKind::Line
                                        | crate::feature::FeatureSegmentKind::Arc
                                )
                            })
                            || (saved_section_entity_fallback_allowed(
                                definition,
                                target.entity_id,
                            ) && section_saved_entity(definition, target.entity_id)
                                .is_some_and(|saved| {
                                    matches!(
                                        saved,
                                        crate::feature::FeatureSavedEntity::Line(_)
                                            | crate::feature::FeatureSavedEntity::Arc(_)
                                    )
                                })))
                })
                .count()
                == 1
        });
        if solver_only_point_from_midpoint {
            evidence.insert(SectionEntityIncidenceFamily::Point);
        }
    }
    let mut evidence = evidence.into_iter();
    let family = evidence.next()?;
    evidence.next().is_none().then_some(family)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(external_id: u32) -> crate::feature::FeatureOpaqueSegment {
        crate::feature::FeatureOpaqueSegment {
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
            offset: external_id as usize,
        }
    }

    fn midpoint(target: u32, point: u32) -> crate::feature::FeatureSkamp {
        crate::feature::FeatureSkamp {
            id: target,
            kind: 35,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: target,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: point,
                    sense: 0,
                },
            ],
            offset: target as usize,
        }
    }

    fn definition(target: u32, opaque_target: bool) -> crate::feature::FeatureDefinition {
        let opaque_rows: Vec<crate::feature::FeatureOpaqueSegment> =
            opaque_target.then(|| opaque(target)).into_iter().collect();
        let point_rows = vec![crate::feature::FeaturePointSegment {
            point_id: 7,
            external_id: 7,
            offset: 7,
        }];
        crate::feature::FeatureDefinition {
            id: 917,
            owner_feature_id: None,
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::FeatureSegmentTable {
                declared_count: u32::try_from(opaque_rows.len() + point_rows.len())
                    .expect("segment count"),
                has_elided_prototype: false,
                entity_ref: None,
                rows: Vec::new(),
                circle_rows: Vec::new(),
                point_rows,
                centered_line_rows: Vec::new(),
                reference_line_rows: Vec::new(),
                bounded_curve_rows: Vec::new(),
                conic_rows: Vec::new(),
                opaque_rows,
                offset: 0,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::FeatureRelationTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                skamps: vec![midpoint(target, 7)],
                skamp_header: Some(crate::feature::FeatureSolverTableHeader {
                    declared_count: 1,
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
    fn type35_point_locus_establishes_unique_native_line_family() {
        let opaque_target = definition(101, true);
        assert_eq!(
            unique_section_incidence_curve_family(&opaque_target, 101),
            Some(SectionEntityIncidenceFamily::Line)
        );

        let solver_only_target = definition(201, false);
        assert_eq!(
            unique_section_incidence_curve_family(&solver_only_target, 201),
            Some(SectionEntityIncidenceFamily::Line)
        );
        assert_eq!(
            solver_only_section_entity_family(&solver_only_target, 201),
            Some(SectionEntityIncidenceFamily::Line)
        );
    }

    #[test]
    fn type35_line_family_requires_unique_native_target() {
        let mut definition = definition(101, true);
        let segments = definition.segments.as_mut().expect("segments");
        segments.opaque_rows.push(opaque(101));
        segments.declared_count = 2;
        assert_eq!(
            unique_section_incidence_curve_family(&definition, 101),
            None
        );
    }

    #[test]
    fn type_zero_point_locus_establishes_unique_native_point_family() {
        let mut opaque_target = definition(101, true);
        opaque_target.relations.as_mut().expect("relations").skamps[0].kind = 0;
        assert_eq!(
            unique_section_incidence_curve_family(&opaque_target, 101),
            Some(SectionEntityIncidenceFamily::Point)
        );

        let mut solver_only_target = definition(201, false);
        solver_only_target
            .relations
            .as_mut()
            .expect("relations")
            .skamps[0]
            .kind = 0;
        assert_eq!(
            unique_section_incidence_curve_family(&solver_only_target, 201),
            Some(SectionEntityIncidenceFamily::Point)
        );
        assert_eq!(
            solver_only_section_entity_family(&solver_only_target, 201),
            Some(SectionEntityIncidenceFamily::Point)
        );
    }

    #[test]
    fn type_zero_point_family_requires_unique_native_target() {
        let mut definition = definition(101, true);
        let segments = definition.segments.as_mut().expect("segments");
        segments.opaque_rows.push(opaque(101));
        segments.declared_count += 1;
        definition.relations.as_mut().expect("relations").skamps[0].kind = 0;
        assert_eq!(
            unique_section_incidence_curve_family(&definition, 101),
            None
        );
    }
}
