//! Endpoint relations derived from resolved zero-entity support occurrences.

use std::collections::{HashMap, HashSet};

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
    endpoint_pair_candidates(ctx, &zero_entity_oriented_occurrences(ctx, runs)?)
}

pub(super) fn zero_entity_endpoint_pair_candidates_with_budget(
    ctx: &DecodeContext<'_>,
    runs: &[ZeroEntitySupportRun],
    budget: &WorkBudget<'_>,
) -> Result<Option<Vec<ZeroEntityEndpointPairCandidate>>, CodecError> {
    endpoint_pair_candidates_with_budget(ctx, &zero_entity_oriented_occurrences(ctx, runs)?, budget)
}

fn zero_entity_oriented_occurrences(
    ctx: &DecodeContext<'_>,
    runs: &[ZeroEntitySupportRun],
) -> Result<Vec<ZeroEntityOrientedOccurrence>, CodecError> {
    let mut occurrences = Vec::new();
    for run in runs {
        let Some(face) = run.face.as_ref() else {
            continue;
        };
        let mut midpoints = HashMap::new();
        for support in &run.supports {
            if let Some(midpoint) = support.model_midpoint {
                crate::resource::insert_map(
                    ctx,
                    &mut midpoints,
                    support.record_ordinal,
                    midpoint,
                    "catia_zero_midpoints",
                )?;
            }
        }
        for loop_record in face.loops.iter().flatten() {
            for (support_record_ordinal, model_endpoints) in loop_record
                .support_record_ordinals
                .iter()
                .copied()
                .zip(loop_record.oriented_model_endpoints.iter().copied())
            {
                let Some(model_midpoint) = midpoints.get(&support_record_ordinal).copied() else {
                    continue;
                };
                crate::resource::push(
                    ctx,
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

pub(crate) fn endpoint_pair_candidates(
    ctx: &DecodeContext<'_>,
    occurrences: &[ZeroEntityOrientedOccurrence],
) -> Result<Vec<ZeroEntityEndpointPairCandidate>, CodecError> {
    Ok(endpoint_pair_candidates_inner(ctx, occurrences, None)?.unwrap_or_default())
}

fn endpoint_pair_candidates_with_budget(
    ctx: &DecodeContext<'_>,
    occurrences: &[ZeroEntityOrientedOccurrence],
    budget: &WorkBudget<'_>,
) -> Result<Option<Vec<ZeroEntityEndpointPairCandidate>>, CodecError> {
    endpoint_pair_candidates_inner(ctx, occurrences, Some(budget))
}

fn endpoint_pair_candidates_inner(
    ctx: &DecodeContext<'_>,
    occurrences: &[ZeroEntityOrientedOccurrence],
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<Vec<ZeroEntityEndpointPairCandidate>>, CodecError> {
    let Some(endpoint_matches) = endpoint_match_graph(ctx, occurrences, budget)? else {
        return Ok(None);
    };
    let radial_matches = selected_radial_matches(ctx, occurrences, &endpoint_matches)?;
    let mut face_ordinals = HashSet::new();
    for occurrence in occurrences {
        crate::resource::insert_set(
            ctx,
            &mut face_ordinals,
            occurrence.face_record_ordinal,
            "catia_zero_face_ordinals",
        )?;
    }
    let mut face_indices = HashMap::new();
    for (index, ordinal) in face_ordinals.into_iter().enumerate() {
        crate::resource::insert_map(
            ctx,
            &mut face_indices,
            ordinal,
            index,
            "catia_zero_face_indices",
        )?;
    }
    let mut face_components = UnionFind::charged(ctx, face_indices.len(), "catia_zero_face_union")?;
    for (index, neighbors) in radial_matches.iter().enumerate() {
        let [neighbor] = neighbors.as_slice() else {
            continue;
        };
        if radial_matches[*neighbor].as_slice() != [index] {
            continue;
        }
        face_components.union(
            face_indices[&occurrences[index].face_record_ordinal],
            face_indices[&occurrences[*neighbor].face_record_ordinal],
        );
    }

    let mut candidates = Vec::new();
    // Face components can contain several distinct edges with the same
    // endpoint locus. A reciprocal singleton radial match is still a
    // complete geometric relation and must not be merged with its siblings.
    for (index, neighbors) in radial_matches.iter().enumerate() {
        let [neighbor] = neighbors.as_slice() else {
            continue;
        };
        if index >= *neighbor || radial_matches[*neighbor].as_slice() != [index] {
            continue;
        }
        let [first, second] = [occurrences[index], occurrences[*neighbor]];
        crate::resource::push(
            ctx,
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

    let mut visited = ctx.alloc_filled(occurrences.len(), false, "catia_zero_pair_visited")?;
    for start in 0..occurrences.len() {
        if visited[start] {
            continue;
        }
        let mut stack = Vec::new();
        crate::resource::push(ctx, &mut stack, start, "catia_zero_pair_stack")?;
        visited[start] = true;
        let mut group = Vec::new();
        while let Some(index) = stack.pop() {
            crate::resource::push(ctx, &mut group, index, "catia_zero_pair_group")?;
            for neighbor in &endpoint_matches[index] {
                if !visited[*neighbor] {
                    visited[*neighbor] = true;
                    crate::resource::push(ctx, &mut stack, *neighbor, "catia_zero_pair_stack")?;
                }
            }
        }
        let mut by_face_component = HashMap::<usize, Vec<usize>>::new();
        for index in group {
            let component =
                face_components.find(face_indices[&occurrences[index].face_record_ordinal]);
            if let Some(group) = by_face_component.get_mut(&component) {
                crate::resource::push(ctx, group, index, "catia_zero_pair_face_members")?;
            } else {
                let mut group = Vec::new();
                crate::resource::push(ctx, &mut group, index, "catia_zero_pair_face_members")?;
                crate::resource::insert_map(
                    ctx,
                    &mut by_face_component,
                    component,
                    group,
                    "catia_zero_pair_face_components",
                )?;
            }
        }
        for mut pair in by_face_component.into_values() {
            if pair.len() != 2 || !endpoint_matches[pair[0]].contains(&pair[1]) {
                continue;
            }
            if radial_matches[pair[0]].len() == 1 && radial_matches[pair[1]].len() == 1 {
                continue;
            }
            pair.sort_by_key(|index| occurrences[*index].support_record_ordinal);
            let [first, second] = [occurrences[pair[0]], occurrences[pair[1]]];
            crate::resource::push(
                ctx,
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
    candidates.sort_by_key(|candidate| candidate.support_record_ordinals);
    Ok(Some(candidates))
}

pub(crate) fn endpoint_locus_candidates(
    ctx: &DecodeContext<'_>,
    endpoint_pairs: &[ZeroEntityEndpointPairCandidate],
) -> Result<Vec<ZeroEntityEndpointLocusCandidate>, CodecError> {
    Ok(endpoint_locus_candidates_inner(ctx, endpoint_pairs, None)?.unwrap_or_default())
}

pub(super) fn endpoint_locus_candidates_with_budget(
    ctx: &DecodeContext<'_>,
    endpoint_pairs: &[ZeroEntityEndpointPairCandidate],
    budget: &WorkBudget<'_>,
) -> Result<Option<Vec<ZeroEntityEndpointLocusCandidate>>, CodecError> {
    endpoint_locus_candidates_inner(ctx, endpoint_pairs, Some(budget))
}

fn endpoint_locus_candidates_inner(
    ctx: &DecodeContext<'_>,
    endpoint_pairs: &[ZeroEntityEndpointPairCandidate],
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<Vec<ZeroEntityEndpointLocusCandidate>>, CodecError> {
    let mut endpoints = Vec::new();
    for (endpoint_pair, candidate) in endpoint_pairs.iter().enumerate() {
        for (endpoint, point) in [EdgeEnd::Start, EdgeEnd::End]
            .into_iter()
            .zip(candidate.model_endpoints)
        {
            crate::resource::push(
                ctx,
                &mut endpoints,
                (EndpointPairIndex(endpoint_pair), endpoint, point),
                "catia_zero_locus_endpoints",
            )?;
        }
    }
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    for (index, (_, _, point)) in endpoints.iter().enumerate() {
        let cell = endpoint_cell(*point);
        if let Some(indices) = cells.get_mut(&cell) {
            crate::resource::push(ctx, indices, index, "catia_zero_locus_cell_members")?;
        } else {
            let mut indices = Vec::new();
            crate::resource::push(ctx, &mut indices, index, "catia_zero_locus_cell_members")?;
            crate::resource::insert_map(ctx, &mut cells, cell, indices, "catia_zero_locus_cells")?;
        }
    }
    let mut neighbors =
        ctx.alloc_filled(endpoints.len(), Vec::new(), "catia_zero_locus_neighbors")?;
    for (index, (_, _, point)) in endpoints.iter().enumerate() {
        let cell = endpoint_cell(*point);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let neighbor_cell = [
                        cell[0].saturating_add(dx),
                        cell[1].saturating_add(dy),
                        cell[2].saturating_add(dz),
                    ];
                    for other in cells.get(&neighbor_cell).into_iter().flatten() {
                        if *other <= index {
                            continue;
                        }
                        if let Some(budget) = budget {
                            if !budget.charge() {
                                return Ok(None);
                            }
                        }
                        if point.distance(endpoints[*other].2.get()) <= MODEL_POINT_TOLERANCE {
                            crate::resource::push(
                                ctx,
                                &mut neighbors[index],
                                *other,
                                "catia_zero_locus_neighbor_edges",
                            )?;
                            crate::resource::push(
                                ctx,
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

    let mut visited = ctx.alloc_filled(endpoints.len(), false, "catia_zero_locus_visited")?;
    let mut candidates = Vec::new();
    for start in 0..endpoints.len() {
        if visited[start] {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = Vec::new();
        crate::resource::push(ctx, &mut stack, start, "catia_zero_locus_stack")?;
        visited[start] = true;
        while let Some(index) = stack.pop() {
            crate::resource::push(ctx, &mut component, index, "catia_zero_locus_component")?;
            for neighbor in &neighbors[index] {
                if !visited[*neighbor] {
                    visited[*neighbor] = true;
                    crate::resource::push(ctx, &mut stack, *neighbor, "catia_zero_locus_stack")?;
                }
            }
        }
        component.sort_unstable();
        let representative_point = endpoints[component[0]].2;
        let mut maximum_deviation = 0.0_f64;
        let mut complete = true;
        for (position, left) in component.iter().enumerate() {
            for right in &component[position + 1..] {
                if let Some(budget) = budget {
                    if !budget.charge() {
                        return Ok(None);
                    }
                }
                let deviation = endpoints[*left].2.distance(endpoints[*right].2.get());
                maximum_deviation = maximum_deviation.max(deviation);
                complete &= deviation <= MODEL_POINT_TOLERANCE;
            }
        }
        if !complete {
            continue;
        }
        let mut incident = Vec::new();
        for index in component {
            crate::resource::push(
                ctx,
                &mut incident,
                (endpoints[index].0, endpoints[index].1),
                "catia_zero_locus_incident_endpoints",
            )?;
        }
        crate::resource::push(
            ctx,
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
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    for (index, occurrence) in occurrences.iter().enumerate() {
        for endpoint in occurrence.model_endpoints {
            let cell = endpoint_cell(endpoint);
            if let Some(indices) = cells.get_mut(&cell) {
                crate::resource::push(ctx, indices, index, "catia_zero_match_cell_members")?;
            } else {
                let mut indices = Vec::new();
                crate::resource::push(ctx, &mut indices, index, "catia_zero_match_cell_members")?;
                crate::resource::insert_map(
                    ctx,
                    &mut cells,
                    cell,
                    indices,
                    "catia_zero_match_cells",
                )?;
            }
        }
    }
    let mut matches = ctx.alloc_filled(occurrences.len(), Vec::new(), "catia_zero_match_rows")?;
    for (index, occurrence) in occurrences.iter().enumerate() {
        let mut possible = HashSet::new();
        for endpoint in occurrence.model_endpoints {
            let cell = endpoint_cell(endpoint);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let neighbor = [
                            cell[0].saturating_add(dx),
                            cell[1].saturating_add(dy),
                            cell[2].saturating_add(dz),
                        ];
                        if let Some(indices) = cells.get(&neighbor) {
                            for other in indices.iter().copied().filter(|other| *other > index) {
                                if crate::resource::insert_set(
                                    ctx,
                                    &mut possible,
                                    other,
                                    "catia_zero_match_possible",
                                )? {
                                    if let Some(budget) = budget {
                                        if !budget.charge() {
                                            return Ok(None);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for other in possible {
            if unordered_endpoint_pairs_match(
                occurrence.model_endpoints,
                occurrences[other].model_endpoints,
            ) {
                crate::resource::push(ctx, &mut matches[index], other, "catia_zero_match_edges")?;
                crate::resource::push(ctx, &mut matches[other], index, "catia_zero_match_edges")?;
            }
        }
    }
    for neighbors in &mut matches {
        neighbors.sort_unstable();
    }
    Ok(Some(matches))
}

fn selected_radial_matches(
    ctx: &DecodeContext<'_>,
    occurrences: &[ZeroEntityOrientedOccurrence],
    endpoint_matches: &[Vec<usize>],
) -> Result<Vec<Vec<usize>>, CodecError> {
    let mut radial = Vec::new();
    for (index, matches) in endpoint_matches.iter().enumerate() {
        let mut row = Vec::new();
        for &other in matches {
            if occurrences[index]
                .model_midpoint
                .distance(occurrences[other].model_midpoint.get())
                <= MODEL_POINT_TOLERANCE
            {
                crate::resource::push(ctx, &mut row, other, "catia_zero_radial_edges")?;
            }
        }
        crate::resource::push(ctx, &mut radial, row, "catia_zero_radial_rows")?;
    }
    Ok(radial)
}

fn endpoint_cell(point: FinitePoint3) -> [i64; 3] {
    [
        (point.x / MODEL_POINT_TOLERANCE).floor() as i64,
        (point.y / MODEL_POINT_TOLERANCE).floor() as i64,
        (point.z / MODEL_POINT_TOLERANCE).floor() as i64,
    ]
}

fn unordered_endpoint_pairs_match(left: [FinitePoint3; 2], right: [FinitePoint3; 2]) -> bool {
    let [left, right] = [left, right].map(|pair| pair.map(FinitePoint3::get));
    let direct = left[0].distance(right[0]).max(left[1].distance(right[1]));
    let reversed = left[0].distance(right[1]).max(left[1].distance(right[0]));
    direct.min(reversed) <= MODEL_POINT_TOLERANCE
}

#[cfg(test)]
mod tests {
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
    fn bounded_endpoint_matching_refuses_exhausted_work() {
        let endpoints = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let occurrences = [
            occurrence(10, 1, endpoints, Point3::new(0.5, 0.0, 0.0)),
            occurrence(11, 2, endpoints, Point3::new(0.5, 0.0, 0.0)),
        ];
        let budget = WorkBudget::new(0);

        assert!(crate::test_support::with_service_context(|ctx| {
            endpoint_pair_candidates_with_budget(ctx, &occurrences, &budget)
        })
        .expect("test endpoint pairs fit the service profile")
        .is_none());
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
            "catia_zero_face_ordinals",
            "catia_zero_face_indices",
            "catia_zero_face_union",
            "catia_zero_endpoint_pairs",
            "catia_zero_pair_visited",
            "catia_zero_pair_stack",
            "catia_zero_pair_group",
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
