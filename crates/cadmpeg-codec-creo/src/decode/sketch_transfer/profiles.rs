// SPDX-License-Identifier: Apache-2.0
//! Resolved profile chains and solver-only section entities.

use super::super::sketch::radii::trim_segment_id;
use super::super::sketch::skamp::unique_decoded_section_segment;
use super::super::sketch_ids::sketch_entity_id_admitted;
use super::super::uniqueness::exactly_one;
use crate::decode::sketch_transfer::identity::saved_section_entity_fallback_allowed;
use crate::decode::sketch_transfer::loci::{
    section_degenerate_axis_line, section_saved_entity, section_skamp_active,
    unique_bounded_curve_segment, unique_centered_line_segment, unique_circle_segment,
    unique_point_segment, unique_reference_line_segment, visit_section_skamps,
    visit_all_section_skamps,
};
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{SketchEntityUse, SketchId};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

pub(in super::super) fn resolved_profile_chains(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<u32>,
) -> Result<Vec<Vec<SketchEntityUse>>, CodecError> {
    let Some(table) = &definition.trim_entities else {
        return resolved_segment_profile_chains(ctx, definition, sketch, emitted);
    };
    if !table.has_complete_bucket_frame() || !table.has_unique_external_ids() {
        // A present trim table is authoritative; its failure cannot authorize
        // the point-incidence fallback reserved for an absent table.
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for row in ctx.admit_iter(&table.rows, "creo trim profile rows")? {
        if let Some(id) = trim_segment_id(ctx, definition, row)? {
            ctx.reserve_vec(&mut rows, 1, "creo trim profile rows")?;
            rows.push((row, id));
        }
    }
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, row) in ctx
        .admit_iter(&rows, "creo trim profile incidence sources")?
        .enumerate()
    {
        for vertex in row.0.vertices {
            ctx.admit_btree_entry(&incident, &vertex, "creo trim profile incidence nodes")?;
            let indices = incident.entry(vertex).or_default();
            ctx.reserve_vec(indices, 1, "creo trim profile incidence rows")?;
            indices.push(index);
        }
    }
    let mut remaining = BTreeSet::new();
    for (index, _) in ctx.admit_iter(&rows, "creo trim profile remaining sources")?.enumerate() {
        ctx.insert_btree_set(&mut remaining, index, "creo trim profile remaining nodes")?;
    }
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        ctx.charge_collection_items(1, "creo trim profile component nodes")?;
        ctx.charge_work(1, "creo trim profile components")?;
        let mut component = BTreeSet::from([seed]);
        let mut frontier = ctx.alloc_filled(1, seed, "creo trim profile frontier")?;
        while let Some(index) = frontier.pop() {
            ctx.charge_work(1, "creo trim profile frontier visits")?;
            for vertex in rows[index].0.vertices {
                for adjacent in
                    ctx.admit_iter(&incident[&vertex], "creo trim profile adjacent rows")?
                {
                    if ctx.insert_btree_set(
                        &mut component,
                        *adjacent,
                        "creo trim profile component nodes",
                    )? {
                        ctx.reserve_vec(&mut frontier, 1, "creo trim profile frontier")?;
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        remaining.retain(|index| !component.contains(index));
        let mut component_degree_too_high = false;
        for adjacent_rows in ctx.admit_iter(&incident, "creo trim profile vertex rows")?.map(|(_, rows)| rows) {
            let component_degree = ctx
                .admit_iter(adjacent_rows, "creo trim profile component degrees")?
                .filter(|row| component.contains(row))
                .count();
            if component_degree > 2 {
                component_degree_too_high = true;
                break;
            }
        }
        if component_degree_too_high {
            continue;
        }
        if ctx
            .admit_iter(&component, "creo trim profile emitted entities")?
            .any(|index| !emitted.contains(&rows[*index].1))
        {
            continue;
        }
        let mut endpoints = [0u32; 2];
        let mut endpoint_count = 0usize;
        for (&vertex, rows) in ctx.admit_iter(&incident, "creo trim profile endpoints")? {
            let component_degree = ctx
                .admit_iter(rows, "creo trim profile endpoint degree")?
                .filter(|row| component.contains(row))
                .count();
            if component_degree == 1 {
                if endpoint_count < endpoints.len() {
                    endpoints[endpoint_count] = vertex;
                }
                endpoint_count += 1;
            }
        }
        if !matches!(endpoint_count, 0 | 2) {
            continue;
        }
        let Some(first_row) = ctx
            .admit_iter(&component, "creo trim profile canonical row selection")?
            .min_by_key(|index| rows[**index].1)
            .copied()
        else {
            continue;
        };
        let mut vertex = if endpoint_count == 2 {
            endpoints[0]
        } else {
            rows[first_row].0.vertices[0]
        };
        let start_vertex = vertex;
        let mut unused = component;
        let mut profile = Vec::new();
        while !unused.is_empty() {
            ctx.charge_work(1, "creo trim profile row visits")?;
            let mut candidates = ctx
                .admit_iter(&incident[&vertex], "creo trim profile candidates")?
                .filter(|index| unused.contains(index))
                .copied();
            let first_candidate = candidates.next();
            let second_candidate = candidates.next();
            let index = if profile.is_empty() && endpoint_count == 0 {
                if ctx
                    .admit_iter(&incident[&vertex], "creo trim profile cycle start")?
                    .any(|candidate| *candidate == first_row && unused.contains(candidate))
                {
                    first_row
                } else {
                    break;
                }
            } else if let (Some(index), None) = (first_candidate, second_candidate) {
                index
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
                    matches!(
                        segment.kind,
                        crate::feature::definitions::FeatureSegmentKind::Arc(_)
                    ) && segment.arc_orientation == Some(0)
                });
            let Some(entity) = sketch_entity_id_admitted(ctx, sketch, external_id)? else {
                continue;
            };
            ctx.reserve_vec(&mut profile, 1, "creo trim profile entity uses")?;
            profile.push(SketchEntityUse {
                entity,
                reversed: row_reversed ^ arc_orientation_reversed,
            });
            vertex = if row_reversed {
                row.vertices[0]
            } else {
                row.vertices[1]
            };
            unused.remove(&index);
        }
        let terminal_ok = if endpoint_count == 0 {
            vertex == start_vertex
        } else {
            endpoints.contains(&vertex) && vertex != start_vertex
        };
        if unused.is_empty() && terminal_ok {
            ctx.reserve_vec(&mut profiles, 1, "creo resolved trim profiles")?;
            profiles.push(profile);
        }
    }
    Ok(profiles)
}

fn resolved_segment_profile_chains(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<u32>,
) -> Result<Vec<Vec<SketchEntityUse>>, CodecError> {
    let Some(table) = definition
        .segments
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    for segment in ctx
        .admit_iter(table.rows.as_slice(), "creo emitted segment profile rows")?
        .filter_map(|row| match row {
            SegmentRow::Ordinary(segment) => Some(segment),
            _ => None,
        })
        .filter(|segment| {
            emitted.contains(&segment.external_id)
                && matches!(
                    segment.kind,
                    crate::feature::definitions::FeatureSegmentKind::Line(_)
                        | crate::feature::definitions::FeatureSegmentKind::Arc(_)
                )
        })
    {
        ctx.reserve_vec(&mut rows, 1, "creo segment profile rows")?;
        rows.push(segment);
    }
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, segment) in ctx
        .admit_iter(&rows, "creo segment profile incidence sources")?
        .enumerate()
    {
        for point in segment.point_ids() {
            ctx.admit_btree_entry(&incident, &point, "creo segment profile incidence nodes")?;
            let indices = incident.entry(point).or_default();
            ctx.reserve_vec(indices, 1, "creo segment profile incidence rows")?;
            indices.push(index);
        }
    }
    let mut remaining = BTreeSet::new();
    for (index, _) in ctx.admit_iter(&rows, "creo segment profile remaining sources")?.enumerate() {
        ctx.insert_btree_set(
            &mut remaining,
            index,
            "creo segment profile remaining nodes",
        )?;
    }
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        ctx.charge_collection_items(1, "creo segment profile component nodes")?;
        ctx.charge_work(1, "creo segment profile components")?;
        let mut component = BTreeSet::from([seed]);
        let mut frontier = ctx.alloc_filled(1, seed, "creo segment profile frontier")?;
        while let Some(index) = frontier.pop() {
            ctx.charge_work(1, "creo segment profile frontier visits")?;
            for point in rows[index].point_ids() {
                for adjacent in
                    ctx.admit_iter(&incident[&point], "creo segment profile adjacent rows")?
                {
                    if ctx.insert_btree_set(
                        &mut component,
                        *adjacent,
                        "creo segment profile component nodes",
                    )? {
                        ctx.reserve_vec(&mut frontier, 1, "creo segment profile frontier")?;
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        remaining.retain(|index| !component.contains(index));
        let mut invalid_component_degree = false;
        'component_rows: for index in ctx
            .admit_iter(&component, "creo segment profile component rows")?
        {
            for point in rows[*index].point_ids() {
                let component_degree = ctx
                    .admit_iter(&incident[&point], "creo segment profile component degrees")?
                    .filter(|row| component.contains(row))
                    .count();
                if component_degree != 2 {
                    invalid_component_degree = true;
                    break 'component_rows;
                }
            }
        }
        if invalid_component_degree {
            continue;
        }
        let Some(first) = ctx
            .admit_iter(&component, "creo segment profile canonical row selection")?
            .min_by_key(|index| rows[**index].external_id)
            .copied()
        else {
            continue;
        };
        let mut point = rows[first].point_ids()[0].min(rows[first].point_ids()[1]);
        let start = point;
        let mut unused = component;
        let mut profile = Vec::new();
        while !unused.is_empty() {
            ctx.charge_work(1, "creo segment profile row visits")?;
            let mut candidates = ctx
                .admit_iter(&incident[&point], "creo segment profile candidates")?
                .filter(|index| unused.contains(index))
                .copied();
            let first_candidate = candidates.next();
            let second_candidate = candidates.next();
            let index = if profile.is_empty()
                && ctx
                    .admit_iter(&incident[&point], "creo segment profile cycle start")?
                    .any(|candidate| *candidate == first && unused.contains(candidate))
            {
                first
            } else if let (Some(index), None) = (first_candidate, second_candidate) {
                index
            } else {
                break;
            };
            let segment = rows[index];
            let traversal_reversed = segment.point_ids()[1] == point;
            if !traversal_reversed && segment.point_ids()[0] != point {
                break;
            }
            let analytic_reversed = matches!(
                segment.kind,
                crate::feature::definitions::FeatureSegmentKind::Arc(_)
            ) && segment.arc_orientation == Some(0);
            let Some(entity) = sketch_entity_id_admitted(ctx, sketch, segment.external_id)? else {
                continue;
            };
            ctx.reserve_vec(&mut profile, 1, "creo segment profile entity uses")?;
            profile.push(SketchEntityUse {
                entity,
                reversed: traversal_reversed ^ analytic_reversed,
            });
            point = if traversal_reversed {
                segment.point_ids()[0]
            } else {
                segment.point_ids()[1]
            };
            unused.remove(&index);
        }
        if unused.is_empty() && point == start {
            ctx.reserve_vec(&mut profiles, 1, "creo resolved segment profiles")?;
            profiles.push(profile);
        }
    }
    Ok(profiles)
}

pub(in super::super) fn solver_only_section_entities(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeMap<u32, usize>, cadmpeg_core::CodecError> {
    let mut entities = BTreeMap::<u32, usize>::new();
    // discarded-value: The visitor processes all solver SKAMP rows.
    let _ = visit_all_section_skamps::<()>(ctx, definition, |skamp| {
        for item in ctx.admit_iter(&skamp.items, "creo solver-only SKAMP items")? {
            let id = item.entity_id;
            let segment_id_exists = if let Some(table) = definition.segments.as_ref() {
                ctx.admit_iter(
                    table.rows.identity_entries(),
                    "creo solver-only segment identity rows",
                )?
                .any(|(&segment_id, _)| segment_id == id)
            } else {
                false
            };
            if segment_id_exists {
                continue;
            }
            if let Some(first_offset) = entities.get_mut(&id) {
                *first_offset = (*first_offset).min(skamp.offset);
            } else {
                ctx.insert_btree_map(
                    &mut entities,
                    id,
                    skamp.offset,
                    "creo solver-only entity nodes",
                )?;
            }
        }
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(entities)
}

pub(in super::super) fn solver_only_section_entity_offset(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let segment_id_exists = if let Some(table) = definition.segments.as_ref() {
        ctx.admit_iter(
            table.rows.identity_entries(),
            "creo solver-only entity identity rows",
        )?
        .any(|(&segment_id, _)| segment_id == entity_id)
    } else {
        false
    };
    if segment_id_exists {
        return Ok(None);
    }
    let mut first_offset = None;
    // discarded-value: The visitor finds the minimum offset across matching rows.
    let _ = visit_all_section_skamps::<()>(ctx, definition, |skamp| {
        if ctx
            .admit_iter(&skamp.items, "creo solver-only entity SKAMP items")?
            .any(|item| item.entity_id == entity_id)
        {
            first_offset = Some(first_offset.map_or(skamp.offset, |offset: usize| {
                offset.min(skamp.offset)
            }));
        }
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(first_offset)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in super::super) enum SectionEntityIncidenceFamily {
    Point,
    BoundedCurve,
    /// A bounded line or arc, as the target of a type-35 midpoint relation.
    LineOrArc,
    Line,
    Arc,
    Circular,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in super::super) struct IncidenceEvidence([bool; 6]);

impl IncidenceEvidence {
    fn slot(family: SectionEntityIncidenceFamily) -> usize {
        match family {
            SectionEntityIncidenceFamily::Point => 0,
            SectionEntityIncidenceFamily::BoundedCurve => 1,
            SectionEntityIncidenceFamily::LineOrArc => 2,
            SectionEntityIncidenceFamily::Line => 3,
            SectionEntityIncidenceFamily::Arc => 4,
            SectionEntityIncidenceFamily::Circular => 5,
        }
    }

    fn insert(&mut self, family: SectionEntityIncidenceFamily) {
        self.0[Self::slot(family)] = true;
    }

    fn remove(&mut self, family: SectionEntityIncidenceFamily) {
        self.0[Self::slot(family)] = false;
    }

    fn contains(self, family: SectionEntityIncidenceFamily) -> bool {
        self.0[Self::slot(family)]
    }

    pub(in super::super) fn len(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<usize, CodecError> {
        Ok(ctx
            .admit_iter(&self.0, "creo section incidence evidence")?
            .filter(|present| **present)
            .count())
    }

    fn iter(self) -> impl Iterator<Item = SectionEntityIncidenceFamily> {
        [
            SectionEntityIncidenceFamily::Point,
            SectionEntityIncidenceFamily::BoundedCurve,
            SectionEntityIncidenceFamily::LineOrArc,
            SectionEntityIncidenceFamily::Line,
            SectionEntityIncidenceFamily::Arc,
            SectionEntityIncidenceFamily::Circular,
        ]
        .into_iter()
        .filter(move |family| self.contains(*family))
    }
}

impl FromIterator<SectionEntityIncidenceFamily> for IncidenceEvidence {
    fn from_iter<T: IntoIterator<Item = SectionEntityIncidenceFamily>>(iter: T) -> Self {
        let mut evidence = Self::default();
        for family in iter {
            evidence.insert(family);
        }
        evidence
    }
}

fn section_skamp_has_proven_point_locus(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    item: &crate::feature::definitions::FeatureSkampItem,
) -> Result<bool, cadmpeg_core::CodecError> {
    if item.sense == 0 {
        if unique_point_segment(definition, item.entity_id).is_some() {
            return Ok(true);
        }
        if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
            return Ok(matches!(
                segment.kind,
                crate::feature::definitions::FeatureSegmentKind::Point(_)
            ) && !section_degenerate_axis_line(ctx, definition, segment)?);
        }
        return Ok(false);
    }
    let solver_family =
        section_incidence_curve_family_evidence_without_type35(ctx, definition, item.entity_id)?;
    if solver_family.len(ctx)? == 1
        && ((solver_family.contains(SectionEntityIncidenceFamily::BoundedCurve)
            || solver_family.contains(SectionEntityIncidenceFamily::Line)
            || solver_family.contains(SectionEntityIncidenceFamily::Arc))
            && matches!(item.sense, 2 | 3)
            || (solver_family.contains(SectionEntityIncidenceFamily::Arc)
                || solver_family.contains(SectionEntityIncidenceFamily::Circular))
                && matches!(item.sense, 2..=4))
    {
        return Ok(true);
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return Ok(matches!(
            (segment.kind, item.sense),
            (
                crate::feature::definitions::FeatureSegmentKind::Line(_),
                2 | 3
            ) | (
                crate::feature::definitions::FeatureSegmentKind::Arc(_),
                2..=4
            )
        ));
    }
    if unique_centered_line_segment(definition, item.entity_id).is_some() {
        return Ok(matches!(item.sense, 2..=4));
    }
    if let Some(segment) = unique_reference_line_segment(definition, item.entity_id) {
        return Ok(match item.sense {
            2 => segment.point_ids[0].is_some(),
            3 => segment.point_ids[1].is_some(),
            _ => false,
        });
    }
    if unique_bounded_curve_segment(definition, item.entity_id).is_some() {
        return Ok(matches!(item.sense, 2 | 3));
    }
    if unique_circle_segment(definition, item.entity_id).is_some() {
        return Ok(item.sense == 4);
    }
    if !saved_section_entity_fallback_allowed(definition, item.entity_id) {
        return Ok(false);
    }
    let Some(saved) = section_saved_entity(ctx, definition, item.entity_id)? else {
        return Ok(false);
    };
    Ok(matches!(
        (saved, item.sense),
        (crate::feature::definitions::FeatureSavedEntity::Line(_), 2 | 3)
            | (crate::feature::definitions::FeatureSavedEntity::Arc(_), 2..=4)
            | (
                crate::feature::definitions::FeatureSavedEntity::Circle(_)
                    | crate::feature::definitions::FeatureSavedEntity::Conic(_),
                4,
            )
    ))
}

fn section_incidence_curve_family_evidence(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
) -> Result<IncidenceEvidence, cadmpeg_core::CodecError> {
    section_incidence_curve_family_evidence_with_solver_roles(
        ctx,
        definition,
        entity_id,
        SolverRoles::Extended,
    )
}

fn section_incidence_curve_family_evidence_without_type35(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
) -> Result<IncidenceEvidence, cadmpeg_core::CodecError> {
    section_incidence_curve_family_evidence_with_solver_roles(
        ctx,
        definition,
        entity_id,
        SolverRoles::Strict,
    )
}

#[derive(Clone, Copy)]
enum SolverRoles {
    /// Unary, endpoint, center, line-pair and radius-equality roles.
    Strict,
    /// The strict roles and the type-zero point role.
    WithoutType35Target,
    /// The strict roles, the type-zero point role and the type-35 target
    /// line-or-arc role.
    Extended,
}

fn section_incidence_curve_family_evidence_with_solver_roles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
    solver_roles: SolverRoles,
) -> Result<IncidenceEvidence, cadmpeg_core::CodecError> {
    let mut evidence = IncidenceEvidence::default();
    let outcome = visit_section_skamps(ctx, definition, false, |skamp| {
        Ok(if matches!(
            (skamp.kind, skamp.items.as_slice()),
            (1 | 2, [item]) if item.sense == 0 && item.entity_id == entity_id
        ) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        })
    })?;
    if matches!(outcome, ControlFlow::Break(())) {
        evidence.insert(SectionEntityIncidenceFamily::Line);
    }
    // discarded-value: The visitor gathers all section incidence evidence.
    let _ = visit_section_skamps::<()>(ctx, definition, false, |skamp| {
        for item in ctx.admit_iter(&skamp.items, "creo section incidence SKAMP items")? {
            if item.entity_id == entity_id && matches!(item.sense, 2 | 3) {
                evidence.insert(SectionEntityIncidenceFamily::BoundedCurve);
            }
            if item.entity_id == entity_id && item.sense == 4 {
                evidence.insert(SectionEntityIncidenceFamily::Circular);
            }
        }
        if matches!(solver_roles, SolverRoles::Extended) {
            if let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) {
                let roles = [(first, second), (second, first)];
                for (target, point) in ctx.admit_iter(&roles, "creo type-35 incidence roles")? {
                    if target.entity_id != entity_id || target.sense != 0 {
                        continue;
                    }
                    if point.sense == 4
                        && unique_centered_line_segment(definition, point.entity_id).is_some()
                    {
                        continue;
                    }
                    if !section_skamp_has_proven_point_locus(ctx, definition, point)? {
                        continue;
                    }
                    if unique_opaque_section_entity(definition, target.entity_id)
                        || solver_only_section_entity_offset(ctx, definition, target.entity_id)?
                            .is_some()
                    {
                        evidence.insert(SectionEntityIncidenceFamily::LineOrArc);
                        break;
                    }
                }
            }
        }
        if matches!(
            solver_roles,
            SolverRoles::WithoutType35Target | SolverRoles::Extended
        ) {
            if let (0, [first, second]) = (skamp.kind, skamp.items.as_slice()) {
                let roles = [(first, second), (second, first)];
                for (target, point) in ctx.admit_iter(&roles, "creo point incidence roles")? {
                    if target.entity_id != entity_id || target.sense != 0 {
                        continue;
                    }
                    if !section_skamp_has_proven_point_locus(ctx, definition, point)? {
                        continue;
                    }
                    if unique_opaque_section_entity(definition, target.entity_id)
                        || solver_only_section_entity_offset(ctx, definition, target.entity_id)?
                            .is_some()
                    {
                        evidence.insert(SectionEntityIncidenceFamily::Point);
                        break;
                    }
                }
            }
        }
        // Line-family roles are structural; type-six circular evidence is
        // activity-dependent, like its radius-equality constraint.
        if let (5 | 7 | 8, [first, second]) = (skamp.kind, skamp.items.as_slice()) {
            if first.sense == 0 && second.sense == 0 {
                let has_solver_entity =
                    solver_only_section_entity_offset(ctx, definition, entity_id)?.is_some();
                if has_solver_entity
                    && (first.entity_id == entity_id || second.entity_id == entity_id)
                {
                    evidence.insert(SectionEntityIncidenceFamily::Line);
                }
            }
        }
        if section_skamp_active(skamp.status)
            && matches!((skamp.kind, skamp.items.as_slice()), (6, [first, second])
                if first.sense == 0
                    && second.sense == 0
                    && (first.entity_id == entity_id || second.entity_id == entity_id))
        {
            evidence.insert(SectionEntityIncidenceFamily::Circular);
        }
        Ok(ControlFlow::Continue(()))
    })?;
    normalize_section_incidence_curve_family_evidence(&mut evidence);
    Ok(evidence)
}

fn unique_opaque_section_entity(
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
) -> bool {
    definition
        .segments
        .as_ref()
        .is_some_and(|segments| matches!(segments.rows.get(entity_id), Some(SegmentRow::Opaque(_))))
}

pub(in super::super) fn unique_section_incidence_curve_family(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
) -> Result<Option<SectionEntityIncidenceFamily>, cadmpeg_core::CodecError> {
    Ok(exactly_one(
        section_incidence_curve_family_evidence(ctx, definition, entity_id)?.iter(),
    ))
}

/// The unique incidence family of an entity when its sense-zero type-35
/// target roles supply no evidence.
pub(in super::super) fn unique_section_incidence_curve_family_without_type35_target(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
) -> Result<Option<SectionEntityIncidenceFamily>, cadmpeg_core::CodecError> {
    Ok(exactly_one(
        section_incidence_curve_family_evidence_with_solver_roles(
            ctx,
            definition,
            entity_id,
            SolverRoles::WithoutType35Target,
        )?
        .iter(),
    ))
}

/// Narrow the endpoint-bearing families. Line evidence narrows a bounded
/// curve or a line-or-arc to a line. Circular evidence narrows them to an arc.
/// A line-or-arc narrows a bounded curve.
pub(in super::super) fn normalize_section_incidence_curve_family_evidence(
    evidence: &mut IncidenceEvidence,
) {
    if evidence.contains(SectionEntityIncidenceFamily::Line) {
        evidence.remove(SectionEntityIncidenceFamily::BoundedCurve);
        evidence.remove(SectionEntityIncidenceFamily::LineOrArc);
    } else if evidence.contains(SectionEntityIncidenceFamily::Circular)
        && (evidence.contains(SectionEntityIncidenceFamily::BoundedCurve)
            || evidence.contains(SectionEntityIncidenceFamily::LineOrArc))
    {
        evidence.remove(SectionEntityIncidenceFamily::BoundedCurve);
        evidence.remove(SectionEntityIncidenceFamily::LineOrArc);
        evidence.remove(SectionEntityIncidenceFamily::Circular);
        evidence.insert(SectionEntityIncidenceFamily::Arc);
    } else if evidence.contains(SectionEntityIncidenceFamily::LineOrArc) {
        evidence.remove(SectionEntityIncidenceFamily::BoundedCurve);
    }
}

pub(in super::super) fn solver_only_section_entity_family(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity_id: u32,
) -> Result<Option<SectionEntityIncidenceFamily>, cadmpeg_core::CodecError> {
    if solver_only_section_entity_offset(ctx, definition, entity_id)?.is_none() {
        return Ok(None);
    }
    let mut evidence = section_incidence_curve_family_evidence(ctx, definition, entity_id)?;
    if !evidence.contains(SectionEntityIncidenceFamily::Arc) {
        let outcome = visit_section_skamps(ctx, definition, false, |skamp| {
            let has_circular_sense = ctx
                .admit_iter(&skamp.items, "creo solver-only circular SKAMP items")?
                .any(|item| item.entity_id == entity_id && item.sense == 4);
            Ok(if has_circular_sense {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            })
        })?;
        if matches!(outcome, ControlFlow::Break(())) {
        evidence.insert(SectionEntityIncidenceFamily::Circular);
        normalize_section_incidence_curve_family_evidence(&mut evidence);
        }
    }
    if !evidence.contains(SectionEntityIncidenceFamily::Line)
        && !evidence.contains(SectionEntityIncidenceFamily::LineOrArc)
    {
        let outcome = visit_section_skamps(ctx, definition, false, |skamp| {
            let has_centered_line_target =
                if let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) {
                    let roles = [(first, second), (second, first)];
                    ctx.admit_iter(&roles, "creo centered-line target roles")?
                        .any(|(point, target)| {
                            point.entity_id == entity_id
                                && point.sense == 0
                                && target.sense == 4
                                && unique_centered_line_segment(definition, target.entity_id)
                                    .is_some()
                        })
                } else {
                    false
                };
            Ok(if has_centered_line_target {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            })
        })?;
        if matches!(outcome, ControlFlow::Break(())) {
            evidence.insert(SectionEntityIncidenceFamily::Point);
        }
    }
    if !evidence.contains(SectionEntityIncidenceFamily::Point) {
        let solver_only_point_from_midpoint = matches!(
            visit_section_skamps(ctx, definition, false, |skamp| {
                let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
                    return Ok(ControlFlow::Continue(()));
                };
                let mut has_one_match = false;
                let mut has_multiple_matches = false;
                let roles = [(first, second), (second, first)];
                for (point, target) in ctx.admit_iter(&roles, "creo midpoint role pairs")? {
                    let matches_target = point.sense == 0
                        && point.entity_id == entity_id
                        && target.sense == 0
                        && (unique_decoded_section_segment(definition, target.entity_id)
                            .is_some_and(|segment| {
                                matches!(
                                    segment.kind,
                                    crate::feature::definitions::FeatureSegmentKind::Line(_)
                                        | crate::feature::definitions::FeatureSegmentKind::Arc(_)
                                )
                            })
                            || (saved_section_entity_fallback_allowed(
                                definition,
                                target.entity_id,
                            ) && matches!(
                                section_saved_entity(ctx, definition, target.entity_id)?,
                                Some(
                                    crate::feature::definitions::FeatureSavedEntity::Line(_)
                                        | crate::feature::definitions::FeatureSavedEntity::Arc(_)
                                )
                            )));
                    if matches_target {
                        if has_one_match {
                            has_multiple_matches = true;
                        } else {
                            has_one_match = true;
                        }
                    }
                }
                Ok(if has_one_match && !has_multiple_matches {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                })
            })?,
            ControlFlow::Break(())
        );
        if solver_only_point_from_midpoint {
            evidence.insert(SectionEntityIncidenceFamily::Point);
        }
    }
    let mut evidence = evidence.iter();
    let Some(family) = evidence.next() else {
        return Ok(None);
    };
    Ok(evidence.next().is_none().then_some(family))
}

#[cfg(test)]
mod tests {
    use super::{
        resolved_profile_chains, solver_only_section_entities, solver_only_section_entity_family,
        unique_section_incidence_curve_family, SectionEntityIncidenceFamily,
    };
    use crate::decode::tests::opaque;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

    fn single_trim_profile() -> crate::feature::definitions::FeatureDefinition {
        let mut definition = definition(201, false);
        definition.segments = None;
        definition.trim_entities = Some(crate::feature::definitions::FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![crate::feature::definitions::FeatureTrimEntity {
                external_id: 42,
                mode: None,
                vertices: [1, 2],
                kind: crate::feature::definitions::TrimEntityKind::Line,
                offset: 42,
            }],
            solved_external_ids: vec![42],
            offset: 0,
        });
        definition
    }

    #[test]
    fn trim_profile_rows_refuse_before_growth() {
        let definition = single_trim_profile();
        let sketch =
            cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#917").expect("sketch ID");
        let emitted = std::collections::BTreeSet::from([42]);
        let arena = DecodeArena::new();
        let operations = [
            "creo trim profile rows",
            "creo trim profile incidence nodes",
            "creo trim profile incidence rows",
            "creo trim profile incidence nodes",
            "creo trim profile incidence rows",
            "creo trim profile remaining nodes",
            "creo trim profile component nodes",
            "creo trim profile frontier",
            "creo trim profile entity uses",
            "creo resolved trim profiles",
        ];
        for (cap, operation) in operations.into_iter().enumerate() {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cadmpeg_core::decode::u64_from_index(cap);
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let error = resolved_profile_chains(&ctx, &definition, &sketch, &emitted)
                .expect_err("profile needs the next collection item");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == operation),
                "cap {cap}: {error}"
            );
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo sketch entity identity"),
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = policy;
                trial_policy.limits.max_retained_bytes = cap;
                let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                resolved_profile_chains(&trial_ctx, &definition, &sketch, &emitted)
            },
        );
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = resolved_profile_chains(&ctx, &definition, &sketch, &emitted)
            .expect_err("profile entity ID exceeds retained cap");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo sketch entity identity")
        );
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        let profiles = resolved_profile_chains(&ctx, &definition, &sketch, &emitted)
            .expect("service trim profile");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].len(), 1);
        assert_eq!(
            profiles[0][0].entity.as_str(),
            "creo:featdefs:sketch_entity#917:42"
        );
    }

    #[test]
    fn trim_profile_scans_refuse_before_component_frontier_and_path_visits() {
        let definition = single_trim_profile();
        let sketch =
            cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#917").expect("sketch ID");
        let emitted = std::collections::BTreeSet::from([42]);
        let profiles = crate::test_support::assert_work_boundaries(
            &[
                "creo trim profile components",
                "creo trim profile frontier visits",
                "creo trim profile row visits",
            ],
            |ctx| resolved_profile_chains(ctx, &definition, &sketch, &emitted),
        );
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].len(), 1);
    }

    #[test]
    fn segment_profile_scans_refuse_before_component_frontier_and_path_visits() {
        let mut definition = definition(201, false);
        let segment = |external_id| crate::feature::definitions::FeatureSegment {
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
        definition.segments = Some(crate::feature::definitions::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: [segment(10), segment(11)]
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .collect(),
            offset: 0,
        });
        let sketch =
            cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#917").expect("sketch ID");
        let emitted = std::collections::BTreeSet::from([10, 11]);
        let profiles = crate::test_support::assert_work_boundaries(
            &[
                "creo segment profile components",
                "creo segment profile frontier visits",
                "creo segment profile row visits",
            ],
            |ctx| resolved_profile_chains(ctx, &definition, &sketch, &emitted),
        );
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].len(), 2);
    }

    #[test]
    fn segment_profile_rows_refuse_before_growth() {
        let mut definition = definition(201, false);
        let segment = |external_id| crate::feature::definitions::FeatureSegment {
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
        definition.segments = Some(crate::feature::definitions::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: [segment(10), segment(11)]
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .collect(),
            offset: 0,
        });
        let sketch =
            cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#917").expect("sketch ID");
        let emitted = std::collections::BTreeSet::from([10, 11]);
        let arena = DecodeArena::new();
        let operations = [
            "creo segment profile rows",
            "creo segment profile rows",
            "creo segment profile incidence nodes",
            "creo segment profile incidence rows",
            "creo segment profile incidence nodes",
            "creo segment profile incidence rows",
            "creo segment profile incidence rows",
            "creo segment profile incidence rows",
            "creo segment profile remaining nodes",
            "creo segment profile remaining nodes",
            "creo segment profile component nodes",
            "creo segment profile frontier",
            "creo segment profile component nodes",
            "creo segment profile frontier",
            "creo segment profile entity uses",
            "creo segment profile entity uses",
            "creo resolved segment profiles",
        ];
        for (cap, operation) in operations.into_iter().enumerate() {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cadmpeg_core::decode::u64_from_index(cap);
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let error = resolved_profile_chains(&ctx, &definition, &sketch, &emitted)
                .expect_err("segment profile needs the next collection item");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == operation),
                "cap {cap}: {error}"
            );
        }
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        let profiles = resolved_profile_chains(&ctx, &definition, &sketch, &emitted)
            .expect("service segment profile");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].len(), 2);
        assert_eq!(
            profiles[0][0].entity.as_str(),
            "creo:featdefs:sketch_entity#917:10"
        );
    }

    #[test]
    fn solver_only_entity_nodes_refuse_before_insertion() {
        let definition = definition(201, false);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = solver_only_section_entities(&ctx, &definition)
            .expect_err("one solver-only node exceeds zero items");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo solver-only entity nodes")
        );
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        assert_eq!(
            solver_only_section_entities(&ctx, &definition)
                .expect("service entities")
                .get(&201),
            Some(&201)
        );
    }

    fn midpoint(target: u32, point: u32) -> crate::feature::definitions::FeatureSkamp {
        crate::feature::definitions::FeatureSkamp {
            id: target,
            kind: 35,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: target,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: point,
                    sense: 0,
                },
            ],
            offset: usize::try_from(target).expect("fixture index fits usize"),
        }
    }

    #[test]
    fn midpoint_point_evidence_accepts_one_role_in_each_of_two_rows() {
        let mut definition = definition(201, false);
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
            offset: usize::try_from(external_id).expect("fixture index"),
        };
        let segments = definition.segments.as_mut().expect("segments");
        segments
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(line(30)));
        segments
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(line(31)));
        segments.declared_count = 3;
        let relations = definition.relations.as_mut().expect("relations");
        let skamps = crate::decode::tests::declared_solver_rows(&mut relations.skamps);
        skamps[0].id = 200;
        skamps[0].items[1].entity_id = 30;
        skamps.push(midpoint(201, 31));
        crate::decode::tests::synchronize_skamp_count(&mut definition);

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| {
                solver_only_section_entity_family(ctx, &definition, 201)
            })
            .expect("admitted midpoint role rows"),
            Some(SectionEntityIncidenceFamily::Point)
        );
    }

    fn definition(
        target: u32,
        opaque_target: bool,
    ) -> crate::feature::definitions::FeatureDefinition {
        let opaque_rows: Vec<crate::feature::definitions::FeatureOpaqueSegment> =
            opaque_target.then(|| opaque(target)).into_iter().collect();
        let point_rows = vec![crate::feature::definitions::FeaturePointSegment {
            point_id: 7,
            external_id: 7,
            offset: 7,
        }];
        crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: u32::try_from(opaque_rows.len() + point_rows.len())
                    .expect("segment count"),
                has_elided_prototype: false,
                entity_ref: None,
                rows: (point_rows)
                    .into_iter()
                    .map(crate::feature::segment_rows::SegmentRow::Point)
                    .chain(
                        (opaque_rows)
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
                    rows: vec![midpoint(target, 7)],
                }),
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        }
    }

    #[test]
    fn type35_point_locus_establishes_unique_native_line_or_arc_family() {
        let opaque_target = definition(101, true);
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| unique_section_incidence_curve_family(ctx, &opaque_target, 101)).expect("admitted incidence family rows"),
            Some(SectionEntityIncidenceFamily::LineOrArc)
        );

        let solver_only_target = definition(201, false);
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| unique_section_incidence_curve_family(ctx, &solver_only_target, 201)).expect("admitted incidence family rows"),
            Some(SectionEntityIncidenceFamily::LineOrArc)
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| solver_only_section_entity_family(ctx, &solver_only_target, 201)).expect("admitted solver entity roles"),
            Some(SectionEntityIncidenceFamily::LineOrArc)
        );
    }

    /// The type-35 fixture with one further incidence of `kind` whose items
    /// select `target` with `sense` and, for a two-item kind, the point 7.
    fn with_target_role(
        target: u32,
        opaque_target: bool,
        kind: u32,
        sense: u32,
    ) -> crate::feature::definitions::FeatureDefinition {
        let mut definition = definition(target, opaque_target);
        let mut items = vec![crate::feature::definitions::FeatureSkampItem {
            entity_id: target,
            sense,
        }];
        if kind == 0 {
            items.push(crate::feature::definitions::FeatureSkampItem {
                entity_id: 7,
                sense: 0,
            });
        }
        let relations = definition.relations.as_mut().expect("relations");
        crate::decode::tests::declared_solver_rows(&mut relations.skamps).push(
            crate::feature::definitions::FeatureSkamp {
                id: target + 1,
                kind,
                flags: 0,
                status: 0,
                items,
                offset: usize::try_from(target).expect("fixture index fits usize") + 1,
            },
        );
        crate::decode::tests::synchronize_skamp_count(&mut definition);
        definition
    }

    /// The unique family of `target` in the opaque-row and solver-only forms.
    fn target_families(
        ctx: &DecodeContext<'_>,
        kind: u32,
        sense: u32,
    ) -> Result<[Option<SectionEntityIncidenceFamily>; 3], CodecError> {
        let opaque_target = with_target_role(101, true, kind, sense);
        let solver_only_target = with_target_role(201, false, kind, sense);
        Ok([
            unique_section_incidence_curve_family(ctx, &opaque_target, 101)?,
            unique_section_incidence_curve_family(ctx, &solver_only_target, 201)?,
            solver_only_section_entity_family(ctx, &solver_only_target, 201)?,
        ])
    }

    #[test]
    fn an_endpoint_role_keeps_the_type35_line_or_arc_family() {
        // An inactive type-0 incidence selects the first endpoint of the target.
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| target_families(ctx, 0, 2))
                .expect("admitted endpoint role rows"),
            [Some(SectionEntityIncidenceFamily::LineOrArc); 3]
        );
    }

    #[test]
    fn a_unary_line_role_narrows_the_type35_family_to_line() {
        // An inactive unary horizontal incidence on the target.
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| target_families(ctx, 1, 0))
                .expect("admitted unary role rows"),
            [Some(SectionEntityIncidenceFamily::Line); 3]
        );
    }

    #[test]
    fn a_center_role_narrows_the_type35_family_to_arc() {
        // An inactive type-0 incidence selects the center of the target.
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| target_families(ctx, 0, 4))
                .expect("admitted center role rows"),
            [Some(SectionEntityIncidenceFamily::Arc); 3]
        );
    }

    #[test]
    fn a_type35_target_is_a_curve_entity_and_not_a_line() {
        let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#917".to_string())
            .expect("valid test fixture");
        for (definition, target) in [(definition(101, true), 101), (definition(201, false), 201)] {
            let item = crate::feature::definitions::FeatureSkampItem {
                entity_id: target,
                sense: 0,
            };
            assert!(!crate::decode::with_test_decode_ctx(|ctx| {
                super::super::loci::section_skamp_is_line(ctx, &definition, &item)
            })
            .expect("admitted SKAMP line rows"));
            assert!(super::super::loci::with_test_locus(|ctx, refusal| {
                super::super::loci::section_skamp_curve_entity(
                    ctx,
                    refusal,
                    &definition,
                    &sketch,
                    &item,
                )
            })
            .expect("test section curve entity resources")
            .is_some());
        }
    }

    #[test]
    fn type35_line_family_requires_unique_native_target() {
        let mut definition = definition(101, true);
        let segments = definition.segments.as_mut().expect("segments");
        segments
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Opaque(opaque(
                101,
            )));
        segments.declared_count = 2;
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| unique_section_incidence_curve_family(ctx, &definition, 101)).expect("admitted incidence family rows"),
            None
        );
    }

    #[test]
    fn type_zero_point_locus_establishes_unique_native_point_family() {
        let mut opaque_target = definition(101, true);
        opaque_target
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("skamp table")
            .rows_mut()[0]
            .kind = 0;
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| unique_section_incidence_curve_family(ctx, &opaque_target, 101)).expect("admitted incidence family rows"),
            Some(SectionEntityIncidenceFamily::Point)
        );

        let mut solver_only_target = definition(201, false);
        solver_only_target
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("skamp table")
            .rows_mut()[0]
            .kind = 0;
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| unique_section_incidence_curve_family(ctx, &solver_only_target, 201)).expect("admitted incidence family rows"),
            Some(SectionEntityIncidenceFamily::Point)
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| solver_only_section_entity_family(ctx, &solver_only_target, 201)).expect("admitted solver entity roles"),
            Some(SectionEntityIncidenceFamily::Point)
        );
    }

    #[test]
    fn type_zero_point_family_requires_unique_native_target() {
        let mut definition = definition(101, true);
        let segments = definition.segments.as_mut().expect("segments");
        segments
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Opaque(opaque(
                101,
            )));
        segments.declared_count += 1;
        definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("skamp table")
            .rows_mut()[0]
            .kind = 0;
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| unique_section_incidence_curve_family(ctx, &definition, 101)).expect("admitted incidence family rows"),
            None
        );
    }

    #[test]
    fn center_role_normalizes_bounded_curve_solver_family_to_arc() {
        let mut definition = definition(201, false);
        definition.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Circle(
                crate::feature::definitions::FeatureCircleSegment {
                    center_id: 0,
                    radius_ref: 1,
                    external_id: 22,
                    offset: 22,
                },
            ),
        );
        definition
            .segments
            .as_mut()
            .expect("segments")
            .declared_count += 1;
        let relations = definition.relations.as_mut().expect("relations");
        crate::decode::tests::declared_solver_rows(&mut relations.skamps).extend([
            crate::feature::definitions::FeatureSkamp {
                id: 0,
                kind: 0,
                flags: 0,
                status: 35,
                items: vec![
                    crate::feature::definitions::FeatureSkampItem {
                        entity_id: 21,
                        sense: 4,
                    },
                    crate::feature::definitions::FeatureSkampItem {
                        entity_id: 22,
                        sense: 4,
                    },
                ],
                offset: 23,
            },
            crate::feature::definitions::FeatureSkamp {
                id: 1,
                kind: 3,
                flags: 0,
                status: 34,
                items: vec![
                    crate::feature::definitions::FeatureSkampItem {
                        entity_id: 22,
                        sense: 0,
                    },
                    crate::feature::definitions::FeatureSkampItem {
                        entity_id: 21,
                        sense: 2,
                    },
                ],
                offset: 24,
            },
        ]);
        crate::decode::tests::synchronize_skamp_count(&mut definition);

        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| solver_only_section_entity_family(ctx, &definition, 21)).expect("admitted solver entity roles"),
            Some(SectionEntityIncidenceFamily::Arc)
        );
    }
}
