//! Mesh-quotient constraint solver for standard nested B-rep topology.
//!
//! Closes vertex-coordinate quotients and enumerates face endpoint configurations.

use crate::families::standard::fbb::{largest_fbb_run, parse_edge_tables, parse_vertex_table};
use crate::families::standard::topology::{
    incidence_cycles, orient_face_cycles, reconstruct_mesh_selection, EdgeRow, StandardTopology,
};
use crate::solve::incidence::{
    compact_boundary_domain_viable, deferred_boundary_cycle_matches,
    incidence_endpoint_pair_solutions, labeled_assignment_endpoint_cycles_viable,
};
use crate::solve::matching::{
    distinct_domain_matching_with_budget, domains_have_distinct_matching, MatchingEdgeConstraint,
};
use crate::solve::missing_edge::{
    same_unordered_pair, standard_edge_port_identities, standard_mesh_boundary_assignments,
    standard_mesh_boundary_domains_impl, MeshBoundaryEdgeCandidate, MeshDeferredFaceBoundary,
    MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
};
use crate::solve::UnionFind;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

pub(crate) const MAX_FACE_EQUATION_CACHE_ENTRIES: usize = 4_096;
pub(crate) const MAX_MESH_CONSTRAINT_OPERATIONS: usize = 100_000;
pub(crate) type MeshQuotientGaugeState = (MeshQuotient, HashSet<usize>);

#[derive(Clone)]
pub(crate) struct MeshQuotient {
    pub(crate) union: UnionFind,
    pub(crate) domains: Vec<Arc<HashSet<usize>>>,
    pub(crate) members: Vec<Vec<usize>>,
}

pub(crate) fn initial_mesh_quotient(
    edge_candidates: &[Vec<[usize; 2]>],
    point_count: usize,
    port_identities: &[[u32; 2]],
) -> Option<MeshQuotient> {
    if port_identities.len() != edge_candidates.len() {
        return None;
    }
    let all_points = Arc::new((0..point_count).collect::<HashSet<_>>());
    let mut domains = Vec::with_capacity(edge_candidates.len() * 2);
    for candidates in edge_candidates {
        let domain = if candidates.is_empty() {
            all_points.clone()
        } else {
            Arc::new(candidates.iter().flatten().copied().collect::<HashSet<_>>())
        };
        if domain.is_empty() || domain.iter().any(|point| *point >= point_count) {
            return None;
        }
        domains.push(domain.clone());
        domains.push(domain);
    }
    let mut quotient = MeshQuotient {
        union: UnionFind::new(edge_candidates.len() * 2),
        domains,
        members: (0..edge_candidates.len() * 2)
            .map(|node| vec![node])
            .collect(),
    };
    let mut node_by_identity = HashMap::new();
    for (edge, ports) in port_identities.iter().enumerate() {
        for (port, identity) in ports.iter().copied().enumerate() {
            let node = edge * 2 + port;
            if let Some(&previous) = node_by_identity.get(&identity) {
                quotient.merge(previous, node)?;
            } else {
                node_by_identity.insert(identity, node);
            }
        }
    }
    quotient
        .edge_domains_viable(edge_candidates)
        .then_some(quotient)
}

pub(crate) fn complete_mesh_endpoint_candidates_from_quotient(
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
    max_pairs_per_edge: usize,
    max_pairs_total: usize,
) -> Option<Vec<Vec<[usize; 2]>>> {
    if quotient.union.len() != edge_candidates.len().checked_mul(2)? {
        return None;
    }
    let mut pair_count = 0usize;
    edge_candidates
        .iter()
        .enumerate()
        .map(|(edge, candidates)| {
            if !candidates.is_empty() {
                pair_count = pair_count.checked_add(candidates.len())?;
                return (pair_count <= max_pairs_total).then(|| candidates.clone());
            }
            let left = quotient.union.find(edge * 2);
            let right = quotient.union.find(edge * 2 + 1);
            let relation_count = if left == right {
                quotient.domains[left].len()
            } else {
                quotient.domains[left]
                    .len()
                    .checked_mul(quotient.domains[right].len())?
            };
            if relation_count > max_pairs_per_edge {
                return None;
            }
            pair_count = pair_count.checked_add(relation_count)?;
            if pair_count > max_pairs_total {
                return None;
            }
            let mut completed = if left == right {
                quotient.domains[left]
                    .iter()
                    .copied()
                    .map(|point| [point, point])
                    .collect::<Vec<_>>()
            } else {
                quotient.domains[left]
                    .iter()
                    .flat_map(|&left_point| {
                        quotient.domains[right]
                            .iter()
                            .copied()
                            .filter(move |&right_point| right_point != left_point)
                            .map(move |right_point| {
                                if left_point < right_point {
                                    [left_point, right_point]
                                } else {
                                    [right_point, left_point]
                                }
                            })
                    })
                    .collect::<Vec<_>>()
            };
            completed.sort_unstable();
            completed.dedup();
            (!completed.is_empty()).then_some(completed)
        })
        .collect()
}

impl MeshQuotient {
    fn signature_work(&mut self) -> usize {
        let mut work = 0usize;
        for node in 0..self.union.len() {
            if self.union.find(node) == node {
                work = work
                    .saturating_add(self.members[node].len())
                    .saturating_add(self.domains[node].len());
            }
        }
        work.max(1)
    }

    fn monotone_measure(&mut self) -> (usize, usize) {
        let mut root_count = 0usize;
        let mut domain_cardinality = 0usize;
        for node in 0..self.union.len() {
            if self.union.find(node) == node {
                root_count += 1;
                domain_cardinality = domain_cardinality.saturating_add(self.domains[node].len());
            }
        }
        (root_count, domain_cardinality)
    }

    pub(crate) fn signature(&mut self) -> Vec<(Vec<usize>, Vec<usize>)> {
        let mut components = Vec::new();
        for node in 0..self.union.len() {
            if self.union.find(node) != node {
                continue;
            }
            let mut members = self.members[node].clone();
            members.sort_unstable();
            let mut domain = self.domains[node].iter().copied().collect::<Vec<_>>();
            domain.sort_unstable();
            components.push((members, domain));
        }
        components.sort_unstable();
        components
    }

    pub(crate) fn root_count(&mut self) -> usize {
        (0..self.union.len())
            .filter(|node| self.union.find(*node) == *node)
            .count()
    }

    pub(crate) fn merge(&mut self, left: usize, right: usize) -> Option<usize> {
        let left = self.union.find(left);
        let right = self.union.find(right);
        if left == right {
            return Some(left);
        }
        let intersection = self.domains[left]
            .intersection(&self.domains[right])
            .copied()
            .collect::<HashSet<_>>();
        if intersection.is_empty() {
            return None;
        }
        self.union.union(left, right);
        let root = self.union.find(left);
        self.domains[root] = Arc::new(intersection);
        let child = if root == left { right } else { left };
        let child_members = std::mem::take(&mut self.members[child]);
        self.members[root].extend(child_members);
        Some(root)
    }

    pub(crate) fn edge_domains_viable(&mut self, edge_candidates: &[Vec<[usize; 2]>]) -> bool {
        self.propagate_edge_domains(
            edge_candidates
                .iter()
                .enumerate()
                .filter_map(|(edge, candidates)| (!candidates.is_empty()).then_some(edge)),
            edge_candidates,
            None,
        )
    }

    fn propagate_component_edge_domains(
        &mut self,
        root: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&MeshConstraintBudget>,
    ) -> bool {
        let edges = self.members[root]
            .iter()
            .map(|node| node / 2)
            .filter(|edge| !edge_candidates[*edge].is_empty())
            .collect::<HashSet<_>>();
        self.propagate_edge_domains(edges, edge_candidates, budget)
    }

    fn propagate_edge_domains(
        &mut self,
        edges: impl IntoIterator<Item = usize>,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&MeshConstraintBudget>,
    ) -> bool {
        fn enqueue_component_edges(
            root: usize,
            members: &[Vec<usize>],
            edge_candidates: &[Vec<[usize; 2]>],
            queue: &mut VecDeque<usize>,
            queued: &mut HashSet<usize>,
        ) {
            for edge in members[root].iter().map(|node| node / 2) {
                if !edge_candidates[edge].is_empty() && queued.insert(edge) {
                    queue.push_back(edge);
                }
            }
        }

        let mut queue = VecDeque::new();
        let mut queued = HashSet::new();
        for edge in edges {
            if queued.insert(edge) {
                queue.push_back(edge);
            }
        }
        while let Some(edge) = queue.pop_front() {
            queued.remove(&edge);
            let candidates = &edge_candidates[edge];
            if budget.is_some_and(|budget| !budget.charge_by(candidates.len().max(1))) {
                return false;
            }
            if candidates.is_empty() {
                continue;
            }
            let start = self.union.find(edge * 2);
            let end = self.union.find(edge * 2 + 1);
            if start == end {
                let supported = candidates
                    .iter()
                    .filter(|pair| pair[0] == pair[1])
                    .map(|pair| pair[0])
                    .filter(|point| self.domains[start].contains(point))
                    .collect::<HashSet<_>>();
                if supported.is_empty() {
                    return false;
                }
                if supported != *self.domains[start] {
                    self.domains[start] = Arc::new(supported);
                    enqueue_component_edges(
                        start,
                        &self.members,
                        edge_candidates,
                        &mut queue,
                        &mut queued,
                    );
                }
                continue;
            }

            let starts = self.domains[start].clone();
            let ends = self.domains[end].clone();
            let mut supported_starts = HashSet::new();
            let mut supported_ends = HashSet::new();
            for &[left, right] in candidates {
                if starts.contains(&left) && ends.contains(&right) {
                    supported_starts.insert(left);
                    supported_ends.insert(right);
                }
                if starts.contains(&right) && ends.contains(&left) {
                    supported_starts.insert(right);
                    supported_ends.insert(left);
                }
            }
            if supported_starts.is_empty() || supported_ends.is_empty() {
                return false;
            }
            if supported_starts != *self.domains[start] {
                self.domains[start] = Arc::new(supported_starts);
                enqueue_component_edges(
                    start,
                    &self.members,
                    edge_candidates,
                    &mut queue,
                    &mut queued,
                );
            }
            if supported_ends != *self.domains[end] {
                self.domains[end] = Arc::new(supported_ends);
                enqueue_component_edges(
                    end,
                    &self.members,
                    edge_candidates,
                    &mut queue,
                    &mut queued,
                );
            }
        }
        true
    }

    pub(crate) fn merge_singleton_coordinate_roots(
        &mut self,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> bool {
        loop {
            let mut roots_by_point = HashMap::<usize, Vec<usize>>::new();
            for node in 0..self.union.len() {
                let root = self.union.find(node);
                if root != node || self.domains[root].len() != 1 {
                    continue;
                }
                let Some(&point) = self.domains[root].iter().next() else {
                    return false;
                };
                roots_by_point.entry(point).or_default().push(root);
            }
            let mut changed = false;
            for roots in roots_by_point.into_values() {
                let Some((&first, rest)) = roots.split_first() else {
                    continue;
                };
                for &root in rest {
                    if self.merge(first, root).is_none() {
                        return false;
                    }
                    changed = true;
                }
            }
            if !changed {
                return true;
            }
            if !self.edge_domains_viable(edge_candidates) {
                return false;
            }
        }
    }

    pub(crate) fn close_coordinate_roots(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&MeshConstraintBudget>,
    ) -> Option<HashMap<usize, usize>> {
        self.close_coordinate_roots_with_incidence(point_count, edge_candidates, None, budget)
    }

    pub(crate) fn close_coordinate_roots_for_incidence_with_budget(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        edge_faces: &[[usize; 2]],
        face_count: usize,
        boundary_domains: &[MeshFaceBoundaryDomain],
        budget: Option<&MeshConstraintBudget>,
    ) -> Option<HashMap<usize, usize>> {
        (edge_faces.len() == edge_candidates.len()
            && edge_faces.iter().flatten().all(|face| *face < face_count)
            && boundary_domains.len() == face_count)
            .then_some(())?;
        self.close_coordinate_roots_with_incidence(
            point_count,
            edge_candidates,
            Some((edge_faces, boundary_domains)),
            budget,
        )
    }

    fn close_coordinate_roots_with_incidence(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
        budget: Option<&MeshConstraintBudget>,
    ) -> Option<HashMap<usize, usize>> {
        fn pair_supported(candidates: &[[usize; 2]], left: usize, right: usize) -> bool {
            candidates.is_empty()
                || candidates
                    .iter()
                    .any(|pair| same_unordered_pair(*pair, [left, right]))
        }

        fn enforce_edge_arc_consistency(
            domains: &mut [Vec<usize>],
            edges: &[[usize; 2]],
            edge_ids: &[usize],
            root_edges: &[Vec<usize>],
            edge_candidates: &[Vec<[usize; 2]>],
            budget: Option<&MeshConstraintBudget>,
        ) -> bool {
            let support_work = edge_ids
                .iter()
                .map(|edge| edge_candidates[*edge].len().saturating_mul(2))
                .sum::<usize>();
            if support_work > 0 && budget.is_some_and(|budget| !budget.charge_by(support_work)) {
                return false;
            }
            let supports = edge_ids
                .iter()
                .map(|edge| {
                    let mut supports = HashMap::<usize, HashSet<usize>>::new();
                    for [left, right] in edge_candidates[*edge].iter().copied() {
                        supports.entry(left).or_default().insert(right);
                        supports.entry(right).or_default().insert(left);
                    }
                    supports
                })
                .collect::<Vec<_>>();
            let mut queued = vec![[true; 2]; edges.len()];
            let mut queue = (0..edges.len())
                .flat_map(|edge| [(edge, 0usize), (edge, 1usize)])
                .collect::<VecDeque<_>>();
            while let Some((edge, side)) = queue.pop_front() {
                queued[edge][side] = false;
                if supports[edge].is_empty() {
                    continue;
                }
                let root = edges[edge][side];
                let other = edges[edge][1 - side];
                let other_domain = domains[other].iter().copied().collect::<HashSet<_>>();
                let before = domains[root].len();
                domains[root].retain(|point| {
                    let Some(supported) = supports[edge].get(point) else {
                        return false;
                    };
                    if budget.is_some_and(|budget| !budget.charge_by(supported.len().max(1))) {
                        return false;
                    }
                    supported.iter().any(|point| other_domain.contains(point))
                });
                if budget.is_some_and(|budget| budget.exhausted.get()) || domains[root].is_empty() {
                    return false;
                }
                if domains[root].len() == before {
                    continue;
                }
                for &neighbor in &root_edges[root] {
                    let neighbor_side = usize::from(edges[neighbor][1] == root);
                    let revised_side = 1 - neighbor_side;
                    if !queued[neighbor][revised_side] {
                        queued[neighbor][revised_side] = true;
                        queue.push_back((neighbor, revised_side));
                    }
                }
            }
            true
        }

        fn enforce_sparse_endpoint_membership(
            domains: &mut [Vec<usize>],
            edges: &[[usize; 2]],
            edge_ids: &[usize],
            edge_candidates: &[Vec<[usize; 2]>],
            budget: Option<&MeshConstraintBudget>,
        ) -> bool {
            let mut ordered = (0..edges.len()).collect::<Vec<_>>();
            ordered.sort_unstable_by_key(|edge| edge_candidates[edge_ids[*edge]].len());
            for edge in ordered {
                let candidates = &edge_candidates[edge_ids[edge]];
                if candidates.is_empty() {
                    continue;
                }
                let [left, right] = edges[edge];
                let domain_work = domains[left].len()
                    + usize::from(right != left).saturating_mul(domains[right].len());
                let support_work = candidates.len().saturating_mul(2);
                if support_work >= domain_work {
                    continue;
                }
                let work = support_work.saturating_add(domain_work);
                if budget.is_some_and(|budget| work > budget.remaining.get()) {
                    continue;
                }
                if budget.is_some_and(|budget| !budget.charge_by(work)) {
                    return false;
                }
                let allowed = candidates.iter().flatten().copied().collect::<HashSet<_>>();
                domains[left].retain(|point| allowed.contains(point));
                if right != left {
                    domains[right].retain(|point| allowed.contains(point));
                }
                if domains[left].is_empty() || domains[right].is_empty() {
                    return false;
                }
            }
            true
        }

        #[allow(clippy::too_many_arguments)]
        fn partial_ordered_assignment_viable(
            assignment: &MeshFaceBoundaryAssignment,
            local_edge_by_id: &HashMap<usize, usize>,
            edges: &[[usize; 2]],
            domains: &[Vec<usize>],
            assigned: &[Option<usize>],
            candidate: (usize, usize),
            budget: Option<&MeshConstraintBudget>,
        ) -> bool {
            let directions = |use_: MeshBoundaryEdgeCandidate| match use_.reversed {
                Some(reversed) => [Some(reversed), None],
                None => [Some(false), Some(true)],
            };
            let port_root = |use_: MeshBoundaryEdgeCandidate, reversed: bool, end: bool| {
                let local = *local_edge_by_id.get(&use_.edge)?;
                Some(edges[local][usize::from(if end { !reversed } else { reversed })])
            };
            let compatible = |left: usize, right: usize| {
                if budget.is_some_and(|budget| !budget.charge()) {
                    return false;
                }
                let value = |root| {
                    if root == candidate.0 {
                        Some(candidate.1)
                    } else {
                        assigned[root]
                    }
                };
                match (value(left), value(right)) {
                    (Some(left), Some(right)) => left == right,
                    (Some(point), None) => domains[right].contains(&point),
                    (None, Some(point)) => domains[left].contains(&point),
                    (None, None) => !domains[left]
                        .iter()
                        .all(|point| !domains[right].contains(point)),
                }
            };

            assignment.boundaries.iter().all(|boundary| {
                let Some(first) = boundary.first().copied() else {
                    return false;
                };
                directions(first)
                    .into_iter()
                    .flatten()
                    .any(|first_direction| {
                        let mut previous = vec![first_direction];
                        for index in 1..boundary.len() {
                            let mut next = Vec::new();
                            for direction in directions(boundary[index]).into_iter().flatten() {
                                if previous.iter().copied().any(|previous_direction| {
                                    let Some(left) =
                                        port_root(boundary[index - 1], previous_direction, true)
                                    else {
                                        return false;
                                    };
                                    let Some(right) = port_root(boundary[index], direction, false)
                                    else {
                                        return false;
                                    };
                                    compatible(left, right)
                                }) {
                                    next.push(direction);
                                }
                            }
                            if next.is_empty() {
                                return false;
                            }
                            previous = next;
                        }
                        previous.into_iter().any(|previous_direction| {
                            let Some(left) = port_root(
                                *boundary.last().expect("nonempty boundary"),
                                previous_direction,
                                true,
                            ) else {
                                return false;
                            };
                            let Some(right) = port_root(first, first_direction, false) else {
                                return false;
                            };
                            compatible(left, right)
                        })
                    })
            })
        }

        #[allow(clippy::too_many_arguments)]
        fn partial_compact_assignment_viable(
            domain: &MeshFaceBoundaryDomain,
            local_edge_by_id: &HashMap<usize, usize>,
            edges: &[[usize; 2]],
            global_edge_count: usize,
            assigned: &[Option<usize>],
            candidate: (usize, usize),
            budget: Option<&MeshConstraintBudget>,
        ) -> bool {
            fn augment(
                component: usize,
                compatible: &[Vec<bool>],
                seen: &mut [bool],
                matched: &mut [Option<usize>],
            ) -> bool {
                for cycle in 0..matched.len() {
                    if !compatible[component][cycle] || seen[cycle] {
                        continue;
                    }
                    seen[cycle] = true;
                    if matched[cycle]
                        .is_none_or(|previous| augment(previous, compatible, seen, matched))
                    {
                        matched[cycle] = Some(component);
                        return true;
                    }
                }
                false
            }

            let relevant = match domain {
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
                    edges.sort_unstable();
                    edges.dedup();
                    edges
                }
            };
            if budget.is_some_and(|budget| !budget.charge_by(relevant.len().max(1))) {
                return false;
            }
            let value = |root| {
                if root == candidate.0 {
                    Some(candidate.1)
                } else {
                    assigned[root]
                }
            };
            let mut selected = HashMap::new();
            let mut selected_edges = Vec::new();
            let mut adjacency = HashMap::<usize, Vec<usize>>::new();
            for &edge in &relevant {
                let Some(&local) = local_edge_by_id.get(&edge) else {
                    return false;
                };
                let [left, right] = edges[local];
                let [Some(left), Some(right)] = [value(left), value(right)] else {
                    continue;
                };
                selected.insert(edge, [left, right]);
                selected_edges.push(edge);
                adjacency.entry(left).or_default().push(edge);
                adjacency.entry(right).or_default().push(edge);
            }
            let mut closed_components = Vec::new();
            let mut seen_edges = HashSet::new();
            for &first in &selected_edges {
                if seen_edges.contains(&first) {
                    continue;
                }
                let mut stack = vec![first];
                let mut component = Vec::new();
                let mut vertices = HashSet::new();
                while let Some(edge) = stack.pop() {
                    if !seen_edges.insert(edge) {
                        continue;
                    }
                    component.push(edge);
                    for point in selected[&edge] {
                        vertices.insert(point);
                        stack.extend(adjacency[&point].iter().copied());
                    }
                }
                if vertices.iter().all(|point| adjacency[point].len() == 2) {
                    closed_components.push(component);
                }
            }
            match domain {
                MeshFaceBoundaryDomain::Ordered(_) => true,
                MeshFaceBoundaryDomain::UnorderedFullCycle(_) => {
                    closed_components.is_empty()
                        || (selected_edges.len() == relevant.len() && closed_components.len() == 1)
                }
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    if closed_components.len() > domain.cycles.len() {
                        return false;
                    }
                    let missing = domain.missing_edges.iter().copied().collect::<HashSet<_>>();
                    let mut edge_points = vec![[0; 2]; global_edge_count];
                    for (&edge, &points) in &selected {
                        edge_points[edge] = points;
                    }
                    let compatible = closed_components
                        .iter()
                        .map(|component| {
                            let incidence = incidence_cycles(component, &edge_points);
                            let Some([incidence]) = incidence.as_deref() else {
                                return vec![false; domain.cycles.len()];
                            };
                            domain
                                .cycles
                                .iter()
                                .map(|cycle| {
                                    deferred_boundary_cycle_matches(cycle, incidence, &missing)
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>();
                    let mut matched = vec![None; domain.cycles.len()];
                    (0..closed_components.len()).all(|component| {
                        augment(
                            component,
                            &compatible,
                            &mut vec![false; domain.cycles.len()],
                            &mut matched,
                        )
                    })
                }
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn walk(
            domains: &[Vec<usize>],
            edges: &[[usize; 2]],
            edge_ids: &[usize],
            local_edge_by_id: &HashMap<usize, usize>,
            root_edges: &[Vec<usize>],
            edge_candidates: &[Vec<[usize; 2]>],
            edge_faces: Option<&[[usize; 2]]>,
            face_edges: Option<&[Vec<usize>]>,
            closed_faces: Option<&[bool]>,
            boundary_domains: Option<&[MeshFaceBoundaryDomain]>,
            component_points: &HashSet<usize>,
            assigned: &mut [Option<usize>],
            point_uses: &mut [usize],
            solutions: &mut Vec<Vec<usize>>,
            states: &mut usize,
            exhausted: &mut bool,
            budget: Option<&MeshConstraintBudget>,
        ) {
            const MAX_COORDINATE_CLOSURE_STATES: usize = 256;

            fn rollback(
                assigned: &mut [Option<usize>],
                point_uses: &mut [usize],
                propagated: Vec<(usize, usize)>,
            ) {
                for (root, point) in propagated.into_iter().rev() {
                    point_uses[point] -= 1;
                    assigned[root] = None;
                }
            }

            fn affected_roots(
                root: usize,
                root_edges: &[Vec<usize>],
                edges: &[[usize; 2]],
                edge_faces: Option<&[[usize; 2]]>,
                face_edges: Option<&[Vec<usize>]>,
            ) -> HashSet<usize> {
                let mut affected = HashSet::new();
                for &edge in &root_edges[root] {
                    affected.extend(edges[edge]);
                    let (Some(edge_faces), Some(face_edges)) = (edge_faces, face_edges) else {
                        continue;
                    };
                    let faces = edge_faces[edge];
                    for (rank, face) in faces.into_iter().enumerate() {
                        if rank > 0 && face == faces[0] {
                            continue;
                        }
                        for &face_edge in &face_edges[face] {
                            affected.extend(edges[face_edge]);
                        }
                    }
                }
                affected.remove(&root);
                affected
            }

            if solutions.len() > 1 || *exhausted {
                return;
            }
            if budget.is_some_and(|budget| !budget.charge()) {
                *exhausted = true;
                return;
            }
            let viable_values =
                |root: usize,
                 assigned: &[Option<usize>],
                 base_degrees: &HashMap<(usize, usize), u8>| {
                    domains[root]
                        .iter()
                        .copied()
                        .filter(|point| {
                            let pair_viable = root_edges[root].iter().all(|edge| {
                                let [left, right] = edges[*edge];
                                let other = if left == root { right } else { left };
                                assigned[other].is_none_or(|other_point| {
                                    pair_supported(
                                        &edge_candidates[edge_ids[*edge]],
                                        *point,
                                        other_point,
                                    )
                                })
                            });
                            if !pair_viable {
                                return false;
                            }
                            let Some(edge_faces) = edge_faces else {
                                return true;
                            };
                            if budget.is_some_and(|budget| {
                                !budget.charge_by(root_edges[root].len().max(1))
                            }) {
                                return false;
                            }
                            let value = |endpoint| {
                                if endpoint == root {
                                    Some(*point)
                                } else {
                                    assigned[endpoint]
                                }
                            };
                            let mut degrees = base_degrees.clone();
                            let mut affected_faces = HashSet::new();
                            for &edge in &root_edges[root] {
                                let [left, right] = edges[edge];
                                let (Some(left), Some(right)) = (value(left), value(right)) else {
                                    continue;
                                };
                                let faces = edge_faces[edge];
                                for (rank, face) in faces.into_iter().enumerate() {
                                    if rank > 0 && face == faces[0] {
                                        continue;
                                    }
                                    affected_faces.insert(face);
                                    for endpoint in [left, right] {
                                        let degree = degrees.entry((face, endpoint)).or_default();
                                        *degree = degree.saturating_add(1);
                                        if *degree > 2 {
                                            return false;
                                        }
                                    }
                                }
                            }
                            for (&(face, point), &degree) in &degrees {
                                if degree != 1 || !affected_faces.contains(&face) {
                                    continue;
                                }
                                let Some(face_edges) = face_edges else {
                                    return false;
                                };
                                if budget.is_some_and(|budget| {
                                    !budget.charge_by(face_edges[face].len().max(1))
                                }) {
                                    return false;
                                }
                                let supported = face_edges[face].iter().copied().any(|edge| {
                                    let [left, right] = edges[edge];
                                    if value(left).is_some() && value(right).is_some() {
                                        return false;
                                    }
                                    let supports = |endpoint| {
                                        value(endpoint).is_some_and(|value| value == point)
                                            || (value(endpoint).is_none()
                                                && domains[endpoint].contains(&point))
                                    };
                                    supports(left) || supports(right)
                                });
                                if !supported {
                                    return false;
                                }
                            }
                            if let (Some(boundary_domains), Some(closed_faces)) =
                                (boundary_domains, closed_faces)
                            {
                                let boundaries_viable =
                                    boundary_domains.iter().enumerate().all(|(face, domain)| {
                                        if !closed_faces[face] || !affected_faces.contains(&face) {
                                            return true;
                                        }
                                        match domain {
                                            MeshFaceBoundaryDomain::Ordered(assignments) => {
                                                assignments.iter().any(|assignment| {
                                                    partial_ordered_assignment_viable(
                                                        assignment,
                                                        local_edge_by_id,
                                                        edges,
                                                        domains,
                                                        assigned,
                                                        (root, *point),
                                                        budget,
                                                    )
                                                })
                                            }
                                            _ => partial_compact_assignment_viable(
                                                domain,
                                                local_edge_by_id,
                                                edges,
                                                edge_candidates.len(),
                                                assigned,
                                                (root, *point),
                                                budget,
                                            ),
                                        }
                                    });
                                if !boundaries_viable {
                                    return false;
                                }
                            }
                            true
                        })
                        .collect::<Vec<_>>()
                };

            let mut propagated = Vec::new();
            let mut pending_roots = None::<HashSet<usize>>;
            let branch = loop {
                let mut scanned_roots = pending_roots.take().map_or_else(
                    || (0..domains.len()).collect::<Vec<_>>(),
                    |roots| roots.into_iter().collect(),
                );
                scanned_roots.sort_unstable_by_key(|root| domains[*root].len());
                let partial_scan = scanned_roots.len() < domains.len();
                let bounded_scan = !partial_scan
                    && budget.is_some_and(|budget| {
                        assigned
                            .iter()
                            .enumerate()
                            .filter(|(_, point)| point.is_none())
                            .map(|(root, _)| domains[root].len().saturating_add(1))
                            .fold(0usize, usize::saturating_add)
                            > budget.remaining.get()
                    });
                let remaining = assigned.iter().filter(|point| point.is_none()).count();
                let unused = component_points
                    .iter()
                    .filter(|point| point_uses[**point] == 0)
                    .count();
                if remaining < unused {
                    break None;
                }
                let mut viable_domains = Vec::new();
                let mut dead = false;
                let mut progress = false;
                let mut scan_truncated = false;
                let mut supported_unused = HashSet::new();
                let mut unused_point_roots = HashMap::<usize, Vec<usize>>::new();
                let mut base_degrees = HashMap::<(usize, usize), u8>::new();
                if let Some(edge_faces) = edge_faces {
                    if budget.is_some_and(|budget| !budget.charge_by(edges.len().max(1))) {
                        *exhausted = true;
                        break None;
                    }
                    for (edge, [left, right]) in edges.iter().copied().enumerate() {
                        let [Some(left), Some(right)] = [assigned[left], assigned[right]] else {
                            continue;
                        };
                        let faces = edge_faces[edge];
                        for (rank, face) in faces.into_iter().enumerate() {
                            if rank > 0 && face == faces[0] {
                                continue;
                            }
                            for point in [left, right] {
                                *base_degrees.entry((face, point)).or_default() += 1;
                            }
                        }
                    }
                }
                for root in scanned_roots {
                    if assigned[root].is_some() {
                        continue;
                    }
                    if budget.is_some_and(|budget| {
                        !budget.charge_by(domains[root].len().saturating_add(1))
                    }) {
                        *exhausted = true;
                        break;
                    }
                    let values = viable_values(root, assigned, &base_degrees);
                    if budget.is_some_and(|budget| budget.exhausted.get()) {
                        *exhausted = true;
                        break;
                    }
                    if values.is_empty() {
                        dead = true;
                        break;
                    }
                    supported_unused.extend(
                        values
                            .iter()
                            .copied()
                            .filter(|point| point_uses[*point] == 0),
                    );
                    for &point in values.iter().filter(|point| point_uses[**point] == 0) {
                        unused_point_roots.entry(point).or_default().push(root);
                    }
                    if let [point] = values.as_slice() {
                        assigned[root] = Some(*point);
                        point_uses[*point] += 1;
                        propagated.push((root, *point));
                        progress = true;
                        if edge_faces.is_some() || bounded_scan {
                            pending_roots = Some(affected_roots(
                                root, root_edges, edges, edge_faces, face_edges,
                            ));
                            break;
                        }
                    } else {
                        viable_domains.push((root, values));
                        if bounded_scan {
                            scan_truncated = true;
                            break;
                        }
                    }
                }
                if *exhausted {
                    break None;
                }
                if dead {
                    break None;
                }
                if progress {
                    continue;
                }
                if scan_truncated {
                    let best = viable_domains
                        .into_iter()
                        .min_by_key(|(_, values)| values.len());
                    break Some(best);
                }
                if partial_scan {
                    pending_roots = None;
                    continue;
                }
                if component_points
                    .iter()
                    .any(|point| point_uses[*point] == 0 && !supported_unused.contains(point))
                {
                    break None;
                }
                let mut point_supports = unused_point_roots.into_iter().collect::<Vec<_>>();
                point_supports.sort_unstable_by_key(|(point, _)| *point);
                let uniquely_required = point_supports
                    .iter()
                    .filter_map(|(point, roots)| {
                        <&[usize; 1]>::try_from(roots.as_slice())
                            .ok()
                            .map(|[root]| (*point, *root))
                    })
                    .collect::<Vec<_>>();
                if let Some(&(point, root)) = uniquely_required.first() {
                    if uniquely_required.iter().any(|&(other_point, other_root)| {
                        other_root == root && other_point != point
                    }) {
                        break None;
                    }
                    assigned[root] = Some(point);
                    point_uses[point] += 1;
                    propagated.push((root, point));
                    pending_roots = Some(affected_roots(
                        root, root_edges, edges, edge_faces, face_edges,
                    ));
                    continue;
                }
                let matching_budget =
                    budget.map(|budget| MeshConstraintBudget::new(budget.remaining.get()));
                let support_domains = point_supports
                    .iter()
                    .map(|(_, roots)| roots.as_slice())
                    .collect::<Vec<_>>();
                let coverage_matching = distinct_domain_matching_with_budget(
                    support_domains.iter().copied(),
                    assigned.len(),
                    matching_budget.as_ref(),
                    None,
                );
                let mut matching_forced = None;
                let mut unsupported_matches = HashSet::new();
                if let Some(matching) = &coverage_matching {
                    for (support, &root) in matching.iter().enumerate() {
                        if distinct_domain_matching_with_budget(
                            support_domains.iter().copied(),
                            assigned.len(),
                            matching_budget.as_ref(),
                            Some(MatchingEdgeConstraint::Exclude(support, root)),
                        )
                        .is_none()
                        {
                            if matching_budget
                                .as_ref()
                                .is_some_and(|budget| budget.exhausted.get())
                            {
                                break;
                            }
                            matching_forced = Some((point_supports[support].0, root));
                            break;
                        }
                    }
                    if matching_forced.is_none() {
                        'supports: for (support, (_, roots)) in point_supports.iter().enumerate() {
                            for &root in roots {
                                if matching[support] == root {
                                    continue;
                                }
                                if distinct_domain_matching_with_budget(
                                    support_domains.iter().copied(),
                                    assigned.len(),
                                    matching_budget.as_ref(),
                                    Some(MatchingEdgeConstraint::Require(support, root)),
                                )
                                .is_none()
                                {
                                    if matching_budget
                                        .as_ref()
                                        .is_some_and(|budget| budget.exhausted.get())
                                    {
                                        break 'supports;
                                    }
                                    unsupported_matches.insert((root, point_supports[support].0));
                                }
                            }
                        }
                    }
                }
                if matching_budget
                    .as_ref()
                    .is_none_or(|budget| !budget.exhausted.get())
                {
                    if let (Some(budget), Some(matching_budget)) =
                        (budget, matching_budget.as_ref())
                    {
                        let work = budget.remaining.get() - matching_budget.remaining.get();
                        if !budget.charge_by(work) {
                            *exhausted = true;
                            break None;
                        }
                    }
                    if coverage_matching.is_none() {
                        break None;
                    }
                    if let Some((point, root)) = matching_forced {
                        assigned[root] = Some(point);
                        point_uses[point] += 1;
                        propagated.push((root, point));
                        pending_roots = Some(affected_roots(
                            root, root_edges, edges, edge_faces, face_edges,
                        ));
                        continue;
                    }
                    for (root, values) in &mut viable_domains {
                        values.retain(|point| !unsupported_matches.contains(&(*root, *point)));
                        if values.is_empty() {
                            break;
                        }
                    }
                    if viable_domains.iter().any(|(_, values)| values.is_empty()) {
                        break None;
                    }
                    if let Some(&(root, ref values)) =
                        viable_domains.iter().find(|(_, values)| values.len() == 1)
                    {
                        let point = values[0];
                        assigned[root] = Some(point);
                        point_uses[point] += 1;
                        propagated.push((root, point));
                        pending_roots = Some(affected_roots(
                            root, root_edges, edges, edge_faces, face_edges,
                        ));
                        continue;
                    }
                }
                let best = viable_domains
                    .into_iter()
                    .min_by_key(|(_, values)| values.len());
                break Some(best);
            };
            let Some(branch) = branch else {
                rollback(assigned, point_uses, propagated);
                return;
            };
            let Some((root, values)) = branch else {
                let incidence_closed =
                    edge_faces
                        .zip(closed_faces)
                        .is_none_or(|(edge_faces, closed_faces)| {
                            if budget.is_some_and(|budget| !budget.charge_by(edges.len())) {
                                return false;
                            }
                            let mut degrees = HashMap::<(usize, usize), u8>::new();
                            for (edge, [left, right]) in edges.iter().copied().enumerate() {
                                let [Some(left), Some(right)] = [assigned[left], assigned[right]]
                                else {
                                    return false;
                                };
                                let faces = edge_faces[edge];
                                for (rank, face) in faces.into_iter().enumerate() {
                                    if rank > 0 && face == faces[0] {
                                        continue;
                                    }
                                    for point in [left, right] {
                                        *degrees.entry((face, point)).or_default() += 1;
                                    }
                                }
                            }
                            degrees
                                .into_iter()
                                .all(|((face, _), degree)| !closed_faces[face] || degree == 2)
                        });
                let boundaries_close = boundary_domains.zip(closed_faces).is_none_or(
                    |(boundary_domains, closed_faces)| {
                        if budget.is_some_and(|budget| !budget.charge_by(edge_candidates.len())) {
                            return false;
                        }
                        let mut selected = vec![None; edge_candidates.len()];
                        for (local_edge, &edge) in edge_ids.iter().enumerate() {
                            let [left, right] = edges[local_edge];
                            let [Some(left), Some(right)] = [assigned[left], assigned[right]]
                            else {
                                return false;
                            };
                            selected[edge] = Some([left, right]);
                        }
                        boundary_domains.iter().enumerate().all(|(face, domain)| {
                            if !closed_faces[face] {
                                return true;
                            }
                            match domain {
                                MeshFaceBoundaryDomain::Ordered(assignments) => {
                                    assignments.iter().any(|assignment| {
                                        labeled_assignment_endpoint_cycles_viable(
                                            assignment, &selected, budget,
                                        )
                                    })
                                }
                                _ => compact_boundary_domain_viable(domain, &selected, None),
                            }
                        })
                    },
                );
                if incidence_closed
                    && boundaries_close
                    && component_points.iter().all(|point| point_uses[*point] > 0)
                {
                    solutions.push(
                        assigned
                            .iter()
                            .copied()
                            .collect::<Option<Vec<_>>>()
                            .expect("complete coordinate assignment"),
                    );
                }
                rollback(assigned, point_uses, propagated);
                return;
            };
            if budget.is_none() && *states >= MAX_COORDINATE_CLOSURE_STATES {
                *exhausted = true;
                rollback(assigned, point_uses, propagated);
                return;
            }
            *states += 1;
            for point in values {
                assigned[root] = Some(point);
                point_uses[point] += 1;
                walk(
                    domains,
                    edges,
                    edge_ids,
                    local_edge_by_id,
                    root_edges,
                    edge_candidates,
                    edge_faces,
                    face_edges,
                    closed_faces,
                    boundary_domains,
                    component_points,
                    assigned,
                    point_uses,
                    solutions,
                    states,
                    exhausted,
                    budget,
                );
                point_uses[point] -= 1;
                assigned[root] = None;
                if solutions.len() > 1 || *exhausted {
                    break;
                }
            }
            rollback(assigned, point_uses, propagated);
        }

        let mut roots = Vec::new();
        for node in 0..self.union.len() {
            if self.union.find(node) == node {
                roots.push(node);
            }
        }
        if roots.len() < point_count {
            return None;
        }
        if roots.len() == point_count && incidence.is_none() {
            return self.point_assignment(point_count, edge_candidates, budget);
        }
        let root_indices = roots
            .iter()
            .enumerate()
            .map(|(index, root)| (*root, index))
            .collect::<HashMap<_, _>>();
        let edges = edge_candidates
            .iter()
            .enumerate()
            .map(|(edge, _)| {
                Some([
                    *root_indices.get(&self.union.find(edge * 2))?,
                    *root_indices.get(&self.union.find(edge * 2 + 1))?,
                ])
            })
            .collect::<Option<Vec<_>>>()?;
        let domains = roots
            .iter()
            .map(|root| {
                self.domains[*root]
                    .iter()
                    .copied()
                    .filter(|point| *point < point_count)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if domains.iter().any(Vec::is_empty) {
            return None;
        }
        if domains
            .iter()
            .flatten()
            .copied()
            .collect::<HashSet<_>>()
            .len()
            != point_count
        {
            return None;
        }
        let mut dependency = UnionFind::new(roots.len());
        for [left, right] in &edges {
            dependency.union(*left, *right);
        }
        let mut root_by_point = HashMap::new();
        for (root, domain) in domains.iter().enumerate() {
            for point in domain {
                if let Some(previous) = root_by_point.insert(*point, root) {
                    dependency.union(previous, root);
                }
            }
        }
        let mut components = HashMap::<usize, Vec<usize>>::new();
        for root in 0..roots.len() {
            components
                .entry(dependency.find(root))
                .or_default()
                .push(root);
        }
        let mut components = components.into_values().collect::<Vec<_>>();
        components.sort_by_key(|component| component[0]);
        let mut assignment = vec![None; roots.len()];
        for component in components {
            let component_set = component.iter().copied().collect::<HashSet<_>>();
            let local_index = component
                .iter()
                .enumerate()
                .map(|(local, global)| (*global, local))
                .collect::<HashMap<_, _>>();
            let edge_ids = edges
                .iter()
                .enumerate()
                .filter_map(|(edge, [left, _])| component_set.contains(left).then_some(edge))
                .collect::<Vec<_>>();
            let local_edges = edge_ids
                .iter()
                .map(|edge| {
                    let [left, right] = edges[*edge];
                    Some([*local_index.get(&left)?, *local_index.get(&right)?])
                })
                .collect::<Option<Vec<_>>>()?;
            let local_edge_by_id = edge_ids
                .iter()
                .copied()
                .enumerate()
                .map(|(local, edge)| (edge, local))
                .collect::<HashMap<_, _>>();
            let local_edge_faces = incidence.map(|(edge_faces, _)| {
                edge_ids
                    .iter()
                    .map(|edge| edge_faces[*edge])
                    .collect::<Vec<_>>()
            });
            let face_edges = incidence.and_then(|(_, boundary_domains)| {
                if budget.is_some_and(|budget| !budget.charge_by(edge_ids.len().max(1))) {
                    return None;
                }
                let mut face_edges = vec![Vec::new(); boundary_domains.len()];
                for (edge, faces) in local_edge_faces.as_ref()?.iter().copied().enumerate() {
                    for (rank, face) in faces.into_iter().enumerate() {
                        if rank == 0 || face != faces[0] {
                            face_edges[face].push(edge);
                        }
                    }
                }
                Some(face_edges)
            });
            if incidence.is_some() && face_edges.is_none() {
                return None;
            }
            let closed_faces = incidence.map(|(edge_faces, boundary_domains)| {
                let face_count = boundary_domains.len();
                if budget.is_some_and(|budget| !budget.charge_by(edge_faces.len())) {
                    return Vec::new();
                }
                let local_edges = edge_ids.iter().copied().collect::<HashSet<_>>();
                let mut closed = vec![true; face_count];
                for (edge, faces) in edge_faces.iter().copied().enumerate() {
                    for (rank, face) in faces.into_iter().enumerate() {
                        if (rank == 0 || face != faces[0]) && !local_edges.contains(&edge) {
                            closed[face] = false;
                        }
                    }
                }
                closed
            });
            if closed_faces.as_ref().is_some_and(Vec::is_empty) {
                return None;
            }
            let mut local_domains = component
                .iter()
                .map(|root| domains[*root].clone())
                .collect::<Vec<_>>();
            let mut root_edges = vec![Vec::new(); component.len()];
            for (edge, [left, right]) in local_edges.iter().copied().enumerate() {
                root_edges[left].push(edge);
                if right != left {
                    root_edges[right].push(edge);
                }
            }
            let component_points = local_domains
                .iter()
                .flatten()
                .copied()
                .collect::<HashSet<_>>();
            if !enforce_sparse_endpoint_membership(
                &mut local_domains,
                &local_edges,
                &edge_ids,
                edge_candidates,
                budget,
            ) {
                return None;
            }
            let mut arc_domains = local_domains.clone();
            let arc_budget = budget.map(|budget| MeshConstraintBudget::new(budget.remaining.get()));
            let arc_consistent = enforce_edge_arc_consistency(
                &mut arc_domains,
                &local_edges,
                &edge_ids,
                &root_edges,
                edge_candidates,
                arc_budget.as_ref(),
            );
            if arc_consistent {
                if let (Some(budget), Some(arc_budget)) = (budget, arc_budget.as_ref()) {
                    let work = budget.remaining.get() - arc_budget.remaining.get();
                    if !budget.charge_by(work) {
                        return None;
                    }
                }
                local_domains = arc_domains;
            } else if arc_budget
                .as_ref()
                .is_none_or(|budget| !budget.exhausted.get())
            {
                return None;
            }
            if local_domains
                .iter()
                .flatten()
                .copied()
                .collect::<HashSet<_>>()
                != component_points
            {
                return None;
            }
            let mut solutions = Vec::new();
            let mut states = 0;
            let mut exhausted = false;
            walk(
                &local_domains,
                &local_edges,
                &edge_ids,
                &local_edge_by_id,
                &root_edges,
                edge_candidates,
                local_edge_faces.as_deref(),
                face_edges.as_deref(),
                closed_faces.as_deref(),
                incidence.map(|(_, boundary_domains)| boundary_domains),
                &component_points,
                &mut vec![None; component.len()],
                &mut vec![0; point_count],
                &mut solutions,
                &mut states,
                &mut exhausted,
                budget,
            );
            if exhausted {
                return None;
            }
            let [local_assignment] = solutions.as_slice() else {
                return None;
            };
            for (&root, &point) in component.iter().zip(local_assignment) {
                assignment[root] = Some(point);
            }
        }
        let assignment = assignment.into_iter().collect::<Option<Vec<_>>>()?;
        for (&root, &point) in roots.iter().zip(&assignment) {
            self.domains[root] = Arc::new(HashSet::from([point]));
        }
        let mut root_by_point = HashMap::new();
        for (&root, &point) in roots.iter().zip(&assignment) {
            if let Some(previous) = root_by_point.insert(point, root) {
                let merged = self.merge(previous, root)?;
                root_by_point.insert(point, merged);
            }
        }
        if !self.edge_domains_viable(edge_candidates) {
            return None;
        }
        self.point_assignment(point_count, edge_candidates, None)
    }

    pub(crate) fn assignment_has_option(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&MeshConstraintBudget>,
    ) -> bool {
        fn edge_start(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge.checked_mul(2)?.checked_add(usize::from(reversed))
        }

        fn edge_end(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(!reversed))
        }

        #[derive(Clone)]
        struct State {
            boundary_index: usize,
            at: usize,
            directions: Vec<bool>,
            quotient: MeshQuotient,
        }

        fn advance(
            state: &mut State,
            boundary: &[MeshBoundaryEdgeCandidate],
            reversed: bool,
        ) -> bool {
            if state.at > 0 {
                let Some(previous_end) =
                    edge_end(boundary[state.at - 1], state.directions[state.at - 1])
                else {
                    return false;
                };
                let Some(current_start) = edge_start(boundary[state.at], reversed) else {
                    return false;
                };
                if state.quotient.merge(previous_end, current_start).is_none() {
                    return false;
                }
            }
            state.directions.push(reversed);
            state.at += 1;
            true
        }

        let mut states = vec![State {
            boundary_index: 0,
            at: 0,
            directions: Vec::new(),
            quotient: self.clone(),
        }];
        while let Some(mut state) = states.pop() {
            loop {
                if budget.is_some_and(|budget| !budget.charge()) {
                    return false;
                }
                if state.boundary_index == assignment.boundaries.len() {
                    return true;
                }
                let boundary = &assignment.boundaries[state.boundary_index];
                if boundary.is_empty() {
                    break;
                }
                if state.at == boundary.len() {
                    let Some(last_end) =
                        edge_end(boundary[state.at - 1], state.directions[state.at - 1])
                    else {
                        break;
                    };
                    let Some(first_start) = edge_start(boundary[0], state.directions[0]) else {
                        break;
                    };
                    if state.quotient.merge(last_end, first_start).is_none() {
                        break;
                    }
                    if !state.quotient.edge_domains_viable(edge_candidates) {
                        break;
                    }
                    state.boundary_index += 1;
                    state.at = 0;
                    state.directions.clear();
                    continue;
                }
                if let Some(reversed) = boundary[state.at].reversed {
                    if !advance(&mut state, boundary, reversed) {
                        break;
                    }
                    continue;
                }
                for reversed in [true, false] {
                    if budget.is_some_and(|budget| !budget.charge()) {
                        return false;
                    }
                    let mut next = state.clone();
                    if advance(&mut next, boundary, reversed)
                        && next.quotient.edge_domains_viable(edge_candidates)
                    {
                        states.push(next);
                    }
                }
                break;
            }
        }
        false
    }

    #[cfg(test)]
    pub(crate) fn assignment_options(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Vec<(Vec<Vec<bool>>, Self)> {
        const MAX_ORIENTED_OPTIONS: usize = 4_096;

        fn edge_start(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge.checked_mul(2)?.checked_add(usize::from(reversed))
        }

        fn edge_end(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(!reversed))
        }

        fn boundary_options(
            quotient: MeshQuotient,
            boundary: &[MeshBoundaryEdgeCandidate],
            edge_candidates: &[Vec<[usize; 2]>],
        ) -> Vec<(Vec<bool>, MeshQuotient)> {
            fn advance(
                boundary: &[MeshBoundaryEdgeCandidate],
                at: usize,
                reversed: bool,
                directions: &mut Vec<bool>,
                mut quotient: MeshQuotient,
                edge_candidates: &[Vec<[usize; 2]>],
                output: &mut Vec<(Vec<bool>, MeshQuotient)>,
            ) {
                if at > 0 {
                    let Some(previous_end) = edge_end(boundary[at - 1], directions[at - 1]) else {
                        return;
                    };
                    let Some(current_start) = edge_start(boundary[at], reversed) else {
                        return;
                    };
                    let Some(root) = quotient.merge(previous_end, current_start) else {
                        return;
                    };
                    if !quotient.propagate_component_edge_domains(root, edge_candidates, None) {
                        return;
                    }
                }
                directions.push(reversed);
                walk(
                    boundary,
                    at + 1,
                    directions,
                    quotient,
                    edge_candidates,
                    output,
                );
                directions.pop();
            }

            fn walk(
                boundary: &[MeshBoundaryEdgeCandidate],
                at: usize,
                directions: &mut Vec<bool>,
                mut quotient: MeshQuotient,
                edge_candidates: &[Vec<[usize; 2]>],
                output: &mut Vec<(Vec<bool>, MeshQuotient)>,
            ) {
                if output.len() >= MAX_ORIENTED_OPTIONS {
                    return;
                }
                if at == boundary.len() {
                    let Some(last_end) = edge_end(boundary[at - 1], directions[at - 1]) else {
                        return;
                    };
                    let Some(first_start) = edge_start(boundary[0], directions[0]) else {
                        return;
                    };
                    let Some(root) = quotient.merge(last_end, first_start) else {
                        return;
                    };
                    if quotient.propagate_component_edge_domains(root, edge_candidates, None) {
                        output.push((directions.clone(), quotient));
                    }
                    return;
                }
                if let Some(reversed) = boundary[at].reversed {
                    advance(
                        boundary,
                        at,
                        reversed,
                        directions,
                        quotient,
                        edge_candidates,
                        output,
                    );
                } else {
                    advance(
                        boundary,
                        at,
                        false,
                        directions,
                        quotient.clone(),
                        edge_candidates,
                        output,
                    );
                    advance(
                        boundary,
                        at,
                        true,
                        directions,
                        quotient,
                        edge_candidates,
                        output,
                    );
                }
            }

            if boundary.is_empty() {
                return Vec::new();
            }
            let mut output = Vec::new();
            walk(
                boundary,
                0,
                &mut Vec::new(),
                quotient,
                edge_candidates,
                &mut output,
            );
            output
        }

        let mut options = vec![(Vec::new(), self.clone())];
        for boundary in &assignment.boundaries {
            let mut next = Vec::new();
            for (directions, quotient) in options {
                for (boundary_directions, quotient) in
                    boundary_options(quotient, boundary, edge_candidates)
                {
                    let mut directions = directions.clone();
                    directions.push(boundary_directions);
                    next.push((directions, quotient));
                    if next.len() >= MAX_ORIENTED_OPTIONS {
                        break;
                    }
                }
                if next.len() >= MAX_ORIENTED_OPTIONS {
                    break;
                }
            }
            options = next;
            if options.is_empty() {
                break;
            }
        }
        options
    }

    pub(crate) fn assignment_options_limited(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        oriented_edges: &HashSet<usize>,
        limit: usize,
        budget: Option<&MeshConstraintBudget>,
    ) -> Vec<(Vec<Vec<bool>>, Self)> {
        fn edge_start(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge.checked_mul(2)?.checked_add(usize::from(reversed))
        }

        fn edge_end(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(!reversed))
        }

        #[allow(clippy::too_many_arguments)]
        fn walk(
            boundaries: &[Vec<MeshBoundaryEdgeCandidate>],
            boundary_index: usize,
            at: usize,
            boundary_directions: &mut Vec<bool>,
            directions: &mut Vec<Vec<bool>>,
            mut quotient: MeshQuotient,
            edge_candidates: &[Vec<[usize; 2]>],
            output: &mut Vec<(Vec<Vec<bool>>, MeshQuotient)>,
            seen: &mut HashSet<MeshOrientationSignature>,
            oriented: &mut HashSet<usize>,
            limit: usize,
            budget: Option<&MeshConstraintBudget>,
        ) {
            if output.len() >= limit {
                return;
            }
            if budget.is_some_and(|budget| !budget.charge()) {
                return;
            }
            if boundary_index == boundaries.len() {
                let canonical_directions = directions
                    .iter()
                    .map(|boundary| {
                        let complement = boundary.iter().map(|value| !value).collect::<Vec<_>>();
                        if complement < *boundary {
                            complement
                        } else {
                            boundary.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                let signature = (quotient.signature(), canonical_directions);
                if seen.insert(signature) {
                    output.push((directions.clone(), quotient));
                }
                return;
            }
            let boundary = &boundaries[boundary_index];
            if boundary.is_empty() {
                return;
            }
            if at == boundary.len() {
                let Some(last_end) = edge_end(boundary[at - 1], boundary_directions[at - 1]) else {
                    return;
                };
                let Some(first_start) = edge_start(boundary[0], boundary_directions[0]) else {
                    return;
                };
                let Some(root) = quotient.merge(last_end, first_start) else {
                    return;
                };
                if !quotient.propagate_component_edge_domains(root, edge_candidates, budget) {
                    return;
                }
                directions.push(std::mem::take(boundary_directions));
                walk(
                    boundaries,
                    boundary_index + 1,
                    0,
                    boundary_directions,
                    directions,
                    quotient,
                    edge_candidates,
                    output,
                    seen,
                    oriented,
                    limit,
                    budget,
                );
                *boundary_directions = directions.pop().unwrap_or_default();
                return;
            }
            let edge = boundary[at].edge;
            let first = oriented.insert(edge);
            let mut advance = |reversed: bool, mut quotient: MeshQuotient| {
                if at > 0 {
                    let Some(previous_end) =
                        edge_end(boundary[at - 1], boundary_directions[at - 1])
                    else {
                        return;
                    };
                    let Some(current_start) = edge_start(boundary[at], reversed) else {
                        return;
                    };
                    let Some(root) = quotient.merge(previous_end, current_start) else {
                        return;
                    };
                    if !quotient.propagate_component_edge_domains(root, edge_candidates, budget) {
                        return;
                    }
                }
                boundary_directions.push(reversed);
                walk(
                    boundaries,
                    boundary_index,
                    at + 1,
                    boundary_directions,
                    directions,
                    quotient,
                    edge_candidates,
                    output,
                    seen,
                    oriented,
                    limit,
                    budget,
                );
                boundary_directions.pop();
            };
            match (boundary[at].reversed, first) {
                (Some(reversed), _) => advance(reversed, quotient),
                (None, true) => advance(false, quotient),
                (None, false) => {
                    advance(false, quotient.clone());
                    advance(true, quotient);
                }
            }
            if first {
                oriented.remove(&edge);
            }
        }

        if limit == 0 {
            return Vec::new();
        }
        if assignment.boundaries.iter().any(Vec::is_empty) {
            return Vec::new();
        }
        let mut oriented = oriented_edges.clone();
        let mut variable_count = 0usize;
        let orientation_plan = assignment
            .boundaries
            .iter()
            .map(|boundary| {
                boundary
                    .iter()
                    .map(|use_| match use_.reversed {
                        Some(reversed) => (reversed, None),
                        None if oriented.insert(use_.edge) => (false, None),
                        None => {
                            let variable = variable_count;
                            variable_count += 1;
                            (false, Some(variable))
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if variable_count <= 8 {
            let mut output = Vec::new();
            let mut seen = HashSet::new();
            let combinations = 1usize << variable_count;
            let orientation_work = assignment
                .boundaries
                .iter()
                .map(Vec::len)
                .sum::<usize>()
                .max(1);
            for mask in 0..combinations {
                if output.len() >= limit {
                    break;
                }
                if budget.is_some_and(|budget| !budget.charge_by(orientation_work)) {
                    break;
                }
                let directions = orientation_plan
                    .iter()
                    .map(|boundary| {
                        boundary
                            .iter()
                            .map(|(fixed, variable)| {
                                variable.map_or(*fixed, |variable| {
                                    let shift = variable_count - variable - 1;
                                    mask & (1usize << shift) != 0
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                if !uses_canonical_edge_direction_gauge(
                    &assignment.boundaries,
                    &directions,
                    oriented_edges,
                ) {
                    continue;
                }
                let mut quotient = self.clone();
                let mut merged_nodes = Vec::new();
                let merged =
                    assignment
                        .boundaries
                        .iter()
                        .zip(&directions)
                        .all(|(boundary, directions)| {
                            (0..boundary.len()).all(|index| {
                                let next = (index + 1) % boundary.len();
                                let Some(left_end) = edge_end(boundary[index], directions[index])
                                else {
                                    return false;
                                };
                                let Some(right_start) =
                                    edge_start(boundary[next], directions[next])
                                else {
                                    return false;
                                };
                                let Some(root) = quotient.merge(left_end, right_start) else {
                                    return false;
                                };
                                merged_nodes.push(root);
                                true
                            })
                        });
                if !merged {
                    continue;
                }
                let affected_edges = merged_nodes
                    .into_iter()
                    .flat_map(|node| {
                        let root = quotient.union.find(node);
                        quotient.members[root].clone()
                    })
                    .map(|node| node / 2)
                    .filter(|edge| !edge_candidates[*edge].is_empty())
                    .collect::<HashSet<_>>();
                if !quotient.propagate_edge_domains(affected_edges, edge_candidates, budget) {
                    continue;
                }
                let canonical_directions = directions
                    .iter()
                    .map(|boundary| {
                        let complement = boundary.iter().map(|value| !value).collect::<Vec<_>>();
                        if complement < *boundary {
                            complement
                        } else {
                            boundary.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                if seen.insert((quotient.signature(), canonical_directions)) {
                    output.push((directions, quotient));
                }
            }
            return output;
        }
        let mut output = Vec::new();
        let mut seen = HashSet::new();
        let mut oriented = oriented_edges.clone();
        walk(
            &assignment.boundaries,
            0,
            0,
            &mut Vec::new(),
            &mut Vec::new(),
            self.clone(),
            edge_candidates,
            &mut output,
            &mut seen,
            &mut oriented,
            limit,
            budget,
        );
        output
    }

    pub(crate) fn point_assignment(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&MeshConstraintBudget>,
    ) -> Option<HashMap<usize, usize>> {
        let mut solutions =
            self.point_assignments_with_budget(point_count, edge_candidates, 2, budget);
        (solutions.len() == 1).then(|| solutions.remove(0))
    }

    #[cfg(test)]
    pub(crate) fn point_assignment_exists(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&MeshConstraintBudget>,
    ) -> bool {
        !self
            .point_assignments_with_budget(point_count, edge_candidates, 1, budget)
            .is_empty()
    }

    fn point_assignments_with_budget(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        solution_limit: usize,
        budget: Option<&MeshConstraintBudget>,
    ) -> Vec<HashMap<usize, usize>> {
        type PointNeighbors = HashMap<usize, HashSet<usize>>;

        fn remaining_domains_match(values: &[(usize, Vec<usize>)], point_count: usize) -> bool {
            domains_have_distinct_matching(
                values.iter().map(|(_, values)| values.as_slice()),
                point_count,
            )
        }

        #[allow(clippy::too_many_arguments)]
        fn value_viable(
            root: usize,
            point: usize,
            domains: &[Arc<HashSet<usize>>],
            edge_roots: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            edge_candidates: &[Vec<[usize; 2]>],
            edge_neighbors: &[PointNeighbors],
            assigned: &[Option<usize>],
            used: &HashSet<usize>,
        ) -> bool {
            root_edges[root].iter().all(|&edge_index| {
                let edge = edge_roots[edge_index];
                let candidates = &edge_candidates[edge_index];
                let other = if edge[0] == root {
                    edge[1]
                } else if edge[1] == root {
                    edge[0]
                } else {
                    return true;
                };
                if other == root {
                    return candidates.is_empty()
                        || edge_neighbors[edge_index]
                            .get(&point)
                            .is_some_and(|neighbors| neighbors.contains(&point));
                }
                if let Some(other_point) = assigned[other] {
                    return candidates.is_empty()
                        || edge_neighbors[edge_index]
                            .get(&point)
                            .is_some_and(|neighbors| neighbors.contains(&other_point));
                }
                if candidates.is_empty() {
                    domains[other]
                        .iter()
                        .any(|other_point| *other_point != point && !used.contains(other_point))
                } else {
                    edge_neighbors[edge_index]
                        .get(&point)
                        .is_some_and(|neighbors| {
                            neighbors.iter().any(|other_point| {
                                *other_point != point
                                    && !used.contains(other_point)
                                    && domains[other].contains(other_point)
                            })
                        })
                }
            })
        }

        #[allow(clippy::too_many_arguments)]
        fn walk(
            domains: &[Arc<HashSet<usize>>],
            edge_roots: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            edge_candidates: &[Vec<[usize; 2]>],
            edge_neighbors: &[PointNeighbors],
            assigned: &mut [Option<usize>],
            used: &mut HashSet<usize>,
            solutions: &mut Vec<Vec<usize>>,
            solution_limit: usize,
            budget: Option<&MeshConstraintBudget>,
        ) {
            fn rollback(
                assigned: &mut [Option<usize>],
                used: &mut HashSet<usize>,
                propagated: Vec<(usize, usize)>,
            ) {
                for (root, point) in propagated.into_iter().rev() {
                    assigned[root] = None;
                    used.remove(&point);
                }
            }

            if solutions.len() >= solution_limit {
                return;
            }
            if budget.is_some_and(|budget| !budget.charge()) {
                return;
            }
            let values_for = |root: usize, assigned: &[Option<usize>], used: &HashSet<usize>| {
                domains[root]
                    .iter()
                    .copied()
                    .filter(|point| !used.contains(point))
                    .filter(|point| {
                        value_viable(
                            root,
                            *point,
                            domains,
                            edge_roots,
                            root_edges,
                            edge_candidates,
                            edge_neighbors,
                            assigned,
                            used,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let mut propagated = Vec::new();
            let branch = loop {
                let values = assigned
                    .iter()
                    .enumerate()
                    .filter(|(_, point)| point.is_none())
                    .map(|(root, _)| (root, values_for(root, assigned, used)))
                    .collect::<Vec<_>>();
                if values.is_empty() {
                    break Some(None);
                }
                if values.iter().any(|(_, values)| values.is_empty())
                    || !remaining_domains_match(&values, assigned.len())
                {
                    break None;
                }
                let mut dead = false;
                let mut progress = false;
                for root in 0..assigned.len() {
                    if assigned[root].is_some() {
                        continue;
                    }
                    let values = values_for(root, assigned, used);
                    let Some(&point) = values.first() else {
                        dead = true;
                        break;
                    };
                    if values.len() != 1 {
                        continue;
                    }
                    if !used.insert(point) {
                        dead = true;
                        break;
                    }
                    assigned[root] = Some(point);
                    propagated.push((root, point));
                    progress = true;
                }
                if dead {
                    break None;
                }
                if !progress {
                    break Some(
                        values
                            .into_iter()
                            .min_by_key(|(root, values)| (values.len(), *root)),
                    );
                }
            };
            let Some(branch) = branch else {
                rollback(assigned, used, propagated);
                return;
            };
            let Some((root, values)) = branch else {
                if let Some(solution) = assigned.iter().copied().collect::<Option<Vec<_>>>() {
                    solutions.push(solution);
                }
                rollback(assigned, used, propagated);
                return;
            };
            for point in values {
                assigned[root] = Some(point);
                used.insert(point);
                walk(
                    domains,
                    edge_roots,
                    root_edges,
                    edge_candidates,
                    edge_neighbors,
                    assigned,
                    used,
                    solutions,
                    solution_limit,
                    budget,
                );
                used.remove(&point);
                assigned[root] = None;
                if solutions.len() >= solution_limit {
                    break;
                }
            }
            rollback(assigned, used, propagated);
        }

        let mut roots = Vec::new();
        for node in 0..self.union.len() {
            let root = self.union.find(node);
            if root == node {
                roots.push(root);
            }
        }
        if roots.len() != point_count {
            return Vec::new();
        }
        let domains = roots
            .iter()
            .map(|root| self.domains[*root].clone())
            .collect::<Vec<_>>();
        let root_indices = roots
            .iter()
            .enumerate()
            .map(|(index, root)| (*root, index))
            .collect::<HashMap<_, _>>();
        let Some(edge_roots) = edge_candidates
            .iter()
            .enumerate()
            .map(|(edge, _)| {
                Some([
                    *root_indices.get(&self.union.find(edge * 2))?,
                    *root_indices.get(&self.union.find(edge * 2 + 1))?,
                ])
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Vec::new();
        };
        let mut root_edges = vec![Vec::new(); roots.len()];
        for (edge_index, edge) in edge_roots.iter().enumerate() {
            root_edges[edge[0]].push(edge_index);
            if edge[1] != edge[0] {
                root_edges[edge[1]].push(edge_index);
            }
        }
        let edge_neighbors = edge_candidates
            .iter()
            .map(|candidates| {
                let mut neighbors = PointNeighbors::new();
                for [left, right] in candidates {
                    neighbors.entry(*left).or_default().insert(*right);
                    neighbors.entry(*right).or_default().insert(*left);
                }
                neighbors
            })
            .collect::<Vec<_>>();

        let mut solutions = Vec::new();
        walk(
            &domains,
            &edge_roots,
            &root_edges,
            edge_candidates,
            &edge_neighbors,
            &mut vec![None; domains.len()],
            &mut HashSet::new(),
            &mut solutions,
            solution_limit,
            budget,
        );
        solutions
            .into_iter()
            .map(|solution| roots.iter().copied().zip(solution).collect())
            .collect()
    }
}

struct DeferredFaceQuotientOptions {
    alternatives: Vec<MeshQuotient>,
    base_nodes: Vec<usize>,
}

fn materialize_deferred_quotient_option(
    base: &MeshQuotient,
    local: &MeshQuotient,
    base_nodes: &[usize],
    affected_edges: impl IntoIterator<Item = usize>,
    edge_candidates: &[Vec<[usize; 2]>],
    budget: &MeshConstraintBudget,
) -> Option<MeshQuotient> {
    let mut materialized = base.clone();
    for local_node in 0..base_nodes.len() {
        let local_root = local.union.root(local_node);
        if local_root != local_node {
            materialized.merge(base_nodes[local_root], base_nodes[local_node])?;
        }
    }
    materialized
        .propagate_edge_domains(affected_edges, edge_candidates, Some(budget))
        .then_some(materialized)
}

fn deferred_face_quotient_options_limited(
    domain: &MeshDeferredFaceBoundary,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &MeshQuotient,
    limit: usize,
    budget: &MeshConstraintBudget,
) -> Option<DeferredFaceQuotientOptions> {
    #[derive(Clone, Copy)]
    struct Gap {
        left_end: usize,
        right_start: usize,
        capacity: usize,
    }

    #[allow(clippy::too_many_arguments)]
    fn fill_gap(
        gaps: &[Gap],
        gap: usize,
        at: usize,
        target: usize,
        used: u64,
        previous_end: usize,
        missing_edges: &[usize],
        missing_nodes: &[[usize; 2]],
        edge_candidates: &[Vec<[usize; 2]>],
        quotient: MeshQuotient,
        base_quotient: &MeshQuotient,
        base_nodes: &[usize],
        output: &mut Vec<MeshQuotient>,
        limit: usize,
        budget: &MeshConstraintBudget,
    ) {
        if output.len() >= limit || budget.exhausted.get() {
            return;
        }
        if at == target {
            let mut quotient = quotient;
            if quotient
                .merge(previous_end, gaps[gap].right_start)
                .is_none()
            {
                return;
            }
            walk_gaps(
                gaps,
                gap + 1,
                used,
                missing_edges,
                missing_nodes,
                edge_candidates,
                &quotient,
                base_quotient,
                base_nodes,
                output,
                limit,
                budget,
            );
            return;
        }
        let options = (missing_edges.len() - used.count_ones() as usize).saturating_mul(2);
        if options > 1 && !budget.charge_by(options) {
            return;
        }
        let mut seen = HashSet::new();
        for (rank, _) in missing_edges.iter().enumerate() {
            if used & (1 << rank) != 0 {
                continue;
            }
            for reversed in [false, true] {
                let start = missing_nodes[rank][usize::from(reversed)];
                let end = missing_nodes[rank][usize::from(!reversed)];
                let mut next = quotient.clone();
                if next.merge(previous_end, start).is_none() {
                    continue;
                }
                let end_root = next.union.find(end);
                if !seen.insert((rank, end_root, next.signature())) {
                    continue;
                }
                fill_gap(
                    gaps,
                    gap,
                    at + 1,
                    target,
                    used | (1 << rank),
                    end,
                    missing_edges,
                    missing_nodes,
                    edge_candidates,
                    next,
                    base_quotient,
                    base_nodes,
                    output,
                    limit,
                    budget,
                );
                if output.len() >= limit || budget.exhausted.get() {
                    return;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_gaps(
        gaps: &[Gap],
        gap: usize,
        used: u64,
        missing_edges: &[usize],
        missing_nodes: &[[usize; 2]],
        edge_candidates: &[Vec<[usize; 2]>],
        quotient: &MeshQuotient,
        base_quotient: &MeshQuotient,
        base_nodes: &[usize],
        output: &mut Vec<MeshQuotient>,
        limit: usize,
        budget: &MeshConstraintBudget,
    ) {
        if output.len() >= limit || budget.exhausted.get() {
            return;
        }
        if gap == gaps.len() {
            if used.count_ones() as usize != missing_edges.len() {
                return;
            }
            let affected_edges = missing_edges
                .iter()
                .copied()
                .filter(|edge| !edge_candidates[*edge].is_empty())
                .collect::<HashSet<_>>();
            if materialize_deferred_quotient_option(
                base_quotient,
                quotient,
                base_nodes,
                affected_edges,
                edge_candidates,
                budget,
            )
            .is_some()
            {
                output.push(quotient.clone());
            }
            return;
        }
        let remaining_edges = missing_edges.len() - used.count_ones() as usize;
        let remaining_gaps = gaps.len() - gap - 1;
        let minimum = 1;
        let maximum = gaps[gap]
            .capacity
            .min(remaining_edges.saturating_sub(remaining_gaps));
        if maximum < minimum {
            return;
        }
        if maximum > minimum && !budget.charge_by(maximum - minimum + 1) {
            return;
        }
        for target in minimum..=maximum {
            fill_gap(
                gaps,
                gap,
                0,
                target,
                used,
                gaps[gap].left_end,
                missing_edges,
                missing_nodes,
                edge_candidates,
                quotient.clone(),
                base_quotient,
                base_nodes,
                output,
                limit,
                budget,
            );
            if output.len() >= limit || budget.exhausted.get() {
                return;
            }
        }
    }

    if domain.missing_edges.len() > u64::BITS as usize {
        return None;
    }
    let mut gaps = Vec::new();
    for cycle in &domain.cycles {
        if cycle.exact_uses.is_empty() {
            return None;
        }
        for index in 0..cycle.exact_uses.len() {
            let (left, left_span) = cycle.exact_uses[index];
            let right = cycle.exact_uses[(index + 1) % cycle.exact_uses.len()].0;
            let left_end_position = (left.start + left_span) % cycle.length;
            let capacity = (right.start + cycle.length - left_end_position) % cycle.length;
            if capacity == 0 {
                continue;
            }
            let left_reversed = left.reversed?;
            let right_reversed = right.reversed?;
            gaps.push(Gap {
                left_end: left
                    .edge
                    .checked_mul(2)?
                    .checked_add(usize::from(!left_reversed))?,
                right_start: right
                    .edge
                    .checked_mul(2)?
                    .checked_add(usize::from(right_reversed))?,
                capacity,
            });
        }
    }
    if gaps.is_empty() {
        return domain
            .missing_edges
            .is_empty()
            .then(|| DeferredFaceQuotientOptions {
                alternatives: Vec::new(),
                base_nodes: Vec::new(),
            });
    }
    if domain.missing_edges.len() < gaps.len() {
        return Some(DeferredFaceQuotientOptions {
            alternatives: Vec::new(),
            base_nodes: Vec::new(),
        });
    }
    let mut base_nodes = gaps
        .iter()
        .flat_map(|gap| [gap.left_end, gap.right_start])
        .chain(
            domain
                .missing_edges
                .iter()
                .flat_map(|edge| [edge * 2, edge * 2 + 1]),
        )
        .map(|node| quotient.union.root(node))
        .collect::<Vec<_>>();
    base_nodes.sort_unstable();
    base_nodes.dedup();
    let local_by_base = base_nodes
        .iter()
        .enumerate()
        .map(|(local, base)| (*base, local))
        .collect::<HashMap<_, _>>();
    for gap in &mut gaps {
        gap.left_end = local_by_base[&quotient.union.root(gap.left_end)];
        gap.right_start = local_by_base[&quotient.union.root(gap.right_start)];
    }
    let missing_nodes = domain
        .missing_edges
        .iter()
        .map(|edge| {
            [
                local_by_base[&quotient.union.root(edge * 2)],
                local_by_base[&quotient.union.root(edge * 2 + 1)],
            ]
        })
        .collect::<Vec<_>>();
    let local_quotient = MeshQuotient {
        union: UnionFind::new(base_nodes.len()),
        domains: base_nodes
            .iter()
            .map(|root| quotient.domains[*root].clone())
            .collect(),
        members: (0..base_nodes.len()).map(|node| vec![node]).collect(),
    };
    gaps.sort_unstable_by_key(|gap| {
        let single_edge_options = if gap.capacity == 1 {
            domain
                .missing_edges
                .iter()
                .enumerate()
                .flat_map(|(rank, _)| [false, true].map(move |reversed| (rank, reversed)))
                .filter(|(rank, reversed)| {
                    let start = missing_nodes[*rank][usize::from(*reversed)];
                    let end = missing_nodes[*rank][usize::from(!*reversed)];
                    let mut trial = local_quotient.clone();
                    trial.merge(gap.left_end, start).is_some()
                        && trial.merge(end, gap.right_start).is_some()
                })
                .count()
        } else {
            usize::MAX
        };
        (gap.capacity, single_edge_options)
    });
    let mut output = Vec::new();
    walk_gaps(
        &gaps,
        0,
        0,
        &domain.missing_edges,
        &missing_nodes,
        edge_candidates,
        &local_quotient,
        quotient,
        &base_nodes,
        &mut output,
        limit,
        budget,
    );
    (!budget.exhausted.get()).then_some(DeferredFaceQuotientOptions {
        alternatives: output,
        base_nodes,
    })
}

fn propagate_common_deferred_quotients(
    mut options: DeferredFaceQuotientOptions,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
    budget: &MeshConstraintBudget,
) -> Option<()> {
    let node_count = options.base_nodes.len();
    let mut equivalence_classes = HashMap::<Vec<usize>, Vec<usize>>::new();
    for node in 0..node_count {
        let signature = options
            .alternatives
            .iter_mut()
            .map(|alternative| alternative.union.find(node))
            .collect::<Vec<_>>();
        equivalence_classes.entry(signature).or_default().push(node);
    }
    for nodes in equivalence_classes.into_values() {
        let Some((&representative, rest)) = nodes.split_first() else {
            continue;
        };
        for &node in rest {
            quotient.merge(options.base_nodes[representative], options.base_nodes[node])?;
        }
    }
    for local in 0..node_count {
        let mut allowed = HashSet::new();
        for alternative in &mut options.alternatives {
            let root = alternative.union.find(local);
            allowed.extend(alternative.domains[root].iter().copied());
        }
        let root = quotient.union.find(options.base_nodes[local]);
        let narrowed = quotient.domains[root]
            .intersection(&allowed)
            .copied()
            .collect::<HashSet<_>>();
        if narrowed.is_empty() {
            return None;
        }
        quotient.domains[root] = Arc::new(narrowed);
    }
    let affected_edges = options
        .base_nodes
        .into_iter()
        .flat_map(|node| {
            let root = quotient.union.find(node);
            quotient.members[root].clone()
        })
        .map(|node| node / 2)
        .filter(|edge| !edge_candidates[*edge].is_empty())
        .collect::<HashSet<_>>();
    quotient
        .propagate_edge_domains(affected_edges, edge_candidates, Some(budget))
        .then_some(())
}

fn common_supported_corner_equations(
    quotient: &mut MeshQuotient,
    assignments: &[MeshFaceBoundaryAssignment],
    budget: &MeshConstraintBudget,
) -> Option<HashSet<[usize; 2]>> {
    fn port(use_: MeshBoundaryEdgeCandidate, reversed: bool, end: bool) -> Option<usize> {
        use_.edge
            .checked_mul(2)?
            .checked_add(usize::from(if end { !reversed } else { reversed }))
    }

    fn compatible(quotient: &MeshQuotient, left: usize, right: usize) -> bool {
        let left = quotient.union.root(left);
        let right = quotient.union.root(right);
        left == right || !quotient.domains[left].is_disjoint(&quotient.domains[right])
    }

    let mut common = None::<HashSet<[usize; 2]>>;
    'assignments: for assignment in assignments {
        if !budget.charge() {
            return None;
        }
        let mut forced = HashSet::new();
        for boundary in &assignment.boundaries {
            if boundary.is_empty() {
                return None;
            }
            let directions = boundary
                .iter()
                .map(|use_| {
                    use_.reversed
                        .map_or_else(|| vec![false, true], |reversed| vec![reversed])
                })
                .collect::<Vec<_>>();
            let mut supported = (0..boundary.len())
                .map(|index| {
                    vec![
                        vec![false; directions[(index + 1) % boundary.len()].len()];
                        directions[index].len()
                    ]
                })
                .collect::<Vec<_>>();
            for first in 0..directions[0].len() {
                let mut forward = directions
                    .iter()
                    .map(|states| vec![false; states.len()])
                    .collect::<Vec<_>>();
                forward[0][first] = true;
                for index in 0..boundary.len().saturating_sub(1) {
                    for left in 0..directions[index].len() {
                        if !forward[index][left] {
                            continue;
                        }
                        for right in 0..directions[index + 1].len() {
                            let left_node = port(boundary[index], directions[index][left], true)?;
                            let right_node =
                                port(boundary[index + 1], directions[index + 1][right], false)?;
                            if compatible(quotient, left_node, right_node) {
                                forward[index + 1][right] = true;
                            }
                        }
                    }
                }
                let last = boundary.len() - 1;
                let mut backward = directions
                    .iter()
                    .map(|states| vec![false; states.len()])
                    .collect::<Vec<_>>();
                for state in 0..directions[last].len() {
                    let left_node = port(boundary[last], directions[last][state], true)?;
                    let right_node = port(boundary[0], directions[0][first], false)?;
                    backward[last][state] =
                        forward[last][state] && compatible(quotient, left_node, right_node);
                }
                for index in (0..last).rev() {
                    for left in 0..directions[index].len() {
                        backward[index][left] = forward[index][left]
                            && (0..directions[index + 1].len()).any(|right| {
                                if !backward[index + 1][right] {
                                    return false;
                                }
                                let Some(left_node) =
                                    port(boundary[index], directions[index][left], true)
                                else {
                                    return false;
                                };
                                let Some(right_node) =
                                    port(boundary[index + 1], directions[index + 1][right], false)
                                else {
                                    return false;
                                };
                                compatible(quotient, left_node, right_node)
                            });
                    }
                }
                if !backward[0][first] {
                    continue;
                }
                for index in 0..last {
                    for left in 0..directions[index].len() {
                        if !forward[index][left] {
                            continue;
                        }
                        for right in 0..directions[index + 1].len() {
                            if backward[index + 1][right] {
                                let left_node =
                                    port(boundary[index], directions[index][left], true)?;
                                let right_node =
                                    port(boundary[index + 1], directions[index + 1][right], false)?;
                                if compatible(quotient, left_node, right_node) {
                                    supported[index][left][right] = true;
                                }
                            }
                        }
                    }
                }
                for state in 0..directions[last].len() {
                    if backward[last][state] {
                        supported[last][state][first] = true;
                    }
                }
            }
            if supported
                .iter()
                .any(|transitions| transitions.iter().flatten().all(|value| !value))
            {
                continue 'assignments;
            }
            for index in 0..boundary.len() {
                let next = (index + 1) % boundary.len();
                let mut equations = HashSet::new();
                for left in 0..directions[index].len() {
                    for right in 0..directions[next].len() {
                        if supported[index][left][right] {
                            let left = quotient.union.find(port(
                                boundary[index],
                                directions[index][left],
                                true,
                            )?);
                            let right = quotient.union.find(port(
                                boundary[next],
                                directions[next][right],
                                false,
                            )?);
                            equations.insert(if left <= right {
                                [left, right]
                            } else {
                                [right, left]
                            });
                        }
                    }
                }
                if equations.len() == 1 {
                    if let Some(equation) = equations.into_iter().next() {
                        forced.insert(equation);
                    }
                }
            }
        }
        match &mut common {
            Some(common) => common.retain(|equation| forced.contains(equation)),
            None => common = Some(forced),
        }
    }
    common
}

fn propagate_common_full_quotients(
    mut alternatives: Vec<MeshQuotient>,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
) -> Option<()> {
    let node_count = quotient.union.len();
    let mut equivalence_classes = HashMap::<Vec<usize>, Vec<usize>>::new();
    for node in 0..node_count {
        let signature = alternatives
            .iter_mut()
            .map(|alternative| alternative.union.find(node))
            .collect::<Vec<_>>();
        equivalence_classes.entry(signature).or_default().push(node);
    }
    for nodes in equivalence_classes.into_values() {
        let Some((&representative, rest)) = nodes.split_first() else {
            continue;
        };
        for &node in rest {
            quotient.merge(representative, node)?;
        }
    }

    let mut roots = Vec::new();
    for node in 0..node_count {
        if quotient.union.find(node) == node {
            roots.push(node);
        }
    }
    for root in roots {
        let representative = quotient.members[root][0];
        let mut allowed = HashSet::new();
        for alternative in &mut alternatives {
            let alternative_root = alternative.union.find(representative);
            allowed.extend(alternative.domains[alternative_root].iter().copied());
        }
        let narrowed = quotient.domains[root]
            .intersection(&allowed)
            .copied()
            .collect::<HashSet<_>>();
        if narrowed.is_empty() {
            return None;
        }
        quotient.domains[root] = Arc::new(narrowed);
    }
    quotient.edge_domains_viable(edge_candidates).then_some(())
}

pub(crate) fn propagate_common_ordered_face_quotients(
    domains: &[MeshFaceBoundaryDomain],
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
    budget: &MeshConstraintBudget,
) -> Option<()> {
    const MAX_FACE_OPTIONS: usize = 4_096;
    const MAX_ORDERED_FACE_CONSTRAINT_OPERATIONS: usize = 64;
    const MAX_DEFERRED_FACE_CONSTRAINT_OPERATIONS: usize = 512;

    let mut face_order = (0..domains.len()).collect::<Vec<_>>();
    face_order.sort_unstable_by_key(|face| match &domains[*face] {
        MeshFaceBoundaryDomain::DeferredValidation(_) => (0, 0),
        MeshFaceBoundaryDomain::Ordered(assignments) => (1, assignments.len()),
        MeshFaceBoundaryDomain::UnorderedFullCycle(_) => (2, 0),
    });
    loop {
        let before = quotient.monotone_measure();
        for &face in &face_order {
            let domain = &domains[face];
            let face_budget = MeshConstraintBudget::new(match domain {
                MeshFaceBoundaryDomain::DeferredValidation(_) => {
                    MAX_DEFERRED_FACE_CONSTRAINT_OPERATIONS
                }
                MeshFaceBoundaryDomain::Ordered(_)
                | MeshFaceBoundaryDomain::UnorderedFullCycle(_) => {
                    MAX_ORDERED_FACE_CONSTRAINT_OPERATIONS
                }
            });
            if let MeshFaceBoundaryDomain::DeferredValidation(domain) = domain {
                let mut merged_nodes = Vec::new();
                for cycle in &domain.cycles {
                    for index in 0..cycle.exact_uses.len() {
                        let (left, left_span) = cycle.exact_uses[index];
                        let right = cycle.exact_uses[(index + 1) % cycle.exact_uses.len()].0;
                        let left_end = (left.start + left_span) % cycle.length;
                        let capacity = (right.start + cycle.length - left_end) % cycle.length;
                        if capacity != 0 {
                            continue;
                        }
                        let left_reversed = left.reversed?;
                        let right_reversed = right.reversed?;
                        let left_node = left
                            .edge
                            .checked_mul(2)?
                            .checked_add(usize::from(!left_reversed))?;
                        let right_node = right
                            .edge
                            .checked_mul(2)?
                            .checked_add(usize::from(right_reversed))?;
                        merged_nodes.push(quotient.merge(left_node, right_node)?);
                    }
                }
                let affected_edges = merged_nodes
                    .into_iter()
                    .flat_map(|node| {
                        let root = quotient.union.find(node);
                        quotient.members[root].clone()
                    })
                    .map(|node| node / 2)
                    .filter(|edge| !edge_candidates[*edge].is_empty())
                    .collect::<HashSet<_>>();
                if !quotient.propagate_edge_domains(affected_edges, edge_candidates, Some(budget)) {
                    return None;
                }
                let Some(options) = deferred_face_quotient_options_limited(
                    domain,
                    edge_candidates,
                    quotient,
                    MAX_FACE_OPTIONS + 1,
                    &face_budget,
                ) else {
                    continue;
                };
                if options.alternatives.len() <= MAX_FACE_OPTIONS
                    && !options.alternatives.is_empty()
                {
                    propagate_common_deferred_quotients(
                        options,
                        edge_candidates,
                        quotient,
                        budget,
                    )?;
                }
                continue;
            }
            let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                continue;
            };
            if let Some(equations) =
                common_supported_corner_equations(quotient, assignments, &face_budget)
            {
                let mut merged_nodes = Vec::new();
                for [left, right] in equations {
                    merged_nodes.push(quotient.merge(left, right)?);
                }
                let affected_edges = merged_nodes
                    .into_iter()
                    .flat_map(|node| {
                        let root = quotient.union.find(node);
                        quotient.members[root].clone()
                    })
                    .map(|node| node / 2)
                    .filter(|edge| !edge_candidates[*edge].is_empty())
                    .collect::<HashSet<_>>();
                if !quotient.propagate_edge_domains(affected_edges, edge_candidates, Some(budget)) {
                    return None;
                }
            }
            if face_budget.exhausted.get() {
                continue;
            }
            let mut alternatives = Vec::new();
            let mut truncated = false;
            for assignment in assignments {
                if !face_budget.charge_by(quotient.signature_work()) {
                    truncated = true;
                    break;
                }
                let options = quotient.assignment_options_limited(
                    assignment,
                    edge_candidates,
                    &HashSet::new(),
                    MAX_FACE_OPTIONS + 1,
                    Some(&face_budget),
                );
                if face_budget.exhausted.get() {
                    truncated = true;
                    break;
                }
                if options.len() > MAX_FACE_OPTIONS {
                    truncated = true;
                    break;
                }
                alternatives.extend(options.into_iter().map(|(_, quotient)| quotient));
                if alternatives.len() > MAX_FACE_OPTIONS {
                    truncated = true;
                    break;
                }
            }
            if truncated {
                continue;
            }
            if alternatives.len() > MAX_FACE_OPTIONS {
                continue;
            }
            if alternatives.is_empty() {
                continue;
            }
            propagate_common_full_quotients(alternatives, edge_candidates, quotient)?;
        }
        if quotient.monotone_measure() == before {
            return Some(());
        }
    }
}

fn mesh_boundary_domain_edges(domain: &MeshFaceBoundaryDomain) -> Vec<usize> {
    let mut edges = match domain {
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
    edges.sort_unstable();
    edges.dedup();
    edges
}

pub(crate) fn bounded_unordered_cycle_assignments(
    edges: &[usize],
    quotient: &MeshQuotient,
    limit: usize,
    budget: &MeshConstraintBudget,
) -> Option<Vec<MeshFaceBoundaryAssignment>> {
    struct Search<'a> {
        edges: &'a [usize],
        compatible: &'a HashSet<(usize, usize)>,
        limit: usize,
        budget: &'a MeshConstraintBudget,
        assignments: Vec<MeshFaceBoundaryAssignment>,
    }

    impl Search<'_> {
        fn walk(
            &mut self,
            first_start: usize,
            previous_end: usize,
            used: u64,
            boundary: &mut Vec<MeshBoundaryEdgeCandidate>,
        ) -> bool {
            if !self.budget.charge() {
                return false;
            }
            if boundary.len() == self.edges.len() {
                if self.compatible.contains(&(previous_end, first_start)) {
                    self.assignments.push(MeshFaceBoundaryAssignment {
                        boundaries: vec![boundary.clone()],
                    });
                }
                return self.assignments.len() <= self.limit;
            }
            for rank in 1..self.edges.len() {
                if used & (1 << rank) != 0 {
                    continue;
                }
                let edge = self.edges[rank];
                for reversed in [false, true] {
                    let start = edge * 2 + usize::from(reversed);
                    if !self.compatible.contains(&(previous_end, start)) {
                        continue;
                    }
                    boundary.push(MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: Some(reversed),
                    });
                    if !self.walk(
                        first_start,
                        edge * 2 + usize::from(!reversed),
                        used | (1 << rank),
                        boundary,
                    ) {
                        return false;
                    }
                    boundary.pop();
                }
            }
            true
        }
    }

    if edges.is_empty() || edges.len() > u64::BITS as usize {
        return None;
    }
    let edge_count = edges.len();
    let mut edges = edges.to_vec();
    edges.sort_unstable();
    edges.dedup();
    if edges.len() != edge_count {
        return None;
    }
    let mut quotient = quotient.clone();
    let nodes = edges
        .iter()
        .flat_map(|edge| [edge * 2, edge * 2 + 1])
        .collect::<Vec<_>>();
    let mut compatible = HashSet::new();
    for &left in &nodes {
        let left_root = quotient.union.find(left);
        for &right in &nodes {
            let right_root = quotient.union.find(right);
            if left_root == right_root
                || !quotient.domains[left_root].is_disjoint(&quotient.domains[right_root])
            {
                compatible.insert((left, right));
            }
        }
    }
    let first = edges[0];
    let first_start = first * 2;
    let mut boundary = vec![MeshBoundaryEdgeCandidate {
        edge: first,
        start: 0,
        end: 0,
        reversed: Some(false),
    }];
    let mut search = Search {
        edges: &edges,
        compatible: &compatible,
        limit,
        budget,
        assignments: Vec::new(),
    };
    search
        .walk(first_start, first * 2 + 1, 1, &mut boundary)
        .then_some(search.assignments)
}

fn advance_boundary_component_states(
    domain: &MeshFaceBoundaryDomain,
    states: &[MeshQuotientGaugeState],
    edge_candidates: &[Vec<[usize; 2]>],
    limit: usize,
    budget: &MeshConstraintBudget,
) -> Option<Vec<MeshQuotientGaugeState>> {
    let mut next = Vec::new();
    let mut signatures = HashSet::new();
    let domain_edges = mesh_boundary_domain_edges(domain);
    for (state, oriented_edges) in states {
        let remaining = limit.saturating_add(1).saturating_sub(next.len());
        if remaining == 0 {
            return None;
        }
        let candidates = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => assignments
                .iter()
                .flat_map(|assignment| {
                    state
                        .assignment_options_limited(
                            assignment,
                            edge_candidates,
                            oriented_edges,
                            remaining,
                            Some(budget),
                        )
                        .into_iter()
                        .map(|(_, quotient)| quotient)
                })
                .collect::<Vec<_>>(),
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                let options = deferred_face_quotient_options_limited(
                    domain,
                    edge_candidates,
                    state,
                    remaining,
                    budget,
                )?;
                if options.alternatives.is_empty() && domain.missing_edges.is_empty() {
                    vec![state.clone()]
                } else {
                    let affected_edges = domain_edges
                        .iter()
                        .copied()
                        .filter(|edge| !edge_candidates[*edge].is_empty())
                        .collect::<HashSet<_>>();
                    options
                        .alternatives
                        .iter()
                        .filter_map(|local| {
                            materialize_deferred_quotient_option(
                                state,
                                local,
                                &options.base_nodes,
                                affected_edges.iter().copied(),
                                edge_candidates,
                                budget,
                            )
                        })
                        .collect()
                }
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                let assignments =
                    bounded_unordered_cycle_assignments(edges, state, remaining, budget)?;
                assignments
                    .iter()
                    .flat_map(|assignment| {
                        state
                            .assignment_options_limited(
                                assignment,
                                edge_candidates,
                                oriented_edges,
                                remaining,
                                Some(budget),
                            )
                            .into_iter()
                            .map(|(_, quotient)| quotient)
                    })
                    .collect()
            }
        };
        for mut candidate in candidates {
            let mut next_oriented = oriented_edges.clone();
            next_oriented.extend(domain_edges.iter().copied());
            if !budget.charge_by(
                candidate
                    .signature_work()
                    .saturating_add(next_oriented.len().max(1)),
            ) {
                return None;
            }
            let mut oriented_signature = next_oriented.iter().copied().collect::<Vec<_>>();
            oriented_signature.sort_unstable();
            if signatures.insert((candidate.signature(), oriented_signature)) {
                next.push((candidate, next_oriented));
            }
            if next.len() > limit {
                return None;
            }
        }
        if budget.exhausted.get() {
            return None;
        }
    }
    (!next.is_empty()).then_some(next)
}

pub(crate) fn propagate_common_boundary_components(
    domains: &[MeshFaceBoundaryDomain],
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
) -> Option<()> {
    const MAX_COMPONENT_STATES: usize = 128;
    const MAX_COMPONENT_OPERATIONS: usize = 8_192;
    const MAX_COMPONENT_ROUNDS: usize = 8;

    let active_faces = domains
        .iter()
        .enumerate()
        .filter_map(|(face, domain)| {
            mesh_boundary_domain_edges(domain)
                .into_iter()
                .any(|edge| edge_candidates[edge].is_empty())
                .then_some(face)
        })
        .collect::<Vec<_>>();
    let active_index = active_faces
        .iter()
        .enumerate()
        .map(|(index, face)| (*face, index))
        .collect::<HashMap<_, _>>();
    let mut components = UnionFind::new(active_faces.len());
    let mut edge_owner = HashMap::<usize, usize>::new();
    for &face in &active_faces {
        let index = active_index[&face];
        for edge in mesh_boundary_domain_edges(&domains[face]) {
            if let Some(previous) = edge_owner.insert(edge, index) {
                components.union(previous, index);
            }
        }
    }
    let mut faces_by_component = HashMap::<usize, Vec<usize>>::new();
    for face in active_faces {
        let root = components.find(active_index[&face]);
        faces_by_component.entry(root).or_default().push(face);
    }
    let mut face_components = faces_by_component.into_values().collect::<Vec<_>>();
    face_components.sort_by_key(|faces| faces.iter().copied().min().unwrap_or(usize::MAX));

    for mut faces in face_components {
        let face_key = |face: usize| match &domains[face] {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                let direction_work = assignments
                    .iter()
                    .map(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .filter(|use_| use_.reversed.is_none())
                            .count()
                    })
                    .sum::<usize>();
                (0, assignments.len(), direction_work, face)
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                (1, domain.missing_edges.len(), 0, face)
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => (2, edges.len(), 0, face),
        };
        let mut ordered_faces = Vec::with_capacity(faces.len());
        let mut selected_edges = HashSet::new();
        while !faces.is_empty() {
            let next = faces
                .iter()
                .enumerate()
                .min_by_key(|(_, face)| {
                    let shared = mesh_boundary_domain_edges(&domains[**face])
                        .into_iter()
                        .filter(|edge| selected_edges.contains(edge))
                        .count();
                    let key = face_key(**face);
                    (key.0, usize::MAX - shared, key)
                })
                .map(|(index, _)| index)?;
            let face = faces.swap_remove(next);
            selected_edges.extend(mesh_boundary_domain_edges(&domains[face]));
            ordered_faces.push(face);
        }
        let budget = MeshConstraintBudget::new(MAX_COMPONENT_OPERATIONS);
        for _ in 0..MAX_COMPONENT_ROUNDS {
            let before = quotient.monotone_measure();
            let mut cursor = 0usize;
            while cursor < ordered_faces.len() {
                let mut states = vec![(quotient.clone(), HashSet::<usize>::new())];
                let mut processed = 0usize;
                while let Some(&face) = ordered_faces.get(cursor + processed) {
                    let Some(next) = advance_boundary_component_states(
                        &domains[face],
                        &states,
                        edge_candidates,
                        MAX_COMPONENT_STATES,
                        &budget,
                    ) else {
                        break;
                    };
                    states = next;
                    processed += 1;
                }
                if processed == 0 {
                    cursor += 1;
                    continue;
                }
                propagate_common_full_quotients(
                    states.into_iter().map(|(state, _)| state).collect(),
                    edge_candidates,
                    quotient,
                )?;
                cursor += processed;
            }
            if quotient.monotone_measure() == before {
                break;
            }
        }
    }
    Some(())
}

type MeshFaceSelection = Option<(usize, Vec<Vec<bool>>)>;
pub(crate) type MeshEndpointPair = (usize, [usize; 2]);
pub(crate) type MeshEndpointSolutionFilter<'a> = &'a dyn Fn(&[MeshEndpointPair]) -> bool;
type MeshPartialEndpointSolutionFilter<'a> = &'a dyn Fn(&[Option<[usize; 2]>]) -> bool;
#[derive(Clone, Copy)]
pub(crate) struct MeshPartialEndpointConstraint<'a> {
    pub(crate) active_edges: &'a [bool],
    pub(crate) valid: MeshPartialEndpointSolutionFilter<'a>,
}
type MeshFaceEndpointConfiguration = Vec<MeshEndpointPair>;
pub(crate) type MeshFaceEndpointConfigurations = Vec<MeshFaceEndpointConfiguration>;
type MeshQuotientSignature = Vec<(Vec<usize>, Vec<usize>)>;
type MeshOrientationSignature = (MeshQuotientSignature, Vec<Vec<bool>>);
type MeshFaceEquationCache = RefCell<HashMap<(usize, MeshQuotientSignature), Vec<[usize; 2]>>>;

fn changed_quotient_edges(left: &MeshQuotient, right: &MeshQuotient) -> HashSet<usize> {
    let mut left = left.clone();
    let mut right = right.clone();
    (0..left.union.len())
        .filter_map(|node| {
            let left_root = left.union.find(node);
            let right_root = right.union.find(node);
            (left_root != right_root
                || left.members[left_root] != right.members[right_root]
                || left.domains[left_root] != right.domains[right_root])
                .then_some(node / 2)
        })
        .collect()
}

pub(crate) struct MeshSelectionSearch<'a> {
    pub(crate) assignments: &'a [Vec<MeshFaceBoundaryAssignment>],
    #[cfg(test)]
    pub(crate) possible_face_equations: Vec<Vec<[usize; 2]>>,
    pub(crate) possible_face_choices: Vec<Vec<Vec<[usize; 2]>>>,
    pub(crate) face_work: Vec<Option<usize>>,
    pub(crate) edge_candidates: &'a [Vec<[usize; 2]>],
    pub(crate) edge_rows: &'a [EdgeRow],
    pub(crate) vertex_points: &'a [[f64; 3]],
    pub(crate) selected: Vec<MeshFaceSelection>,
    pub(crate) states: usize,
    pub(crate) solution: Option<(StandardTopology, Vec<usize>)>,
    pub(crate) stop_after_first_solution: bool,
    pub(crate) ambiguous: bool,
    pub(crate) exhausted: bool,
    pub(crate) face_equation_cache: MeshFaceEquationCache,
}

pub(crate) fn possible_face_equations(
    faces: &[Vec<MeshFaceBoundaryAssignment>],
) -> Vec<Vec<[usize; 2]>> {
    fn ports(use_: MeshBoundaryEdgeCandidate, end: bool) -> [Option<usize>; 2] {
        let port = |reversed: bool| {
            use_.edge.checked_mul(2)?.checked_add(usize::from(if end {
                !reversed
            } else {
                reversed
            }))
        };
        match use_.reversed {
            Some(reversed) => [port(reversed), None],
            None => [port(false), port(true)],
        }
    }

    faces
        .iter()
        .map(|assignments| {
            let mut equations = HashSet::new();
            for assignment in assignments {
                for boundary in &assignment.boundaries {
                    if boundary.is_empty() {
                        continue;
                    }
                    for index in 0..boundary.len() {
                        let left = ports(boundary[index], true);
                        let right = ports(boundary[(index + 1) % boundary.len()], false);
                        for left in left.into_iter().flatten() {
                            for right in right.into_iter().flatten() {
                                equations.insert(if left <= right {
                                    [left, right]
                                } else {
                                    [right, left]
                                });
                            }
                        }
                    }
                }
            }
            let mut equations = equations.into_iter().collect::<Vec<_>>();
            equations.sort_unstable();
            equations
        })
        .collect()
}

pub(crate) fn possible_face_choices_with_limit(
    faces: &[Vec<MeshFaceBoundaryAssignment>],
    face_equations: &[Vec<[usize; 2]>],
    limit: usize,
) -> Option<Vec<Vec<Vec<[usize; 2]>>>> {
    fn port(use_: MeshBoundaryEdgeCandidate, reversed: bool, end: bool) -> Option<usize> {
        use_.edge
            .checked_mul(2)?
            .checked_add(usize::from(if end { !reversed } else { reversed }))
    }

    let budget = MeshConstraintBudget::new(limit);
    let choices = faces
        .iter()
        .zip(face_equations)
        .map(|(assignments, fallback)| {
            let mut choices = HashSet::new();
            for assignment in assignments {
                if !budget.charge() {
                    return Vec::new();
                }
                let unknown = assignment
                    .boundaries
                    .iter()
                    .flatten()
                    .filter(|use_| use_.reversed.is_none())
                    .count();
                let Some(combinations) = 1usize.checked_shl(unknown as u32) else {
                    return vec![fallback.clone()];
                };
                if combinations > 4_096 {
                    return vec![fallback.clone()];
                }
                for mask in 0..combinations {
                    if !budget.charge() {
                        return Vec::new();
                    }
                    let mut variable = 0usize;
                    let directions = assignment
                        .boundaries
                        .iter()
                        .map(|boundary| {
                            boundary
                                .iter()
                                .map(|use_| {
                                    use_.reversed.unwrap_or_else(|| {
                                        let shift = unknown - variable - 1;
                                        variable += 1;
                                        mask & (1usize << shift) != 0
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>();
                    let Some(mut equations) = assignment
                        .boundaries
                        .iter()
                        .zip(&directions)
                        .map(|(boundary, directions)| {
                            (0..boundary.len())
                                .map(|index| {
                                    let next = (index + 1) % boundary.len();
                                    let left = port(boundary[index], directions[index], true)?;
                                    let right = port(boundary[next], directions[next], false)?;
                                    Some(if left <= right {
                                        [left, right]
                                    } else {
                                        [right, left]
                                    })
                                })
                                .collect::<Option<Vec<_>>>()
                        })
                        .collect::<Option<Vec<_>>>()
                        .map(|boundaries| boundaries.into_iter().flatten().collect::<Vec<_>>())
                    else {
                        continue;
                    };
                    equations.sort_unstable();
                    equations.dedup();
                    choices.insert(equations);
                }
            }
            let mut choices = choices.into_iter().collect::<Vec<_>>();
            choices.sort_unstable();
            choices
        })
        .collect();
    (!budget.exhausted.get()).then_some(choices)
}

#[cfg(test)]
pub(crate) fn possible_face_choices(
    faces: &[Vec<MeshFaceBoundaryAssignment>],
    face_equations: &[Vec<[usize; 2]>],
) -> Vec<Vec<Vec<[usize; 2]>>> {
    possible_face_choices_with_limit(faces, face_equations, usize::MAX)
        .expect("unbounded test face-choice materialization")
}

pub(crate) fn deduplicate_mesh_quotient_assignments(faces: &mut [Vec<MeshFaceBoundaryAssignment>]) {
    fn canonical_cycle(boundary: &[MeshBoundaryEdgeCandidate]) -> Vec<(usize, Option<bool>)> {
        fn rotations(values: &[(usize, Option<bool>)]) -> Vec<Vec<(usize, Option<bool>)>> {
            (0..values.len())
                .map(|start| {
                    values[start..]
                        .iter()
                        .chain(&values[..start])
                        .copied()
                        .collect()
                })
                .collect()
        }

        let forward = boundary
            .iter()
            .map(|use_| (use_.edge, use_.reversed))
            .collect::<Vec<_>>();
        let reversed = boundary
            .iter()
            .rev()
            .map(|use_| (use_.edge, use_.reversed.map(|value| !value)))
            .collect::<Vec<_>>();
        rotations(&forward)
            .into_iter()
            .chain(rotations(&reversed))
            .min()
            .unwrap_or_default()
    }

    for assignments in faces {
        let mut seen = HashSet::new();
        assignments.retain(|assignment| {
            let mut signature = assignment
                .boundaries
                .iter()
                .map(|boundary| canonical_cycle(boundary))
                .collect::<Vec<_>>();
            signature.sort_unstable();
            seen.insert(signature)
        });
    }
}

pub(crate) fn mesh_assignment_endpoint_cycles_viable_where(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&MeshConstraintBudget>,
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Option<bool> {
    const MAX_LOCAL_ENDPOINT_STATES: usize = 65_536;

    for boundary in &assignment.boundaries {
        if boundary.is_empty() {
            return Some(false);
        }
        if boundary
            .iter()
            .any(|use_| edge_candidates.get(use_.edge).is_none_or(Vec::is_empty))
        {
            return None;
        }
        let candidates = |edge: usize| {
            edge_candidates[edge]
                .iter()
                .copied()
                .filter(move |pair| allowed(edge, *pair))
        };
        let mut states = HashSet::new();
        for [left, right] in candidates(boundary[0].edge) {
            if budget.is_some_and(|budget| !budget.charge()) {
                return None;
            }
            states.insert((left, right));
            states.insert((right, left));
            if states.len() > MAX_LOCAL_ENDPOINT_STATES {
                return None;
            }
        }
        for use_ in &boundary[1..] {
            let mut next = HashSet::new();
            for &(start, current) in &states {
                for [left, right] in candidates(use_.edge) {
                    if budget.is_some_and(|budget| !budget.charge()) {
                        return None;
                    }
                    if left == current {
                        next.insert((start, right));
                    }
                    if right == current {
                        next.insert((start, left));
                    }
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

fn mesh_assignment_endpoint_cycles_viable_with(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
    required: Option<(usize, [usize; 2])>,
    budget: Option<&MeshConstraintBudget>,
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
pub(crate) fn mesh_assignment_endpoint_cycles_viable(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
) -> bool {
    mesh_assignment_endpoint_cycles_viable_with(assignment, edge_candidates, None, None)
        .unwrap_or(true)
}

pub(crate) struct MeshConstraintBudget {
    remaining: Cell<usize>,
    pub(crate) exhausted: Cell<bool>,
}

impl MeshConstraintBudget {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            remaining: Cell::new(limit),
            exhausted: Cell::new(false),
        }
    }

    pub(crate) fn charge(&self) -> bool {
        self.charge_by(1)
    }

    pub(crate) fn charge_by(&self, work: usize) -> bool {
        let remaining = self.remaining.get();
        if work > remaining {
            self.exhausted.set(true);
            false
        } else {
            self.remaining.set(remaining - work);
            true
        }
    }
}

pub(crate) fn mesh_face_endpoint_configurations(
    assignments: &[MeshFaceBoundaryAssignment],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[Option<[usize; 2]>],
    budget: &MeshConstraintBudget,
) -> Option<MeshFaceEndpointConfigurations> {
    const MAX_WORK: usize = 4_096;

    fn insert_pair(
        configuration: &mut MeshFaceEndpointConfiguration,
        edge: usize,
        mut pair: [usize; 2],
    ) -> bool {
        pair.sort_unstable();
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
        budget: &MeshConstraintBudget,
    ) -> Option<MeshFaceEndpointConfigurations> {
        let charge = |work: &mut usize| {
            *work = work.checked_add(1)?;
            (*work <= MAX_WORK && budget.charge()).then_some(())
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
            for (start, current) in [(left, right), (right, left)] {
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
                    for endpoint in [
                        (left == current).then_some(right),
                        (right == current).then_some(left),
                    ]
                    .into_iter()
                    .flatten()
                    {
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
        Some(
            states
                .into_iter()
                .filter(|(start, current, _)| start == current)
                .filter_map(|(_, _, mut configuration)| {
                    configuration.sort_unstable();
                    seen.insert(configuration.clone()).then_some(configuration)
                })
                .collect(),
        )
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
                    work = work.checked_add(1)?;
                    if work > MAX_WORK || !budget.charge() {
                        return None;
                    }
                    let mut merged = stored.clone();
                    if candidate
                        .iter()
                        .all(|(edge, pair)| insert_pair(&mut merged, *edge, *pair))
                    {
                        merged.sort_unstable();
                        next.push(merged);
                    }
                }
            }
            combined = next;
        }
        configurations.extend(combined);
    }
    let mut configurations = configurations.into_iter().collect::<Vec<_>>();
    configurations.sort_unstable();
    Some(configurations)
}

pub(crate) fn prune_mesh_endpoint_pair_support(
    assignments: &mut [Vec<MeshFaceBoundaryAssignment>],
    edge_candidates: &mut [Vec<[usize; 2]>],
) -> bool {
    prune_mesh_endpoint_pair_support_with_limit(
        assignments,
        edge_candidates,
        MAX_MESH_CONSTRAINT_OPERATIONS,
    )
}

pub(crate) fn prune_mesh_endpoint_pair_support_with_limit(
    assignments: &mut [Vec<MeshFaceBoundaryAssignment>],
    edge_candidates: &mut [Vec<[usize; 2]>],
    limit: usize,
) -> bool {
    let budget = MeshConstraintBudget::new(limit);
    'fixpoint: loop {
        let mut changed = false;
        for face in assignments.iter_mut() {
            let before = face.len();
            face.retain(|assignment| {
                mesh_assignment_endpoint_cycles_viable_with(
                    assignment,
                    edge_candidates,
                    None,
                    Some(&budget),
                )
                .unwrap_or(true)
            });
            if budget.exhausted.get() {
                // Pair-support pruning is optional. Every removal made before
                // exhaustion was proved locally; the independently bounded
                // quotient search can continue from that sound partial result.
                return true;
            }
            if face.is_empty() {
                return false;
            }
            changed |= face.len() != before;
        }
        for edge in 0..edge_candidates.len() {
            if edge_candidates[edge].is_empty() {
                continue;
            }
            let incident_faces = assignments
                .iter()
                .enumerate()
                .filter_map(|(face, choices)| {
                    choices
                        .iter()
                        .any(|assignment| {
                            assignment
                                .boundaries
                                .iter()
                                .flatten()
                                .any(|use_| use_.edge == edge)
                        })
                        .then_some(face)
                })
                .collect::<Vec<_>>();
            let before = edge_candidates[edge].len();
            let snapshot = edge_candidates.to_vec();
            edge_candidates[edge].retain(|pair| {
                incident_faces.iter().all(|face| {
                    assignments[*face].iter().any(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .any(|use_| use_.edge == edge)
                            && mesh_assignment_endpoint_cycles_viable_with(
                                assignment,
                                &snapshot,
                                Some((edge, *pair)),
                                Some(&budget),
                            )
                            .unwrap_or(true)
                    })
                })
            });
            if budget.exhausted.get() {
                // Do not turn incomplete propagation into a contradiction.
                return true;
            }
            if edge_candidates[edge].is_empty() {
                return false;
            }
            if edge_candidates[edge].len() != before {
                continue 'fixpoint;
            }
        }
        if !changed {
            return true;
        }
    }
}

pub(crate) fn uses_canonical_edge_direction_gauge(
    boundaries: &[Vec<MeshBoundaryEdgeCandidate>],
    directions: &[Vec<bool>],
    oriented_edges: &HashSet<usize>,
) -> bool {
    let mut oriented = oriented_edges.clone();
    boundaries
        .iter()
        .zip(directions)
        .all(|(boundary, directions)| {
            boundary.iter().zip(directions).all(|(use_, direction)| {
                let first = oriented.insert(use_.edge);
                !first || use_.reversed.is_some() || !direction
            })
        })
}

impl MeshSelectionSearch<'_> {
    pub(crate) fn should_stop(&self) -> bool {
        self.ambiguous
            || self.exhausted
            || (self.stop_after_first_solution && self.solution.is_some())
    }

    #[cfg(test)]
    pub(crate) fn remaining_equation_merge_capacity(
        &self,
        quotient: &mut MeshQuotient,
    ) -> Option<usize> {
        fn choice_component_reductions(
            choice: &[[usize; 2]],
            quotient: &mut MeshQuotient,
            possible: &mut UnionFind,
        ) -> HashMap<usize, usize> {
            let mut equations = HashMap::<usize, Vec<[usize; 2]>>::new();
            for [left, right] in choice {
                let left = quotient.union.find(*left);
                let right = quotient.union.find(*right);
                let component = possible.find(left);
                if component == possible.find(right) {
                    equations.entry(component).or_default().push([left, right]);
                }
            }
            equations
                .into_iter()
                .map(|(component, equations)| {
                    let mut roots = HashMap::new();
                    for [left, right] in &equations {
                        for root in [left, right] {
                            let next = roots.len();
                            roots.entry(*root).or_insert(next);
                        }
                    }
                    let mut local = UnionFind::new(roots.len());
                    for [left, right] in equations {
                        local.union(roots[&left], roots[&right]);
                    }
                    let remaining = (0..local.len())
                        .filter(|&node| local.find(node) == node)
                        .count();
                    (component, roots.len().saturating_sub(remaining))
                })
                .collect()
        }

        let node_count = quotient.union.len();
        let mut possible = UnionFind::new(node_count);
        for node in 0..node_count {
            let root = quotient.union.find(node);
            possible.union(node, root);
        }
        let before = (0..node_count)
            .filter(|&node| possible.find(node) == node)
            .count();
        for (face, selected) in self.selected.iter().enumerate() {
            if selected.is_some() {
                continue;
            }
            for [left, right] in &self.possible_face_equations[face] {
                possible.union(*left, *right);
            }
        }
        let after = (0..node_count)
            .filter(|&node| possible.find(node) == node)
            .count();
        let point_count = if self.vertex_points.is_empty() {
            quotient
                .domains
                .iter()
                .flat_map(|domain| domain.iter())
                .max()
                .map_or(0, |point| point + 1)
        } else {
            self.vertex_points.len()
        };
        let mut possible_domains = HashMap::<usize, HashSet<usize>>::new();
        let mut universal_components = HashSet::new();
        let mut possible_root_counts = HashMap::<usize, usize>::new();
        for node in 0..node_count {
            if quotient.union.find(node) != node {
                continue;
            }
            let component = possible.find(node);
            if quotient.domains[node].len() == point_count {
                universal_components.insert(component);
                possible_domains.remove(&component);
            } else if !universal_components.contains(&component) {
                possible_domains
                    .entry(component)
                    .or_default()
                    .extend(quotient.domains[node].iter());
            }
            *possible_root_counts.entry(component).or_default() += 1;
        }
        let mut component_merge_capacity = HashMap::<usize, usize>::new();
        let mut independent_capacity = 0usize;
        for (face, selected) in self.selected.iter().enumerate() {
            if selected.is_some() {
                continue;
            }
            let mut face_capacity = HashMap::<usize, usize>::new();
            let mut independent_face_capacity = 0usize;
            for choice in &self.possible_face_choices[face] {
                let reductions = choice_component_reductions(choice, quotient, &mut possible);
                independent_face_capacity = independent_face_capacity.max(
                    reductions
                        .values()
                        .copied()
                        .fold(0usize, usize::saturating_add),
                );
                for (component, reduction) in reductions {
                    face_capacity
                        .entry(component)
                        .and_modify(|capacity| *capacity = (*capacity).max(reduction))
                        .or_insert(reduction);
                }
            }
            independent_capacity = independent_capacity.saturating_add(independent_face_capacity);
            for (component, capacity) in face_capacity {
                *component_merge_capacity.entry(component).or_default() += capacity;
            }
        }
        let required_root_count = possible_root_counts
            .iter()
            .map(|(component, roots)| {
                roots
                    .saturating_sub(
                        component_merge_capacity
                            .get(component)
                            .copied()
                            .unwrap_or(0),
                    )
                    .max(1)
            })
            .sum::<usize>();
        if required_root_count > point_count {
            return None;
        }
        let required_count = |component: &usize| {
            possible_root_counts[component]
                .saturating_sub(
                    component_merge_capacity
                        .get(component)
                        .copied()
                        .unwrap_or(0),
                )
                .max(1)
        };
        let universal_required = universal_components
            .iter()
            .map(required_count)
            .fold(0usize, usize::saturating_add);
        let mut domains = possible_domains
            .into_iter()
            .flat_map(|(component, domain)| {
                let required = required_count(&component);
                std::iter::repeat_n(domain, required)
            })
            .collect::<Vec<_>>();
        if universal_required > point_count.saturating_sub(domains.len()) {
            return None;
        }
        domains.sort_unstable_by_key(HashSet::len);
        let domains = domains
            .into_iter()
            .map(|domain| domain.into_iter().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        if !domains_have_distinct_matching(domains.iter().map(Vec::as_slice), point_count) {
            return None;
        }
        let mut singleton_component = HashMap::new();
        for node in 0..node_count {
            if quotient.union.find(node) != node || quotient.domains[node].len() != 1 {
                continue;
            }
            let point = *quotient.domains[node].iter().next()?;
            let component = possible.find(node);
            if singleton_component
                .insert(point, component)
                .is_some_and(|previous| previous != component)
            {
                return None;
            }
        }
        Some(before.saturating_sub(after).min(independent_capacity))
    }

    fn face_projection_signature(
        &self,
        face: usize,
        quotient: &mut MeshQuotient,
    ) -> MeshQuotientSignature {
        let mut roots = self.assignments[face]
            .iter()
            .flat_map(|assignment| &assignment.boundaries)
            .flatten()
            .flat_map(|use_| [use_.edge * 2, use_.edge * 2 + 1])
            .map(|node| quotient.union.find(node))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        roots.sort_unstable();
        let mut signature = roots
            .into_iter()
            .map(|root| {
                let mut domain = quotient.domains[root].iter().copied().collect::<Vec<_>>();
                domain.sort_unstable();
                (quotient.members[root].clone(), domain)
            })
            .collect::<Vec<_>>();
        signature.sort_unstable();
        signature
    }

    #[cfg(test)]
    pub(crate) fn propagate_forced_face_equations(&self, quotient: &mut MeshQuotient) -> bool {
        let budget = MeshConstraintBudget::new(usize::MAX);
        self.propagate_forced_face_equations_from(quotient, None, &budget)
    }

    fn propagate_forced_face_equations_from(
        &self,
        quotient: &mut MeshQuotient,
        changed_edges: Option<&HashSet<usize>>,
        budget: &MeshConstraintBudget,
    ) -> bool {
        let mut queue = self
            .selected
            .iter()
            .enumerate()
            .filter_map(|(face, selected)| {
                (selected.is_none()
                    && changed_edges.is_none_or(|changed_edges| {
                        self.assignments[face]
                            .iter()
                            .flat_map(|assignment| &assignment.boundaries)
                            .flatten()
                            .any(|use_| changed_edges.contains(&use_.edge))
                    }))
                .then_some(face)
            })
            .collect::<VecDeque<_>>();
        let mut queued = queue.iter().copied().collect::<HashSet<_>>();
        while let Some(face) = queue.pop_front() {
            if !budget.charge() {
                return true;
            }
            queued.remove(&face);
            if self.selected[face].is_some() {
                continue;
            }
            let before = quotient.clone();
            let mut changed = false;
            let deterministic = self.assignments[face].len() == 1
                && self.assignments[face][0]
                    .boundaries
                    .iter()
                    .flatten()
                    .all(|use_| use_.reversed.is_some());
            let equations = if deterministic {
                let [choice] = self.possible_face_choices[face].as_slice() else {
                    return false;
                };
                choice.clone()
            } else {
                let cache_key = (face, self.face_projection_signature(face, quotient));
                let cached = self.face_equation_cache.borrow().get(&cache_key).cloned();
                if let Some(cached) = cached {
                    cached
                } else {
                    let Some(common) = common_supported_corner_equations(
                        quotient,
                        &self.assignments[face],
                        budget,
                    ) else {
                        return budget.exhausted.get();
                    };
                    let equations = common.into_iter().collect::<Vec<_>>();
                    let mut cache = self.face_equation_cache.borrow_mut();
                    if cache.len() >= MAX_FACE_EQUATION_CACHE_ENTRIES {
                        cache.clear();
                    }
                    cache.insert(cache_key, equations.clone());
                    equations
                }
            };
            for [left, right] in equations {
                if quotient.union.find(left) == quotient.union.find(right) {
                    continue;
                }
                let Some(root) = quotient.merge(left, right) else {
                    return false;
                };
                if !quotient.propagate_component_edge_domains(root, self.edge_candidates, None) {
                    return false;
                }
                changed = true;
            }
            if !changed {
                continue;
            }
            let changed_edges = changed_quotient_edges(&before, quotient);
            for (dependent, assignments) in self.assignments.iter().enumerate() {
                if self.selected[dependent].is_none()
                    && dependent != face
                    && !queued.contains(&dependent)
                    && assignments
                        .iter()
                        .flat_map(|assignment| &assignment.boundaries)
                        .flatten()
                        .any(|use_| changed_edges.contains(&use_.edge))
                {
                    queued.insert(dependent);
                    queue.push_back(dependent);
                }
            }
        }
        true
    }

    fn selection_orientable(&self, selection: &[MeshFaceSelection]) -> bool {
        let mut constraints = Vec::<Vec<(usize, bool)>>::new();
        let mut edge_uses = HashMap::<usize, Vec<(usize, bool)>>::new();
        for (face, selected) in selection.iter().enumerate() {
            let Some((assignment_index, directions)) = selected else {
                continue;
            };
            let Some(assignment) = self.assignments[face].get(*assignment_index) else {
                return false;
            };
            if assignment.boundaries.len() != directions.len() {
                return false;
            }
            for (boundary, directions) in assignment.boundaries.iter().zip(directions) {
                if boundary.len() != directions.len() {
                    return false;
                }
                let node = constraints.len();
                constraints.push(Vec::new());
                for (use_, &direction) in boundary.iter().zip(directions) {
                    let reversed = use_.reversed.unwrap_or(direction);
                    if use_.reversed.is_some() && reversed != direction {
                        return false;
                    }
                    let uses = edge_uses.entry(use_.edge).or_default();
                    if uses.len() == 2 {
                        return false;
                    }
                    uses.push((node, reversed));
                }
            }
        }
        for uses in edge_uses.values() {
            let [(left_node, left_reversed), (right_node, right_reversed)] = uses.as_slice() else {
                continue;
            };
            let parity = left_reversed == right_reversed;
            if left_node == right_node {
                if parity {
                    return false;
                }
            } else {
                constraints[*left_node].push((*right_node, parity));
                constraints[*right_node].push((*left_node, parity));
            }
        }
        let mut flips = vec![None; constraints.len()];
        for root in 0..constraints.len() {
            if flips[root].is_some() {
                continue;
            }
            flips[root] = Some(false);
            let mut stack = vec![root];
            while let Some(node) = stack.pop() {
                let Some(flip) = flips[node] else {
                    return false;
                };
                for &(neighbor, parity) in &constraints[node] {
                    let required = flip ^ parity;
                    match flips[neighbor] {
                        Some(existing) if existing != required => return false,
                        Some(_) => {}
                        None => {
                            flips[neighbor] = Some(required);
                            stack.push(neighbor);
                        }
                    }
                }
            }
        }
        true
    }

    pub(crate) fn selected_orientable(&self) -> bool {
        self.selection_orientable(&self.selected)
    }

    pub(crate) fn fixed_remaining_faces_are_orientable(&self) -> bool {
        let mut completion = self.selected.clone();
        for (face, selected) in completion.iter_mut().enumerate() {
            if selected.is_some() {
                continue;
            }
            let [assignment] = self.assignments[face].as_slice() else {
                continue;
            };
            let Some(directions) = assignment
                .boundaries
                .iter()
                .map(|boundary| {
                    boundary
                        .iter()
                        .map(|use_| use_.reversed)
                        .collect::<Option<Vec<_>>>()
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            *selected = Some((0, directions));
        }
        self.selection_orientable(&completion)
    }

    pub(crate) fn prepare_selected_branch(
        &self,
        quotient: &MeshQuotient,
        changed_edges: &HashSet<usize>,
        propagation_budget: &MeshConstraintBudget,
    ) -> Option<MeshQuotient> {
        let mut measured = quotient.clone();
        if !self.propagate_forced_face_equations_from(
            &mut measured,
            Some(changed_edges),
            propagation_budget,
        ) {
            return None;
        }
        if !measured.merge_singleton_coordinate_roots(self.edge_candidates) {
            return None;
        }
        let root_count = measured.root_count();
        if root_count < self.vertex_points.len() {
            return None;
        }
        self.fixed_remaining_faces_are_orientable()
            .then_some(measured)
    }

    pub(crate) fn search(&mut self, quotient: &MeshQuotient) {
        self.search_with_limit(quotient, MAX_MESH_CONSTRAINT_OPERATIONS);
    }

    pub(crate) fn search_with_limit(&mut self, quotient: &MeshQuotient, limit: usize) {
        let budget = MeshConstraintBudget::new(limit);
        let propagation_budget = MeshConstraintBudget::new(limit);
        self.search_from_state(quotient, false, &budget, &propagation_budget);
    }

    pub(crate) fn search_from_state(
        &mut self,
        quotient: &MeshQuotient,
        prepared: bool,
        budget: &MeshConstraintBudget,
        propagation_budget: &MeshConstraintBudget,
    ) {
        const MAX_SELECTION_STATES: usize = 512;

        if self.should_stop() {
            return;
        }
        if !budget.charge() {
            self.exhausted = true;
            return;
        }
        let mut measured = quotient.clone();
        if !prepared {
            if !self.propagate_forced_face_equations_from(&mut measured, None, propagation_budget) {
                return;
            }
            if !measured.merge_singleton_coordinate_roots(self.edge_candidates) {
                return;
            }
            let root_count = measured.root_count();
            if root_count < self.vertex_points.len() {
                return;
            }
            if !self.fixed_remaining_faces_are_orientable() {
                return;
            }
        }
        let selected_edges = self
            .selected
            .iter()
            .enumerate()
            .filter_map(|(face, selected)| {
                selected
                    .as_ref()
                    .and_then(|(index, _)| self.assignments[face].get(*index))
            })
            .flat_map(|assignment| &assignment.boundaries)
            .flatten()
            .map(|use_| use_.edge)
            .collect::<HashSet<_>>();
        let adjacent_faces = (!selected_edges.is_empty())
            .then(|| {
                self.selected
                    .iter()
                    .enumerate()
                    .filter_map(|(face, selected)| {
                        (selected.is_none()
                            && self.assignments[face]
                                .iter()
                                .flat_map(|assignment| &assignment.boundaries)
                                .flatten()
                                .any(|use_| selected_edges.contains(&use_.edge)))
                        .then_some(face)
                    })
                    .collect::<HashSet<_>>()
            })
            .filter(|faces| !faces.is_empty());
        let next = self
            .selected
            .iter()
            .enumerate()
            .filter(|(_, selected)| selected.is_none())
            .filter(|(face, _)| {
                adjacent_faces
                    .as_ref()
                    .is_none_or(|adjacent| adjacent.contains(face))
            })
            .filter_map(|(face, _)| {
                if !budget.charge() {
                    return None;
                }
                self.face_work[face]?;
                let assignments = &self.assignments[face];
                if assignments.is_empty() {
                    return Some((0, 0, 0, 0, 0, face));
                }
                let direction_work = assignments
                    .iter()
                    .map(|assignment| {
                        let unknown = assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .filter(|use_| use_.reversed.is_none())
                            .count();
                        1usize.checked_shl(unknown as u32).unwrap_or(usize::MAX)
                    })
                    .fold(0usize, usize::saturating_add);
                let can_merge = assignments
                    .iter()
                    .any(|assignment| mesh_assignment_can_merge(assignment, &mut measured));
                let assignment = &assignments[0];
                let selected_incidence = assignment
                    .boundaries
                    .iter()
                    .flatten()
                    .filter(|use_| selected_edges.contains(&use_.edge))
                    .count();
                let constrained = assignment
                    .boundaries
                    .iter()
                    .flatten()
                    .filter(|use_| {
                        let left = measured.union.find(use_.edge * 2);
                        let right = measured.union.find(use_.edge * 2 + 1);
                        measured.domains[left].len() < self.vertex_points.len()
                            || measured.domains[right].len() < self.vertex_points.len()
                    })
                    .count();
                Some((
                    if can_merge { 1 } else { 2 },
                    assignments.len(),
                    direction_work,
                    usize::MAX - selected_incidence,
                    usize::MAX - constrained,
                    face,
                ))
            })
            .min();
        if budget.exhausted.get() {
            self.exhausted = true;
            return;
        }
        let Some((_, supported, _, _, _, face)) = next else {
            let mut quotient = measured.clone();
            let Some(root_points) = quotient.close_coordinate_roots(
                self.vertex_points.len(),
                self.edge_candidates,
                Some(budget),
            ) else {
                if budget.exhausted.get() {
                    self.exhausted = true;
                }
                return;
            };
            let selected = self.selected.iter().cloned().collect::<Option<Vec<_>>>();
            let Some(selected) = selected else {
                return;
            };
            let assignment_indices = selected.iter().map(|(index, _)| *index).collect::<Vec<_>>();
            let directions = selected
                .into_iter()
                .map(|(_, directions)| directions)
                .collect::<Vec<_>>();
            let selected_assignments = self
                .assignments
                .iter()
                .zip(&assignment_indices)
                .map(|(assignments, &index)| assignments.get(index).cloned())
                .collect::<Option<Vec<_>>>();
            let Some(selected_assignments) = selected_assignments else {
                return;
            };
            let candidate = reconstruct_mesh_selection(
                self.edge_rows.to_vec(),
                self.vertex_points.to_vec(),
                &selected_assignments,
                &directions,
            )
            .and_then(|mut topology| {
                let mut use_counts = vec![0usize; topology.edge_rows.len()];
                for coedge in topology
                    .faces
                    .iter()
                    .flat_map(|face| &face.boundaries)
                    .flat_map(|boundary| &boundary.coedges)
                {
                    use_counts[coedge.edge_row] += 1;
                }
                if use_counts.iter().any(|count| *count > 2) {
                    return None;
                }
                if use_counts.iter().all(|count| *count == 2) {
                    orient_face_cycles(&mut topology.faces)?;
                }
                let edge_vertices = topology.edge_vertices()?;
                let mut point_assignment = vec![None; topology.logical_vertex_count];
                for (edge, vertices) in edge_vertices.into_iter().enumerate() {
                    for (port, vertex) in vertices.into_iter().enumerate() {
                        let root = quotient.union.find(edge * 2 + port);
                        let point = *root_points.get(&root)?;
                        match point_assignment[vertex] {
                            Some(stored) if stored != point => return None,
                            Some(_) => {}
                            None => point_assignment[vertex] = Some(point),
                        }
                    }
                    let points = <[usize; 2]>::try_from(
                        vertices
                            .map(|vertex| point_assignment[vertex])
                            .into_iter()
                            .collect::<Option<Vec<_>>>()?,
                    )
                    .ok()?;
                    let closed_ports =
                        quotient.union.find(edge * 2) == quotient.union.find(edge * 2 + 1);
                    if !mesh_edge_points_compatible(
                        closed_ports,
                        &self.edge_candidates[edge],
                        points,
                    ) {
                        return None;
                    }
                }
                Some((
                    topology,
                    point_assignment.into_iter().collect::<Option<Vec<_>>>()?,
                ))
            });
            if let Some(candidate) = candidate {
                match &self.solution {
                    Some(solution)
                        if *solution != candidate
                            && !mesh_candidates_equivalent(solution, &candidate) =>
                    {
                        self.ambiguous = true;
                    }
                    None => self.solution = Some(candidate),
                    Some(_) => {}
                }
            }
            return;
        };
        if supported == 0 {
            return;
        }
        let mut options = Vec::new();
        for assignment_index in 0..self.assignments[face].len() {
            if !budget.charge() {
                self.exhausted = true;
                return;
            }
            let remaining = MAX_SELECTION_STATES
                .saturating_sub(self.states)
                .saturating_add(1)
                .saturating_sub(options.len());
            if remaining == 0 {
                break;
            }
            let assignment = &self.assignments[face][assignment_index];
            let assignment_options = measured.assignment_options_limited(
                assignment,
                self.edge_candidates,
                &selected_edges,
                remaining,
                Some(budget),
            );
            if budget.exhausted.get() {
                self.exhausted = true;
                return;
            }
            options.extend(
                assignment_options
                    .into_iter()
                    .map(|(directions, next_quotient)| {
                        (assignment_index, directions, next_quotient)
                    }),
            );
        }
        options.retain_mut(|(_, _, quotient)| quotient.root_count() >= self.vertex_points.len());
        if options.is_empty() {
            return;
        }
        options.sort_unstable_by_key(|(assignment, directions, quotient)| {
            let mut measured = quotient.clone();
            let root_count = measured.root_count();
            let domain_freedom = (0..measured.union.len())
                .filter(|&node| measured.union.find(node) == node)
                .map(|node| measured.domains[node].len())
                .fold(0usize, usize::saturating_add);
            (root_count, domain_freedom, *assignment, directions.clone())
        });
        let branching = options.len() > 1;
        for (assignment_index, directions, next_quotient) in options {
            let changed_edges = changed_quotient_edges(&measured, &next_quotient);
            self.selected[face] = Some((assignment_index, directions));
            if self.selected_orientable() {
                if let Some(next_quotient) =
                    self.prepare_selected_branch(&next_quotient, &changed_edges, propagation_budget)
                {
                    if branching {
                        if self.states >= MAX_SELECTION_STATES {
                            self.exhausted = true;
                            self.selected[face] = None;
                            return;
                        }
                        self.states += 1;
                    }
                    // `prepare_selected_branch` has already applied the recursive
                    // entry preflight to this quotient.
                    self.search_from_state(&next_quotient, true, budget, propagation_budget);
                } else if budget.exhausted.get() {
                    self.exhausted = true;
                }
            }
            self.selected[face] = None;
            if self.should_stop() {
                return;
            }
        }
    }
}

pub(crate) fn canonicalize_mesh_vertex_labels(
    mut topology: StandardTopology,
    point_assignment: &[usize],
) -> Option<(StandardTopology, Vec<usize>)> {
    if point_assignment.len() != topology.logical_vertex_count
        || point_assignment.len() != topology.vertex_points.len()
    {
        return None;
    }
    let mut seen = vec![false; point_assignment.len()];
    for &point in point_assignment {
        let entry = seen.get_mut(point)?;
        if std::mem::replace(entry, true) {
            return None;
        }
    }
    for coedge in topology
        .faces
        .iter_mut()
        .flat_map(|face| &mut face.boundaries)
        .flat_map(|boundary| &mut boundary.coedges)
    {
        coedge.start_vertex = *point_assignment.get(coedge.start_vertex)?;
        coedge.end_vertex = *point_assignment.get(coedge.end_vertex)?;
    }
    let mut edge_vertices = vec![None; topology.edge_rows.len()];
    for coedge in topology
        .faces
        .iter()
        .flat_map(|face| &face.boundaries)
        .flat_map(|boundary| &boundary.coedges)
    {
        let vertices = if coedge.reversed {
            [coedge.end_vertex, coedge.start_vertex]
        } else {
            [coedge.start_vertex, coedge.end_vertex]
        };
        let stored = edge_vertices.get_mut(coedge.edge_row)?;
        match stored {
            Some(existing) if *existing != vertices => return None,
            Some(_) => {}
            None => *stored = Some(vertices),
        }
    }
    let reverse_edges = edge_vertices
        .into_iter()
        .map(|vertices| vertices.is_some_and(|vertices| vertices[0] > vertices[1]))
        .collect::<Vec<_>>();
    for coedge in topology
        .faces
        .iter_mut()
        .flat_map(|face| &mut face.boundaries)
        .flat_map(|boundary| &mut boundary.coedges)
    {
        if *reverse_edges.get(coedge.edge_row)? {
            coedge.reversed = !coedge.reversed;
        }
    }
    for boundary in topology
        .faces
        .iter_mut()
        .flat_map(|face| &mut face.boundaries)
    {
        let len = boundary.coedges.len();
        if len == 0 {
            return None;
        }
        let best = (0..len).min_by_key(|&start| {
            (0..len)
                .map(|offset| {
                    let coedge = boundary.coedges[(start + offset) % len];
                    (
                        coedge.edge_row,
                        coedge.reversed,
                        coedge.start_vertex,
                        coedge.end_vertex,
                    )
                })
                .collect::<Vec<_>>()
        })?;
        boundary.coedges.rotate_left(best);
    }
    Some((topology, (0..point_assignment.len()).collect::<Vec<_>>()))
}

pub(crate) fn mesh_candidates_equivalent(
    left: &(StandardTopology, Vec<usize>),
    right: &(StandardTopology, Vec<usize>),
) -> bool {
    canonicalize_mesh_vertex_labels(left.0.clone(), &left.1)
        == canonicalize_mesh_vertex_labels(right.0.clone(), &right.1)
}

pub(crate) fn mesh_assignment_can_merge(
    assignment: &MeshFaceBoundaryAssignment,
    quotient: &mut MeshQuotient,
) -> bool {
    fn possible_ports(use_: MeshBoundaryEdgeCandidate, end: bool) -> [Option<usize>; 2] {
        let port = |reversed: bool| {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(reversed != end))
        };
        match use_.reversed {
            Some(reversed) => [port(reversed), None],
            None => [port(false), port(true)],
        }
    }

    assignment.boundaries.iter().any(|boundary| {
        (0..boundary.len()).any(|index| {
            let left = possible_ports(boundary[index], true);
            let right = possible_ports(boundary[(index + 1) % boundary.len()], false);
            left.into_iter().flatten().any(|left| {
                right
                    .into_iter()
                    .flatten()
                    .any(|right| quotient.union.find(left) != quotient.union.find(right))
            })
        })
    })
}

pub(crate) fn mesh_edge_points_compatible(
    closed_ports: bool,
    candidates: &[[usize; 2]],
    points: [usize; 2],
) -> bool {
    (points[0] != points[1] || closed_ports)
        && (candidates.is_empty()
            || candidates
                .iter()
                .any(|candidate| same_unordered_pair(*candidate, points)))
}

/// Resolve standard trim assignments through their abstract physical-port
/// quotient before binding the quotient bijectively to coordinate rows.
#[must_use]
pub fn parse_standard_mesh_endpoint_candidates(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Option<(StandardTopology, Vec<usize>)> {
    let (_, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    if edge_rows.len() != edge_faces.len() || edge_rows.len() != edge_candidates.len() {
        return None;
    }
    let mut assignments =
        standard_mesh_boundary_assignments(bytes, edge_faces, Some(edge_candidates))?;
    if assignments.len() != face_count {
        return None;
    }
    deduplicate_mesh_quotient_assignments(&mut assignments);
    let port_identities = standard_edge_port_identities(bytes)?;
    resolve_standard_mesh_endpoint_candidates(
        &edge_rows,
        &vertex_points,
        edge_candidates,
        assignments,
        &port_identities,
    )
}

fn resolve_standard_mesh_endpoint_candidates(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    mut assignments: Vec<Vec<MeshFaceBoundaryAssignment>>,
    port_identities: &[[u32; 2]],
) -> Option<(StandardTopology, Vec<usize>)> {
    const MAX_SELECTION_WORK: usize = 100_000;

    let face_count = assignments.len();
    let mut edge_candidates = edge_candidates.to_vec();
    if !prune_mesh_endpoint_pair_support(&mut assignments, &mut edge_candidates) {
        return None;
    }
    let quotient = initial_mesh_quotient(&edge_candidates, vertex_points.len(), port_identities)?;
    let option_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    for face in &mut assignments {
        face.retain(|assignment| {
            quotient.assignment_has_option(assignment, &edge_candidates, Some(&option_budget))
        });
        if option_budget.exhausted.get() {
            return None;
        }
        if face.is_empty() {
            return None;
        }
    }
    let face_work = assignments
        .iter()
        .map(|assignments| Some(assignments.len()))
        .collect::<Vec<_>>();
    let total_work = face_work
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .try_fold(0usize, usize::checked_add)?;
    if total_work > MAX_SELECTION_WORK {
        return None;
    }
    let face_equations = possible_face_equations(&assignments);
    let face_choices = possible_face_choices_with_limit(
        &assignments,
        &face_equations,
        MAX_MESH_CONSTRAINT_OPERATIONS,
    )?;
    let mut search = MeshSelectionSearch {
        assignments: &assignments,
        #[cfg(test)]
        possible_face_equations: face_equations,
        possible_face_choices: face_choices,
        face_work,
        edge_candidates: &edge_candidates,
        edge_rows,
        vertex_points,
        selected: vec![None; face_count],
        states: 0,
        solution: None,
        stop_after_first_solution: edge_candidates.iter().all(|pairs| pairs.len() == 1),
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    search.search(&quotient);
    (!search.ambiguous && !search.exhausted)
        .then_some(search.solution)
        .flatten()
}

/// Resolve geometric endpoint alternatives through face incidence before
/// applying the exact trim-mesh endpoint quotient. Endpoint graphs must close
/// every face; all surviving graphs must produce one topology modulo logical
/// vertex labels, intrinsic edge direction, and boundary-cycle start.
/// `pair_solution_valid` receives partial assignments during search. It must be
/// monotone: once it rejects a selected subset, assigning more edges cannot
/// make that subset valid. `partial_constraint_edges` identifies every edge
/// whose assignment can affect that predicate, allowing constrained variables
/// to be selected before unrelated incidence variables.
#[must_use]
pub fn parse_standard_mesh_incidence_candidates<F>(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    partial_constraint_edges: &[bool],
    pair_solution_valid: F,
) -> Option<(StandardTopology, Vec<usize>)>
where
    F: Fn(&[Option<[usize; 2]>]) -> bool,
{
    const MAX_COMPLETED_PAIRS_PER_EDGE: usize = 65_536;
    const MAX_COMPLETED_PAIRS_TOTAL: usize = 1_000_000;

    let (_, face_count, after_faces) = largest_fbb_run(bytes)?;
    let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
    let vertex_points = parse_vertex_table(bytes, vertex_header)?;
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_candidates.len()
        || edge_rows.len() != partial_constraint_edges.len()
        || edge_candidates
            .iter()
            .flatten()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
        return None;
    }
    let mut mesh_domains =
        standard_mesh_boundary_domains_impl(bytes, edge_faces, Some(edge_candidates), true)?;
    if mesh_domains.len() != face_count {
        return None;
    }
    for domain in &mut mesh_domains {
        if let MeshFaceBoundaryDomain::Ordered(assignments) = domain {
            deduplicate_mesh_quotient_assignments(std::slice::from_mut(assignments));
        }
    }
    let port_identities = standard_edge_port_identities(bytes)?;
    if port_identities.len() != edge_rows.len() {
        return None;
    }
    let mut mesh_quotient =
        initial_mesh_quotient(edge_candidates, vertex_points.len(), &port_identities)?;
    let edge_candidates = if edge_candidates.iter().any(Vec::is_empty) {
        let common_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        let mut propagated_quotient = mesh_quotient.clone();
        match propagate_common_ordered_face_quotients(
            &mesh_domains,
            edge_candidates,
            &mut propagated_quotient,
            &common_budget,
        ) {
            Some(()) => mesh_quotient = propagated_quotient,
            None if common_budget.exhausted.get() => {}
            None => return None,
        }
        propagate_common_boundary_components(&mesh_domains, edge_candidates, &mut mesh_quotient)?;
        let completed = complete_mesh_endpoint_candidates_from_quotient(
            edge_candidates,
            &mut mesh_quotient,
            MAX_COMPLETED_PAIRS_PER_EDGE,
            MAX_COMPLETED_PAIRS_TOTAL,
        );
        if let Some(completed) = completed {
            completed
        } else {
            let closure_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
            let mut closed = mesh_quotient.clone();
            closed.close_coordinate_roots_for_incidence_with_budget(
                vertex_points.len(),
                edge_candidates,
                edge_faces,
                face_count,
                &mesh_domains,
                Some(&closure_budget),
            )?;
            mesh_quotient = closed;
            complete_mesh_endpoint_candidates_from_quotient(
                edge_candidates,
                &mut mesh_quotient,
                MAX_COMPLETED_PAIRS_PER_EDGE,
                MAX_COMPLETED_PAIRS_TOTAL,
            )?
        }
    } else {
        edge_candidates.to_vec()
    };
    if !mesh_quotient.edge_domains_viable(&edge_candidates) {
        return None;
    }
    let pair_solutions = incidence_endpoint_pair_solutions(
        &edge_rows,
        &vertex_points,
        edge_faces,
        &edge_candidates,
        face_count,
        Some(&mesh_domains),
        Some(&mesh_quotient),
        Some(MeshPartialEndpointConstraint {
            active_edges: partial_constraint_edges,
            valid: &pair_solution_valid,
        }),
        &|pairs| pair_solution_valid(&pairs.iter().copied().map(Some).collect::<Vec<_>>()),
    )?;
    let mut solution = None;
    for pairs in pair_solutions {
        let singleton = pairs.into_iter().map(|pair| vec![pair]).collect::<Vec<_>>();
        let Some(mut mesh_assignments) =
            standard_mesh_boundary_assignments(bytes, edge_faces, Some(&singleton))
        else {
            continue;
        };
        deduplicate_mesh_quotient_assignments(&mut mesh_assignments);
        let Some(candidate) = resolve_standard_mesh_endpoint_candidates(
            &edge_rows,
            &vertex_points,
            &singleton,
            mesh_assignments.clone(),
            &port_identities,
        ) else {
            continue;
        };
        match &solution {
            Some(stored) if !mesh_candidates_equivalent(stored, &candidate) => return None,
            None => solution = Some(candidate),
            Some(_) => {}
        }
    }
    solution
}
