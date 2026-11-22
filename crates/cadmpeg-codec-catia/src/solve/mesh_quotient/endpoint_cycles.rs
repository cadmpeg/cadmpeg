// SPDX-License-Identifier: Apache-2.0
//! Endpoint-cycle viability and pair support for face assignments.

use super::root_domains::MeshEndpointCandidates;
#[cfg(test)]
use super::root_domains::{MeshImplicitEdgeCandidateSource, MeshImplicitEdgeCandidates};
#[cfg(test)]
use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;
use crate::solve::missing_edge::{same_unordered_pair, MeshFaceBoundaryAssignment};
use cadmpeg_core::decode::WorkBudget;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
#[cfg(test)]
use std::sync::Arc;

pub(in crate::solve) fn mesh_assignment_endpoint_cycles_viable_by<'a>(
    assignment: &MeshFaceBoundaryAssignment,
    budget: Option<&WorkBudget>,
    candidates: impl Fn(usize) -> Option<MeshEndpointCandidates<'a>>,
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Option<bool> {
    const MAX_LOCAL_ENDPOINT_STATES: usize = 65_536;

    fn endpoint_adjacency(
        candidates: impl IntoIterator<Item = [usize; 2]>,
        allowed: impl Fn([usize; 2]) -> bool,
        budget: Option<&WorkBudget>,
    ) -> Option<HashMap<usize, Vec<usize>>> {
        let mut adjacency = HashMap::<usize, Vec<usize>>::new();
        let mut count = 0usize;
        for pair @ [left, right] in candidates {
            if budget.is_some_and(|budget| !budget.charge()) {
                return None;
            }
            count += 1;
            if count > MAX_LOCAL_ENDPOINT_STATES {
                return None;
            }
            if !allowed(pair) {
                continue;
            }

            adjacency.entry(left).or_default().push(right);
            if right != left {
                adjacency.entry(right).or_default().push(left);
            }
        }
        for neighbors in adjacency.values_mut() {
            neighbors.sort_unstable_by(Ord::cmp);
            neighbors.dedup();
        }
        (!adjacency.is_empty()).then_some(adjacency)
    }

    for boundary in &assignment.boundaries {
        if boundary.is_empty() {
            return Some(false);
        }
        let mut prepared = HashMap::<usize, HashMap<usize, Vec<usize>>>::new();
        for use_ in boundary {
            if prepared.contains_key(&use_.edge) {
                continue;
            }
            let candidate = candidates(use_.edge)?;
            let adjacency = match candidate {
                MeshEndpointCandidates::Explicit(values) => endpoint_adjacency(
                    values.iter().copied(),
                    |pair| allowed(use_.edge, pair),
                    budget,
                ),
                MeshEndpointCandidates::Implicit(values) => {
                    endpoint_adjacency(values, |pair| allowed(use_.edge, pair), budget)
                }
                MeshEndpointCandidates::Selected(value) => {
                    endpoint_adjacency([value], |pair| allowed(use_.edge, pair), budget)
                }
            };
            let adjacency = adjacency?;
            prepared.insert(use_.edge, adjacency);
        }
        let mut states = HashSet::new();
        for (&left, neighbors) in &prepared[&boundary[0].edge] {
            for &right in neighbors {
                if budget.is_some_and(|budget| !budget.charge()) {
                    return None;
                }
                states.insert((left, right));
            }
            if states.len() > MAX_LOCAL_ENDPOINT_STATES {
                return None;
            }
        }
        for use_ in &boundary[1..] {
            let mut next = HashSet::new();
            for &(start, current) in &states {
                for &next_point in prepared[&use_.edge].get(&current).into_iter().flatten() {
                    if budget.is_some_and(|budget| !budget.charge()) {
                        return None;
                    }
                    next.insert((start, next_point));
                    if next.len() > MAX_LOCAL_ENDPOINT_STATES {
                        return None;
                    }
                }
            }
            states = next;
            if states.is_empty() {
                return Some(false);
            }
        }
        if !states.into_iter().any(|(start, current)| start == current) {
            return Some(false);
        }
    }
    Some(true)
}

pub(in crate::solve) fn mesh_assignment_endpoint_cycles_viable_where(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&WorkBudget>,
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Option<bool> {
    mesh_assignment_endpoint_cycles_viable_by(
        assignment,
        budget,
        |edge| {
            edge_candidates
                .get(edge)
                .filter(|candidates| !candidates.is_empty())
                .map(|candidates| MeshEndpointCandidates::Explicit(candidates.as_slice()))
        },
        allowed,
    )
}

#[derive(Debug)]
pub(in crate::solve) struct MeshEndpointPairSupport {
    pub(in crate::solve) by_edge: HashMap<usize, HashSet<[usize; 2]>>,
}

pub(in crate::solve) fn mesh_assignment_endpoint_cycle_support_by<'a>(
    assignment: &MeshFaceBoundaryAssignment,
    budget: Option<&WorkBudget>,
    candidates: impl Fn(usize) -> Option<MeshEndpointCandidates<'a>>,
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Option<MeshEndpointPairSupport> {
    const MAX_LOCAL_ENDPOINT_STATES: usize = 65_536;

    type EndpointRelation = BTreeMap<usize, BTreeSet<usize>>;

    fn compose_relations(
        left: &EndpointRelation,
        right: &EndpointRelation,
        budget: Option<&WorkBudget>,
    ) -> Option<EndpointRelation> {
        let mut composed = EndpointRelation::new();
        let mut state_count = 0usize;
        for (&start, middles) in left {
            for middle in middles {
                let Some(ends) = right.get(middle) else {
                    continue;
                };
                for &end in ends {
                    if budget.is_some_and(|budget| !budget.charge()) {
                        return None;
                    }
                    if composed.entry(start).or_default().insert(end) {
                        state_count += 1;
                        if state_count > MAX_LOCAL_ENDPOINT_STATES {
                            return None;
                        }
                    }
                }
            }
        }
        Some(composed)
    }

    let charge = || budget.is_none_or(WorkBudget::charge);
    let mut assignment_support = HashMap::<usize, HashSet<[usize; 2]>>::new();
    for boundary in &assignment.boundaries {
        if boundary.is_empty() {
            return Some(MeshEndpointPairSupport {
                by_edge: HashMap::new(),
            });
        }
        let mut points = BTreeSet::new();
        let mut layers = Vec::<(usize, Vec<[usize; 2]>, EndpointRelation)>::new();
        for use_ in boundary {
            let values = match candidates(use_.edge)? {
                MeshEndpointCandidates::Explicit(values) => values.to_vec(),
                MeshEndpointCandidates::Implicit(values) => {
                    let mut collected = Vec::new();
                    for value in values.take(MAX_LOCAL_ENDPOINT_STATES + 1) {
                        collected.push(value);
                    }
                    collected
                }
                MeshEndpointCandidates::Selected(value) => {
                    vec![value]
                }
            };
            if values.len() > MAX_LOCAL_ENDPOINT_STATES {
                return None;
            }
            let mut retained = Vec::new();
            let mut relation = EndpointRelation::new();
            for mut pair in values {
                pair.sort_unstable_by(Ord::cmp);
                if !allowed(use_.edge, pair) {
                    continue;
                }
                retained.push(pair);
                for point in pair {
                    points.insert(point);
                }
                for (rank, (start, end)) in [(pair[0], pair[1]), (pair[1], pair[0])]
                    .into_iter()
                    .enumerate()
                {
                    if rank == 1 && pair[0] == pair[1] {
                        continue;
                    }
                    if !charge() {
                        return None;
                    }
                    relation.entry(start).or_default().insert(end);
                }
            }
            retained.sort_unstable_by(Ord::cmp);
            retained.dedup();
            if retained.is_empty() {
                return Some(MeshEndpointPairSupport {
                    by_edge: HashMap::new(),
                });
            }
            layers.push((use_.edge, retained, relation));
        }
        if points.len() > MAX_LOCAL_ENDPOINT_STATES {
            return None;
        }
        let identity = points
            .iter()
            .map(|&point| (point, BTreeSet::from([point])))
            .collect::<EndpointRelation>();
        let mut prefixes = vec![identity.clone()];
        for (index, (_, _, relation)) in layers.iter().enumerate() {
            let composed = compose_relations(&prefixes[index], relation, budget)?;
            prefixes.push(composed);
        }
        let mut suffixes = vec![EndpointRelation::new(); layers.len() + 1];
        suffixes[layers.len()] = identity;
        for layer in (0..layers.len()).rev() {
            suffixes[layer] = compose_relations(&layers[layer].2, &suffixes[layer + 1], budget)?;
        }
        let mut boundary_support = HashMap::<usize, HashSet<[usize; 2]>>::new();
        for (layer, (edge, candidates, _)) in layers.into_iter().enumerate() {
            let mut layer_support = HashSet::new();
            for pair in candidates {
                let supported = [(pair[0], pair[1]), (pair[1], pair[0])]
                    .into_iter()
                    .enumerate()
                    .any(|(rank, (start, end))| {
                        if rank == 1 && pair[0] == pair[1] {
                            return false;
                        }
                        let Some(anchors) = suffixes[layer + 1].get(&end) else {
                            return false;
                        };
                        anchors.iter().any(|anchor| {
                            if !charge() {
                                return false;
                            }
                            prefixes[layer]
                                .get(anchor)
                                .is_some_and(|ends| ends.contains(&start))
                        })
                    });
                if budget.is_some_and(WorkBudget::exhausted) {
                    return None;
                }
                if supported {
                    layer_support.insert(pair);
                }
            }
            boundary_support
                .entry(edge)
                .and_modify(|retained| retained.retain(|pair| layer_support.contains(pair)))
                .or_insert(layer_support);
        }
        if boundary.iter().any(|use_| {
            boundary_support
                .get(&use_.edge)
                .is_none_or(HashSet::is_empty)
        }) {
            return Some(MeshEndpointPairSupport {
                by_edge: HashMap::new(),
            });
        }
        for (edge, supported) in boundary_support {
            assignment_support
                .entry(edge)
                .and_modify(|retained| retained.retain(|pair| supported.contains(pair)))
                .or_insert(supported);
        }
        if assignment_support.values().any(HashSet::is_empty) {
            return Some(MeshEndpointPairSupport {
                by_edge: HashMap::new(),
            });
        }
    }
    Some(MeshEndpointPairSupport {
        by_edge: assignment_support,
    })
}

pub(super) fn mesh_assignment_endpoint_cycles_viable_with(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
    required: Option<(usize, [usize; 2])>,
    budget: Option<&WorkBudget>,
) -> Option<bool> {
    mesh_assignment_endpoint_cycles_viable_where(
        assignment,
        edge_candidates,
        budget,
        |edge, pair| {
            required.is_none_or(|(required_edge, required_pair)| {
                edge != required_edge || same_unordered_pair(pair, required_pair)
            })
        },
    )
}

#[cfg(test)]
pub(in crate::solve) fn mesh_assignment_endpoint_cycles_viable(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
) -> bool {
    mesh_assignment_endpoint_cycles_viable_with(assignment, edge_candidates, None, None)
        .unwrap_or(true)
}

#[test]
fn endpoint_cycle_adjacency_charges_implicit_candidate_enumeration() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    };
    let budget = WorkBudget::new(2);

    assert_eq!(
        mesh_assignment_endpoint_cycles_viable_by(
            &assignment,
            Some(&budget),
            |_| {
                Some(MeshEndpointCandidates::Implicit(
                    MeshImplicitEdgeCandidates {
                        source: MeshImplicitEdgeCandidateSource::Cartesian {
                            domains: Arc::new(vec![vec![0, 1], vec![2, 3]]),
                            left_root: 0,
                            right_root: 1,
                            left_index: 0,
                            right_index: 0,
                            same_root: false,
                        },
                    },
                ))
            },
            |_, _| true
        ),
        None
    );
    assert!(budget.exhausted());
}

#[test]
fn two_edge_endpoint_cycle_is_viable() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 0,
                reversed: None,
            },
        ]],
    };
    let candidates = [vec![[0, 1]], vec![[1, 0]]];
    let run = || {
        mesh_assignment_endpoint_cycles_viable_where(&assignment, &candidates, None, |_, _| true)
    };
    assert_eq!(run(), Some(true));
}
