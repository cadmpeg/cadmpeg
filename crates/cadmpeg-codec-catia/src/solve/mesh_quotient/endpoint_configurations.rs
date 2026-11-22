// SPDX-License-Identifier: Apache-2.0
//! Face endpoint configurations and their boundary directions.

use super::{
    MeshFaceDirectionOptions, MeshFaceEndpointConfiguration, MeshFaceEndpointConfigurations,
    MAX_FACE_ENDPOINT_CONFIGURATION_WORK,
};
use crate::solve::missing_edge::{
    same_unordered_pair, MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment,
};
use cadmpeg_core::decode::WorkBudget;
use std::collections::{HashMap, HashSet};

pub(in crate::solve) fn mesh_face_endpoint_configurations(
    assignments: &[MeshFaceBoundaryAssignment],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[Option<[usize; 2]>],
    budget: &WorkBudget,
) -> Option<MeshFaceEndpointConfigurations> {
    fn insert_pair(
        configuration: &mut MeshFaceEndpointConfiguration,
        edge: usize,
        mut pair: [usize; 2],
    ) -> bool {
        pair.sort_unstable_by(Ord::cmp);
        match configuration.iter().find(|(stored, _)| *stored == edge) {
            Some((_, stored)) => *stored == pair,
            None => {
                configuration.push((edge, pair));
                true
            }
        }
    }

    fn boundary_configurations(
        boundary: &[MeshBoundaryEdgeCandidate],
        edge_candidates: &[Vec<[usize; 2]>],
        selected: &[Option<[usize; 2]>],
        work: &mut usize,
        budget: &WorkBudget,
    ) -> Option<MeshFaceEndpointConfigurations> {
        let charge = |work: &mut usize| {
            *work = work.checked_add(1)?;
            (*work <= MAX_FACE_ENDPOINT_CONFIGURATION_WORK && budget.charge()).then_some(())
        };
        if boundary.is_empty()
            || boundary
                .iter()
                .any(|use_| edge_candidates.get(use_.edge).is_none_or(Vec::is_empty))
        {
            return None;
        }
        let allowed = |edge: usize, pair: [usize; 2]| {
            selected
                .get(edge)
                .copied()
                .flatten()
                .is_none_or(|stored| same_unordered_pair(stored, pair))
        };
        let mut states = Vec::<(usize, usize, MeshFaceEndpointConfiguration)>::new();
        for &pair @ [left, right] in &edge_candidates[boundary[0].edge] {
            if !allowed(boundary[0].edge, pair) {
                continue;
            }
            let directions = [(left, right), (right, left)];
            let direction_count = usize::from(left != right) + 1;
            for &(start, current) in &directions[..direction_count] {
                let mut configuration = Vec::new();
                if insert_pair(&mut configuration, boundary[0].edge, pair) {
                    charge(work)?;
                    states.push((start, current, configuration));
                }
            }
        }
        for use_ in &boundary[1..] {
            let mut next = Vec::new();
            for (start, current, configuration) in states {
                for &pair @ [left, right] in &edge_candidates[use_.edge] {
                    if !allowed(use_.edge, pair) {
                        continue;
                    }
                    let endpoints = [
                        (left == current).then_some(right),
                        (right == current).then_some(left),
                    ];
                    for (index, endpoint) in endpoints.into_iter().enumerate() {
                        if left == right && index == 1 {
                            break;
                        }
                        let Some(endpoint) = endpoint else {
                            continue;
                        };
                        charge(work)?;
                        let mut configuration = configuration.clone();
                        if insert_pair(&mut configuration, use_.edge, pair) {
                            next.push((start, endpoint, configuration));
                        }
                    }
                }
            }
            states = next;
            if states.is_empty() {
                return Some(Vec::new());
            }
        }
        let mut seen = HashSet::new();
        let mut completed = Vec::new();
        for (_, _, mut configuration) in states
            .into_iter()
            .filter(|(start, current, _)| start == current)
        {
            configuration.sort_unstable_by(Ord::cmp);
            if seen.contains(&configuration) {
                continue;
            }
            let key = configuration.clone();
            seen.insert(key);
            completed.push(configuration);
        }
        Some(completed)
    }

    if selected.len() != edge_candidates.len() {
        return None;
    }
    let mut work = 0usize;
    let mut configurations = HashSet::new();
    for assignment in assignments {
        let mut combined = vec![Vec::new()];
        for boundary in &assignment.boundaries {
            let boundary =
                boundary_configurations(boundary, edge_candidates, selected, &mut work, budget)?;
            let mut next = Vec::new();
            for stored in combined {
                for candidate in &boundary {
                    let next_work = work.checked_add(1)?;
                    work = next_work;
                    if work > MAX_FACE_ENDPOINT_CONFIGURATION_WORK || !budget.charge() {
                        return None;
                    }
                    let mut merged = stored.clone();
                    let mut compatible = true;
                    for &(edge, pair) in candidate {
                        if !insert_pair(&mut merged, edge, pair) {
                            compatible = false;
                            break;
                        }
                    }
                    if compatible {
                        merged.sort_unstable_by(Ord::cmp);
                        next.push(merged);
                    }
                }
            }
            combined = next;
        }
        for configuration in combined {
            configurations.insert(configuration);
        }
    }
    let mut results = Vec::new();
    for configuration in configurations {
        results.push(configuration);
    }
    results.sort_unstable_by(Ord::cmp);
    Some(results)
}

/// Return whether an unordered endpoint configuration can close every
/// boundary cycle of one face assignment. Configuration pairs are normalized
/// by `mesh_face_endpoint_configurations`; the checks below still compare
/// them as unordered pairs so callers cannot depend on that representation.
pub(super) fn endpoint_configuration_boundary_cycle_viable(
    boundary: &[MeshBoundaryEdgeCandidate],
    pairs: &HashMap<usize, [usize; 2]>,
) -> Option<bool> {
    if boundary.is_empty() {
        return None;
    }
    let first = boundary.first()?;
    let &first_pair = pairs.get(&first.edge)?;
    let mut states = Vec::new();
    states.push((first_pair[0], first_pair[1]));
    if first_pair[0] != first_pair[1] {
        states.push((first_pair[1], first_pair[0]));
    }
    for use_ in &boundary[1..] {
        let &pair = pairs.get(&use_.edge)?;
        let mut next = Vec::new();
        for (start, current) in states {
            for direction in [false, true] {
                if pair[0] == pair[1] && direction {
                    continue;
                }
                let edge_start = if direction { pair[1] } else { pair[0] };
                let edge_end = if direction { pair[0] } else { pair[1] };
                if edge_start == current {
                    next.push((start, edge_end));
                }
            }
        }
        states = next;
        if states.is_empty() {
            return Some(false);
        }
    }
    Some(states.into_iter().any(|(start, current)| start == current))
}

pub(super) fn endpoint_configuration_cycles_viable(
    assignment: &MeshFaceBoundaryAssignment,
    configuration: &MeshFaceEndpointConfiguration,
) -> Option<bool> {
    let mut pairs = HashMap::new();
    for &(edge, pair) in configuration {
        pairs.insert(edge, pair);
    }
    if pairs.len() != configuration.len() {
        return None;
    }
    if assignment.boundaries.is_empty() {
        return Some(false);
    }
    for boundary in &assignment.boundaries {
        if endpoint_configuration_boundary_cycle_viable(boundary, &pairs) != Some(true) {
            return Some(false);
        }
    }
    Some(true)
}

pub(super) fn endpoint_configuration_for_assignment(
    assignment: &MeshFaceBoundaryAssignment,
    edge_pairs: &[[usize; 2]],
) -> Option<MeshFaceEndpointConfiguration> {
    let mut pairs = HashMap::<usize, [usize; 2]>::new();
    for use_ in assignment.boundaries.iter().flatten() {
        let &pair = edge_pairs.get(use_.edge)?;
        let mut pair = pair;
        pair.sort_unstable_by(Ord::cmp);
        match pairs.get(&use_.edge) {
            Some(previous) if *previous != pair => return None,
            Some(_) => {}
            None => {
                pairs.insert(use_.edge, pair);
            }
        }
    }
    let mut configuration = Vec::new();
    for pair in pairs {
        configuration.push(pair);
    }
    configuration.sort_unstable_by(Ord::cmp);
    Some(configuration)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MeshDirectionEnumerationError {
    Invalid,
    Overflow,
}

pub(super) fn endpoint_configuration_boundary_directions(
    boundary: &[MeshBoundaryEdgeCandidate],
    pairs: &HashMap<usize, [usize; 2]>,
) -> Result<Vec<Vec<bool>>, MeshDirectionEnumerationError> {
    if boundary.is_empty() {
        return Err(MeshDirectionEnumerationError::Invalid);
    }
    let first = &boundary[0];
    let Some(&first_pair) = pairs.get(&first.edge) else {
        return Err(MeshDirectionEnumerationError::Invalid);
    };
    let mut states = Vec::new();
    for direction in [false, true] {
        if first_pair[0] == first_pair[1] && direction {
            continue;
        }
        let start = if direction {
            first_pair[1]
        } else {
            first_pair[0]
        };
        let current = if direction {
            first_pair[0]
        } else {
            first_pair[1]
        };
        let directions = vec![direction];
        states.push((start, current, directions));
    }
    for use_ in &boundary[1..] {
        let Some(&pair) = pairs.get(&use_.edge) else {
            return Err(MeshDirectionEnumerationError::Invalid);
        };
        let mut next = Vec::new();
        for (start, current, directions) in states {
            for direction in [false, true] {
                if pair[0] == pair[1] && direction {
                    continue;
                }
                let edge_start = if direction { pair[1] } else { pair[0] };
                let edge_end = if direction { pair[0] } else { pair[1] };
                if edge_start != current {
                    continue;
                }
                let mut directions = directions.clone();
                directions.push(direction);
                next.push((start, edge_end, directions));
                if next.len() > MAX_FACE_ENDPOINT_CONFIGURATION_WORK {
                    return Err(MeshDirectionEnumerationError::Overflow);
                }
            }
        }
        states = next;
        if states.is_empty() {
            return Ok(Vec::new());
        }
    }
    let mut solutions = Vec::new();
    for (start, current, directions) in states {
        if start == current {
            solutions.push(directions);
        }
    }
    solutions.sort_unstable_by(Ord::cmp);
    solutions.dedup();
    if solutions.len() == 2 && boundary.iter().all(|use_| use_.reversed.is_none()) {
        // An unresolved closed boundary has two traversal orientations. They
        // are the boundary-reversal gauge; retain one deterministic member
        // before combining independent boundaries.
        solutions.truncate(1);
    }
    Ok(solutions)
}

pub(super) fn endpoint_configuration_directions(
    assignment: &MeshFaceBoundaryAssignment,
    configuration: &MeshFaceEndpointConfiguration,
) -> Result<MeshFaceDirectionOptions, MeshDirectionEnumerationError> {
    let mut pairs = HashMap::new();
    for &(edge, pair) in configuration {
        pairs.insert(edge, pair);
    }
    if pairs.len() != configuration.len() {
        return Err(MeshDirectionEnumerationError::Invalid);
    }
    let mut alternatives = vec![Vec::new()];
    for boundary in &assignment.boundaries {
        let boundary_options = endpoint_configuration_boundary_directions(boundary, &pairs)?;
        let mut next = Vec::new();
        for prefix in &alternatives {
            for boundary_directions in &boundary_options {
                let mut alternative = prefix.clone();
                alternative.push(boundary_directions.clone());
                next.push(alternative);
                if next.len() > MAX_FACE_ENDPOINT_CONFIGURATION_WORK {
                    return Err(MeshDirectionEnumerationError::Overflow);
                }
            }
        }
        alternatives = next;
        if alternatives.is_empty() {
            break;
        }
    }
    Ok(alternatives)
}

#[test]
fn endpoint_configurations_do_not_duplicate_closed_point_transitions() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..3)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: edge,
                end: edge + 1,
                reversed: None,
            })
            .collect()],
    };
    let candidates = vec![vec![[0, 0]], vec![[0, 0]], vec![[0, 0]]];
    let budget = WorkBudget::new(4);

    let configurations = {
        mesh_face_endpoint_configurations(
            std::slice::from_ref(&assignment),
            &candidates,
            &[None; 3],
            &budget,
        )
    }
    .expect("closed-point transitions should be deduplicated");

    assert_eq!(configurations.len(), 1);
    assert!(!budget.exhausted());
}

#[test]
fn endpoint_configuration_unresolved_boundary_reversal_is_a_gauge() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![
            vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 1,
                    reversed: None,
                },
            ],
            vec![
                MeshBoundaryEdgeCandidate {
                    edge: 2,
                    start: 2,
                    end: 3,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 3,
                    start: 2,
                    end: 3,
                    reversed: None,
                },
            ],
        ],
    };
    let configuration = vec![(0, [0, 1]), (1, [0, 1]), (2, [2, 3]), (3, [2, 3])];

    let directions = endpoint_configuration_directions(&assignment, &configuration)
        .expect("unresolved boundary directions should enumerate");

    assert_eq!(directions.len(), 1);
    assert_eq!(directions[0].len(), 2);
    assert_eq!(directions[0][0].len(), 2);
    assert_eq!(directions[0][1].len(), 2);
}

#[test]
fn endpoint_configuration_helpers_refuse_missing_or_duplicate_edges() {
    let use_edge = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let invalid_assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![use_edge(0), use_edge(1)]],
    };
    let assignment_result =
        || endpoint_configuration_for_assignment(&invalid_assignment, &[[0, 1]]);
    assert!(assignment_result().is_none());
    let valid_assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![use_edge(0)]],
    };
    let duplicate_configuration = vec![(0, [0, 1]), (0, [0, 1])];
    let cycle_result =
        || endpoint_configuration_cycles_viable(&valid_assignment, &duplicate_configuration);
    assert!(cycle_result().is_none());
    let pairs = HashMap::from([(0, [0, 1]), (1, [1, 0])]);
    let invalid_boundary = vec![use_edge(0), use_edge(1), use_edge(2)];
    let boundary_result =
        || endpoint_configuration_boundary_cycle_viable(&invalid_boundary, &pairs);
    assert!(boundary_result().is_none());
}

#[test]
fn face_endpoint_configurations_combine_two_self_loop_boundaries() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![
            vec![MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 0,
                reversed: None,
            }],
            vec![MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 0,
                end: 0,
                reversed: None,
            }],
        ],
    };
    let candidates = [vec![[0, 0]], vec![[0, 0]]];
    let run = || {
        let budget = WorkBudget::new(32);
        mesh_face_endpoint_configurations(
            std::slice::from_ref(&assignment),
            &candidates,
            &[None; 2],
            &budget,
        )
    };
    assert_eq!(run(), Some(vec![vec![(0, [0, 0]), (1, [0, 0])]]));
}

#[test]
fn two_edge_cycle_has_one_endpoint_direction_assignment() {
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
                start: 0,
                end: 1,
                reversed: None,
            },
        ]],
    };
    let configuration = vec![(0, [0, 1]), (1, [0, 1])];
    assert_eq!(
        endpoint_configuration_directions(&assignment, &configuration)
            .expect("directions")
            .len(),
        1
    );
}
