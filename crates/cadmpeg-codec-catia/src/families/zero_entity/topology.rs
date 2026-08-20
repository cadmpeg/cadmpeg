//! Endpoint relations derived from resolved zero-entity support occurrences.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::math::Point3;

use super::records::ZeroEntitySupportRun;

const MODEL_POINT_TOLERANCE: f64 = 2e-3;
pub(crate) const MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS: usize = 1_000_000;

/// One sense-oriented support occurrence owned by a face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ZeroEntityOrientedOccurrence {
    pub(crate) face_record_ordinal: u32,
    pub(crate) support_record_ordinal: u32,
    pub(crate) model_endpoints: [Point3; 2],
    pub(crate) model_midpoint: Point3,
}

/// Two radial occurrences with matching bounded model-space witnesses.
///
/// This relation does not establish curve coincidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ZeroEntityEndpointPairCandidate {
    pub(crate) face_record_ordinals: [u32; 2],
    pub(crate) support_record_ordinals: [u32; 2],
    pub(crate) model_endpoints: [Point3; 2],
    pub(crate) model_midpoint: Point3,
}

/// One geometric endpoint-locus candidate established by a complete endpoint clique.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ZeroEntityEndpointLocusCandidate {
    pub(crate) incident_endpoint_pair_endpoints: Vec<(usize, u8)>,
    pub(crate) representative_point: Point3,
    pub(crate) maximum_deviation: f64,
}

pub(crate) fn zero_entity_endpoint_pair_candidates(
    runs: &[ZeroEntitySupportRun],
) -> Vec<ZeroEntityEndpointPairCandidate> {
    endpoint_pair_candidates(&zero_entity_oriented_occurrences(runs))
}

pub(crate) fn zero_entity_endpoint_pair_candidates_with_budget(
    runs: &[ZeroEntitySupportRun],
    budget: &WorkBudget<'_>,
) -> Option<Vec<ZeroEntityEndpointPairCandidate>> {
    endpoint_pair_candidates_with_budget(&zero_entity_oriented_occurrences(runs), budget)
}

fn zero_entity_oriented_occurrences(
    runs: &[ZeroEntitySupportRun],
) -> Vec<ZeroEntityOrientedOccurrence> {
    let mut occurrences = Vec::new();
    for run in runs {
        let Some(face) = run.face.as_ref() else {
            continue;
        };
        let midpoints = run
            .supports
            .iter()
            .filter_map(|support| Some((support.record_ordinal, support.model_midpoint?)))
            .collect::<HashMap<_, _>>();
        for loop_record in &face.loops {
            for (support_record_ordinal, model_endpoints) in loop_record
                .support_record_ordinals
                .iter()
                .copied()
                .zip(loop_record.oriented_model_endpoints.iter().copied())
            {
                let Some(model_midpoint) = midpoints.get(&support_record_ordinal).copied() else {
                    continue;
                };
                occurrences.push(ZeroEntityOrientedOccurrence {
                    face_record_ordinal: face.record_ordinal,
                    support_record_ordinal,
                    model_endpoints,
                    model_midpoint,
                });
            }
        }
    }
    occurrences
}

pub(crate) fn endpoint_pair_candidates(
    occurrences: &[ZeroEntityOrientedOccurrence],
) -> Vec<ZeroEntityEndpointPairCandidate> {
    endpoint_pair_candidates_inner(occurrences, None).unwrap_or_default()
}

pub(crate) fn endpoint_pair_candidates_with_budget(
    occurrences: &[ZeroEntityOrientedOccurrence],
    budget: &WorkBudget<'_>,
) -> Option<Vec<ZeroEntityEndpointPairCandidate>> {
    endpoint_pair_candidates_inner(occurrences, Some(budget))
}

fn endpoint_pair_candidates_inner(
    occurrences: &[ZeroEntityOrientedOccurrence],
    budget: Option<&WorkBudget<'_>>,
) -> Option<Vec<ZeroEntityEndpointPairCandidate>> {
    let endpoint_matches = endpoint_match_graph(occurrences, budget)?;
    let radial_matches = selected_radial_matches(occurrences, &endpoint_matches);
    let face_indices = occurrences
        .iter()
        .map(|occurrence| occurrence.face_record_ordinal)
        .collect::<HashSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(index, ordinal)| (ordinal, index))
        .collect::<HashMap<_, _>>();
    let mut face_components = DisjointSet::new(face_indices.len());
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
        candidates.push(ZeroEntityEndpointPairCandidate {
            face_record_ordinals: [first.face_record_ordinal, second.face_record_ordinal],
            support_record_ordinals: [first.support_record_ordinal, second.support_record_ordinal],
            model_endpoints: first.model_endpoints,
            model_midpoint: first.model_midpoint,
        });
    }

    let mut visited = vec![false; occurrences.len()];
    for start in 0..occurrences.len() {
        if visited[start] {
            continue;
        }
        let mut stack = vec![start];
        visited[start] = true;
        let mut group = Vec::new();
        while let Some(index) = stack.pop() {
            group.push(index);
            for neighbor in &endpoint_matches[index] {
                if !visited[*neighbor] {
                    visited[*neighbor] = true;
                    stack.push(*neighbor);
                }
            }
        }
        let mut by_face_component = BTreeMap::<usize, Vec<usize>>::new();
        for index in group {
            let component =
                face_components.find(face_indices[&occurrences[index].face_record_ordinal]);
            by_face_component.entry(component).or_default().push(index);
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
            candidates.push(ZeroEntityEndpointPairCandidate {
                face_record_ordinals: [first.face_record_ordinal, second.face_record_ordinal],
                support_record_ordinals: [
                    first.support_record_ordinal,
                    second.support_record_ordinal,
                ],
                model_endpoints: first.model_endpoints,
                model_midpoint: first.model_midpoint,
            });
        }
    }
    candidates.sort_by_key(|candidate| candidate.support_record_ordinals);
    Some(candidates)
}

pub(crate) fn endpoint_locus_candidates(
    endpoint_pairs: &[ZeroEntityEndpointPairCandidate],
) -> Vec<ZeroEntityEndpointLocusCandidate> {
    endpoint_locus_candidates_inner(endpoint_pairs, None).unwrap_or_default()
}

pub(crate) fn endpoint_locus_candidates_with_budget(
    endpoint_pairs: &[ZeroEntityEndpointPairCandidate],
    budget: &WorkBudget<'_>,
) -> Option<Vec<ZeroEntityEndpointLocusCandidate>> {
    endpoint_locus_candidates_inner(endpoint_pairs, Some(budget))
}

fn endpoint_locus_candidates_inner(
    endpoint_pairs: &[ZeroEntityEndpointPairCandidate],
    budget: Option<&WorkBudget<'_>>,
) -> Option<Vec<ZeroEntityEndpointLocusCandidate>> {
    let endpoints = endpoint_pairs
        .iter()
        .enumerate()
        .flat_map(|(endpoint_pair, candidate)| {
            candidate
                .model_endpoints
                .into_iter()
                .enumerate()
                .map(move |(endpoint, point)| (endpoint_pair, endpoint as u8, point))
        })
        .collect::<Vec<_>>();
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    for (index, (_, _, point)) in endpoints.iter().enumerate() {
        cells.entry(endpoint_cell(*point)).or_default().push(index);
    }
    let mut neighbors = vec![Vec::new(); endpoints.len()];
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
                                return None;
                            }
                        }
                        if point.distance(endpoints[*other].2) <= MODEL_POINT_TOLERANCE {
                            neighbors[index].push(*other);
                            neighbors[*other].push(index);
                        }
                    }
                }
            }
        }
    }

    let mut visited = vec![false; endpoints.len()];
    let mut candidates = Vec::new();
    for start in 0..endpoints.len() {
        if visited[start] {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        visited[start] = true;
        while let Some(index) = stack.pop() {
            component.push(index);
            for neighbor in &neighbors[index] {
                if !visited[*neighbor] {
                    visited[*neighbor] = true;
                    stack.push(*neighbor);
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
                        return None;
                    }
                }
                let deviation = endpoints[*left].2.distance(endpoints[*right].2);
                maximum_deviation = maximum_deviation.max(deviation);
                complete &= deviation <= MODEL_POINT_TOLERANCE;
            }
        }
        if !complete {
            continue;
        }
        candidates.push(ZeroEntityEndpointLocusCandidate {
            incident_endpoint_pair_endpoints: component
                .into_iter()
                .map(|index| (endpoints[index].0, endpoints[index].1))
                .collect(),
            representative_point,
            maximum_deviation,
        });
    }
    Some(candidates)
}

fn endpoint_match_graph(
    occurrences: &[ZeroEntityOrientedOccurrence],
    budget: Option<&WorkBudget<'_>>,
) -> Option<Vec<Vec<usize>>> {
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    for (index, occurrence) in occurrences.iter().enumerate() {
        for endpoint in occurrence.model_endpoints {
            cells
                .entry(endpoint_cell(endpoint))
                .or_default()
                .push(index);
        }
    }
    let mut matches = vec![Vec::new(); occurrences.len()];
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
                                if possible.insert(other) {
                                    if let Some(budget) = budget {
                                        if !budget.charge() {
                                            return None;
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
                matches[index].push(other);
                matches[other].push(index);
            }
        }
    }
    for neighbors in &mut matches {
        neighbors.sort_unstable();
    }
    Some(matches)
}

fn selected_radial_matches(
    occurrences: &[ZeroEntityOrientedOccurrence],
    endpoint_matches: &[Vec<usize>],
) -> Vec<Vec<usize>> {
    endpoint_matches
        .iter()
        .enumerate()
        .map(|(index, matches)| {
            matches
                .iter()
                .copied()
                .filter(|other| {
                    occurrences[index]
                        .model_midpoint
                        .distance(occurrences[*other].model_midpoint)
                        <= MODEL_POINT_TOLERANCE
                })
                .collect()
        })
        .collect()
}

fn endpoint_cell(point: Point3) -> [i64; 3] {
    [
        (point.x / MODEL_POINT_TOLERANCE).floor() as i64,
        (point.y / MODEL_POINT_TOLERANCE).floor() as i64,
        (point.z / MODEL_POINT_TOLERANCE).floor() as i64,
    ]
}

fn unordered_endpoint_pairs_match(left: [Point3; 2], right: [Point3; 2]) -> bool {
    let direct = left[0].distance(right[0]).max(left[1].distance(right[1]));
    let reversed = left[0].distance(right[1]).max(left[1].distance(right[0]));
    direct.min(reversed) <= MODEL_POINT_TOLERANCE
}

struct DisjointSet {
    parents: Vec<usize>,
}

impl DisjointSet {
    fn new(len: usize) -> Self {
        Self {
            parents: (0..len).collect(),
        }
    }

    fn find(&mut self, index: usize) -> usize {
        if self.parents[index] != index {
            self.parents[index] = self.find(self.parents[index]);
        }
        self.parents[index]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left = self.find(left);
        let right = self.find(right);
        if left != right {
            self.parents[right] = left;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn occurrence(
        face_record_ordinal: u32,
        support_record_ordinal: u32,
        model_endpoints: [Point3; 2],
        model_midpoint: Point3,
    ) -> ZeroEntityOrientedOccurrence {
        ZeroEntityOrientedOccurrence {
            face_record_ordinal,
            support_record_ordinal,
            model_endpoints,
            model_midpoint,
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

        let candidates = endpoint_pair_candidates(&occurrences);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.support_record_ordinals)
                .collect::<Vec<_>>(),
            [[1, 2], [3, 4], [5, 6], [7, 8]]
        );
        assert!(endpoint_pair_candidates(&occurrences[..4]).is_empty());
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
            endpoint_pair_candidates(&occurrences)[0].support_record_ordinals,
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

        assert!(endpoint_pair_candidates_with_budget(&occurrences, &budget).is_none());
        assert!(budget.exhausted());
    }

    #[test]
    fn unique_endpoint_pair_requires_equal_parameter_midpoints() {
        let endpoints = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let occurrences = [
            occurrence(10, 1, endpoints, Point3::new(0.4, 0.1, 0.0)),
            occurrence(11, 2, endpoints, Point3::new(0.6, 0.1, 0.0)),
        ];

        assert!(endpoint_pair_candidates(&occurrences).is_empty());
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
            endpoint_pair_candidates(&occurrences)
                .iter()
                .map(|candidate| candidate.support_record_ordinals)
                .collect::<Vec<_>>(),
            [[1, 3], [2, 4]]
        );
    }

    #[test]
    fn endpoint_locus_candidates_require_complete_endpoint_cliques() {
        let pair = |support_record_ordinals, model_endpoints| ZeroEntityEndpointPairCandidate {
            face_record_ordinals: support_record_ordinals,
            support_record_ordinals,
            model_endpoints,
            model_midpoint: Point3::new(0.0, 0.0, 0.0),
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

        let candidates = endpoint_locus_candidates(&pairs);
        assert_eq!(candidates.len(), 3);
        assert_eq!(
            candidates[0].incident_endpoint_pair_endpoints,
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
        let candidates = endpoint_locus_candidates(&ambiguous);
        assert_eq!(candidates.len(), 3);
        assert!(candidates
            .iter()
            .all(|candidate| candidate.incident_endpoint_pair_endpoints.len() == 1));
    }
}
