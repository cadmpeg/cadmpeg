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
            choices[edge].retain(|pair| fits(&degrees, edge_faces, edge, *pair));
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
        for domain in domains {
            let mut constrained = match domain {
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
            constrained.sort_unstable();
            constrained.dedup();
            let mut ambiguous = constrained
                .into_iter()
                .filter(|edge| choices[*edge].len() > 1);
            if let Some(first) = ambiguous.next() {
                for edge in ambiguous {
                    union.union(first, edge);
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
    pub(crate) states: usize,
    pub(crate) exhausted: bool,
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
    let Some(selected_pairs) = edges
        .into_iter()
        .map(|edge| {
            selected
                .filter(|(selected_edge, _)| *selected_edge == edge)
                .map(|(_, pair)| pair)
                .or(assignment[edge])
                .map(|pair| (edge, pair))
        })
        .collect::<Option<Vec<_>>>()
    else {
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
    fn charge_branch(&mut self, option_count: usize) -> bool {
        const MAX_STATES: usize = 4_096;

        if option_count <= 1 {
            return true;
        }
        if self.states >= MAX_STATES {
            self.exhausted = true;
            return false;
        }
        self.states += 1;
        true
    }

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

    pub(crate) fn candidate_fits(&self, edge: usize, pair: [usize; 2]) -> bool {
        if !self.degree_candidate_fits(edge, pair) {
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
            if self.assignment[edge].is_some() {
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
            .filter(|&edge| self.assignment[edge].is_none())
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

    fn face_configuration_options(&self) -> Option<MeshFaceEndpointConfigurations> {
        let mesh_assignments = self.mesh_assignments?;
        let mut best = None::<Vec<Vec<(usize, [usize; 2])>>>;
        for (face, domain) in mesh_assignments.iter().enumerate() {
            let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                continue;
            };
            if !self.budget.charge() {
                return Some(Vec::new());
            }
            if !self.face_edges[face]
                .iter()
                .any(|edge| self.active[*edge] && self.assignment[*edge].is_none())
            {
                continue;
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
            if best
                .as_ref()
                .is_none_or(|stored| projected.len() < stored.len())
            {
                best = Some(projected);
            }
        }
        best
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
            if !options.is_empty() && self.charge_branch(options.len()) {
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
        if !self.charge_branch(options.len()) {
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
    const MAX_PAIR_SOLUTIONS: usize = 256;
    if partial_solution_valid
        .is_some_and(|constraint| constraint.active_edges.len() != choices.len())
    {
        return None;
    }
    let components = incidence_choice_components(choices, edge_faces, mesh_assignments);
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
        return (boundary_domains_close(mesh_assignments, &pairs) && solution_valid(&pairs))
            .then_some(vec![pairs]);
    }
    let component_count = components.len();
    let mut combined = vec![fixed.clone()];
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    for (component_index, component) in components.into_iter().enumerate() {
        let mut active = vec![false; choices.len()];
        let mut constraints = HashSet::<(usize, usize)>::new();
        let mut component_faces = HashSet::new();
        for &edge in &component {
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
            combined.iter().any(|prefix| {
                let mut assignment = prefix.clone();
                for &(edge, pair) in solution {
                    assignment[edge] = Some(pair);
                }
                completed_incidence_faces_close(
                    &component_faces,
                    &assignment,
                    &face_edges,
                    mesh_assignments,
                ) && partial_solution_valid.is_none_or(|constraint| (constraint.valid)(&assignment))
                    && (component_index + 1 != component_count
                        || assignment
                            .into_iter()
                            .collect::<Option<Vec<_>>>()
                            .is_some_and(|pairs| {
                                boundary_domains_close(mesh_assignments, &pairs)
                                    && solution_valid(&pairs)
                            }))
            })
        };
        let solution_filter = Some(&filter as &dyn Fn(&[MeshEndpointPair]) -> bool);
        let (exhausted, solutions) = {
            let mut search = IncidenceComponentSearch {
                choices,
                edge_faces,
                face_edges: &face_edges,
                mesh_assignments,
                mesh_quotient,
                active,
                edges: &component,
                constraints,
                assignment: fixed.clone(),
                degrees: degrees.clone(),
                solutions: Vec::new(),
                solution_filter,
                partial_solution_filter: partial_solution_valid,
                dead_states: HashSet::new(),
                budget: &budget,
                states: 0,
                exhausted: false,
            };
            search.search();
            (search.exhausted, search.solutions)
        };
        if exhausted || solutions.is_empty() {
            return None;
        }
        let result_count = combined.len().checked_mul(solutions.len())?;
        if result_count > MAX_PAIR_SOLUTIONS {
            return None;
        }
        combined = combined
            .into_iter()
            .flat_map(|assignment| {
                solutions.iter().map(move |solution| {
                    let mut assignment = assignment.clone();
                    for &(edge, pair) in solution {
                        assignment[edge] = Some(pair);
                    }
                    assignment
                })
            })
            .collect();
    }
    combined
        .into_iter()
        .map(|assignment| assignment.into_iter().collect::<Option<Vec<_>>>())
        .collect()
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
    let mut choices = edge_candidates.to_vec();
    for candidates in &mut choices {
        for pair in candidates.iter_mut() {
            pair.sort_unstable();
        }
        candidates.sort_unstable();
        candidates.dedup();
    }
    prune_incidence_choices(&mut choices, edge_faces, face_count, vertex_points.len())?;
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
    let solutions = component_incidence_pair_solutions(
        &choices,
        edge_faces,
        face_count,
        vertex_points.len(),
        mesh_assignments,
        mesh_quotient,
        partial_solution_valid,
        &complete_valid,
    )?;
    (!solutions.is_empty()).then_some(solutions)
}
