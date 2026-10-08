//! Endpoint relations derived from resolved zero-entity support occurrences.

use cadmpeg_core::convert::truncate_f64_to_i64;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use serde::{Deserialize, Serialize};

use super::records::ZeroEntitySupportRun;
use crate::solve::union_find::UnionFind;

const MODEL_POINT_TOLERANCE: f64 = 2e-3;
pub(super) const MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS: usize = 1_000_000;

/// One sense-oriented support occurrence owned by a face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ZeroEntityOrientedOccurrence {
    pub(crate) face_record_ordinal: u32,
    pub(crate) support_record_ordinal: u32,
    pub(crate) model_endpoints: [FinitePoint3; 2],
    pub(crate) model_midpoint: FinitePoint3,
}

/// Two radial occurrences with matching bounded model-space witnesses.
///
/// This relation does not establish curve coincidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ZeroEntityEndpointPairCandidate {
    pub(crate) face_record_ordinals: [u32; 2],
    pub(crate) support_record_ordinals: [u32; 2],
    pub(crate) model_endpoints: [FinitePoint3; 2],
    pub(crate) model_midpoint: FinitePoint3,
}

/// Start or end of an oriented endpoint pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum EdgeEnd {
    /// First oriented endpoint.
    Start,
    /// Second oriented endpoint.
    End,
}

impl From<EdgeEnd> for u8 {
    fn from(value: EdgeEnd) -> Self {
        match value {
            EdgeEnd::Start => 0,
            EdgeEnd::End => 1,
        }
    }
}

impl TryFrom<u8> for EdgeEnd {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Start),
            1 => Ok(Self::End),
            other => Err(format!("endpoint_index {other} is not start or end")),
        }
    }
}

/// Ordinal of an enumerated endpoint-pair candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EndpointPairIndex(usize);

impl EndpointPairIndex {
    pub(crate) fn ordinal(self) -> usize {
        self.0
    }
}

/// One geometric endpoint-locus candidate established by a complete endpoint clique.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ZeroEntityEndpointLocusCandidate {
    pub(crate) incident_endpoint_pair_endpoints: Vec<(EndpointPairIndex, EdgeEnd)>,
    pub(crate) representative_point: FinitePoint3,
    pub(crate) maximum_deviation: f64,
}

pub(crate) fn zero_entity_endpoint_pair_candidates(
    ctx: &DecodeContext<'_>,
    runs: &[ZeroEntitySupportRun],
) -> Result<Vec<ZeroEntityEndpointPairCandidate>, CodecError> {
    let (occurrences, _occurrence_storage) = ctx
        .with_scoped_storage("catia_zero_occurrences", || {
            zero_entity_oriented_occurrences(ctx, runs)
        })?;
    endpoint_pair_candidates(ctx, &occurrences)
}

pub(super) fn zero_entity_endpoint_pair_candidates_with_budget(
    ctx: &DecodeContext<'_>,
    runs: &[ZeroEntitySupportRun],
    budget: &WorkBudget<'_>,
) -> Result<Option<Vec<ZeroEntityEndpointPairCandidate>>, CodecError> {
    let (occurrences, _occurrence_storage) = ctx
        .with_scoped_storage("catia_zero_occurrences", || {
            zero_entity_oriented_occurrences(ctx, runs)
        })?;
    endpoint_pair_candidates_with_budget(ctx, &occurrences, budget)
}

fn zero_entity_oriented_occurrences(
    ctx: &DecodeContext<'_>,
    runs: &[ZeroEntitySupportRun],
) -> Result<Vec<ZeroEntityOrientedOccurrence>, CodecError> {
    let mut occurrences = Vec::new();
    for run in ctx.admit_iter(runs, "catia_zero_occurrence_run_visits")? {
        let Some(face) = run.face.as_ref() else {
            continue;
        };
        let (midpoints, _midpoint_storage) =
            ctx.with_scoped_storage("catia_zero_midpoints", || {
                let mut midpoints = HashMap::new();
                for support in
                    ctx.admit_iter(&run.supports, "catia_zero_occurrence_support_visits")?
                {
                    if let Some(midpoint) = support.model_midpoint {
                        ctx.insert_hash_map(
                            &mut midpoints,
                            support.record_ordinal,
                            midpoint,
                            "catia_zero_midpoints",
                        )?;
                    }
                }
                Ok::<_, CodecError>(midpoints)
            })?;
        for loop_record in ctx.admit_iter(
            match face.loops.as_deref() {
                Some(loops) => loops,
                None => &[],
            },
            "catia_zero_occurrence_loop_visits",
        )? {
            for (support_record_ordinal, model_endpoints) in ctx
                .admit_iter(
                    &loop_record.support_record_ordinals,
                    "catia_zero_occurrence_ordinal_visits",
                )?
                .copied()
                .zip(loop_record.oriented_model_endpoints.iter().copied())
            {
                let Some(model_midpoint) = ctx
                    .get_hash_map(
                        &midpoints,
                        &support_record_ordinal,
                        "catia_zero_midpoint_lookup",
                    )?
                    .copied()
                else {
                    continue;
                };
                ctx.push_vec(
                    &mut occurrences,
                    ZeroEntityOrientedOccurrence {
                        face_record_ordinal: face.record_ordinal,
                        support_record_ordinal,
                        model_endpoints,
                        model_midpoint,
                    },
                    "catia_zero_occurrences",
                )?;
            }
        }
    }
    Ok(occurrences)
}

/// Refuse a local search ceiling without replacing a session refusal.
fn charge_endpoint_work(
    ctx: &DecodeContext<'_>,
    budget: &WorkBudget<'_>,
) -> Result<(), CodecError> {
    if budget.charge() {
        return Ok(());
    }
    if let Some(limit) = ctx.resource_refusal() {
        return Err(limit.into());
    }
    let limit = cadmpeg_core::decode::u64_from_index(budget.consumed());
    let requested = limit.checked_add(1).ok_or_else(|| {
        ctx.refuse_codec_limit("catia_zero_endpoint_work", u64::MAX - 1, u64::MAX)
    })?;
    Err(ctx.refuse_codec_limit("catia_zero_endpoint_work", limit, requested))
}

pub(crate) fn endpoint_pair_candidates(
    ctx: &DecodeContext<'_>,
    occurrences: &[ZeroEntityOrientedOccurrence],
) -> Result<Vec<ZeroEntityEndpointPairCandidate>, CodecError> {
    let budget = ctx.work_budget(ctx.policy().limits.max_work_units);
    Ok(endpoint_pair_candidates_with_budget(ctx, occurrences, &budget)?.unwrap_or_default())
}

fn endpoint_pair_candidates_with_budget(
    ctx: &DecodeContext<'_>,
    occurrences: &[ZeroEntityOrientedOccurrence],
    budget: &WorkBudget<'_>,
) -> Result<Option<Vec<ZeroEntityEndpointPairCandidate>>, CodecError> {
    // The match graphs, face indexes and traversal state are scratch; only
    // the candidates are kept.
    let mut scratch = ctx.reserve_scoped(0, "catia_zero_pair_workspace")?;
    let Some(endpoint_matches) =
        scratch.with_storage(|| endpoint_match_graph(ctx, occurrences, budget))?
    else {
        return Ok(None);
    };
    let radial_matches =
        scratch.with_storage(|| selected_radial_matches(ctx, occurrences, &endpoint_matches))?;
    // Each distinct face ordinal gets the next union-find index.
    let face_indices = scratch.with_storage(|| {
        let mut face_indices = HashMap::new();
        for occurrence in ctx.admit_iter(occurrences, "catia_zero_pair_occurrence_visits")? {
            let next = face_indices.len();
            ctx.entry_hash_map(
                &mut face_indices,
                occurrence.face_record_ordinal,
                "catia_zero_face_indices",
            )?
            .or_insert(next);
        }
        Ok::<_, CodecError>(face_indices)
    })?;
    let face_index = |occurrence: usize| -> Result<usize, CodecError> {
        ctx.get_hash_map(
            &face_indices,
            &occurrences[occurrence].face_record_ordinal,
            "catia_zero_face_indices",
        )?
        .copied()
        .ok_or_else(|| ctx.refuse_codec_limit("catia_zero_face_indices", 0, 1))
    };
    let mut face_components = scratch
        .with_storage(|| UnionFind::charged(ctx, face_indices.len(), "catia_zero_face_union"))?;
    for (index, neighbors) in ctx
        .admit_iter(&radial_matches, "catia_zero_pair_radial_visits")?
        .enumerate()
    {
        let [neighbor] = neighbors.as_slice() else {
            continue;
        };
        if !matches!(radial_matches[*neighbor].as_slice(), [only] if *only == index) {
            continue;
        }
        face_components.union(ctx, face_index(index)?, face_index(*neighbor)?)?;
    }

    let mut candidates = Vec::new();
    // Face components can contain several distinct edges with the same
    // endpoint locus. A reciprocal singleton radial match is still a
    // complete geometric relation and must not be merged with its siblings.
    for (index, neighbors) in ctx
        .admit_iter(&radial_matches, "catia_zero_pair_radial_visits")?
        .enumerate()
    {
        let [neighbor] = neighbors.as_slice() else {
            continue;
        };
        if index >= *neighbor
            || !matches!(radial_matches[*neighbor].as_slice(), [only] if *only == index)
        {
            continue;
        }
        let [first, second] = [occurrences[index], occurrences[*neighbor]];
        ctx.push_vec(
            &mut candidates,
            ZeroEntityEndpointPairCandidate {
                face_record_ordinals: [first.face_record_ordinal, second.face_record_ordinal],
                support_record_ordinals: [
                    first.support_record_ordinal,
                    second.support_record_ordinal,
                ],
                model_endpoints: first.model_endpoints,
                model_midpoint: first.model_midpoint,
            },
            "catia_zero_endpoint_pairs",
        )?;
    }

    let mut visited = scratch
        .with_storage(|| ctx.alloc_filled(occurrences.len(), false, "catia_zero_pair_visited"))?;
    for (start, _) in ctx
        .admit_iter(occurrences, "catia_zero_pair_roots")?
        .enumerate()
    {
        if visited[start] {
            continue;
        }
        let (by_face_component, _component_storage) =
            ctx.with_scoped_storage("catia_zero_pair_component_workspace", || {
                let mut stack = Vec::new();
                let mut stack_storage = ctx.reserve_scoped(0, "catia_zero_pair_stack")?;
                stack_storage
                    .with_storage(|| ctx.push_vec(&mut stack, start, "catia_zero_pair_stack"))?;
                visited[start] = true;
                let mut by_face_component = BTreeMap::<usize, Vec<usize>>::new();
                while let Some(index) = stack.pop() {
                    let component = face_components.find(ctx, face_index(index)?)?;
                    ctx.push_btree_group(
                        &mut by_face_component,
                        component,
                        index,
                        "catia_zero_pair_face_components",
                        "catia_zero_pair_face_members",
                    )?;
                    for neighbor in
                        ctx.admit_iter(&endpoint_matches[index], "catia_zero_pair_neighbor_visits")?
                    {
                        if !visited[*neighbor] {
                            visited[*neighbor] = true;
                            stack_storage.with_storage(|| {
                                ctx.push_vec(&mut stack, *neighbor, "catia_zero_pair_stack")
                            })?;
                        }
                    }
                }
                Ok::<_, CodecError>(by_face_component)
            })?;
        for (_, pair) in ctx.admit_iter(&by_face_component, "catia_zero_pair_component_visits")? {
            let &[left, right] = pair.as_slice() else {
                continue;
            };
            if !ctx.contains(
                &endpoint_matches[left],
                &right,
                "catia_zero_pair_endpoint_match",
            )? {
                continue;
            }
            if radial_matches[left].len() == 1 && radial_matches[right].len() == 1 {
                continue;
            }
            let pair = if occurrences[right].support_record_ordinal
                < occurrences[left].support_record_ordinal
            {
                [right, left]
            } else {
                [left, right]
            };
            let [first, second] = [occurrences[pair[0]], occurrences[pair[1]]];
            ctx.push_vec(
                &mut candidates,
                ZeroEntityEndpointPairCandidate {
                    face_record_ordinals: [first.face_record_ordinal, second.face_record_ordinal],
                    support_record_ordinals: [
                        first.support_record_ordinal,
                        second.support_record_ordinal,
                    ],
                    model_endpoints: first.model_endpoints,
                    model_midpoint: first.model_midpoint,
                },
                "catia_zero_endpoint_pairs",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut candidates,
        |value| &value.support_record_ordinals,
        Ord::cmp,
        "catia_zero_endpoint_pairs_sort",
    )?;
    Ok(Some(candidates))
}

pub(crate) fn endpoint_locus_candidates(
    ctx: &DecodeContext<'_>,
    endpoint_pairs: &[ZeroEntityEndpointPairCandidate],
) -> Result<Vec<ZeroEntityEndpointLocusCandidate>, CodecError> {
    let budget = ctx.work_budget(ctx.policy().limits.max_work_units);
    Ok(endpoint_locus_candidates_with_budget(ctx, endpoint_pairs, &budget)?.unwrap_or_default())
}

pub(super) fn endpoint_locus_candidates_with_budget(
    ctx: &DecodeContext<'_>,
    endpoint_pairs: &[ZeroEntityEndpointPairCandidate],
    budget: &WorkBudget<'_>,
) -> Result<Option<Vec<ZeroEntityEndpointLocusCandidate>>, CodecError> {
    // The endpoint lane, cell index, neighbor graph and traversal state are
    // scratch; only the locus candidates are kept.
    let mut scratch = ctx.reserve_scoped(0, "catia_zero_locus_workspace")?;
    let mut endpoints = Vec::new();
    for (endpoint_pair, candidate) in ctx
        .admit_iter(endpoint_pairs, "catia_zero_locus_pair_visits")?
        .enumerate()
    {
        for (endpoint, point) in [EdgeEnd::Start, EdgeEnd::End]
            .into_iter()
            .zip(candidate.model_endpoints)
        {
            ctx.push_scoped_vec(
                &mut scratch,
                &mut endpoints,
                (EndpointPairIndex(endpoint_pair), endpoint, point),
                "catia_zero_locus_endpoints",
            )?;
        }
    }
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    for (index, (_, _, point)) in ctx
        .admit_iter(&endpoints, "catia_zero_locus_endpoint_visits")?
        .enumerate()
    {
        let Some(cell) = endpoint_cell(*point) else {
            return Ok(None);
        };
        scratch.with_storage(|| {
            ctx.push_hash_group(
                &mut cells,
                cell,
                index,
                "catia_zero_locus_cells",
                "catia_zero_locus_cell_members",
            )
        })?;
    }
    let mut neighbors = scratch.with_storage(|| {
        ctx.collect_indexed_vec(endpoints.len(), "catia_zero_locus_neighbors", |_| {
            Ok(Vec::new())
        })
    })?;
    for (index, (_, _, point)) in ctx
        .admit_iter(&endpoints, "catia_zero_locus_endpoint_visits")?
        .enumerate()
    {
        let Some(cell) = endpoint_cell(*point) else {
            return Ok(None);
        };
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let (Some(x), Some(y), Some(z)) = (
                        cell[0].checked_add(dx),
                        cell[1].checked_add(dy),
                        cell[2].checked_add(dz),
                    ) else {
                        continue;
                    };
                    let neighbor_cell = [x, y, z];
                    for other in ctx.admit_iter(
                        match ctx.get_hash_map(
                            &cells,
                            &neighbor_cell,
                            "catia_zero_locus_cell_lookup",
                        )? {
                            Some(indices) => indices.as_slice(),
                            None => &[],
                        },
                        "catia_zero_locus_cell_member_visits",
                    )? {
                        if *other <= index {
                            continue;
                        }
                        charge_endpoint_work(ctx, budget)?;
                        if point.distance(endpoints[*other].2.get()) <= MODEL_POINT_TOLERANCE {
                            ctx.push_scoped_vec(
                                &mut scratch,
                                &mut neighbors[index],
                                *other,
                                "catia_zero_locus_neighbor_edges",
                            )?;
                            ctx.push_scoped_vec(
                                &mut scratch,
                                &mut neighbors[*other],
                                index,
                                "catia_zero_locus_neighbor_edges",
                            )?;
                        }
                    }
                }
            }
        }
    }

    let mut visited = scratch
        .with_storage(|| ctx.alloc_filled(endpoints.len(), false, "catia_zero_locus_visited"))?;
    let mut candidates = Vec::new();
    for (start, _) in ctx
        .admit_iter(&endpoints, "catia_zero_locus_roots")?
        .enumerate()
    {
        if visited[start] {
            continue;
        }
        let (mut component, _component_storage) =
            ctx.with_scoped_storage("catia_zero_locus_component_workspace", || {
                let mut component = Vec::new();
                let mut stack = Vec::new();
                let mut stack_storage = ctx.reserve_scoped(0, "catia_zero_locus_stack")?;
                stack_storage
                    .with_storage(|| ctx.push_vec(&mut stack, start, "catia_zero_locus_stack"))?;
                visited[start] = true;
                while let Some(index) = stack.pop() {
                    ctx.push_vec(&mut component, index, "catia_zero_locus_component")?;
                    for neighbor in
                        ctx.admit_iter(&neighbors[index], "catia_zero_locus_neighbor_visits")?
                    {
                        if !visited[*neighbor] {
                            visited[*neighbor] = true;
                            stack_storage.with_storage(|| {
                                ctx.push_vec(&mut stack, *neighbor, "catia_zero_locus_stack")
                            })?;
                        }
                    }
                }
                Ok::<_, CodecError>(component)
            })?;
        ctx.sort_unstable_by(
            &mut component,
            |value| value,
            Ord::cmp,
            "catia_zero_locus_component_sort",
        )?;
        let representative_point = endpoints[component[0]].2;
        let mut maximum_deviation = 0.0_f64;
        let mut complete = true;
        for (position, left) in ctx
            .admit_iter(&component, "catia_zero_locus_component_visits")?
            .enumerate()
        {
            for right in ctx.admit_iter(
                &component[position + 1..],
                "catia_zero_locus_component_tail_visits",
            )? {
                charge_endpoint_work(ctx, budget)?;
                let deviation = endpoints[*left].2.distance(endpoints[*right].2.get());
                maximum_deviation = maximum_deviation.max(deviation);
                complete &= deviation <= MODEL_POINT_TOLERANCE;
            }
        }
        if !complete {
            continue;
        }
        let mut incident = Vec::new();
        for &index in ctx.admit_iter(&component, "catia_zero_locus_incident_visits")? {
            ctx.push_vec(
                &mut incident,
                (endpoints[index].0, endpoints[index].1),
                "catia_zero_locus_incident_endpoints",
            )?;
        }
        ctx.push_vec(
            &mut candidates,
            ZeroEntityEndpointLocusCandidate {
                incident_endpoint_pair_endpoints: incident,
                representative_point,
                maximum_deviation,
            },
            "catia_zero_endpoint_loci",
        )?;
    }
    Ok(Some(candidates))
}

fn endpoint_match_graph(
    ctx: &DecodeContext<'_>,
    occurrences: &[ZeroEntityOrientedOccurrence],
    budget: &WorkBudget<'_>,
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    for (index, occurrence) in ctx
        .admit_iter(occurrences, "catia_zero_match_occurrence_visits")?
        .enumerate()
    {
        for endpoint in occurrence.model_endpoints {
            let Some(cell) = endpoint_cell(endpoint) else {
                return Ok(None);
            };
            ctx.push_hash_group(
                &mut cells,
                cell,
                index,
                "catia_zero_match_cells",
                "catia_zero_match_cell_members",
            )?;
        }
    }
    let mut matches =
        ctx.collect_indexed_vec(occurrences.len(), "catia_zero_match_rows", |_| {
            Ok(Vec::new())
        })?;
    for (index, occurrence) in ctx
        .admit_iter(occurrences, "catia_zero_match_occurrence_visits")?
        .enumerate()
    {
        let mut possible = BTreeSet::new();
        for endpoint in occurrence.model_endpoints {
            let Some(cell) = endpoint_cell(endpoint) else {
                return Ok(None);
            };
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let (Some(x), Some(y), Some(z)) = (
                            cell[0].checked_add(dx),
                            cell[1].checked_add(dy),
                            cell[2].checked_add(dz),
                        ) else {
                            continue;
                        };
                        let neighbor = [x, y, z];
                        if let Some(indices) =
                            ctx.get_hash_map(&cells, &neighbor, "catia_zero_match_cell_lookup")?
                        {
                            for other in ctx
                                .admit_iter(indices, "catia_zero_match_cell_member_visits")?
                                .copied()
                                .filter(|other| *other > index)
                            {
                                if ctx.insert_btree_set(
                                    &mut possible,
                                    other,
                                    "catia_zero_match_possible",
                                )? {
                                    charge_endpoint_work(ctx, budget)?;
                                }
                            }
                        }
                    }
                }
            }
        }
        for &other in ctx.admit_iter(&possible, "catia_zero_match_possible_visits")? {
            if unordered_endpoint_pairs_match(
                occurrence.model_endpoints,
                occurrences[other].model_endpoints,
            ) {
                ctx.push_vec(&mut matches[index], other, "catia_zero_match_edges")?;
                ctx.push_vec(&mut matches[other], index, "catia_zero_match_edges")?;
            }
        }
    }
    for (index, _) in ctx
        .admit_iter(occurrences, "catia_zero_match_sort_rows")?
        .enumerate()
    {
        let neighbors = &mut matches[index];
        ctx.sort_unstable_by(
            neighbors,
            |value| value,
            Ord::cmp,
            "catia_zero_match_edges_sort",
        )?;
    }
    Ok(Some(matches))
}

fn selected_radial_matches(
    ctx: &DecodeContext<'_>,
    occurrences: &[ZeroEntityOrientedOccurrence],
    endpoint_matches: &[Vec<usize>],
) -> Result<Vec<Vec<usize>>, CodecError> {
    let mut radial = Vec::new();
    for (index, matches) in ctx
        .admit_iter(endpoint_matches, "catia_zero_radial_match_visits")?
        .enumerate()
    {
        let mut row = Vec::new();
        for &other in ctx.admit_iter(matches, "catia_zero_radial_neighbor_visits")? {
            if occurrences[index]
                .model_midpoint
                .distance(occurrences[other].model_midpoint.get())
                <= MODEL_POINT_TOLERANCE
            {
                ctx.push_vec(&mut row, other, "catia_zero_radial_edges")?;
            }
        }
        ctx.push_vec(&mut radial, row, "catia_zero_radial_rows")?;
    }
    Ok(radial)
}

fn endpoint_cell(point: FinitePoint3) -> Option<[i64; 3]> {
    Some([
        truncate_f64_to_i64((point.x / MODEL_POINT_TOLERANCE).floor())?,
        truncate_f64_to_i64((point.y / MODEL_POINT_TOLERANCE).floor())?,
        truncate_f64_to_i64((point.z / MODEL_POINT_TOLERANCE).floor())?,
    ])
}

fn unordered_endpoint_pairs_match(left: [FinitePoint3; 2], right: [FinitePoint3; 2]) -> bool {
    let [left, right] = [left, right].map(|pair| pair.map(FinitePoint3::get));
    let direct = left[0].distance(right[0]).max(left[1].distance(right[1]));
    let reversed = left[0].distance(right[1]).max(left[1].distance(right[0]));
    direct.min(reversed) <= MODEL_POINT_TOLERANCE
}

#[cfg(test)]
mod tests {
    mod work_admission;
    use super::{
        endpoint_locus_candidates, endpoint_pair_candidates, endpoint_pair_candidates_with_budget,
        ZeroEntityEndpointPairCandidate, ZeroEntityOrientedOccurrence,
    };
    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget,
    };
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    use std::collections::HashSet;

    fn finite(point: Point3) -> FinitePoint3 {
        FinitePoint3::new(point).expect("finite test point")
    }

    fn occurrence(
        face_record_ordinal: u32,
        support_record_ordinal: u32,
        model_endpoints: [Point3; 2],
        model_midpoint: Point3,
    ) -> ZeroEntityOrientedOccurrence {
        ZeroEntityOrientedOccurrence {
            face_record_ordinal,
            support_record_ordinal,
            model_endpoints: model_endpoints.map(finite),
            model_midpoint: finite(model_midpoint),
        }
    }

    #[test]
    fn endpoint_match_graph_refuses_positive_spatial_overflow() {
        let endpoints = [Point3::new(f64::MAX, 0.0, 0.0); 2];
        let occurrences = [occurrence(1, 1, endpoints, endpoints[0])];
        let result = crate::test_support::with_service_context(|ctx| {
            super::endpoint_match_graph(ctx, &occurrences, &ctx.work_budget(100))
        })
        .expect("service budget");
        assert!(result.is_none());
    }

    #[test]
    fn endpoint_match_graph_refuses_negative_spatial_overflow() {
        let endpoints = [Point3::new(-f64::MAX, 0.0, 0.0); 2];
        let occurrences = [occurrence(1, 1, endpoints, endpoints[0])];
        let result = crate::test_support::with_service_context(|ctx| {
            super::endpoint_match_graph(ctx, &occurrences, &ctx.work_budget(100))
        })
        .expect("service budget");
        assert!(result.is_none());
    }

    #[test]
    fn endpoint_locus_refuses_positive_spatial_overflow() {
        let endpoints = [finite(Point3::new(f64::MAX, 0.0, 0.0)); 2];
        let pairs = [super::ZeroEntityEndpointPairCandidate {
            face_record_ordinals: [1, 2],
            support_record_ordinals: [1, 2],
            model_endpoints: endpoints,
            model_midpoint: endpoints[0],
        }];
        let result = crate::test_support::with_service_context(|ctx| {
            super::endpoint_locus_candidates_with_budget(ctx, &pairs, &ctx.work_budget(100))
        })
        .expect("service budget");
        assert!(result.is_none());
    }

    #[test]
    fn endpoint_locus_refuses_negative_spatial_overflow() {
        let endpoints = [finite(Point3::new(-f64::MAX, 0.0, 0.0)); 2];
        let pairs = [super::ZeroEntityEndpointPairCandidate {
            face_record_ordinals: [1, 2],
            support_record_ordinals: [1, 2],
            model_endpoints: endpoints,
            model_midpoint: endpoints[0],
        }];
        let result = crate::test_support::with_service_context(|ctx| {
            super::endpoint_locus_candidates_with_budget(ctx, &pairs, &ctx.work_budget(100))
        })
        .expect("service budget");
        assert!(result.is_none());
    }

    #[test]
    fn matching_endpoint_pairs_partition_by_face_incidence_components() {
        let shared_endpoints = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let first_link = [Point3::new(0.0, 1.0, 0.0), Point3::new(1.0, 1.0, 0.0)];
        let second_link = [Point3::new(0.0, 2.0, 0.0), Point3::new(1.0, 2.0, 0.0)];
        let occurrences = [
            occurrence(10, 1, shared_endpoints, Point3::new(0.5, 0.0, 0.0)),
            occurrence(11, 2, shared_endpoints, Point3::new(0.5, 0.0, 0.0)),
            occurrence(12, 3, shared_endpoints, Point3::new(0.5, 0.0, 0.0)),
            occurrence(13, 4, shared_endpoints, Point3::new(0.5, 0.0, 0.0)),
            occurrence(10, 5, first_link, Point3::new(0.5, 1.0, 0.0)),
            occurrence(11, 6, first_link, Point3::new(0.5, 1.0, 0.0)),
            occurrence(12, 7, second_link, Point3::new(0.5, 2.0, 0.0)),
            occurrence(13, 8, second_link, Point3::new(0.5, 2.0, 0.0)),
        ];

        let candidates = crate::test_support::with_service_context(|ctx| {
            endpoint_pair_candidates(ctx, &occurrences)
        })
        .expect("test endpoint pairs fit the service profile");
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.support_record_ordinals)
                .collect::<Vec<_>>(),
            [[1, 2], [3, 4], [5, 6], [7, 8]]
        );
        assert!(crate::test_support::with_service_context(|ctx| {
            endpoint_pair_candidates(ctx, &occurrences[..4])
        })
        .expect("test endpoint pairs fit the service profile")
        .is_empty());
    }

    #[test]
    fn singleton_radial_pairs_survive_a_shared_face_component() {
        let shared_endpoints = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let occurrences = [
            occurrence(10, 1, shared_endpoints, Point3::new(0.5, 1.0, 0.0)),
            occurrence(11, 2, shared_endpoints, Point3::new(0.5, 1.0, 0.0)),
            occurrence(10, 3, shared_endpoints, Point3::new(0.5, 2.0, 0.0)),
            occurrence(11, 4, shared_endpoints, Point3::new(0.5, 2.0, 0.0)),
        ];

        assert_eq!(
            crate::test_support::with_service_context(|ctx| endpoint_pair_candidates(
                ctx,
                &occurrences
            ))
            .expect("test endpoint pairs fit the service profile")
            .iter()
            .map(|candidate| candidate.support_record_ordinals)
            .collect::<Vec<_>>(),
            [[1, 2], [3, 4]]
        );
    }

    #[test]
    fn radial_match_crosses_spatial_cells_and_accepts_reversed_endpoints() {
        let occurrences = [
            occurrence(
                10,
                1,
                [Point3::new(-0.000_1, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                Point3::new(0.5, 0.0, 0.0),
            ),
            occurrence(
                11,
                2,
                [
                    Point3::new(1.001_9, 0.0, 0.0),
                    Point3::new(0.001_8, 0.0, 0.0),
                ],
                Point3::new(0.501, 0.0, 0.0),
            ),
        ];

        assert_eq!(
            crate::test_support::with_service_context(|ctx| endpoint_pair_candidates(
                ctx,
                &occurrences
            ))
            .expect("test endpoint pairs fit the service profile")[0]
                .support_record_ordinals,
            [1, 2]
        );
    }

    #[test]
    fn zero_entity_pair_component_source_propagates_caller_work_refusal() {
        let endpoints = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let midpoint = Point3::new(0.5, 0.0, 0.0);
        let occurrences = [
            occurrence(10, 1, endpoints, midpoint),
            occurrence(11, 2, endpoints, midpoint),
        ];
        let operation = "catia_zero_pair_component_visits";
        let result = crate::test_support::with_work_refusal(operation, |ctx| {
            let result = endpoint_pair_candidates(ctx, &occurrences);
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        });
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.operation == operation)
        );
    }

    #[test]
    fn native_endpoint_inventory_propagates_zero_work_refusal() {
        let endpoints = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let occurrences = [
            occurrence(10, 1, endpoints, Point3::new(0.5, 0.0, 0.0)),
            occurrence(11, 2, endpoints, Point3::new(0.5, 0.0, 0.0)),
        ];
        crate::test_support::with_work_limit(0, |ctx| {
            let error =
                endpoint_pair_candidates(ctx, &occurrences).expect_err("inventory work refused");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("resource refusal required")
            };
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
        let pairs = [super::ZeroEntityEndpointPairCandidate {
            face_record_ordinals: [10, 11],
            support_record_ordinals: [1, 2],
            model_endpoints: [finite(endpoints[0]); 2],
            model_midpoint: finite(endpoints[0]),
        }];
        crate::test_support::with_work_limit(0, |ctx| {
            let error = endpoint_locus_candidates(ctx, &pairs).expect_err("locus work refused");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("resource refusal required")
            };
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }

    #[test]
    fn bounded_endpoint_matching_refuses_exhausted_work() {
        let endpoints = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let occurrences = [
            occurrence(10, 1, endpoints, Point3::new(0.5, 0.0, 0.0)),
            occurrence(11, 2, endpoints, Point3::new(0.5, 0.0, 0.0)),
        ];
        let budget = WorkBudget::new(0);

        let result = crate::test_support::with_service_context(|ctx| {
            endpoint_pair_candidates_with_budget(ctx, &occurrences, &budget)
        });
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_zero_endpoint_work" && limit.limit == 0 && limit.additional == 1));
        assert!(budget.exhausted());
    }

    #[test]
    fn unique_endpoint_pair_requires_equal_parameter_midpoints() {
        let endpoints = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let occurrences = [
            occurrence(10, 1, endpoints, Point3::new(0.4, 0.1, 0.0)),
            occurrence(11, 2, endpoints, Point3::new(0.6, 0.1, 0.0)),
        ];

        assert!(
            crate::test_support::with_service_context(|ctx| endpoint_pair_candidates(
                ctx,
                &occurrences
            ))
            .expect("test endpoint pairs fit the service profile")
            .is_empty()
        );
    }

    #[test]
    fn coincident_endpoint_pairs_partition_by_model_midpoint() {
        let endpoints = [Point3::new(-1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let occurrences = [
            occurrence(10, 1, endpoints, Point3::new(0.0, 1.0, 0.0)),
            occurrence(11, 2, endpoints, Point3::new(0.0, -1.0, 0.0)),
            occurrence(12, 3, endpoints, Point3::new(0.0, 1.0, 0.0)),
            occurrence(13, 4, endpoints, Point3::new(0.0, -1.0, 0.0)),
        ];

        assert_eq!(
            crate::test_support::with_service_context(|ctx| endpoint_pair_candidates(
                ctx,
                &occurrences
            ))
            .expect("test endpoint pairs fit the service profile")
            .iter()
            .map(|candidate| candidate.support_record_ordinals)
            .collect::<Vec<_>>(),
            [[1, 3], [2, 4]]
        );
    }

    #[test]
    fn endpoint_locus_candidates_require_complete_endpoint_cliques() {
        let pair = |support_record_ordinals, model_endpoints: [Point3; 2]| {
            ZeroEntityEndpointPairCandidate {
                face_record_ordinals: support_record_ordinals,
                support_record_ordinals,
                model_endpoints: model_endpoints.map(finite),
                model_midpoint: finite(Point3::new(0.0, 0.0, 0.0)),
            }
        };
        let pairs = [
            pair(
                [1, 2],
                [Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
            ),
            pair(
                [3, 4],
                [Point3::new(0.001, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)],
            ),
        ];

        let candidates =
            crate::test_support::with_service_context(|ctx| endpoint_locus_candidates(ctx, &pairs))
                .expect("test endpoint loci fit the service profile");
        assert_eq!(candidates.len(), 3);
        assert_eq!(
            candidates[0]
                .incident_endpoint_pair_endpoints
                .iter()
                .map(|(pair, end)| (pair.ordinal(), u8::from(*end)))
                .collect::<Vec<_>>(),
            [(0, 0), (1, 0)]
        );
        assert_eq!(candidates[0].maximum_deviation, 0.001);

        let ambiguous = [
            pair(
                [1, 2],
                [Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
            ),
            pair(
                [3, 4],
                [Point3::new(0.001_5, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)],
            ),
            pair(
                [5, 6],
                [Point3::new(0.003, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)],
            ),
        ];
        let candidates = crate::test_support::with_service_context(|ctx| {
            endpoint_locus_candidates(ctx, &ambiguous)
        })
        .expect("test endpoint loci fit the service profile");
        assert_eq!(candidates.len(), 3);
        assert!(candidates
            .iter()
            .all(|candidate| candidate.incident_endpoint_pair_endpoints.len() == 1));
    }

    #[test]
    fn endpoint_pair_graph_refuses_each_collection_limit() {
        let endpoints = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let occurrences = [
            occurrence(10, 1, endpoints, Point3::new(0.5, 0.0, 0.0)),
            occurrence(11, 2, endpoints, Point3::new(0.5, 0.0, 0.0)),
        ];
        let mut operations = HashSet::new();
        for cap in 0..=128 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits input limit");
            match endpoint_pair_candidates(&ctx, &occurrences) {
                Ok(pairs) => assert_eq!(pairs[0].support_record_ordinals, [1, 2]),
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    operations.insert(limit.operation);
                }
                Err(error) => panic!("unexpected endpoint-pair refusal: {error}"),
            }
        }
        for operation in [
            "catia_zero_match_cells",
            "catia_zero_match_cell_members",
            "catia_zero_match_rows",
            "catia_zero_match_possible",
            "catia_zero_match_edges",
            "catia_zero_radial_edges",
            "catia_zero_radial_rows",
            "catia_zero_face_indices",
            "catia_zero_face_union",
            "catia_zero_endpoint_pairs",
            "catia_zero_pair_visited",
            "catia_zero_pair_stack",
            "catia_zero_pair_face_members",
            "catia_zero_pair_face_components",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn endpoint_locus_graph_refuses_each_collection_limit() {
        let pair = ZeroEntityEndpointPairCandidate {
            face_record_ordinals: [10, 11],
            support_record_ordinals: [1, 2],
            model_endpoints: [
                finite(Point3::new(0.0, 0.0, 0.0)),
                finite(Point3::new(1.0, 0.0, 0.0)),
            ],
            model_midpoint: finite(Point3::new(0.5, 0.0, 0.0)),
        };
        let mut operations = HashSet::new();
        for cap in 0..=128 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits input limit");
            match endpoint_locus_candidates(&ctx, &[pair]) {
                Ok(loci) => assert_eq!(loci.len(), 2),
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    operations.insert(limit.operation);
                }
                Err(error) => panic!("unexpected endpoint-locus refusal: {error}"),
            }
        }
        for operation in [
            "catia_zero_locus_endpoints",
            "catia_zero_locus_cell_members",
            "catia_zero_locus_cells",
            "catia_zero_locus_neighbors",
            "catia_zero_locus_visited",
            "catia_zero_locus_stack",
            "catia_zero_locus_component",
            "catia_zero_locus_incident_endpoints",
            "catia_zero_endpoint_loci",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
    }
}
