//! Incidence backtracking constraint solver for standard B-rep topology.
//!
//! Reconstructs face/edge incidence from serialized boundary domains.

use crate::families::standard::topology::{
    incidence_cycles, reconstruct_incidence, EdgeRow, StandardTopology,
};
use crate::solve::mesh_quotient::{
    mesh_assignment_endpoint_cycles_viable_where, mesh_face_endpoint_configurations,
    MeshConstraintBudget, MeshEndpointPair, MeshEndpointSolutionFilter,
    MeshFaceEndpointConfigurations, MeshPartialEndpointConstraint, MeshQuotient,
    MeshQuotientGaugeState, MAX_MESH_CONSTRAINT_OPERATIONS,
};
use crate::solve::missing_edge::{
    bind_edge_port_candidates, same_unordered_pair, MeshBoundaryEdgeCandidate,
    MeshDeferredBoundaryCycle, MeshDeferredFaceBoundary, MeshFaceBoundaryAssignment,
    MeshFaceBoundaryDomain,
};
use crate::solve::UnionFind;
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

pub(crate) fn prune_incidence_choices(
    choices: &mut [Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
) -> Option<()> {
    fn unique_faces(faces: [usize; 2]) -> impl Iterator<Item = usize> {
        faces
            .into_iter()
            .enumerate()
            .filter_map(move |(rank, face)| (rank == 0 || face != faces[0]).then_some(face))
    }

    fn fits(degrees: &[Vec<u8>], edge_faces: &[[usize; 2]], edge: usize, pair: [usize; 2]) -> bool {
        unique_faces(edge_faces[edge]).all(|face| {
            pair.iter().enumerate().all(|(rank, &point)| {
                let multiplicity = 1 + usize::from(rank == 0 && pair[0] == pair[1]);
                usize::from(degrees[face][point]) + multiplicity <= 2
            })
        })
    }

    fn preserves_degree_support(
        choices: &[Vec<[usize; 2]>],
        fixed: &[bool],
        degrees: &[Vec<u8>],
        edge_faces: &[[usize; 2]],
        face_edges: &[Vec<usize>],
        edge: usize,
        pair: [usize; 2],
    ) -> bool {
        unique_faces(edge_faces[edge]).all(|face| {
            degrees[face]
                .iter()
                .copied()
                .enumerate()
                .all(|(point, degree)| {
                    let selected_degree =
                        pair.iter().filter(|candidate| **candidate == point).count();
                    degree + selected_degree as u8 != 1
                        || face_edges[face].iter().copied().any(|supporting_edge| {
                            supporting_edge != edge
                                && !fixed[supporting_edge]
                                && choices[supporting_edge]
                                    .iter()
                                    .any(|candidate| candidate.contains(&point))
                        })
                })
        })
    }

    if choices.len() != edge_faces.len()
        || choices.iter().any(Vec::is_empty)
        || edge_faces.iter().flatten().any(|face| *face >= face_count)
        || choices
            .iter()
            .flatten()
            .flatten()
            .any(|point| *point >= point_count)
    {
        return None;
    }
    let mut face_edges = vec![Vec::new(); face_count];
    for (edge, faces) in edge_faces.iter().copied().enumerate() {
        for face in unique_faces(faces) {
            face_edges[face].push(edge);
        }
    }
    let mut fixed = vec![false; choices.len()];
    let mut degrees = vec![vec![0u8; point_count]; face_count];
    loop {
        let mut changed = false;
        for edge in 0..choices.len() {
            if fixed[edge] {
                continue;
            }
            let before = choices[edge].len();
            let retained = choices[edge]
                .iter()
                .copied()
                .filter(|pair| {
                    fits(&degrees, edge_faces, edge, *pair)
                        && preserves_degree_support(
                            choices,
                            &fixed,
                            &degrees,
                            edge_faces,
                            &face_edges,
                            edge,
                            *pair,
                        )
                })
                .collect::<Vec<_>>();
            choices[edge] = retained;
            changed |= choices[edge].len() != before;
            let [pair] = choices[edge].as_slice() else {
                if choices[edge].is_empty() {
                    return None;
                }
                continue;
            };
            for face in unique_faces(edge_faces[edge]) {
                for point in pair {
                    degrees[face][*point] = degrees[face][*point].checked_add(1)?;
                }
            }
            fixed[edge] = true;
            changed = true;
        }
        for face in 0..face_count {
            for (point, &degree) in degrees[face].iter().enumerate() {
                if degree != 1 {
                    continue;
                }
                let supporting_edges = face_edges[face]
                    .iter()
                    .copied()
                    .filter(|&edge| {
                        !fixed[edge] && choices[edge].iter().any(|pair| pair.contains(&point))
                    })
                    .collect::<Vec<_>>();
                let (&edge, rest) = supporting_edges.split_first()?;
                if rest.iter().all(|candidate| *candidate == edge) {
                    let before = choices[edge].len();
                    choices[edge].retain(|pair| pair.contains(&point));
                    if choices[edge].is_empty() {
                        return None;
                    }
                    changed |= choices[edge].len() != before;
                }
            }
        }
        if !changed {
            return Some(());
        }
    }
}

pub(crate) fn incidence_choice_components(
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    boundary_domains: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
) -> Vec<Vec<usize>> {
    let mut union = UnionFind::new(choices.len());
    let mut owner = HashMap::<(usize, usize), usize>::new();
    let ambiguous = choices
        .iter()
        .enumerate()
        .filter_map(|(edge, pairs)| (pairs.len() > 1).then_some(edge))
        .collect::<Vec<_>>();
    for &edge in &ambiguous {
        let faces = edge_faces[edge];
        for (rank, face) in faces.into_iter().enumerate() {
            if rank > 0 && face == faces[0] {
                continue;
            }
            for point in choices[edge].iter().flatten().copied() {
                match owner.entry((face, point)) {
                    std::collections::hash_map::Entry::Occupied(entry) => {
                        union.union(*entry.get(), edge);
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(edge);
                    }
                }
            }
        }
    }
    if let Some(domains) = boundary_domains {
        let mut connect = |mut edges: Vec<usize>| {
            edges.sort_unstable();
            edges.dedup();
            let mut ambiguous = edges.into_iter().filter(|edge| choices[*edge].len() > 1);
            if let Some(first) = ambiguous.next() {
                for edge in ambiguous {
                    union.union(first, edge);
                }
            }
        };
        for domain in domains {
            match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) if assignments.len() == 1 => {
                    for boundary in &assignments[0].boundaries {
                        connect(boundary.iter().map(|use_| use_.edge).collect());
                    }
                }
                MeshFaceBoundaryDomain::Ordered(assignments) => connect(
                    assignments
                        .iter()
                        .flat_map(|assignment| assignment.boundaries.iter().flatten())
                        .map(|use_| use_.edge)
                        .collect(),
                ),
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => connect(edges.clone()),
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    let mut edges = domain.missing_edges.clone();
                    edges.extend(
                        domain
                            .cycles
                            .iter()
                            .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
                    );
                    connect(edges);
                }
            }
        }
    }
    if let Some(mesh_quotient) = mesh_quotient {
        let mut quotient = mesh_quotient.clone();
        if quotient.union.len() == choices.len().saturating_mul(2) {
            let mut owner = HashMap::<usize, usize>::new();
            for &edge in &ambiguous {
                for port in [edge * 2, edge * 2 + 1] {
                    let root = quotient.union.find(port);
                    for point in quotient.domains[root].iter().copied() {
                        match owner.entry(point) {
                            std::collections::hash_map::Entry::Occupied(entry) => {
                                union.union(*entry.get(), edge);
                            }
                            std::collections::hash_map::Entry::Vacant(entry) => {
                                entry.insert(edge);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut by_root = HashMap::<usize, Vec<usize>>::new();
    for edge in ambiguous {
        by_root.entry(union.find(edge)).or_default().push(edge);
    }
    let mut components = by_root.into_values().collect::<Vec<_>>();
    for component in &mut components {
        component.sort_unstable();
    }
    components.sort_by_key(|component| component[0]);
    components
}

pub(crate) fn join_partial_constraint_components(
    components: Vec<Vec<usize>>,
    coupled_edges: &[bool],
    assignment_predecessors: Option<&[Option<usize>]>,
) -> Vec<Vec<usize>> {
    let component_by_edge = components
        .iter()
        .enumerate()
        .flat_map(|(component, edges)| edges.iter().copied().map(move |edge| (edge, component)))
        .collect::<HashMap<_, _>>();
    let mut union = UnionFind::new(components.len());
    let mut coupled_owner = None;
    for (edge, active) in coupled_edges.iter().copied().enumerate() {
        if !active {
            continue;
        }
        let Some(&component) = component_by_edge.get(&edge) else {
            continue;
        };
        if let Some(owner) = coupled_owner {
            union.union(owner, component);
        } else {
            coupled_owner = Some(component);
        }
    }
    if let Some(predecessors) = assignment_predecessors {
        for (edge, predecessor) in predecessors.iter().copied().enumerate() {
            let Some(predecessor) = predecessor else {
                continue;
            };
            if let (Some(&left), Some(&right)) = (
                component_by_edge.get(&edge),
                component_by_edge.get(&predecessor),
            ) {
                union.union(left, right);
            }
        }
    }
    let mut joined = HashMap::<usize, (usize, Vec<usize>)>::new();
    for (index, component) in components.into_iter().enumerate() {
        let root = union.find(index);
        let entry = joined.entry(root).or_insert_with(|| (index, Vec::new()));
        entry.0 = entry.0.min(index);
        entry.1.extend(component);
    }
    let mut joined = joined.into_values().collect::<Vec<_>>();
    for (_, edges) in &mut joined {
        edges.sort_unstable();
    }
    joined.sort_unstable_by_key(|(first, _)| *first);
    joined.into_iter().map(|(_, edges)| edges).collect()
}

pub(crate) fn order_incidence_components_by_branch_width(
    components: &mut [Vec<usize>],
    choices: &[Vec<[usize; 2]>],
) -> Option<()> {
    if components
        .iter()
        .flatten()
        .any(|edge| *edge >= choices.len())
    {
        return None;
    }
    let branch_width = |component: &[usize]| {
        component.iter().fold(1usize, |width, edge| {
            width.saturating_mul(choices[*edge].len())
        })
    };
    components.sort_by_key(|component| {
        (
            branch_width(component),
            component.len(),
            component.first().copied().unwrap_or_default(),
        )
    });
    Some(())
}

pub(crate) struct IncidenceComponentSearch<'a> {
    pub(crate) choices: &'a [Vec<[usize; 2]>],
    pub(crate) edge_faces: &'a [[usize; 2]],
    pub(crate) face_edges: &'a [Vec<usize>],
    pub(crate) mesh_assignments: Option<&'a [MeshFaceBoundaryDomain]>,
    pub(crate) mesh_quotient: Option<&'a MeshQuotient>,
    pub(crate) active: Vec<bool>,
    pub(crate) edges: &'a [usize],
    pub(crate) constraints: Vec<(usize, usize)>,
    pub(crate) assignment: Vec<Option<[usize; 2]>>,
    pub(crate) degrees: Vec<Vec<u8>>,
    pub(crate) solutions: Vec<Vec<(usize, [usize; 2])>>,
    pub(crate) solution_filter: Option<MeshEndpointSolutionFilter<'a>>,
    pub(crate) partial_solution_filter: Option<MeshPartialEndpointConstraint<'a>>,
    pub(crate) dead_states: HashSet<Vec<Option<[usize; 2]>>>,
    pub(crate) budget: &'a MeshConstraintBudget,
    pub(crate) exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IncidenceSolve<T> {
    Solved(T),
    Rejected,
    Exhausted,
}

impl<T> IncidenceSolve<T> {
    fn into_option(self) -> Option<T> {
        match self {
            Self::Solved(value) => Some(value),
            Self::Rejected | Self::Exhausted => None,
        }
    }
}

pub(crate) fn compact_boundary_domain_viable(
    domain: &MeshFaceBoundaryDomain,
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
) -> bool {
    let edges = match domain {
        MeshFaceBoundaryDomain::Ordered(_) => return true,
        MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => edges.clone(),
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            let mut edges = domain.missing_edges.clone();
            edges.extend(
                domain
                    .cycles
                    .iter()
                    .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
            );
            edges
        }
    };
    let selected_pairs = edges
        .iter()
        .copied()
        .map(|edge| {
            selected
                .filter(|(selected_edge, _)| *selected_edge == edge)
                .map(|(_, pair)| pair)
                .or(assignment[edge])
                .map(|pair| (edge, pair))
        })
        .collect::<Vec<_>>();
    let complete = selected_pairs.iter().all(Option::is_some);
    if matches!(domain, MeshFaceBoundaryDomain::UnorderedFullCycle(_)) && !complete {
        let mut point_nodes = HashMap::new();
        let mut degrees = Vec::<u8>::new();
        let mut components = UnionFind::new(0);
        for (_, pair) in selected_pairs.iter().flatten().copied() {
            let nodes = pair.map(|point| {
                *point_nodes.entry(point).or_insert_with(|| {
                    degrees.push(0);
                    components.push()
                })
            });
            if nodes[0] == nodes[1] {
                degrees[nodes[0]] += 2;
            } else {
                degrees[nodes[0]] += 1;
                degrees[nodes[1]] += 1;
                components.union(nodes[0], nodes[1]);
            }
        }
        if degrees.iter().any(|degree| *degree > 2) {
            return false;
        }
        let mut open_components = HashSet::new();
        for (node, degree) in degrees.into_iter().enumerate() {
            if degree < 2 {
                open_components.insert(components.find(node));
            }
        }
        return (0..components.len()).all(|node| open_components.contains(&components.find(node)));
    }
    let Some(selected_pairs) = selected_pairs.into_iter().collect::<Option<Vec<_>>>() else {
        return true;
    };
    let mut edge_points = vec![[0; 2]; assignment.len()];
    for (edge, pair) in selected_pairs {
        edge_points[edge] = pair;
    }
    match domain {
        MeshFaceBoundaryDomain::Ordered(_) => true,
        MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
            incidence_cycles(edges, &edge_points).is_some_and(|cycles| cycles.len() == 1)
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            deferred_boundary_closes(domain, &edge_points)
        }
    }
}

pub(crate) fn labeled_assignment_endpoint_cycles_viable(
    assignment: &MeshFaceBoundaryAssignment,
    edge_points: &[Option<[usize; 2]>],
    budget: Option<&MeshConstraintBudget>,
) -> bool {
    let directions = |use_: MeshBoundaryEdgeCandidate| match use_.reversed {
        Some(reversed) => [Some(reversed), None],
        None => [Some(false), Some(true)],
    };
    assignment.boundaries.iter().all(|boundary| {
        let Some(first) = boundary.first().copied() else {
            return false;
        };
        directions(first)
            .into_iter()
            .flatten()
            .any(|first_reversed| {
                let Some(first_points) = edge_points.get(first.edge).copied().flatten() else {
                    return false;
                };
                let first_start = first_points[usize::from(first_reversed)];
                let mut ends = HashSet::from([first_points[usize::from(!first_reversed)]]);
                for use_ in &boundary[1..] {
                    let Some(points) = edge_points.get(use_.edge).copied().flatten() else {
                        return false;
                    };
                    let mut next = HashSet::new();
                    for current in ends {
                        for reversed in directions(*use_).into_iter().flatten() {
                            if budget.is_some_and(|budget| !budget.charge()) {
                                return false;
                            }
                            if points[usize::from(reversed)] == current {
                                next.insert(points[usize::from(!reversed)]);
                            }
                        }
                    }
                    if next.is_empty() {
                        return false;
                    }
                    ends = next;
                }
                ends.contains(&first_start)
            })
    })
}

pub(crate) fn advance_compact_boundary_domains<'a>(
    domains: impl IntoIterator<Item = &'a MeshFaceBoundaryDomain>,
    choices: &[Vec<[usize; 2]>],
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
    mut states: Vec<MeshQuotientGaugeState>,
    budget: &MeshConstraintBudget,
) -> Option<Vec<MeshQuotientGaugeState>> {
    const MAX_QUOTIENT_STATES: usize = 4_096;

    let mut ordered = Vec::<Vec<MeshFaceBoundaryAssignment>>::new();
    for domain in domains {
        let edges = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => assignments
                .iter()
                .flat_map(|assignment| assignment.boundaries.iter().flatten())
                .map(|use_| use_.edge)
                .collect::<Vec<_>>(),
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => edges.clone(),
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                let mut edges = domain.missing_edges.clone();
                edges.extend(
                    domain
                        .cycles
                        .iter()
                        .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
                );
                edges
            }
        };
        let Some(edge_points) = edges
            .iter()
            .map(|edge| {
                selected
                    .filter(|(selected_edge, _)| selected_edge == edge)
                    .map(|(_, pair)| pair)
                    .or(assignment[*edge])
                    .map(|pair| (*edge, pair))
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let mut points = vec![[0; 2]; assignment.len()];
        for (edge, pair) in edge_points {
            points[edge] = pair;
        }
        let alternatives = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => assignments.clone(),
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                let [cycle] = incidence_cycles(edges, &points)
                    .and_then(|cycles| <[Vec<(usize, bool)>; 1]>::try_from(cycles).ok())?;
                vec![MeshFaceBoundaryAssignment {
                    boundaries: vec![cycle
                        .into_iter()
                        .map(|(edge, _)| MeshBoundaryEdgeCandidate {
                            edge,
                            start: 0,
                            end: 0,
                            reversed: None,
                        })
                        .collect()],
                }]
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                let materialized = deferred_boundary_assignment(domain, &points)?;
                vec![materialized]
            }
        };
        ordered.push(alternatives);
    }
    if ordered.is_empty() {
        return Some(states);
    }
    let candidates = assignment
        .iter()
        .enumerate()
        .map(|(edge, pair)| {
            selected
                .filter(|(selected_edge, _)| *selected_edge == edge)
                .map(|(_, pair)| vec![pair])
                .or_else(|| pair.map(|pair| vec![pair]))
                .unwrap_or_else(|| choices[edge].clone())
        })
        .collect::<Vec<_>>();
    for alternatives in ordered {
        let mut next = Vec::new();
        let mut signatures = HashSet::new();
        for (state, oriented_edges) in states {
            for face in &alternatives {
                for (_, mut candidate) in state.assignment_options_limited(
                    face,
                    &candidates,
                    &oriented_edges,
                    MAX_QUOTIENT_STATES.saturating_sub(next.len()),
                    Some(budget),
                ) {
                    let mut next_oriented = oriented_edges.clone();
                    next_oriented.extend(face.boundaries.iter().flatten().map(|use_| use_.edge));
                    let mut oriented_signature = next_oriented.iter().copied().collect::<Vec<_>>();
                    oriented_signature.sort_unstable();
                    if signatures.insert((candidate.signature(), oriented_signature)) {
                        next.push((candidate, next_oriented));
                    }
                    if next.len() == MAX_QUOTIENT_STATES {
                        break;
                    }
                }
                if next.len() == MAX_QUOTIENT_STATES || budget.exhausted.get() {
                    break;
                }
            }
            if next.len() == MAX_QUOTIENT_STATES || budget.exhausted.get() {
                break;
            }
        }
        if next.is_empty() || budget.exhausted.get() {
            return None;
        }
        states = next;
    }
    Some(states)
}

#[cfg(test)]
pub(crate) fn compact_boundary_domains_jointly_viable<'a>(
    domains: impl IntoIterator<Item = &'a MeshFaceBoundaryDomain>,
    choices: &[Vec<[usize; 2]>],
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
    quotient: &MeshQuotient,
    budget: &MeshConstraintBudget,
) -> bool {
    advance_compact_boundary_domains(
        domains,
        choices,
        assignment,
        selected,
        vec![(quotient.clone(), HashSet::new())],
        budget,
    )
    .is_some()
}

impl IncidenceComponentSearch<'_> {
    fn degree_candidate_fits(&self, edge: usize, pair: [usize; 2]) -> bool {
        let faces = self.edge_faces[edge];
        faces.into_iter().enumerate().all(|(rank, face)| {
            (rank > 0 && face == faces[0])
                || pair.iter().enumerate().all(|(point_rank, &point)| {
                    let multiplicity = 1 + usize::from(point_rank == 0 && pair[0] == pair[1]);
                    usize::from(self.degrees[face][point]) + multiplicity <= 2
                })
        })
    }

    fn branch_edge_ready(&self, edge: usize) -> bool {
        self.partial_solution_filter
            .and_then(|constraint| constraint.assignment_predecessors)
            .and_then(|predecessors| predecessors.get(edge).copied().flatten())
            .is_none_or(|predecessor| {
                !self.active[predecessor] || self.assignment[predecessor].is_some()
            })
    }

    fn degree_support_preserved(&self, edge: usize, pair: [usize; 2]) -> bool {
        let selected_faces = self.edge_faces[edge];
        let selected_degree = |face: usize, point: usize| {
            let incident = selected_faces[0] == face || selected_faces[1] == face;
            incident.then(|| pair.iter().filter(|candidate| **candidate == point).count())
        };
        let degree_after_selection = |face: usize, point: usize| {
            usize::from(self.degrees[face][point])
                + selected_degree(face, point).unwrap_or_default()
        };
        let supporting_pair_fits = |supporting_edge: usize, supporting_pair: [usize; 2]| {
            let faces = self.edge_faces[supporting_edge];
            faces.into_iter().enumerate().all(|(rank, face)| {
                (rank > 0 && face == faces[0])
                    || supporting_pair
                        .iter()
                        .enumerate()
                        .all(|(point_rank, &point)| {
                            let multiplicity = 1 + usize::from(
                                point_rank == 0 && supporting_pair[0] == supporting_pair[1],
                            );
                            degree_after_selection(face, point) + multiplicity <= 2
                        })
            })
        };

        self.constraints.iter().all(|&(face, point)| {
            degree_after_selection(face, point) != 1
                || self.face_edges[face]
                    .iter()
                    .copied()
                    .any(|supporting_edge| {
                        supporting_edge != edge
                            && self.active[supporting_edge]
                            && self.assignment[supporting_edge].is_none()
                            && self.choices[supporting_edge].iter().copied().any(
                                |supporting_pair| {
                                    supporting_pair.contains(&point)
                                        && supporting_pair_fits(supporting_edge, supporting_pair)
                                },
                            )
                    })
        })
    }

    pub(crate) fn candidate_fits(&self, edge: usize, pair: [usize; 2]) -> bool {
        if !self.degree_candidate_fits(edge, pair) || !self.degree_support_preserved(edge, pair) {
            return false;
        }
        let Some(mesh_assignments) = self.mesh_assignments else {
            return true;
        };
        let mut faces = self.edge_faces[edge].to_vec();
        faces.sort_unstable();
        faces.dedup();
        let viable = faces.into_iter().all(|face| {
            mesh_assignments
                .get(face)
                .is_some_and(|domain| match domain {
                    MeshFaceBoundaryDomain::Ordered(assignments) => {
                        assignments.iter().any(|assignment| {
                            mesh_assignment_endpoint_cycles_viable_where(
                                assignment,
                                self.choices,
                                Some(self.budget),
                                |candidate_edge, candidate_pair| {
                                    let selected = if candidate_edge == edge {
                                        Some(pair)
                                    } else {
                                        self.assignment[candidate_edge]
                                    };
                                    selected.is_none_or(|selected| {
                                        same_unordered_pair(selected, candidate_pair)
                                    })
                                },
                            )
                            .unwrap_or(true)
                        })
                    }
                    _ => {
                        compact_boundary_domain_viable(domain, &self.assignment, Some((edge, pair)))
                    }
                })
        });
        viable && !self.budget.exhausted.get()
    }

    fn constraint_options(&self, face: usize, point: usize) -> Vec<(usize, [usize; 2])> {
        let mut options = self.face_edges[face]
            .iter()
            .copied()
            .filter(|&edge| self.active[edge] && self.assignment[edge].is_none())
            .flat_map(|edge| {
                self.choices[edge]
                    .iter()
                    .copied()
                    .filter(move |pair| pair.contains(&point))
                    .map(move |pair| (edge, pair))
            })
            .filter(|(edge, pair)| self.candidate_fits(*edge, *pair))
            .collect::<Vec<_>>();
        options.sort_unstable();
        options.dedup();
        options
    }

    pub(crate) fn branch_options(&self) -> Option<Vec<(usize, [usize; 2])>> {
        for &edge in self.edges {
            if self.assignment[edge].is_some() || !self.branch_edge_ready(edge) {
                continue;
            }
            let mut viable = self.choices[edge]
                .iter()
                .copied()
                .filter(|pair| self.candidate_fits(edge, *pair));
            let pair = viable.next()?;
            if viable.next().is_none() {
                return Some(vec![(edge, pair)]);
            }
        }
        if let Some(constraint) = self.partial_solution_filter {
            let edge = self
                .edges
                .iter()
                .copied()
                .filter(|&edge| {
                    constraint.active_edges.get(edge) == Some(&true)
                        && self.assignment[edge].is_none()
                        && self.branch_edge_ready(edge)
                })
                .min_by_key(|&edge| {
                    self.choices[edge]
                        .iter()
                        .filter(|pair| self.candidate_fits(edge, **pair))
                        .count()
                });
            if let Some(edge) = edge {
                let options = self.choices[edge]
                    .iter()
                    .copied()
                    .filter(|pair| self.candidate_fits(edge, *pair))
                    .map(|pair| (edge, pair))
                    .collect::<Vec<_>>();
                return (!options.is_empty()).then_some(options);
            }
        }
        let mut constrained = None::<Vec<(usize, [usize; 2])>>;
        for &(face, point) in &self.constraints {
            if self.degrees[face][point] != 1 {
                continue;
            }
            let options = self.constraint_options(face, point);
            if options.is_empty() {
                return None;
            }
            let options = options
                .into_iter()
                .filter(|(edge, _)| self.branch_edge_ready(*edge))
                .collect::<Vec<_>>();
            if options.is_empty() {
                continue;
            }
            if constrained
                .as_ref()
                .is_none_or(|stored| options.len() < stored.len())
            {
                constrained = Some(options);
            }
        }
        if constrained.is_some() {
            return constrained;
        }
        let edge = self
            .edges
            .iter()
            .copied()
            .filter(|&edge| self.assignment[edge].is_none() && self.branch_edge_ready(edge))
            .min_by_key(|&edge| {
                self.choices[edge]
                    .iter()
                    .filter(|pair| self.candidate_fits(edge, **pair))
                    .count()
            });
        Some(edge.map_or_else(Vec::new, |edge| {
            self.choices[edge]
                .iter()
                .copied()
                .filter(|pair| self.candidate_fits(edge, *pair))
                .map(|pair| (edge, pair))
                .collect()
        }))
    }

    pub(crate) fn adjust(&mut self, edge: usize, pair: [usize; 2], increase: bool) {
        let faces = self.edge_faces[edge];
        for (rank, face) in faces.into_iter().enumerate() {
            if rank > 0 && face == faces[0] {
                continue;
            }
            for point in pair {
                if increase {
                    self.degrees[face][point] += 1;
                } else {
                    self.degrees[face][point] -= 1;
                }
            }
        }
    }

    fn advance_ordered_faces(
        &self,
        faces: impl IntoIterator<Item = usize>,
        quotient_states: Vec<MeshQuotientGaugeState>,
    ) -> Option<Vec<MeshQuotientGaugeState>> {
        let Some(mesh_assignments) = self.mesh_assignments else {
            return Some(quotient_states);
        };
        let mut faces = faces.into_iter().collect::<Vec<_>>();
        faces.sort_unstable();
        faces.dedup();
        let viable = faces.iter().copied().all(|face| {
            mesh_assignments
                .get(face)
                .is_some_and(|domain| match domain {
                    MeshFaceBoundaryDomain::Ordered(assignments) => {
                        assignments.iter().any(|assignment| {
                            mesh_assignment_endpoint_cycles_viable_where(
                                assignment,
                                self.choices,
                                Some(self.budget),
                                |edge, pair| {
                                    self.assignment[edge]
                                        .is_none_or(|selected| same_unordered_pair(selected, pair))
                                },
                            )
                            .unwrap_or(true)
                        })
                    }
                    _ => compact_boundary_domain_viable(domain, &self.assignment, None),
                })
        });
        if !viable || self.budget.exhausted.get() {
            return None;
        }
        if quotient_states.is_empty() {
            Some(quotient_states)
        } else {
            advance_compact_boundary_domains(
                faces.iter().filter_map(|face| mesh_assignments.get(*face)),
                self.choices,
                &self.assignment,
                None,
                quotient_states,
                self.budget,
            )
        }
    }

    #[cfg(test)]
    pub(crate) fn ordered_faces_feasible(&self, faces: impl IntoIterator<Item = usize>) -> bool {
        let states = self.mesh_quotient.map_or_else(Vec::new, |quotient| {
            vec![(quotient.clone(), HashSet::new())]
        });
        self.advance_ordered_faces(faces, states).is_some()
    }

    pub(crate) fn face_configuration_options(&self) -> Option<MeshFaceEndpointConfigurations> {
        let mesh_assignments = self.mesh_assignments?;
        let mut faces = mesh_assignments
            .iter()
            .enumerate()
            .filter_map(|(face, domain)| {
                let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                    return None;
                };
                let mut has_unresolved = false;
                let width = self.face_edges[face]
                    .iter()
                    .copied()
                    .filter(|edge| self.active[*edge] && self.assignment[*edge].is_none())
                    .fold(assignments.len().max(1), |width, edge| {
                        has_unresolved = true;
                        width.saturating_mul(self.choices[edge].len())
                    });
                if !has_unresolved {
                    return None;
                }
                Some((width, face, assignments))
            })
            .collect::<Vec<_>>();
        faces.sort_by_key(|(width, face, _)| (*width, *face));
        for (_, _, assignments) in faces {
            if !self.budget.charge() {
                return Some(Vec::new());
            }
            let Some(configurations) = mesh_face_endpoint_configurations(
                assignments,
                self.choices,
                &self.assignment,
                self.budget,
            ) else {
                continue;
            };
            let mut projected = configurations
                .into_iter()
                .map(|configuration| {
                    configuration
                        .into_iter()
                        .filter(|(edge, _)| self.active[*edge] && self.assignment[*edge].is_none())
                        .collect::<Vec<_>>()
                })
                .collect::<HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            projected.sort_unstable();
            if projected.is_empty() {
                return Some(Vec::new());
            }
            if projected.iter().all(Vec::is_empty) {
                continue;
            }
            return Some(projected);
        }
        None
    }

    fn search_face_configurations(
        &mut self,
        options: MeshFaceEndpointConfigurations,
        quotient_states: &[MeshQuotientGaugeState],
    ) {
        for option in options {
            let mut assigned = Vec::new();
            let mut affected_faces = HashSet::new();
            let mut viable = true;
            for (edge, pair) in option {
                if !self.active[edge] || self.assignment[edge].is_some() {
                    continue;
                }
                if !self.candidate_fits(edge, pair) {
                    if self.budget.exhausted.get() {
                        self.exhausted = true;
                    }
                    viable = false;
                    break;
                }
                self.adjust(edge, pair, true);
                self.assignment[edge] = Some(pair);
                assigned.push((edge, pair));
                affected_faces.extend(self.edge_faces[edge]);
            }
            if viable
                && !assigned.is_empty()
                && self
                    .partial_solution_filter
                    .is_none_or(|constraint| (constraint.valid)(&self.assignment))
            {
                if let Some(next_states) =
                    self.advance_ordered_faces(affected_faces, quotient_states.to_vec())
                {
                    self.search_with_quotient(&next_states);
                }
            }
            for (edge, pair) in assigned.into_iter().rev() {
                self.assignment[edge] = None;
                self.adjust(edge, pair, false);
            }
            if self.exhausted {
                return;
            }
        }
    }

    pub(crate) fn search(&mut self) {
        let quotient_states = self.mesh_quotient.map_or_else(Vec::new, |quotient| {
            vec![(quotient.clone(), HashSet::new())]
        });
        self.search_with_quotient(&quotient_states);
    }

    fn search_with_quotient(&mut self, quotient_states: &[MeshQuotientGaugeState]) {
        if self.exhausted {
            return;
        }
        if !self.budget.charge() {
            self.exhausted = true;
            return;
        }
        let state = self
            .edges
            .iter()
            .map(|&edge| self.assignment[edge])
            .collect::<Vec<_>>();
        if self.dead_states.contains(&state) {
            return;
        }
        let solutions_before = self.solutions.len();
        self.search_state(quotient_states);
        if !self.exhausted && self.solutions.len() == solutions_before {
            self.dead_states.insert(state);
        }
    }

    fn search_state(&mut self, quotient_states: &[MeshQuotientGaugeState]) {
        const MAX_SOLUTIONS: usize = 256;
        if self.exhausted {
            return;
        }
        if self.solutions.len() >= MAX_SOLUTIONS {
            self.exhausted = true;
            return;
        }
        let face_options = self.face_configuration_options();
        if self.budget.exhausted.get() {
            self.exhausted = true;
            return;
        }
        if let Some(options) = face_options {
            if !options.is_empty() {
                self.search_face_configurations(options, quotient_states);
            }
            return;
        }
        let Some(options) = self.branch_options() else {
            return;
        };
        if options.is_empty() {
            if self
                .edges
                .iter()
                .any(|&edge| self.assignment[edge].is_none())
                || self
                    .constraints
                    .iter()
                    .any(|&(face, point)| self.degrees[face][point] == 1)
            {
                return;
            }
            let solution = self
                .edges
                .iter()
                .map(|&edge| Some((edge, self.assignment[edge]?)))
                .collect::<Option<Vec<_>>>()
                .expect("every component edge is assigned");
            if self
                .solution_filter
                .is_some_and(|filter| !filter(&solution))
            {
                return;
            }
            self.solutions.push(solution);
            return;
        }
        for (edge, pair) in options {
            if self.assignment[edge].is_some() {
                continue;
            }
            if !self.candidate_fits(edge, pair) {
                if self.budget.exhausted.get() {
                    self.exhausted = true;
                    return;
                }
                continue;
            }
            self.adjust(edge, pair, true);
            self.assignment[edge] = Some(pair);
            let mut faces = self.edge_faces[edge].to_vec();
            faces.sort_unstable();
            faces.dedup();
            if self
                .partial_solution_filter
                .is_none_or(|constraint| (constraint.valid)(&self.assignment))
            {
                if let Some(next_states) =
                    self.advance_ordered_faces(faces, quotient_states.to_vec())
                {
                    self.search_with_quotient(&next_states);
                }
            }
            self.assignment[edge] = None;
            self.adjust(edge, pair, false);
        }
    }
}

fn deferred_boundary_cycle_assignment(
    mesh: &MeshDeferredBoundaryCycle,
    incidence: &[(usize, bool)],
    missing: &HashSet<usize>,
) -> Option<Vec<MeshBoundaryEdgeCandidate>> {
    if mesh.exact_uses.is_empty() {
        return (incidence.len() <= mesh.length
            && incidence.iter().all(|(edge, _)| missing.contains(edge)))
        .then(|| {
            incidence
                .iter()
                .map(|(edge, _)| MeshBoundaryEdgeCandidate {
                    edge: *edge,
                    start: 0,
                    end: 0,
                    reversed: None,
                })
                .collect()
        });
    }
    let expected = mesh
        .exact_uses
        .iter()
        .map(|(use_, _)| use_.edge)
        .collect::<Vec<_>>();
    for reversed in [false, true] {
        let mut actual = incidence.iter().map(|(edge, _)| *edge).collect::<Vec<_>>();
        if reversed {
            actual.reverse();
        }
        let Some(anchor) = actual.iter().position(|edge| *edge == expected[0]) else {
            continue;
        };
        actual.rotate_left(anchor);
        let mut positions = Vec::with_capacity(expected.len());
        let mut after = 0usize;
        let mut valid = true;
        for edge in &expected {
            let Some(offset) = actual[after..].iter().position(|actual| actual == edge) else {
                valid = false;
                break;
            };
            let position = after + offset;
            positions.push(position);
            after = position + 1;
        }
        if !valid || positions.len() != expected.len() {
            continue;
        }
        for index in 0..expected.len() {
            let left_position = positions[index];
            let right_position = if index + 1 == expected.len() {
                positions[0] + actual.len()
            } else {
                positions[index + 1]
            };
            let between = right_position - left_position - 1;
            let (left, left_span) = mesh.exact_uses[index];
            let right = mesh.exact_uses[(index + 1) % expected.len()].0;
            let left_end = (left.start + left_span) % mesh.length;
            let capacity = (right.start + mesh.length - left_end) % mesh.length;
            if (capacity == 0 && between != 0)
                || (capacity > 0 && !(1..=capacity).contains(&between))
            {
                valid = false;
                break;
            }
            if (1..=between).any(|offset| {
                let edge = actual[(left_position + offset) % actual.len()];
                !missing.contains(&edge)
            }) {
                valid = false;
                break;
            }
        }
        if valid {
            let exact = mesh
                .exact_uses
                .iter()
                .map(|(use_, _)| (use_.edge, *use_))
                .collect::<HashMap<_, _>>();
            return Some(
                actual
                    .into_iter()
                    .map(|edge| {
                        exact
                            .get(&edge)
                            .copied()
                            .unwrap_or(MeshBoundaryEdgeCandidate {
                                edge,
                                start: 0,
                                end: 0,
                                reversed: None,
                            })
                    })
                    .collect(),
            );
        }
    }
    None
}

pub(crate) fn deferred_boundary_cycle_matches(
    mesh: &MeshDeferredBoundaryCycle,
    incidence: &[(usize, bool)],
    missing: &HashSet<usize>,
) -> bool {
    deferred_boundary_cycle_assignment(mesh, incidence, missing).is_some()
}

fn augment_cycle_matching(
    mesh: usize,
    compatible: &[Vec<bool>],
    seen: &mut [bool],
    matched_mesh: &mut [Option<usize>],
) -> bool {
    for incidence in 0..compatible[mesh].len() {
        if !compatible[mesh][incidence] || seen[incidence] {
            continue;
        }
        seen[incidence] = true;
        let previous = matched_mesh[incidence];
        if previous.is_none()
            || augment_cycle_matching(
                previous.expect("occupied incidence match"),
                compatible,
                seen,
                matched_mesh,
            )
        {
            matched_mesh[incidence] = Some(mesh);
            return true;
        }
    }
    false
}

pub(crate) fn deferred_boundary_assignment(
    domain: &MeshDeferredFaceBoundary,
    edge_points: &[[usize; 2]],
) -> Option<MeshFaceBoundaryAssignment> {
    let mut incident = domain.missing_edges.clone();
    incident.extend(
        domain
            .cycles
            .iter()
            .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
    );
    incident.sort_unstable();
    incident.dedup();
    let incidence = incidence_cycles(&incident, edge_points)?;
    if incidence.len() != domain.cycles.len() {
        return None;
    }
    let missing = domain.missing_edges.iter().copied().collect::<HashSet<_>>();
    let compatible = domain
        .cycles
        .iter()
        .map(|mesh| {
            incidence
                .iter()
                .map(|candidate| deferred_boundary_cycle_assignment(mesh, candidate, &missing))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let boolean_compatible = compatible
        .iter()
        .map(|cycles| cycles.iter().map(Option::is_some).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut matched_mesh = vec![None; incidence.len()];
    for mesh in 0..domain.cycles.len() {
        if !augment_cycle_matching(
            mesh,
            &boolean_compatible,
            &mut vec![false; incidence.len()],
            &mut matched_mesh,
        ) {
            return None;
        }
    }
    let mut boundaries = vec![None; domain.cycles.len()];
    for (incidence, mesh) in matched_mesh.into_iter().enumerate() {
        let mesh = mesh?;
        boundaries[mesh].clone_from(&compatible[mesh][incidence]);
    }
    Some(MeshFaceBoundaryAssignment {
        boundaries: boundaries.into_iter().collect::<Option<Vec<_>>>()?,
    })
}

pub(crate) fn deferred_boundary_closes(
    domain: &MeshDeferredFaceBoundary,
    edge_points: &[[usize; 2]],
) -> bool {
    let mut incident = domain.missing_edges.clone();
    incident.extend(
        domain
            .cycles
            .iter()
            .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
    );
    incident.sort_unstable();
    incident.dedup();
    let Some(incidence) = incidence_cycles(&incident, edge_points) else {
        return false;
    };
    if incidence.len() != domain.cycles.len() {
        return false;
    }
    let missing = domain.missing_edges.iter().copied().collect::<HashSet<_>>();
    let compatible = domain
        .cycles
        .iter()
        .map(|mesh| {
            incidence
                .iter()
                .map(|candidate| deferred_boundary_cycle_matches(mesh, candidate, &missing))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut matched_mesh = vec![None; incidence.len()];
    (0..domain.cycles.len()).all(|mesh| {
        augment_cycle_matching(
            mesh,
            &compatible,
            &mut vec![false; incidence.len()],
            &mut matched_mesh,
        )
    })
}

fn boundary_domains_close(
    domains: Option<&[MeshFaceBoundaryDomain]>,
    edge_points: &[[usize; 2]],
) -> bool {
    domains.is_none_or(|domains| {
        domains.iter().all(|domain| match domain {
            MeshFaceBoundaryDomain::Ordered(_) => true,
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                incidence_cycles(edges, edge_points).is_some_and(|cycles| cycles.len() == 1)
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                deferred_boundary_closes(domain, edge_points)
            }
        })
    })
}

fn completed_incidence_faces_close(
    faces: &HashSet<usize>,
    assignment: &[Option<[usize; 2]>],
    face_edges: &[Vec<usize>],
    domains: Option<&[MeshFaceBoundaryDomain]>,
) -> bool {
    faces.iter().copied().all(|face| {
        let mut points = vec![[0; 2]; assignment.len()];
        for &edge in &face_edges[face] {
            let Some(pair) = assignment[edge] else {
                return false;
            };
            points[edge] = pair;
        }
        if incidence_cycles(&face_edges[face], &points).is_none() {
            return false;
        }
        let Some(domain) = domains.and_then(|domains| domains.get(face)) else {
            return true;
        };
        match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                assignments.iter().any(|boundary_assignment| {
                    labeled_assignment_endpoint_cycles_viable(boundary_assignment, assignment, None)
                })
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                incidence_cycles(edges, &points).is_some_and(|cycles| cycles.len() == 1)
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                deferred_boundary_closes(domain, &points)
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn component_incidence_pair_solutions<F>(
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
) -> Option<Vec<Vec<[usize; 2]>>>
where
    F: Fn(&[[usize; 2]]) -> bool,
{
    component_incidence_pair_solution_outcome(
        choices,
        edge_faces,
        face_count,
        point_count,
        mesh_assignments,
        mesh_quotient,
        partial_solution_valid,
        solution_valid,
    )
    .into_option()
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn component_incidence_pair_solution_outcome<F>(
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
) -> IncidenceSolve<Vec<Vec<[usize; 2]>>>
where
    F: Fn(&[[usize; 2]]) -> bool,
{
    const MAX_PAIR_SOLUTIONS: usize = 256;
    let mut solutions = Vec::new();
    let mut result_limit_exhausted = false;
    let outcome = visit_component_incidence_pair_solutions(
        choices,
        edge_faces,
        face_count,
        point_count,
        mesh_assignments,
        mesh_quotient,
        partial_solution_valid,
        solution_valid,
        &mut |pairs| {
            if solutions.len() == MAX_PAIR_SOLUTIONS {
                result_limit_exhausted = true;
                ControlFlow::Break(())
            } else {
                solutions.push(pairs.to_vec());
                ControlFlow::Continue(())
            }
        },
    );
    match outcome {
        IncidenceSolve::Solved(_) if result_limit_exhausted => IncidenceSolve::Exhausted,
        IncidenceSolve::Solved(_) => IncidenceSolve::Solved(solutions),
        IncidenceSolve::Rejected => IncidenceSolve::Rejected,
        IncidenceSolve::Exhausted => IncidenceSolve::Exhausted,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn visit_component_incidence_pair_solutions<F, V>(
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
    visitor: &mut V,
) -> IncidenceSolve<usize>
where
    F: Fn(&[[usize; 2]]) -> bool,
    V: FnMut(&[[usize; 2]]) -> ControlFlow<()>,
{
    struct ComponentDomain {
        solutions: Vec<Vec<MeshEndpointPair>>,
        exhausted: bool,
    }

    #[allow(clippy::too_many_arguments)]
    fn solve_component_domain(
        component: &[usize],
        choices: &[Vec<[usize; 2]>],
        edge_faces: &[[usize; 2]],
        face_edges: &[Vec<usize>],
        mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
        mesh_quotient: Option<&MeshQuotient>,
        partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
        assignment: &[Option<[usize; 2]>],
        degrees: &[Vec<u8>],
        budget: &MeshConstraintBudget,
    ) -> ComponentDomain {
        let mut active = vec![false; choices.len()];
        let mut constraints = HashSet::<(usize, usize)>::new();
        let mut component_faces = HashSet::new();
        for &edge in component {
            active[edge] = true;
            let faces = edge_faces[edge];
            for (rank, face) in faces.into_iter().enumerate() {
                if rank > 0 && face == faces[0] {
                    continue;
                }
                component_faces.insert(face);
                for point in choices[edge].iter().flatten() {
                    constraints.insert((face, *point));
                }
            }
        }
        let mut constraints = constraints.into_iter().collect::<Vec<_>>();
        constraints.sort_unstable();
        let filter = |solution: &[MeshEndpointPair]| {
            let mut completed = assignment.to_vec();
            for &(edge, pair) in solution {
                completed[edge] = Some(pair);
            }
            completed_incidence_faces_close(
                &component_faces,
                &completed,
                face_edges,
                mesh_assignments,
            ) && partial_solution_valid.is_none_or(|constraint| (constraint.valid)(&completed))
        };
        let solution_filter = Some(&filter as &dyn Fn(&[MeshEndpointPair]) -> bool);
        let mut search = IncidenceComponentSearch {
            choices,
            edge_faces,
            face_edges,
            mesh_assignments,
            mesh_quotient,
            active,
            edges: component,
            constraints,
            assignment: assignment.to_vec(),
            degrees: degrees.to_vec(),
            solutions: Vec::new(),
            solution_filter,
            partial_solution_filter: partial_solution_valid,
            dead_states: HashSet::new(),
            budget,
            exhausted: false,
        };
        search.search();
        ComponentDomain {
            solutions: search.solutions,
            exhausted: search.exhausted,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_components<F, V>(
        component_index: usize,
        edge_faces: &[[usize; 2]],
        mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
        mesh_quotient: Option<&MeshQuotient>,
        solution_valid: &F,
        assignment: &mut [Option<[usize; 2]>],
        degrees: &mut [Vec<u8>],
        point_count: usize,
        budget: &MeshConstraintBudget,
        visitor: &mut V,
        visited: &mut usize,
        component_domains: &[ComponentDomain],
    ) -> Result<ControlFlow<()>, ()>
    where
        F: Fn(&[[usize; 2]]) -> bool,
        V: FnMut(&[[usize; 2]]) -> ControlFlow<()>,
    {
        let Some(domain) = component_domains.get(component_index) else {
            let pairs = assignment
                .iter()
                .copied()
                .collect::<Option<Vec<_>>>()
                .ok_or(())?;
            if !boundary_domains_close(mesh_assignments, &pairs) || !solution_valid(&pairs) {
                return Ok(ControlFlow::Continue(()));
            }
            if mesh_assignments.is_none() {
                if let Some(quotient) = mesh_quotient {
                    let singleton = pairs
                        .iter()
                        .copied()
                        .map(|pair| vec![pair])
                        .collect::<Vec<_>>();
                    let mut quotient = quotient.clone();
                    if !quotient.point_assignment_exists(point_count, &singleton, Some(budget)) {
                        if budget.exhausted.get() {
                            return Err(());
                        }
                        return Ok(ControlFlow::Continue(()));
                    }
                }
            }
            *visited = visited.checked_add(1).ok_or(())?;
            return Ok(visitor(&pairs));
        };

        let mut downstream_exhausted = domain.exhausted;
        for solution in &domain.solutions {
            if !budget.charge() {
                downstream_exhausted = true;
                break;
            }
            for &(edge, pair) in solution {
                assignment[edge] = Some(pair);
                for (rank, face) in edge_faces[edge].into_iter().enumerate() {
                    if rank > 0 && face == edge_faces[edge][0] {
                        continue;
                    }
                    for point in pair {
                        degrees[face][point] += 1;
                    }
                }
            }
            let control = visit_components(
                component_index + 1,
                edge_faces,
                mesh_assignments,
                mesh_quotient,
                solution_valid,
                assignment,
                degrees,
                point_count,
                budget,
                visitor,
                visited,
                component_domains,
            );
            for &(edge, pair) in solution.iter().rev() {
                assignment[edge] = None;
                for (rank, face) in edge_faces[edge].into_iter().enumerate() {
                    if rank > 0 && face == edge_faces[edge][0] {
                        continue;
                    }
                    for point in pair {
                        degrees[face][point] -= 1;
                    }
                }
            }
            match control {
                Ok(ControlFlow::Break(())) => return Ok(ControlFlow::Break(())),
                Ok(ControlFlow::Continue(())) => {}
                Err(()) => downstream_exhausted = true,
            }
        }
        if downstream_exhausted {
            Err(())
        } else {
            Ok(ControlFlow::Continue(()))
        }
    }

    let mut exhausted = false;
    let mut visited = 0usize;
    let result = (|| {
        if partial_solution_valid.is_some_and(|constraint| {
            constraint.active_edges.len() != choices.len()
                || constraint.coupled_edges.len() != choices.len()
                || constraint
                    .assignment_predecessors
                    .is_some_and(|predecessors| predecessors.len() != choices.len())
        }) {
            return None;
        }
        let mut components =
            incidence_choice_components(choices, edge_faces, mesh_assignments, mesh_quotient);
        if let Some(constraint) = partial_solution_valid {
            components = join_partial_constraint_components(
                components,
                constraint.coupled_edges,
                constraint.assignment_predecessors,
            );
        }
        order_incidence_components_by_branch_width(&mut components, choices)?;
        let mut face_edges = vec![Vec::new(); face_count];
        for (edge, faces) in edge_faces.iter().copied().enumerate() {
            for (rank, face) in faces.into_iter().enumerate() {
                if (rank == 0 || face != faces[0]) && !face_edges[face].contains(&edge) {
                    face_edges[face].push(edge);
                }
            }
        }
        let mut fixed = vec![None; choices.len()];
        let mut degrees = vec![vec![0u8; point_count]; face_count];
        for (edge, pairs) in choices.iter().enumerate() {
            let [pair] = pairs.as_slice() else {
                continue;
            };
            fixed[edge] = Some(*pair);
            let faces = edge_faces[edge];
            for (rank, face) in faces.into_iter().enumerate() {
                if rank > 0 && face == faces[0] {
                    continue;
                }
                for point in pair {
                    degrees[face][*point] = degrees[face][*point].checked_add(1)?;
                }
            }
        }
        if components.is_empty() {
            let pairs = fixed.into_iter().collect::<Option<Vec<_>>>()?;
            if boundary_domains_close(mesh_assignments, &pairs) && solution_valid(&pairs) {
                visited = 1;
                let _ = visitor(&pairs);
                return Some(());
            }
            return None;
        }
        let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        let mut component_domains = Vec::with_capacity(components.len());
        for component in components {
            let domain = solve_component_domain(
                &component,
                choices,
                edge_faces,
                &face_edges,
                mesh_assignments,
                mesh_quotient,
                partial_solution_valid,
                &fixed,
                &degrees,
                &budget,
            );
            if domain.solutions.is_empty() && domain.exhausted {
                exhausted = true;
                return None;
            }
            if domain.solutions.is_empty() {
                return None;
            }
            component_domains.push((component, domain));
        }
        component_domains.sort_by_key(|(component, domain)| {
            (
                domain.exhausted,
                domain.solutions.len(),
                component.len(),
                component.first().copied().unwrap_or_default(),
            )
        });
        let component_domains = component_domains
            .into_iter()
            .map(|(_, domain)| domain)
            .collect::<Vec<_>>();
        if visit_components(
            0,
            edge_faces,
            mesh_assignments,
            mesh_quotient,
            solution_valid,
            &mut fixed,
            &mut degrees,
            point_count,
            &budget,
            visitor,
            &mut visited,
            &component_domains,
        )
        .is_err()
        {
            exhausted = true;
            return None;
        }
        Some(())
    })();
    match result {
        Some(()) if visited > 0 => IncidenceSolve::Solved(visited),
        Some(()) => IncidenceSolve::Rejected,
        None if exhausted => IncidenceSolve::Exhausted,
        None => IncidenceSolve::Rejected,
    }
}

pub(crate) fn reconstruct_incidence_candidates(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_ports: Option<&[[u32; 2]]>,
    face_count: usize,
) -> Option<StandardTopology> {
    if edge_ports.is_some_and(|ports| ports.len() != edge_candidates.len()) {
        return None;
    }
    let port_compatible = |pairs: &[[usize; 2]]| {
        edge_ports.is_none_or(|ports| {
            let singleton = pairs
                .iter()
                .copied()
                .map(|pair| vec![pair])
                .collect::<Vec<_>>();
            bind_edge_port_candidates(ports, &singleton).is_some()
        })
    };
    let pair_solutions = incidence_endpoint_pair_solutions(
        edge_rows,
        vertex_points,
        edge_faces,
        edge_candidates,
        face_count,
        None,
        None,
        None,
        &port_compatible,
    )?;
    let mut solution = None;
    for pairs in pair_solutions {
        let pairs = match edge_ports {
            Some(ports) => {
                let singleton = pairs.into_iter().map(|pair| vec![pair]).collect::<Vec<_>>();
                bind_edge_port_candidates(ports, &singleton)?
            }
            None => pairs,
        };
        let candidate = reconstruct_incidence(
            edge_rows.to_vec(),
            vertex_points.to_vec(),
            edge_faces,
            &pairs,
            face_count,
        )?;
        match &solution {
            Some(stored) if *stored != candidate => return None,
            None => solution = Some(candidate),
            Some(_) => {}
        }
    }
    solution
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn incidence_endpoint_pair_solutions<F>(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    face_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
) -> Option<Vec<Vec<[usize; 2]>>>
where
    F: Fn(&[[usize; 2]]) -> bool,
{
    incidence_endpoint_pair_solution_outcome(
        edge_rows,
        vertex_points,
        edge_faces,
        edge_candidates,
        face_count,
        mesh_assignments,
        mesh_quotient,
        partial_solution_valid,
        solution_valid,
    )
    .into_option()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn incidence_endpoint_pair_solution_outcome<F>(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    face_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
) -> IncidenceSolve<Vec<Vec<[usize; 2]>>>
where
    F: Fn(&[[usize; 2]]) -> bool,
{
    const MAX_PAIR_SOLUTIONS: usize = 256;
    let mut solutions = Vec::new();
    let mut result_limit_exhausted = false;
    let outcome = visit_incidence_endpoint_pair_solutions(
        edge_rows,
        vertex_points,
        edge_faces,
        edge_candidates,
        face_count,
        mesh_assignments,
        mesh_quotient,
        partial_solution_valid,
        solution_valid,
        &mut |pairs| {
            if solutions.len() == MAX_PAIR_SOLUTIONS {
                result_limit_exhausted = true;
                ControlFlow::Break(())
            } else {
                solutions.push(pairs.to_vec());
                ControlFlow::Continue(())
            }
        },
    );
    match outcome {
        IncidenceSolve::Solved(_) if result_limit_exhausted => IncidenceSolve::Exhausted,
        IncidenceSolve::Solved(_) if solutions.is_empty() => IncidenceSolve::Rejected,
        IncidenceSolve::Solved(_) => IncidenceSolve::Solved(solutions),
        IncidenceSolve::Rejected => IncidenceSolve::Rejected,
        IncidenceSolve::Exhausted => IncidenceSolve::Exhausted,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn visit_incidence_endpoint_pair_solutions<F, V>(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    face_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
    visitor: &mut V,
) -> IncidenceSolve<usize>
where
    F: Fn(&[[usize; 2]]) -> bool,
    V: FnMut(&[[usize; 2]]) -> ControlFlow<()>,
{
    let Some(choices) = (|| {
        let mut choices = edge_candidates.to_vec();
        for candidates in &mut choices {
            for pair in candidates.iter_mut() {
                pair.sort_unstable();
            }
            candidates.sort_unstable();
            candidates.dedup();
        }
        prune_incidence_choices(&mut choices, edge_faces, face_count, vertex_points.len())?;
        Some(choices)
    })() else {
        return IncidenceSolve::Rejected;
    };
    let complete_valid = |points: &[[usize; 2]]| {
        solution_valid(points)
            && reconstruct_incidence(
                edge_rows.to_vec(),
                vertex_points.to_vec(),
                edge_faces,
                points,
                face_count,
            )
            .is_some()
    };
    visit_component_incidence_pair_solutions(
        &choices,
        edge_faces,
        face_count,
        vertex_points.len(),
        mesh_assignments,
        mesh_quotient,
        partial_solution_valid,
        &complete_valid,
        visitor,
    )
}
