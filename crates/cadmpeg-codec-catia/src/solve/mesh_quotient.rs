//! Mesh-quotient constraint solver for standard nested B-rep topology.
//!
//! Closes vertex-coordinate quotients and enumerates face endpoint configurations.

use cadmpeg_core::decode::{alloc_filled, WorkBudget};

use super::mesh_gauge::{
    build_mesh_coordinate_gauge, canonicalize_complete_endpoint_pairs,
    canonicalize_coordinate_endpoint_pairs, canonicalize_endpoint_relation_state,
    mesh_candidates_equivalent_with_context, MeshCandidateGauge,
};
use crate::families::standard::fbb::{largest_fbb_run, parse_edge_tables, parse_vertex_table};
#[cfg(test)]
use crate::families::standard::topology::EdgeBoundaryLayout;
use crate::families::standard::topology::{
    incidence_cycles, orient_face_cycles, reconstruct_mesh_selection, EdgeRow, StandardTopology,
};
use crate::solve::incidence::{
    compact_boundary_domain_viable, deferred_boundary_cycle_matches,
    visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy, CoordinateRootPolicy,
    IncidenceRejection, IncidenceSolve,
};
use crate::solve::matching::{
    distinct_domain_matching_with_budget, domains_have_distinct_matching,
    repair_distinct_domain_matching_with_budget, retain_distinct_matching_supports,
    MatchingEdgeConstraint,
};
#[cfg(test)]
use crate::solve::missing_edge::standard_mesh_boundary_assignments;
use crate::solve::missing_edge::{
    same_unordered_pair, standard_mesh_boundary_assignments_from_context,
    standard_mesh_boundary_domains_from_context, MeshBoundaryEdgeCandidate,
    MeshDeferredFaceBoundary, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
    StandardMeshBoundaryContext,
};
use crate::solve::UnionFind;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::ops::ControlFlow;
use std::sync::Arc;

pub(crate) const MAX_FACE_EQUATION_CACHE_ENTRIES: usize = 4_096;
/// Caps the optional exact-state memo without turning it into a search refusal.
pub(crate) const MAX_SELECTION_STATE_MEMO_ENTRIES: usize = 4_096;
/// Bounds reuse of deterministic endpoint-resolution results across incidence
/// assignments without retaining an unbounded set of complete topologies.
pub(crate) const MAX_ENDPOINT_RESOLUTION_MEMO_ENTRIES: usize = 256;
pub(crate) const MAX_FACE_ENDPOINT_CONFIGURATION_WORK: usize = 4_096;
/// Bounds one complete mesh-constraint phase, including exhaustive endpoint
/// orientation selection. The decode session applies its own global work cap.
pub(crate) const MAX_MESH_CONSTRAINT_OPERATIONS: usize = 1_000_000;
/// The relation walk and its endpoint materialization proof are independent
/// bounded phases and each uses the complete mesh-constraint allowance.
pub(crate) const MAX_MESH_TOPOLOGY_OPERATIONS: usize =
    MAX_MESH_CONSTRAINT_OPERATIONS.saturating_mul(2);
pub(crate) type MeshQuotientGaugeState = (MeshQuotient, HashSet<usize>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MeshCandidateRejection {
    InputStructure,
    InputCardinality,
    FaceBoundaryCardinality,
    PortCardinality,
    QuotientPreparation,
    EdgeClassConstraint,
    EndpointIncidence(MeshEndpointIncidenceRejection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MeshEndpointIncidenceRejection {
    NoAssignment(IncidenceRejection),
    BoundaryReconstruction,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CoordinateRootClosure {
    Solved(HashMap<usize, usize>),
    Rejected,
    Ambiguous,
    Exhausted,
}

enum PointAssignmentOutcome {
    Complete(Vec<HashMap<usize, usize>>),
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MeshCandidateAmbiguity {
    CoordinateRootClosure,
    EndpointResolution,
    DistinctTopologySolutions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MeshCandidateExhaustion {
    QuotientPreparation,
    IncidenceEnumeration,
    EndpointResolution,
    PreferredSolutionSearch,
}

#[derive(Debug)]
pub(crate) enum MeshCandidateSolve {
    Solved(StandardTopology, Vec<usize>),
    Rejected(MeshCandidateRejection),
    Ambiguous(MeshCandidateAmbiguity),
    Exhausted(MeshCandidateExhaustion),
}

#[derive(Clone)]
enum MeshEndpointResolve {
    Solved(StandardTopology, Vec<usize>),
    Rejected,
    Ambiguous,
    Exhausted,
}

impl MeshEndpointResolve {
    #[cfg(test)]
    fn into_option(self) -> Option<(StandardTopology, Vec<usize>)> {
        match self {
            Self::Solved(topology, assignment) => Some((topology, assignment)),
            Self::Rejected | Self::Ambiguous | Self::Exhausted => None,
        }
    }
}

fn enforce_edge_arc_consistency(
    domains: &mut [Vec<usize>],
    edges: &[[usize; 2]],
    edge_ids: &[usize],
    root_edges: &[Vec<usize>],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&WorkBudget<'_>>,
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
        if budget.is_some_and(WorkBudget::exhausted) || domains[root].is_empty() {
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

fn enforce_edge_arc_consistency_from(
    domains: &mut [Vec<usize>],
    edges: &[[usize; 2]],
    root_edges: &[Vec<usize>],
    edge_candidates: &[Vec<[usize; 2]>],
    initial_edges: &[usize],
    budget: Option<&WorkBudget<'_>>,
) -> bool {
    let mut queued = vec![[true; 2]; edges.len()];
    let mut queue = initial_edges
        .iter()
        .copied()
        .flat_map(|edge| [(edge, 0usize), (edge, 1usize)])
        .collect::<VecDeque<_>>();
    queued.fill([false; 2]);
    for &edge in initial_edges {
        queued[edge] = [true; 2];
    }
    while let Some((edge, side)) = queue.pop_front() {
        queued[edge][side] = false;
        let candidates = &edge_candidates[edge];
        if candidates.is_empty() {
            continue;
        }
        let root = edges[edge][side];
        let other = edges[edge][1 - side];
        let other_domain = domains[other].iter().copied().collect::<HashSet<_>>();
        let before = domains[root].len();
        domains[root].retain(|point| {
            if budget.is_some_and(|budget| !budget.charge_by(candidates.len().max(1))) {
                return false;
            }
            candidates.iter().any(|pair| {
                (pair[0] == *point && other_domain.contains(&pair[1]))
                    || (pair[1] == *point && other_domain.contains(&pair[0]))
            })
        });
        if budget.is_some_and(WorkBudget::exhausted) || domains[root].is_empty() {
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
    budget: Option<&WorkBudget<'_>>,
) -> bool {
    let mut ordered = (0..edges.len()).collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|edge| edge_candidates[edge_ids[*edge]].len());
    for edge in ordered {
        let candidates = &edge_candidates[edge_ids[edge]];
        if candidates.is_empty() {
            continue;
        }
        let [left, right] = edges[edge];
        let domain_work =
            domains[left].len() + usize::from(right != left).saturating_mul(domains[right].len());
        let support_work = candidates.len().saturating_mul(2);
        if support_work >= domain_work {
            continue;
        }
        let work = support_work.saturating_add(domain_work);
        if budget.is_some_and(|budget| work > budget.remaining()) {
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

#[derive(Clone)]
pub(crate) struct MeshQuotient {
    pub(crate) union: UnionFind,
    pub(crate) domains: Vec<Arc<HashSet<usize>>>,
    pub(crate) members: Vec<Vec<usize>>,
}

#[derive(Clone)]
pub(crate) struct MeshCoordinateRootDomains {
    domains: Vec<Vec<usize>>,
    edges: Arc<Vec<[usize; 2]>>,
    root_edges: Arc<Vec<Vec<usize>>>,
    edge_candidates: Arc<Vec<Vec<[usize; 2]>>>,
    coverage_matching: Vec<usize>,
    point_count: usize,
}

pub(crate) struct MeshImplicitEdgeCandidates {
    source: MeshImplicitEdgeCandidateSource,
}

enum MeshImplicitEdgeCandidateSource {
    Cartesian {
        left: Vec<usize>,
        right: Vec<usize>,
        left_index: usize,
        right_index: usize,
        same_root: bool,
    },
    Required {
        points: std::vec::IntoIter<usize>,
        required: usize,
    },
}

impl MeshImplicitEdgeCandidates {
    pub(crate) fn width_upper_bound(&self) -> usize {
        match &self.source {
            MeshImplicitEdgeCandidateSource::Cartesian { left, right, .. } => {
                left.len().saturating_mul(right.len())
            }
            MeshImplicitEdgeCandidateSource::Required { points, .. } => points.len(),
        }
    }
}

impl Iterator for MeshImplicitEdgeCandidates {
    type Item = [usize; 2];

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.source {
            MeshImplicitEdgeCandidateSource::Required { points, required } => {
                points.next().map(|point| {
                    if *required <= point {
                        [*required, point]
                    } else {
                        [point, *required]
                    }
                })
            }
            MeshImplicitEdgeCandidateSource::Cartesian {
                left,
                right,
                left_index,
                right_index,
                same_root,
            } => {
                while *left_index < left.len() {
                    let left_point = left[*left_index];
                    let right_point = right[*right_index];
                    *right_index += 1;
                    if *right_index == right.len() {
                        *left_index += 1;
                        *right_index = 0;
                    }
                    if !*same_root && left_point == right_point {
                        continue;
                    }
                    if left_point > right_point
                        && left.binary_search(&right_point).is_ok()
                        && right.binary_search(&left_point).is_ok()
                    {
                        continue;
                    }
                    return Some(if left_point <= right_point {
                        [left_point, right_point]
                    } else {
                        [right_point, left_point]
                    });
                }
                None
            }
        }
    }
}

pub(crate) enum MeshEndpointCandidates<'a> {
    Explicit(&'a [[usize; 2]]),
    Implicit(MeshImplicitEdgeCandidates),
    Selected([usize; 2]),
}

impl MeshCoordinateRootDomains {
    pub(crate) fn edge_candidates(&self) -> &[Vec<[usize; 2]>] {
        &self.edge_candidates
    }

    pub(crate) fn supports_edge_candidate(&self, edge: usize, pair: [usize; 2]) -> bool {
        let Some(&[left, right]) = self.edges.get(edge) else {
            return false;
        };
        if left != right && pair[0] == pair[1] {
            return false;
        }
        (self.domains[left].binary_search(&pair[0]).is_ok()
            && self.domains[right].binary_search(&pair[1]).is_ok())
            || (self.domains[left].binary_search(&pair[1]).is_ok()
                && self.domains[right].binary_search(&pair[0]).is_ok())
    }

    pub(crate) fn edge_candidate_points(&self, edge: usize) -> Option<Vec<usize>> {
        let candidates = self.edge_candidates.get(edge)?;
        if !candidates.is_empty() {
            let mut points = candidates.iter().flatten().copied().collect::<Vec<_>>();
            points.sort_unstable();
            points.dedup();
            return Some(points);
        }
        let &[left, right] = self.edges.get(edge)?;
        let mut points = self.domains[left].clone();
        if right != left {
            points.extend_from_slice(&self.domains[right]);
            points.sort_unstable();
            points.dedup();
        }
        Some(points)
    }

    pub(crate) fn implicit_edge_candidates(
        &self,
        edge: usize,
        required_point: Option<usize>,
    ) -> Option<MeshImplicitEdgeCandidates> {
        self.edge_candidates.get(edge)?.is_empty().then_some(())?;
        let &[left, right] = self.edges.get(edge)?;
        if let Some(required) = required_point {
            let required_in_left = self.domains[left].binary_search(&required).is_ok();
            let required_in_right = self.domains[right].binary_search(&required).is_ok();
            let mut points = match (required_in_left, required_in_right) {
                (true, false) => self.domains[right].clone(),
                (false, true) => self.domains[left].clone(),
                (false, false) => Vec::new(),
                (true, true) if left == right => self.domains[left].clone(),
                (true, true) => {
                    let mut points =
                        Vec::with_capacity(self.domains[left].len() + self.domains[right].len());
                    let (mut left_index, mut right_index) = (0, 0);
                    while left_index < self.domains[left].len()
                        || right_index < self.domains[right].len()
                    {
                        let point = match (
                            self.domains[left].get(left_index),
                            self.domains[right].get(right_index),
                        ) {
                            (Some(left), Some(right)) if left < right => {
                                left_index += 1;
                                *left
                            }
                            (Some(left), Some(right)) if right < left => {
                                right_index += 1;
                                *right
                            }
                            (Some(left), Some(_)) => {
                                left_index += 1;
                                right_index += 1;
                                *left
                            }
                            (Some(left), None) => {
                                left_index += 1;
                                *left
                            }
                            (None, Some(right)) => {
                                right_index += 1;
                                *right
                            }
                            (None, None) => break,
                        };
                        points.push(point);
                    }
                    points
                }
            };
            if left != right {
                points.retain(|point| *point != required);
            }
            return Some(MeshImplicitEdgeCandidates {
                source: MeshImplicitEdgeCandidateSource::Required {
                    points: points.into_iter(),
                    required,
                },
            });
        }
        Some(MeshImplicitEdgeCandidates {
            source: MeshImplicitEdgeCandidateSource::Cartesian {
                left: self.domains[left].clone(),
                right: self.domains[right].clone(),
                left_index: 0,
                right_index: 0,
                same_root: left == right,
            },
        })
    }

    pub(crate) fn any_implicit_edge_candidate_with_point(
        &self,
        edge: usize,
        required: usize,
        budget: Option<&WorkBudget<'_>>,
        mut valid: impl FnMut([usize; 2]) -> bool,
    ) -> Option<bool> {
        self.edge_candidates.get(edge)?.is_empty().then_some(())?;
        let &[left, right] = self.edges.get(edge)?;
        let pair = |point| {
            if required <= point {
                [required, point]
            } else {
                [point, required]
            }
        };
        let required_in_left = self.domains[left].binary_search(&required).is_ok();
        if required_in_left {
            for point in self.domains[right]
                .iter()
                .copied()
                .filter(|point| left == right || *point != required)
            {
                if budget.is_some_and(|budget| !budget.charge()) {
                    return None;
                }
                if valid(pair(point)) {
                    return Some(true);
                }
            }
        }
        if self.domains[right].binary_search(&required).is_err() {
            return Some(false);
        }
        for point in self.domains[left]
            .iter()
            .copied()
            .filter(|point| left == right || *point != required)
            .filter(|point| !required_in_left || self.domains[right].binary_search(point).is_err())
        {
            if budget.is_some_and(|budget| !budget.charge()) {
                return None;
            }
            if valid(pair(point)) {
                return Some(true);
            }
        }
        Some(false)
    }

    fn coverage_matching(
        domains: &[Vec<usize>],
        point_count: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<Vec<usize>> {
        let mut roots_by_point =
            alloc_filled(point_count, Vec::new(), "catia_quotient_roots_by_point").ok()?;
        for (root, domain) in domains.iter().enumerate() {
            for &point in domain {
                roots_by_point[point].push(root);
            }
        }
        (!roots_by_point.iter().any(Vec::is_empty))
            .then(|| {
                distinct_domain_matching_with_budget(
                    roots_by_point.iter().map(Vec::as_slice),
                    domains.len(),
                    budget,
                    None,
                )
            })
            .flatten()
    }

    fn refine_domains(
        &self,
        mut domains: Vec<Vec<usize>>,
        edge_candidates: &[Vec<[usize; 2]>],
        initial_edges: &[usize],
        mut propagate_all_different: bool,
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<(Vec<Vec<usize>>, Vec<usize>)> {
        let mut affected_edges = initial_edges.to_vec();
        let mut coverage_matching = self.coverage_matching.clone();
        let propagate_globally = propagate_all_different;
        loop {
            let domain_lengths = domains.iter().map(Vec::len).collect::<Vec<_>>();
            if !enforce_edge_arc_consistency_from(
                &mut domains,
                &self.edges,
                &self.root_edges,
                edge_candidates,
                &affected_edges,
                budget,
            ) {
                return None;
            }
            let mut roots_by_point =
                alloc_filled(self.point_count, Vec::new(), "catia_quotient_refine_roots").ok()?;
            for (root, domain) in domains.iter().enumerate() {
                for &point in domain {
                    roots_by_point[point].push(root);
                }
            }
            let repaired_matching = repair_distinct_domain_matching_with_budget(
                roots_by_point.iter().map(Vec::as_slice),
                domains.len(),
                &coverage_matching,
                budget,
            )?;
            propagate_all_different |= repaired_matching != coverage_matching;
            coverage_matching = repaired_matching;
            if !propagate_all_different {
                return Some((domains, coverage_matching));
            }
            let changed_roots = domains
                .iter()
                .zip(domain_lengths)
                .enumerate()
                .filter_map(|(root, (domain, before))| (domain.len() != before).then_some(root))
                .collect::<Vec<_>>();
            let affected_points = if propagate_globally {
                (0..self.point_count).collect::<Vec<_>>()
            } else {
                if changed_roots.is_empty() {
                    return Some((domains, coverage_matching));
                }
                let mut reached_roots =
                    alloc_filled(domains.len(), false, "catia_quotient_reached_roots").ok()?;
                let mut reached_points =
                    alloc_filled(self.point_count, false, "catia_quotient_reached_points").ok()?;
                let mut root_queue = VecDeque::from(changed_roots);
                while let Some(root) = root_queue.pop_front() {
                    if reached_roots[root] {
                        continue;
                    }
                    reached_roots[root] = true;
                    for &point in &domains[root] {
                        if reached_points[point] {
                            continue;
                        }
                        reached_points[point] = true;
                        for &neighbor in &roots_by_point[point] {
                            if !reached_roots[neighbor] {
                                root_queue.push_back(neighbor);
                            }
                        }
                    }
                }
                reached_points
                    .into_iter()
                    .enumerate()
                    .filter_map(|(point, reached)| reached.then_some(point))
                    .collect()
            };
            let mut affected_domains = affected_points
                .iter()
                .map(|point| roots_by_point[*point].clone())
                .collect::<Vec<_>>();
            let affected_matching = affected_points
                .iter()
                .map(|point| coverage_matching[*point])
                .collect::<Vec<_>>();
            let support_count = affected_domains.iter().map(Vec::len).sum::<usize>();
            let propagation_work = support_count.saturating_mul(4);
            if budget.is_some_and(|budget| propagation_work > budget.remaining()) {
                return Some((domains, coverage_matching));
            }
            retain_distinct_matching_supports(
                &mut affected_domains,
                domains.len(),
                &affected_matching,
                budget,
            )?;
            for (point, supported) in affected_points.into_iter().zip(affected_domains) {
                roots_by_point[point] = supported;
            }
            let mut affected_roots = Vec::new();
            for (root, domain) in domains.iter_mut().enumerate() {
                let before = domain.len();
                domain.retain(|point| roots_by_point[*point].binary_search(&root).is_ok());
                if domain.is_empty() {
                    return None;
                }
                if domain.len() != before {
                    affected_roots.push(root);
                }
            }
            if affected_roots.is_empty() {
                return Some((domains, coverage_matching));
            }
            affected_edges = affected_roots
                .into_iter()
                .flat_map(|root| self.root_edges[root].iter().copied())
                .collect();
            affected_edges.sort_unstable();
            affected_edges.dedup();
        }
    }

    pub(crate) fn refine_edge_candidate_arc(
        &self,
        edge: usize,
        pair: [usize; 2],
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<Self> {
        let candidates = self.edge_candidates.get(edge)?;
        if candidates.as_slice() == [pair] {
            return Some(self.clone());
        }
        if !candidates.is_empty() && !candidates.contains(&pair) {
            return None;
        }
        if candidates.is_empty() && !self.supports_edge_candidate(edge, pair) {
            return None;
        }
        let mut edge_candidates = self.edge_candidates.as_ref().clone();
        edge_candidates[edge] = vec![pair];
        let (domains, coverage_matching) = self.refine_domains(
            self.domains.clone(),
            &edge_candidates,
            &[edge],
            false,
            budget,
        )?;
        Some(Self {
            domains,
            edges: Arc::clone(&self.edges),
            root_edges: Arc::clone(&self.root_edges),
            edge_candidates: Arc::new(edge_candidates),
            coverage_matching,
            point_count: self.point_count,
        })
    }

    pub(crate) fn refine_candidates(
        &self,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<Self> {
        if edge_candidates.len() != self.edge_candidates.len() {
            return None;
        }
        let changed = edge_candidates
            .iter()
            .zip(self.edge_candidates.iter())
            .enumerate()
            .filter_map(|(edge, (current, base))| (current != base).then_some(edge))
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return Some(self.clone());
        }
        let (domains, coverage_matching) = self.refine_domains(
            self.domains.clone(),
            edge_candidates,
            &changed,
            false,
            budget,
        )?;
        Some(Self {
            domains,
            edges: Arc::clone(&self.edges),
            root_edges: Arc::clone(&self.root_edges),
            edge_candidates: Arc::new(edge_candidates.to_vec()),
            coverage_matching,
            point_count: self.point_count,
        })
    }
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

#[cfg(test)]
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
    pub(crate) fn coordinate_domain_preparation_limit(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Option<usize> {
        if self.union.len() != edge_candidates.len().checked_mul(2)? {
            return None;
        }
        let mut root_count = 0usize;
        let mut root_supports = 0usize;
        for node in 0..self.union.len() {
            if self.union.find(node) != node {
                continue;
            }
            root_count += 1;
            root_supports = root_supports.saturating_add(
                self.domains[node]
                    .iter()
                    .filter(|point| **point < point_count)
                    .count(),
            );
        }
        let explicit_pair_supports = edge_candidates.iter().map(Vec::len).sum::<usize>();
        let matching_phase_bound = root_count
            .saturating_add(point_count)
            .isqrt()
            .saturating_add(1);
        let traversal_bound = matching_phase_bound.saturating_add(8);
        Some(
            root_supports
                .saturating_add(explicit_pair_supports)
                .saturating_mul(traversal_bound)
                .max(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
    }

    pub(crate) fn signature_work(&mut self) -> usize {
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

    pub(crate) fn prepare_coordinate_root_domains(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<MeshCoordinateRootDomains> {
        if self.union.len() != edge_candidates.len().saturating_mul(2) {
            return None;
        }
        let roots = (0..self.union.len())
            .filter(|node| self.union.find(*node) == *node)
            .collect::<Vec<_>>();
        if roots.len() < point_count {
            return None;
        }
        let root_indices = roots
            .iter()
            .enumerate()
            .map(|(index, root)| (*root, index))
            .collect::<HashMap<_, _>>();
        let edges = (0..edge_candidates.len())
            .map(|edge| {
                Some([
                    *root_indices.get(&self.union.find(edge * 2))?,
                    *root_indices.get(&self.union.find(edge * 2 + 1))?,
                ])
            })
            .collect::<Option<Vec<_>>>();
        let edges = edges?;
        let mut domains = roots
            .iter()
            .map(|root| {
                let mut domain = self.domains[*root]
                    .iter()
                    .copied()
                    .filter(|point| *point < point_count)
                    .collect::<Vec<_>>();
                domain.sort_unstable();
                domain
            })
            .collect::<Vec<_>>();
        if domains.iter().any(Vec::is_empty) {
            return None;
        }
        let edge_ids = (0..edges.len()).collect::<Vec<_>>();
        let mut root_edges =
            alloc_filled(roots.len(), Vec::new(), "catia_quotient_root_edges").ok()?;
        for (edge, [left, right]) in edges.iter().copied().enumerate() {
            root_edges[left].push(edge);
            if right != left {
                root_edges[right].push(edge);
            }
        }
        if !enforce_sparse_endpoint_membership(
            &mut domains,
            &edges,
            &edge_ids,
            edge_candidates,
            budget,
        ) {
            return None;
        }
        if !enforce_edge_arc_consistency(
            &mut domains,
            &edges,
            &edge_ids,
            &root_edges,
            edge_candidates,
            budget,
        ) {
            return None;
        }
        let mut supported_candidates = edge_candidates.to_vec();
        loop {
            let mut changed = Vec::new();
            for (edge, candidates) in supported_candidates.iter_mut().enumerate() {
                if candidates.is_empty() {
                    continue;
                }
                let [left, right] = edges[edge];
                let before = candidates.len();
                if budget.is_some_and(|budget| !budget.charge_by(before)) {
                    return None;
                }
                candidates.retain(|pair| {
                    (domains[left].binary_search(&pair[0]).is_ok()
                        && domains[right].binary_search(&pair[1]).is_ok())
                        || (domains[left].binary_search(&pair[1]).is_ok()
                            && domains[right].binary_search(&pair[0]).is_ok())
                });
                if candidates.is_empty() {
                    return None;
                }
                if candidates.len() != before {
                    changed.push(edge);
                }
            }
            if changed.is_empty() {
                break;
            }
            if !enforce_edge_arc_consistency_from(
                &mut domains,
                &edges,
                &root_edges,
                &supported_candidates,
                &changed,
                budget,
            ) {
                return None;
            }
        }
        let coverage_matching =
            MeshCoordinateRootDomains::coverage_matching(&domains, point_count, budget)?;
        let coordinate_domains = MeshCoordinateRootDomains {
            domains,
            edges: Arc::new(edges),
            root_edges: Arc::new(root_edges),
            edge_candidates: Arc::new(supported_candidates),
            coverage_matching,
            point_count,
        };
        let (domains, coverage_matching) = coordinate_domains.refine_domains(
            coordinate_domains.domains.clone(),
            &coordinate_domains.edge_candidates,
            // The full edge set already reached arc consistency above. This pass
            // starts with Hall support and only revisits edges narrowed by it.
            &[],
            true,
            budget,
        )?;
        Some(MeshCoordinateRootDomains {
            domains,
            coverage_matching,
            ..coordinate_domains
        })
    }

    fn propagate_component_edge_domains(
        &mut self,
        root: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
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
        budget: Option<&WorkBudget<'_>>,
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
            let mut affected_edges = HashSet::new();
            for roots in roots_by_point.into_values() {
                let Some((&first, rest)) = roots.split_first() else {
                    continue;
                };
                for &root in rest {
                    affected_edges.extend(
                        self.members[first]
                            .iter()
                            .chain(&self.members[root])
                            .map(|node| node / 2)
                            .filter(|edge| !edge_candidates[*edge].is_empty()),
                    );
                    if self.merge(first, root).is_none() {
                        return false;
                    }
                    changed = true;
                }
            }
            if !changed {
                return true;
            }
            if !self.propagate_edge_domains(affected_edges, edge_candidates, None) {
                return false;
            }
        }
    }

    pub(crate) fn close_coordinate_roots(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<HashMap<usize, usize>> {
        match self.coordinate_root_closure_outcome(point_count, edge_candidates, None, budget) {
            CoordinateRootClosure::Solved(assignment) => Some(assignment),
            CoordinateRootClosure::Rejected
            | CoordinateRootClosure::Ambiguous
            | CoordinateRootClosure::Exhausted => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn close_coordinate_roots_for_incidence_with_budget(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        edge_faces: &[[usize; 2]],
        face_count: usize,
        boundary_domains: &[MeshFaceBoundaryDomain],
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<HashMap<usize, usize>> {
        (edge_faces.len() == edge_candidates.len()
            && edge_faces.iter().flatten().all(|face| *face < face_count)
            && boundary_domains.len() == face_count)
            .then_some(())?;
        match self.coordinate_root_closure_outcome(
            point_count,
            edge_candidates,
            Some((edge_faces, boundary_domains)),
            budget,
        ) {
            CoordinateRootClosure::Solved(assignment) => Some(assignment),
            CoordinateRootClosure::Rejected
            | CoordinateRootClosure::Ambiguous
            | CoordinateRootClosure::Exhausted => None,
        }
    }

    pub(crate) fn coordinate_root_closure_outcome_for_incidence(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        edge_faces: &[[usize; 2]],
        face_count: usize,
        boundary_domains: &[MeshFaceBoundaryDomain],
        budget: Option<&WorkBudget<'_>>,
    ) -> CoordinateRootClosure {
        if edge_faces.len() != edge_candidates.len()
            || edge_faces.iter().flatten().any(|face| *face >= face_count)
            || boundary_domains.len() != face_count
        {
            return CoordinateRootClosure::Rejected;
        }
        self.coordinate_root_closure_outcome_with_component_budget(
            point_count,
            edge_candidates,
            Some((edge_faces, boundary_domains)),
            budget,
            Some(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
    }

    pub(crate) fn coordinate_root_closure_outcome(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
        budget: Option<&WorkBudget<'_>>,
    ) -> CoordinateRootClosure {
        self.coordinate_root_closure_outcome_with_component_budget(
            point_count,
            edge_candidates,
            incidence,
            budget,
            None,
        )
    }

    fn coordinate_root_closure_outcome_with_component_budget(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
        budget: Option<&WorkBudget<'_>>,
        component_search_budget: Option<usize>,
    ) -> CoordinateRootClosure {
        let ambiguous = Cell::new(false);
        let exhausted = Cell::new(false);
        let result = self.close_coordinate_roots_with_incidence(
            point_count,
            edge_candidates,
            incidence,
            budget,
            component_search_budget,
            &ambiguous,
            &exhausted,
        );
        match result {
            Some(assignment) => CoordinateRootClosure::Solved(assignment),
            None if exhausted.get() || budget.is_some_and(WorkBudget::exhausted) => {
                CoordinateRootClosure::Exhausted
            }
            None if ambiguous.get() => CoordinateRootClosure::Ambiguous,
            None => CoordinateRootClosure::Rejected,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn close_coordinate_roots_with_incidence(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
        budget: Option<&WorkBudget<'_>>,
        component_search_budget: Option<usize>,
        ambiguous: &Cell<bool>,
        exhausted: &Cell<bool>,
    ) -> Option<HashMap<usize, usize>> {
        const MAX_COORDINATE_CLOSURE_STATES: usize = 256;

        fn pair_supported(candidates: &[[usize; 2]], left: usize, right: usize) -> bool {
            candidates.is_empty()
                || candidates
                    .iter()
                    .any(|pair| same_unordered_pair(*pair, [left, right]))
        }

        #[allow(clippy::too_many_arguments)]
        fn partial_ordered_assignment_viable(
            assignment: &MeshFaceBoundaryAssignment,
            local_edge_by_id: &HashMap<usize, usize>,
            edges: &[[usize; 2]],
            domains: &[Vec<usize>],
            assigned: &[Option<usize>],
            candidate: Option<(usize, usize)>,
            budget: Option<&WorkBudget<'_>>,
        ) -> bool {
            let directions = |use_: MeshBoundaryEdgeCandidate| match use_.reversed {
                Some(reversed) => [Some(reversed), None],
                None => [Some(false), Some(true)],
            };
            let port_root = |use_: MeshBoundaryEdgeCandidate, reversed: bool, end: bool| {
                let local = *local_edge_by_id.get(&use_.edge)?;
                Some(edges[local][usize::from(if end { !reversed } else { reversed })])
            };
            let compatible = |left: usize, right: usize, charge: bool| {
                if charge && budget.is_some_and(|budget| !budget.charge()) {
                    return false;
                }
                let value = |root| {
                    candidate
                        .filter(|(candidate_root, _)| *candidate_root == root)
                        .map(|(_, point)| point)
                        .or_else(|| assigned.get(root).copied().flatten())
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
                                    compatible(left, right, true)
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
                            compatible(left, right, false)
                        })
                    })
            })
        }

        fn complete_ordered_assignment_viable(
            assignment: &MeshFaceBoundaryAssignment,
            edge_points: &[Option<[usize; 2]>],
            budget: Option<&WorkBudget<'_>>,
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
                        let Some(first_points) = edge_points.get(first.edge).copied().flatten()
                        else {
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

        #[allow(clippy::too_many_arguments)]
        fn partial_compact_assignment_viable(
            domain: &MeshFaceBoundaryDomain,
            local_edge_by_id: &HashMap<usize, usize>,
            edges: &[[usize; 2]],
            global_edge_count: usize,
            assigned: &[Option<usize>],
            candidate: (usize, usize),
            budget: Option<&WorkBudget<'_>>,
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
                                return alloc_filled(
                                    domain.cycles.len(),
                                    false,
                                    "catia_deferred_incompatible",
                                )
                                .ok();
                            };
                            Some(
                                domain
                                    .cycles
                                    .iter()
                                    .map(|cycle| {
                                        deferred_boundary_cycle_matches(cycle, incidence, &missing)
                                    })
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect::<Option<Vec<_>>>();
                    let Some(compatible) = compatible else {
                        return false;
                    };
                    let Ok(mut matched) =
                        alloc_filled(domain.cycles.len(), None, "catia_deferred_matched")
                    else {
                        return false;
                    };
                    (0..closed_components.len()).all(|component| {
                        let Ok(mut visited) = alloc_filled(
                            domain.cycles.len(),
                            false,
                            "catia_deferred_augment_visit",
                        ) else {
                            return false;
                        };
                        augment(component, &compatible, &mut visited, &mut matched)
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
            state_limit: usize,
            exhausted: &mut bool,
            base_degrees: &mut HashMap<(usize, usize), u8>,
            budget: Option<&WorkBudget<'_>>,
        ) {
            fn adjust_assignment_degrees(
                root: usize,
                increase: bool,
                assigned: &[Option<usize>],
                edges: &[[usize; 2]],
                root_edges: &[Vec<usize>],
                edge_faces: Option<&[[usize; 2]]>,
                degrees: &mut HashMap<(usize, usize), u8>,
            ) {
                let Some(edge_faces) = edge_faces else {
                    return;
                };
                for &edge in &root_edges[root] {
                    let [left, right] = edges[edge];
                    let [Some(left), Some(right)] = [assigned[left], assigned[right]] else {
                        continue;
                    };
                    let faces = edge_faces[edge];
                    for (rank, face) in faces.into_iter().enumerate() {
                        if rank > 0 && face == faces[0] {
                            continue;
                        }
                        for point in [left, right] {
                            if increase {
                                *degrees.entry((face, point)).or_default() += 1;
                            } else {
                                let degree = degrees
                                    .get_mut(&(face, point))
                                    .expect("assigned edge contributes its face degree");
                                *degree -= 1;
                                if *degree == 0 {
                                    degrees.remove(&(face, point));
                                }
                            }
                        }
                    }
                }
            }

            #[allow(clippy::too_many_arguments)]
            fn assign(
                root: usize,
                point: usize,
                assigned: &mut [Option<usize>],
                edges: &[[usize; 2]],
                root_edges: &[Vec<usize>],
                edge_faces: Option<&[[usize; 2]]>,
                degrees: &mut HashMap<(usize, usize), u8>,
                budget: Option<&WorkBudget<'_>>,
            ) -> bool {
                if budget.is_some_and(|budget| !budget.charge_by(root_edges[root].len())) {
                    return false;
                }
                assigned[root] = Some(point);
                adjust_assignment_degrees(
                    root, true, assigned, edges, root_edges, edge_faces, degrees,
                );
                true
            }

            fn unassign(
                root: usize,
                assigned: &mut [Option<usize>],
                edges: &[[usize; 2]],
                root_edges: &[Vec<usize>],
                edge_faces: Option<&[[usize; 2]]>,
                degrees: &mut HashMap<(usize, usize), u8>,
            ) {
                adjust_assignment_degrees(
                    root, false, assigned, edges, root_edges, edge_faces, degrees,
                );
                assigned[root] = None;
            }

            #[allow(clippy::too_many_arguments)]
            fn rollback(
                assigned: &mut [Option<usize>],
                point_uses: &mut [usize],
                propagated: Vec<(usize, usize)>,
                edges: &[[usize; 2]],
                root_edges: &[Vec<usize>],
                edge_faces: Option<&[[usize; 2]]>,
                degrees: &mut HashMap<(usize, usize), u8>,
            ) {
                for (root, point) in propagated.into_iter().rev() {
                    point_uses[point] -= 1;
                    unassign(root, assigned, edges, root_edges, edge_faces, degrees);
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
                 base_degrees: &HashMap<(usize, usize), u8>,
                 work_budget: Option<&WorkBudget<'_>>| {
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
                            if work_budget.is_some_and(|budget| {
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
                                if work_budget.is_some_and(|budget| {
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
                                                        Some((root, *point)),
                                                        work_budget,
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
                                                work_budget,
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
                scanned_roots.sort_unstable_by_key(|root| (domains[*root].len(), *root));
                let partial_scan = scanned_roots.len() < domains.len();
                let bounded_scan = !partial_scan
                    && budget.is_some_and(|budget| {
                        assigned
                            .iter()
                            .enumerate()
                            .filter(|(_, point)| point.is_none())
                            .map(|(root, _)| domains[root].len().saturating_add(1))
                            .fold(0usize, usize::saturating_add)
                            > budget.remaining()
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
                let mut scan_deferred = false;
                let mut supported_unused = HashSet::new();
                let mut unused_point_roots = HashMap::<usize, Vec<usize>>::new();
                for root in scanned_roots {
                    if assigned[root].is_some() {
                        continue;
                    }
                    let work_budget = budget.map(|budget| WorkBudget::new(budget.remaining()));
                    if work_budget.as_ref().is_some_and(|budget| {
                        !budget.charge_by(domains[root].len().saturating_add(1))
                    }) {
                        scan_deferred = true;
                        continue;
                    }
                    let values = viable_values(root, assigned, base_degrees, work_budget.as_ref());
                    if work_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                        scan_deferred = true;
                        continue;
                    }
                    if let (Some(budget), Some(work_budget)) = (budget, work_budget.as_ref()) {
                        let work = budget.remaining() - work_budget.remaining();
                        if !budget.charge_by(work) {
                            *exhausted = true;
                            break;
                        }
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
                        if !assign(
                            root,
                            *point,
                            assigned,
                            edges,
                            root_edges,
                            edge_faces,
                            base_degrees,
                            budget,
                        ) {
                            *exhausted = true;
                            break;
                        }
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
                if scan_truncated || scan_deferred {
                    let best = viable_domains
                        .into_iter()
                        .min_by_key(|(_, values)| values.len());
                    if best.is_some() {
                        break Some(best);
                    }
                    if partial_scan {
                        pending_roots = None;
                        continue;
                    }
                    if let Some(budget) = budget {
                        budget.exhaust();
                    }
                    *exhausted = true;
                    break None;
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
                    if !assign(
                        root,
                        point,
                        assigned,
                        edges,
                        root_edges,
                        edge_faces,
                        base_degrees,
                        budget,
                    ) {
                        *exhausted = true;
                        break None;
                    }
                    point_uses[point] += 1;
                    propagated.push((root, point));
                    pending_roots = Some(affected_roots(
                        root, root_edges, edges, edge_faces, face_edges,
                    ));
                    continue;
                }
                let matching_budget = budget.map(|budget| WorkBudget::new(budget.remaining()));
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
                            if matching_budget.as_ref().is_some_and(WorkBudget::exhausted) {
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
                                    if matching_budget.as_ref().is_some_and(WorkBudget::exhausted) {
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
                    .is_none_or(|budget| !budget.exhausted())
                {
                    if let (Some(budget), Some(matching_budget)) =
                        (budget, matching_budget.as_ref())
                    {
                        let work = budget.remaining() - matching_budget.remaining();
                        if !budget.charge_by(work) {
                            *exhausted = true;
                            break None;
                        }
                    }
                    if coverage_matching.is_none() {
                        break None;
                    }
                    if let Some((point, root)) = matching_forced {
                        if !assign(
                            root,
                            point,
                            assigned,
                            edges,
                            root_edges,
                            edge_faces,
                            base_degrees,
                            budget,
                        ) {
                            *exhausted = true;
                            break None;
                        }
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
                        if !assign(
                            root,
                            point,
                            assigned,
                            edges,
                            root_edges,
                            edge_faces,
                            base_degrees,
                            budget,
                        ) {
                            *exhausted = true;
                            break None;
                        }
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
                rollback(
                    assigned,
                    point_uses,
                    propagated,
                    edges,
                    root_edges,
                    edge_faces,
                    base_degrees,
                );
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
                        let closed_face_count =
                            closed_faces.iter().filter(|closed| **closed).count();
                        if budget.is_some_and(|budget| {
                            !budget.charge_by(edge_ids.len().saturating_add(closed_face_count))
                        }) {
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
                        closed_faces
                            .iter()
                            .enumerate()
                            .filter(|(_, closed)| **closed)
                            .all(|(face, _)| match &boundary_domains[face] {
                                MeshFaceBoundaryDomain::Ordered(assignments) => {
                                    assignments.iter().any(|assignment| {
                                        complete_ordered_assignment_viable(
                                            assignment, &selected, budget,
                                        )
                                    })
                                }
                                domain => compact_boundary_domain_viable(domain, &selected, None),
                            })
                    },
                );
                if budget.is_some_and(WorkBudget::exhausted) {
                    *exhausted = true;
                }
                if !*exhausted
                    && incidence_closed
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
                rollback(
                    assigned,
                    point_uses,
                    propagated,
                    edges,
                    root_edges,
                    edge_faces,
                    base_degrees,
                );
                return;
            };
            if *states >= state_limit {
                if let Some(budget) = budget {
                    budget.exhaust();
                }
                *exhausted = true;
                rollback(
                    assigned,
                    point_uses,
                    propagated,
                    edges,
                    root_edges,
                    edge_faces,
                    base_degrees,
                );
                return;
            }
            *states += 1;
            for point in values {
                if !assign(
                    root,
                    point,
                    assigned,
                    edges,
                    root_edges,
                    edge_faces,
                    base_degrees,
                    budget,
                ) {
                    *exhausted = true;
                    break;
                }
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
                    state_limit,
                    exhausted,
                    base_degrees,
                    budget,
                );
                point_uses[point] -= 1;
                unassign(root, assigned, edges, root_edges, edge_faces, base_degrees);
                if solutions.len() > 1 || *exhausted {
                    break;
                }
            }
            rollback(
                assigned,
                point_uses,
                propagated,
                edges,
                root_edges,
                edge_faces,
                base_degrees,
            );
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
            return match self.point_assignments_with_budget(point_count, edge_candidates, 2, budget)
            {
                PointAssignmentOutcome::Complete(mut assignments) => match assignments.len() {
                    1 => Some(assignments.remove(0)),
                    length if length > 1 => {
                        ambiguous.set(true);
                        None
                    }
                    _ => None,
                },
                PointAssignmentOutcome::Exhausted => None,
            };
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
                let mut domain = self.domains[*root]
                    .iter()
                    .copied()
                    .filter(|point| *point < point_count)
                    .collect::<Vec<_>>();
                domain.sort_unstable();
                domain
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
        let face_incidence_counts = incidence.map(|(edge_faces, boundary_domains)| {
            if budget.is_some_and(|budget| !budget.charge_by(edge_faces.len())) {
                return Vec::new();
            }
            let mut counts = vec![0usize; boundary_domains.len()];
            for faces in edge_faces {
                for (rank, face) in faces.iter().copied().enumerate() {
                    if rank == 0 || face != faces[0] {
                        counts[face] += 1;
                    }
                }
            }
            counts
        });
        if face_incidence_counts.as_ref().is_some_and(Vec::is_empty) {
            return None;
        }
        let mut assignment = vec![None; roots.len()];
        let shared_budget = budget;
        for component in components {
            let support_count = component
                .iter()
                .map(|root| domains[*root].len())
                .fold(0usize, usize::saturating_add);
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
            let component_points = component
                .iter()
                .flat_map(|root| domains[*root].iter())
                .copied()
                .collect::<HashSet<_>>();
            let explicit_pair_supports = edge_ids
                .iter()
                .map(|edge| edge_candidates[*edge].len())
                .fold(0usize, usize::saturating_add);
            let traversal_bound = component
                .len()
                .saturating_add(component_points.len())
                .isqrt()
                .saturating_add(9);
            // A component may require one branch state for every explicit
            // root-point support before propagation distinguishes a solution.
            let state_limit = MAX_COORDINATE_CLOSURE_STATES.max(support_count);
            let component_limit = component_search_budget.map(|base| {
                // Reserve the same graph-traversal allowance used by coordinate
                // preparation plus face-incidence scans for every permitted
                // branch state.
                let state_work = support_count
                    .saturating_add(explicit_pair_supports)
                    .saturating_mul(traversal_bound)
                    .saturating_add(if incidence.is_some() {
                        support_count.saturating_mul(edge_ids.len())
                    } else {
                        0
                    });
                base.saturating_add(state_work.saturating_mul(state_limit))
            });
            let component_budget = component_limit.map(WorkBudget::new);
            let budget = component_budget.as_ref().or(shared_budget);
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
                if budget.is_some_and(WorkBudget::exhausted) {
                    exhausted.set(true);
                }
                return None;
            }
            let closed_faces = face_incidence_counts.as_ref().map(|counts| {
                face_edges
                    .as_ref()
                    .expect("incidence face edges accompany incidence counts")
                    .iter()
                    .zip(counts)
                    .map(|(local, total)| local.len() == *total)
                    .collect::<Vec<_>>()
            });
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
            if !enforce_sparse_endpoint_membership(
                &mut local_domains,
                &local_edges,
                &edge_ids,
                edge_candidates,
                budget,
            ) {
                if budget.is_some_and(WorkBudget::exhausted) {
                    exhausted.set(true);
                }
                return None;
            }
            let mut arc_domains = local_domains.clone();
            let arc_budget = budget.map(|budget| WorkBudget::new(budget.remaining()));
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
                    let work = budget.remaining() - arc_budget.remaining();
                    if !budget.charge_by(work) {
                        exhausted.set(true);
                        return None;
                    }
                }
                local_domains = arc_domains;
            } else {
                if arc_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                    exhausted.set(true);
                }
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
            let mut base_degrees = HashMap::new();
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
                state_limit,
                &mut exhausted,
                &mut base_degrees,
                budget,
            );
            if exhausted {
                if component_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                    if let Some(shared_budget) = shared_budget {
                        shared_budget.exhaust();
                    }
                }
                return None;
            }
            let [local_assignment] = solutions.as_slice() else {
                if solutions.len() > 1 {
                    ambiguous.set(true);
                }
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
        budget: Option<&WorkBudget<'_>>,
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
        budget: Option<&WorkBudget<'_>>,
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
            gaugeable_edges: &HashSet<usize>,
            limit: usize,
            budget: Option<&WorkBudget<'_>>,
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
                    gaugeable_edges,
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
                    gaugeable_edges,
                    limit,
                    budget,
                );
                boundary_directions.pop();
            };
            match (boundary[at].reversed, first) {
                (Some(reversed), _) => advance(reversed, quotient),
                (None, true) if gaugeable_edges.contains(&edge) => advance(false, quotient),
                (None, _) => {
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
        // Fix a new edge's direction only while its two endpoint labels remain
        // exchangeable. Distinct domains or prior quotient merges make the
        // direction observable and require both orientations.
        let mut direction_union = self.union.clone();
        let gaugeable_edges = assignment
            .boundaries
            .iter()
            .flatten()
            .map(|use_| use_.edge)
            .collect::<HashSet<_>>()
            .into_iter()
            .filter(|edge| {
                let Some(right_node) = edge.checked_mul(2).and_then(|node| node.checked_add(1))
                else {
                    return false;
                };
                if right_node >= self.domains.len() {
                    return false;
                }
                let left_node = right_node - 1;
                let left_root = direction_union.find(left_node);
                let right_root = direction_union.find(right_node);
                left_root == right_root
                    || (self.domains[left_root] == self.domains[right_root]
                        && self.members[left_root].as_slice() == [left_node]
                        && self.members[right_root].as_slice() == [right_node])
            })
            .collect::<HashSet<_>>();
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
                        None if oriented.insert(use_.edge)
                            && gaugeable_edges.contains(&use_.edge) =>
                        {
                            (false, None)
                        }
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
                let mut oriented = oriented_edges.clone();
                let uses_gauge =
                    assignment
                        .boundaries
                        .iter()
                        .zip(&directions)
                        .all(|(boundary, directions)| {
                            boundary.iter().zip(directions).all(|(use_, direction)| {
                                let first = oriented.insert(use_.edge);
                                !first
                                    || use_.reversed.is_some()
                                    || !gaugeable_edges.contains(&use_.edge)
                                    || !direction
                            })
                        });
                if !uses_gauge {
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
        let mut seen = HashSet::<MeshOrientationSignature>::new();
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
            &gaugeable_edges,
            limit,
            budget,
        );
        output
    }

    fn assignment_options_for_directions(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        direction_options: &MeshFaceDirectionOptions,
        limit: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Vec<(Vec<Vec<bool>>, Self)> {
        fn edge_start(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge.checked_mul(2)?.checked_add(usize::from(reversed))
        }

        fn edge_end(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(!reversed))
        }

        if limit == 0
            || assignment.boundaries.len() != direction_options.first().map_or(0, Vec::len)
        {
            return Vec::new();
        }
        let work = assignment
            .boundaries
            .iter()
            .map(Vec::len)
            .sum::<usize>()
            .max(1);
        let mut output = Vec::new();
        let mut seen = HashSet::<MeshOrientationSignature>::new();
        for directions in direction_options.iter().take(limit) {
            if directions.len() != assignment.boundaries.len()
                || directions
                    .iter()
                    .zip(&assignment.boundaries)
                    .any(|(directions, boundary)| directions.len() != boundary.len())
            {
                continue;
            }
            if budget.is_some_and(|budget| !budget.charge_by(work)) {
                break;
            }
            let mut quotient = self.clone();
            let merged = assignment.boundaries.iter().zip(directions).all(
                |(boundary, boundary_directions)| {
                    if boundary.is_empty()
                        || boundary
                            .iter()
                            .zip(boundary_directions)
                            .any(|(use_, direction)| {
                                use_.reversed.is_some_and(|required| required != *direction)
                            })
                    {
                        return false;
                    }
                    (0..boundary.len()).all(|index| {
                        let next = (index + 1) % boundary.len();
                        let Some(left_end) = edge_end(boundary[index], boundary_directions[index])
                        else {
                            return false;
                        };
                        let Some(right_start) =
                            edge_start(boundary[next], boundary_directions[next])
                        else {
                            return false;
                        };
                        quotient.merge(left_end, right_start).is_some()
                    })
                },
            );
            if !merged {
                continue;
            }
            let mut signature_quotient = quotient.clone();
            if seen.insert((signature_quotient.signature(), directions.clone())) {
                output.push((directions.clone(), quotient));
            }
        }
        output
    }

    fn merge_label_directions_in_place(
        &mut self,
        assignment: &MeshFaceBoundaryAssignment,
        label_directions: &[Vec<bool>],
        edge_orientations: &[Option<bool>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<Vec<Vec<bool>>> {
        fn edge_start(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge.checked_mul(2)?.checked_add(usize::from(reversed))
        }

        fn edge_end(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(!reversed))
        }

        if assignment.boundaries.len() != label_directions.len()
            || label_directions
                .iter()
                .zip(&assignment.boundaries)
                .any(|(directions, boundary)| directions.len() != boundary.len())
        {
            return None;
        }
        let work = assignment
            .boundaries
            .iter()
            .map(Vec::len)
            .sum::<usize>()
            .max(1);
        if budget.is_some_and(|budget| !budget.charge_by(work)) {
            return None;
        }
        let directions = assignment
            .boundaries
            .iter()
            .zip(label_directions)
            .map(|(boundary, labels)| {
                boundary
                    .iter()
                    .zip(labels)
                    .map(|(use_, &label_direction)| {
                        let orientation = edge_orientations.get(use_.edge)?.as_ref().copied()?;
                        let direction = orientation ^ label_direction;
                        use_.reversed
                            .is_none_or(|required| required == direction)
                            .then_some(direction)
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .collect::<Option<Vec<_>>>()?;
        let merged =
            assignment
                .boundaries
                .iter()
                .zip(&directions)
                .all(|(boundary, boundary_directions)| {
                    if boundary.is_empty() {
                        return false;
                    }
                    (0..boundary.len()).all(|index| {
                        let next = (index + 1) % boundary.len();
                        let Some(left_end) = edge_end(boundary[index], boundary_directions[index])
                        else {
                            return false;
                        };
                        let Some(right_start) =
                            edge_start(boundary[next], boundary_directions[next])
                        else {
                            return false;
                        };
                        self.merge(left_end, right_start).is_some()
                    })
                });
        if !merged {
            return None;
        }
        Some(directions)
    }

    fn assignment_option_for_label_directions(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        label_directions: &[Vec<bool>],
        edge_orientations: &[Option<bool>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<(Vec<Vec<bool>>, Self)> {
        let mut quotient = self.clone();
        let directions = quotient.merge_label_directions_in_place(
            assignment,
            label_directions,
            edge_orientations,
            budget,
        )?;
        Some((directions, quotient))
    }

    pub(crate) fn point_assignment(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<HashMap<usize, usize>> {
        match self.point_assignments_with_budget(point_count, edge_candidates, 2, budget) {
            PointAssignmentOutcome::Complete(mut solutions) => {
                (solutions.len() == 1).then(|| solutions.remove(0))
            }
            PointAssignmentOutcome::Exhausted => None,
        }
    }

    pub(crate) fn point_assignment_exists(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> bool {
        matches!(
            self.point_assignments_with_budget(point_count, edge_candidates, 1, budget),
            PointAssignmentOutcome::Complete(solutions) if !solutions.is_empty()
        )
    }

    fn point_assignments_with_budget(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        solution_limit: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> PointAssignmentOutcome {
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
            budget: Option<&WorkBudget<'_>>,
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
            return PointAssignmentOutcome::Complete(Vec::new());
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
            return PointAssignmentOutcome::Complete(Vec::new());
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
        if budget.is_some_and(WorkBudget::exhausted) {
            PointAssignmentOutcome::Exhausted
        } else {
            PointAssignmentOutcome::Complete(
                solutions
                    .into_iter()
                    .map(|solution| roots.iter().copied().zip(solution).collect())
                    .collect(),
            )
        }
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
    budget: &WorkBudget<'_>,
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
    budget: &WorkBudget<'_>,
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
        budget: &WorkBudget<'_>,
    ) {
        if output.len() >= limit || budget.exhausted() {
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
                if output.len() >= limit || budget.exhausted() {
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
        budget: &WorkBudget<'_>,
    ) {
        if output.len() >= limit || budget.exhausted() {
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
            if output.len() >= limit || budget.exhausted() {
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
    (!budget.exhausted()).then_some(DeferredFaceQuotientOptions {
        alternatives: output,
        base_nodes,
    })
}

fn propagate_common_deferred_quotients(
    mut options: DeferredFaceQuotientOptions,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
    budget: &WorkBudget<'_>,
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
    budget: &WorkBudget<'_>,
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
                    let width = directions[(index + 1) % boundary.len()].len();
                    let height = directions[index].len();
                    let row = alloc_filled(width, false, "catia_boundary_dir_row").ok()?;
                    alloc_filled(height, row, "catia_boundary_dir_grid").ok()
                })
                .collect::<Option<Vec<_>>>()?;
            for first in 0..directions[0].len() {
                let mut forward = directions
                    .iter()
                    .map(|states| alloc_filled(states.len(), false, "catia_boundary_forward").ok())
                    .collect::<Option<Vec<_>>>()?;
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
                    .map(|states| alloc_filled(states.len(), false, "catia_boundary_backward").ok())
                    .collect::<Option<Vec<_>>>()?;
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
    budget: &WorkBudget<'_>,
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
            let face_budget = WorkBudget::new(match domain {
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
            if face_budget.exhausted() {
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
                if face_budget.exhausted() {
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
    budget: &WorkBudget<'_>,
) -> Option<Vec<MeshFaceBoundaryAssignment>> {
    struct Search<'a> {
        edges: &'a [usize],
        compatible: &'a HashSet<(usize, usize)>,
        limit: usize,
        budget: &'a WorkBudget<'a>,
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
    budget: &WorkBudget<'_>,
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
        if budget.exhausted() {
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
        let budget = WorkBudget::new(MAX_COMPONENT_OPERATIONS);
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
type MeshFaceDirectionOptions = Vec<Vec<Vec<bool>>>;
pub(crate) type MeshEndpointPair = (usize, [usize; 2]);
pub(crate) type MeshEndpointSolutionFilter<'a> = &'a dyn Fn(&[MeshEndpointPair]) -> bool;
type MeshPartialEndpointSolutionFilter<'a> = &'a dyn Fn(&[Option<[usize; 2]>]) -> bool;
#[derive(Clone, Copy)]
pub(crate) struct MeshPartialEndpointConstraint<'a> {
    pub(crate) active_edges: &'a [bool],
    pub(crate) coupled_edges: &'a [bool],
    pub(crate) assignment_predecessors: Option<&'a [Option<usize>]>,
    pub(crate) assignment_dependencies: Option<&'a [Vec<usize>]>,
    pub(crate) valid: MeshPartialEndpointSolutionFilter<'a>,
}
type MeshFaceEndpointConfiguration = Vec<MeshEndpointPair>;
pub(crate) type MeshFaceEndpointConfigurations = Vec<MeshFaceEndpointConfiguration>;
type MeshQuotientSignature = Vec<(Vec<usize>, Vec<usize>)>;
type MeshSelectionStateSignature = (
    bool,
    Vec<MeshFaceSelection>,
    MeshQuotientSignature,
    Vec<Option<bool>>,
);
type MeshOrientationSignature = (MeshQuotientSignature, Vec<Vec<bool>>);
type MeshFaceEquationCache = RefCell<HashMap<(usize, MeshQuotientSignature), Vec<[usize; 2]>>>;

/// Search-order information for same-class rows with identical endpoint
/// domains. The dependency order reduces branching overhead without assigning
/// endpoint values to row positions.
struct EdgeClassSearchConstraint {
    active: Vec<bool>,
    ordered: Vec<(usize, usize)>,
}

fn edge_class_search_constraint(
    edge_classes: &[usize],
    choices: &[Vec<[usize; 2]>],
) -> Option<EdgeClassSearchConstraint> {
    if edge_classes.len() != choices.len() {
        return None;
    }
    let normalized = choices
        .iter()
        .map(|pairs| {
            let mut pairs = pairs
                .iter()
                .copied()
                .map(|mut pair| {
                    pair.sort_unstable();
                    pair
                })
                .collect::<Vec<_>>();
            pairs.sort_unstable();
            pairs.dedup();
            pairs
        })
        .collect::<Vec<_>>();
    let mut active = alloc_filled(choices.len(), false, "catia_edge_class_active").ok()?;
    let mut ordered = Vec::new();
    for left in 0..choices.len() {
        for right in left + 1..choices.len() {
            if edge_classes[left] != edge_classes[right]
                || normalized[left].len() < 2
                || normalized[left] != normalized[right]
            {
                continue;
            }
            active[left] = true;
            active[right] = true;
            ordered.push((left, right));
        }
    }
    Some(EdgeClassSearchConstraint { active, ordered })
}

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
    pub(crate) candidate_gauge: Option<MeshCandidateGauge<'a>>,
    pub(crate) port_identities: Option<&'a [[u32; 2]]>,
    pub(crate) fixed_face_directions: Vec<Option<MeshFaceDirectionOptions>>,
    pub(crate) fixed_edge_orientations: Vec<Option<bool>>,
    pub(crate) edge_has_fixed_direction: Vec<bool>,
    pub(crate) selected: Vec<MeshFaceSelection>,
    pub(crate) visited_states: HashSet<MeshSelectionStateSignature>,
    pub(crate) solution: Option<(StandardTopology, Vec<usize>)>,
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

    let budget = WorkBudget::new(limit);
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
    (!budget.exhausted()).then_some(choices)
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

pub(crate) fn mesh_assignment_endpoint_cycles_viable_by<'a>(
    assignment: &MeshFaceBoundaryAssignment,
    budget: Option<&WorkBudget<'_>>,
    candidates: impl Fn(usize) -> Option<MeshEndpointCandidates<'a>>,
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Option<bool> {
    const MAX_LOCAL_ENDPOINT_STATES: usize = 65_536;

    fn endpoint_adjacency(
        candidates: impl IntoIterator<Item = [usize; 2]>,
        allowed: impl Fn([usize; 2]) -> bool,
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<HashMap<usize, Vec<usize>>> {
        let mut adjacency = HashMap::<usize, Vec<usize>>::new();
        let mut count = 0usize;
        for pair @ [left, right] in candidates {
            if budget.is_some_and(|budget| !budget.charge()) {
                return None;
            }
            count = count.checked_add(1)?;
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
            neighbors.sort_unstable();
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
            let adjacency = match candidates(use_.edge)? {
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
            prepared.insert(use_.edge, adjacency?);
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

pub(crate) fn mesh_assignment_endpoint_cycles_viable_where(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&WorkBudget<'_>>,
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

pub(crate) fn mesh_assignment_endpoint_cycle_support_by<'a>(
    assignment: &MeshFaceBoundaryAssignment,
    budget: Option<&WorkBudget<'_>>,
    candidates: impl Fn(usize) -> Option<MeshEndpointCandidates<'a>>,
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Option<HashMap<usize, HashSet<[usize; 2]>>> {
    const MAX_LOCAL_ENDPOINT_STATES: usize = 65_536;

    type EndpointRelation = BTreeMap<usize, BTreeSet<usize>>;

    fn compose_relations(
        left: &EndpointRelation,
        right: &EndpointRelation,
        budget: Option<&WorkBudget<'_>>,
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
                        state_count = state_count.checked_add(1)?;
                        if state_count > MAX_LOCAL_ENDPOINT_STATES {
                            return None;
                        }
                    }
                }
            }
        }
        Some(composed)
    }

    fn identity_relation(points: &BTreeSet<usize>) -> EndpointRelation {
        points
            .iter()
            .map(|&point| (point, BTreeSet::from([point])))
            .collect()
    }

    let charge = || budget.is_none_or(WorkBudget::charge);
    let mut assignment_support = HashMap::<usize, HashSet<[usize; 2]>>::new();
    for boundary in &assignment.boundaries {
        if boundary.is_empty() {
            return Some(HashMap::new());
        }
        let mut points = BTreeSet::new();
        let mut layers = Vec::<(usize, Vec<[usize; 2]>, EndpointRelation)>::new();
        for use_ in boundary {
            let values = match candidates(use_.edge)? {
                MeshEndpointCandidates::Explicit(values) => values.to_vec(),
                MeshEndpointCandidates::Implicit(values) => values
                    .take(MAX_LOCAL_ENDPOINT_STATES + 1)
                    .collect::<Vec<_>>(),
                MeshEndpointCandidates::Selected(value) => vec![value],
            };
            if values.len() > MAX_LOCAL_ENDPOINT_STATES {
                return None;
            }
            let mut retained = Vec::new();
            let mut relation = EndpointRelation::new();
            for mut pair in values {
                pair.sort_unstable();
                if !allowed(use_.edge, pair) {
                    continue;
                }
                retained.push(pair);
                points.extend(pair);
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
            retained.sort_unstable();
            retained.dedup();
            if retained.is_empty() {
                return Some(HashMap::new());
            }
            layers.push((use_.edge, retained, relation));
        }
        if points.len() > MAX_LOCAL_ENDPOINT_STATES {
            return None;
        }
        let identity = identity_relation(&points);
        let mut prefixes = Vec::with_capacity(layers.len() + 1);
        prefixes.push(identity.clone());
        for (_, _, relation) in &layers {
            let composed =
                compose_relations(prefixes.last().expect("prefix identity"), relation, budget)?;
            prefixes.push(composed);
        }
        let mut suffixes = alloc_filled(
            layers.len() + 1,
            EndpointRelation::new(),
            "catia_endpoint_suffixes",
        )
        .ok()?;
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
            return Some(HashMap::new());
        }
        for (edge, supported) in boundary_support {
            assignment_support
                .entry(edge)
                .and_modify(|retained| retained.retain(|pair| supported.contains(pair)))
                .or_insert(supported);
        }
        if assignment_support.values().any(HashSet::is_empty) {
            return Some(HashMap::new());
        }
    }
    Some(assignment_support)
}

fn mesh_assignment_endpoint_cycles_viable_with(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
    required: Option<(usize, [usize; 2])>,
    budget: Option<&WorkBudget<'_>>,
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

pub(crate) fn mesh_face_endpoint_configurations(
    assignments: &[MeshFaceBoundaryAssignment],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[Option<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> Option<MeshFaceEndpointConfigurations> {
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
        budget: &WorkBudget<'_>,
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
                    if work > MAX_FACE_ENDPOINT_CONFIGURATION_WORK || !budget.charge() {
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

/// Return whether an unordered endpoint configuration can close every
/// boundary cycle of one face assignment. Configuration pairs are normalized
/// by `mesh_face_endpoint_configurations`; the checks below still compare
/// them as unordered pairs so callers cannot depend on that representation.
fn endpoint_configuration_boundary_cycle_viable(
    boundary: &[MeshBoundaryEdgeCandidate],
    pairs: &HashMap<usize, [usize; 2]>,
) -> Option<bool> {
    if boundary.is_empty() {
        return None;
    }
    let first = boundary.first()?;
    let first_pair = *pairs.get(&first.edge)?;
    let first_directions = if first_pair[0] == first_pair[1] {
        vec![false]
    } else {
        vec![false, true]
    };
    let mut states = first_directions
        .into_iter()
        .map(|direction| {
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
            (start, current)
        })
        .collect::<Vec<_>>();
    for use_ in &boundary[1..] {
        let pair = *pairs.get(&use_.edge)?;
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

fn endpoint_configuration_cycles_viable(
    assignment: &MeshFaceBoundaryAssignment,
    configuration: &MeshFaceEndpointConfiguration,
) -> Option<bool> {
    let pairs = configuration.iter().copied().collect::<HashMap<_, _>>();
    if pairs.len() != configuration.len() {
        return None;
    }
    Some(
        !assignment.boundaries.is_empty()
            && assignment.boundaries.iter().all(|boundary| {
                endpoint_configuration_boundary_cycle_viable(boundary, &pairs)
                    .is_some_and(|viable| viable)
            }),
    )
}

fn endpoint_configuration_for_assignment(
    assignment: &MeshFaceBoundaryAssignment,
    edge_pairs: &[[usize; 2]],
) -> Option<MeshFaceEndpointConfiguration> {
    let mut pairs = HashMap::<usize, [usize; 2]>::new();
    for use_ in assignment.boundaries.iter().flatten() {
        let mut pair = *edge_pairs.get(use_.edge)?;
        pair.sort_unstable();
        match pairs.get(&use_.edge) {
            Some(previous) if *previous != pair => return None,
            Some(_) => {}
            None => {
                pairs.insert(use_.edge, pair);
            }
        }
    }
    let mut configuration = pairs.into_iter().collect::<Vec<_>>();
    configuration.sort_unstable();
    Some(configuration)
}

fn endpoint_configuration_boundary_directions(
    boundary: &[MeshBoundaryEdgeCandidate],
    pairs: &HashMap<usize, [usize; 2]>,
) -> Option<Vec<Vec<bool>>> {
    if boundary.is_empty() {
        return None;
    }
    let first = boundary.first()?;
    let first_pair = *pairs.get(&first.edge)?;
    let first_directions = if first_pair[0] == first_pair[1] {
        vec![false]
    } else {
        vec![false, true]
    };
    let mut states = first_directions
        .into_iter()
        .map(|direction| {
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
            (start, current, vec![direction])
        })
        .collect::<Vec<_>>();
    for use_ in &boundary[1..] {
        let pair = *pairs.get(&use_.edge)?;
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
                    return None;
                }
            }
        }
        states = next;
        if states.is_empty() {
            return Some(Vec::new());
        }
    }
    Some(
        states
            .into_iter()
            .filter_map(|(start, current, directions)| (start == current).then_some(directions))
            .collect(),
    )
}

fn endpoint_configuration_directions(
    assignment: &MeshFaceBoundaryAssignment,
    configuration: &MeshFaceEndpointConfiguration,
) -> Option<MeshFaceDirectionOptions> {
    let pairs = configuration.iter().copied().collect::<HashMap<_, _>>();
    if pairs.len() != configuration.len() {
        return None;
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
                    return None;
                }
            }
        }
        alternatives = next;
        if alternatives.is_empty() {
            break;
        }
    }
    Some(alternatives)
}

#[derive(Clone)]
pub(crate) struct MeshEndpointRelationChoice {
    pub(crate) id: usize,
    pub(crate) assignments: Vec<usize>,
    pub(crate) edge_pairs: MeshFaceEndpointConfiguration,
}
type MeshEndpointRelationSelections = Vec<Vec<usize>>;
pub(crate) type MeshEndpointRelationStateSignature = (
    Vec<Option<[usize; 2]>>,
    Vec<Vec<(Vec<usize>, Vec<(usize, [usize; 2])>)>>,
);
type MeshEndpointSolutionPredicate<'a> = dyn Fn(&[Option<[usize; 2]>]) -> bool + 'a;
type MeshFixedDirectionOption = (Vec<Vec<bool>>, MeshQuotient, Vec<Option<bool>>);

fn raw_endpoint_relation_state_signature(
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
) -> MeshEndpointRelationStateSignature {
    let assigned = assigned
        .iter()
        .copied()
        .map(|pair| {
            pair.map(|mut pair| {
                pair.sort_unstable();
                pair
            })
        })
        .collect();
    let domains = domains
        .iter()
        .map(|choices| {
            let mut choices = choices
                .iter()
                .map(|choice| {
                    let mut assignments = choice.assignments.clone();
                    assignments.sort_unstable();
                    assignments.dedup();
                    let mut edge_pairs = choice
                        .edge_pairs
                        .iter()
                        .map(|&(edge, mut pair)| {
                            pair.sort_unstable();
                            (edge, pair)
                        })
                        .collect::<Vec<_>>();
                    edge_pairs.sort_unstable();
                    (assignments, edge_pairs)
                })
                .collect::<Vec<_>>();
            choices.sort_unstable();
            choices
        })
        .collect();
    (assigned, domains)
}

fn endpoint_relation_state_signature(
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Option<MeshEndpointRelationStateSignature> {
    candidate_gauge.map_or_else(
        || Some(raw_endpoint_relation_state_signature(domains, assigned)),
        |gauge| canonicalize_endpoint_relation_state(domains, assigned, gauge),
    )
}

/// Return the edge-pair superset admitted by the surviving relation domains.
/// A relation branch can only select pairs from this set, so coordinate
/// infeasibility of the superset is a sound branch rejection.
fn relation_coordinate_candidate_domains(
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
    base_candidates: &[Vec<[usize; 2]>],
) -> Option<Vec<Vec<[usize; 2]>>> {
    if assigned.len() != base_candidates.len() {
        return None;
    }
    let has_unknown_choice = domains
        .iter()
        .flatten()
        .any(|choice| choice.edge_pairs.is_empty());
    let mut candidates = base_candidates.to_vec();
    let mut possible = (0..base_candidates.len())
        .map(|_| Vec::<[usize; 2]>::new())
        .collect::<Vec<_>>();
    if !has_unknown_choice {
        for choice in domains.iter().flatten() {
            for &(edge, pair) in &choice.edge_pairs {
                possible.get_mut(edge)?.push(pair);
            }
        }
    }
    for (edge, (assigned, base)) in assigned.iter().zip(base_candidates).enumerate() {
        if let Some(pair) = assigned {
            candidates[edge].retain(|candidate| same_unordered_pair(*candidate, *pair));
        } else if !has_unknown_choice && !possible[edge].is_empty() {
            candidates[edge].retain(|candidate| {
                possible[edge]
                    .iter()
                    .any(|possible| same_unordered_pair(*candidate, *possible))
            });
        } else {
            candidates[edge].clone_from(base);
        }
        if candidates[edge].is_empty() {
            return None;
        }
    }
    Some(candidates)
}

#[derive(Clone)]
struct MeshEndpointRelationArc {
    neighbor: usize,
    supports: Vec<Vec<u64>>,
}

struct MeshEndpointRelationConstraints {
    arcs: Vec<Vec<MeshEndpointRelationArc>>,
    incoming: Vec<Vec<(usize, usize)>>,
    choice_counts: Vec<usize>,
}

fn canonical_mesh_boundary_directions(directions: &[Vec<bool>]) -> Vec<Vec<bool>> {
    directions
        .iter()
        .map(|boundary| {
            let complement = boundary
                .iter()
                .map(|direction| !direction)
                .collect::<Vec<_>>();
            if complement < *boundary {
                complement
            } else {
                boundary.clone()
            }
        })
        .collect()
}

fn canonical_endpoint_relation_key(
    choice: &MeshEndpointRelationChoice,
    edges: &[usize],
) -> Option<Vec<[usize; 2]>> {
    edges
        .iter()
        .map(|&edge| {
            choice
                .edge_pairs
                .iter()
                .find_map(|&(candidate, pair)| (candidate == edge).then_some(pair))
                .map(|mut pair| {
                    if pair[1] < pair[0] {
                        pair.swap(0, 1);
                    }
                    pair
                })
        })
        .collect()
}

fn build_endpoint_relation_constraints(
    domains: &[Vec<MeshEndpointRelationChoice>],
    budget: &WorkBudget<'_>,
) -> Option<MeshEndpointRelationConstraints> {
    let mut shared_edges = BTreeMap::<(usize, usize), Vec<usize>>::new();
    let mut edge_faces = HashMap::<usize, BTreeSet<usize>>::new();
    for (face, choices) in domains.iter().enumerate() {
        for choice in choices {
            for &(edge, _) in &choice.edge_pairs {
                edge_faces.entry(edge).or_default().insert(face);
            }
        }
    }
    for (edge, faces) in edge_faces {
        let faces = faces.into_iter().collect::<Vec<_>>();
        for (left_index, &left) in faces.iter().enumerate() {
            for &right in &faces[left_index + 1..] {
                shared_edges.entry((left, right)).or_default().push(edge);
                shared_edges.entry((right, left)).or_default().push(edge);
            }
        }
    }

    let mut arcs = (0..domains.len())
        .map(|_| Vec::<MeshEndpointRelationArc>::new())
        .collect::<Vec<_>>();
    let mut incoming = (0..domains.len())
        .map(|_| Vec::<(usize, usize)>::new())
        .collect::<Vec<_>>();
    let choice_counts = domains.iter().map(Vec::len).collect::<Vec<_>>();
    for ((face, neighbor), edges) in shared_edges {
        let left_complete = domains[face].iter().all(|choice| {
            edges.iter().all(|edge| {
                choice
                    .edge_pairs
                    .iter()
                    .any(|(candidate, _)| candidate == edge)
            })
        });
        let right_complete = domains[neighbor].iter().all(|choice| {
            edges.iter().all(|edge| {
                choice
                    .edge_pairs
                    .iter()
                    .any(|(candidate, _)| candidate == edge)
            })
        });
        let supports = if left_complete && right_complete {
            let index_work = domains[face]
                .len()
                .saturating_add(domains[neighbor].len())
                .max(1);
            if !budget.charge_by(index_work) {
                return None;
            }
            let mut index = HashMap::<Vec<[usize; 2]>, Vec<usize>>::new();
            for choice in &domains[neighbor] {
                let key = canonical_endpoint_relation_key(choice, &edges)?;
                index.entry(key).or_default().push(choice.id);
            }
            domains[face]
                .iter()
                .map(|choice| {
                    let key = canonical_endpoint_relation_key(choice, &edges)
                        .expect("complete relation choice contains shared edge");
                    let mut mask = alloc_filled(
                        (domains[neighbor].len().saturating_add(63) / 64).max(1),
                        0u64,
                        "catia_endpoint_relation_support_mask",
                    )
                    .ok()?;
                    for &other in index.get(&key).into_iter().flatten() {
                        mask[other / 64] |= 1u64 << (other % 64);
                    }
                    Some(mask)
                })
                .collect::<Option<Vec<_>>>()?
        } else {
            let comparison_work = domains[face]
                .len()
                .saturating_mul(domains[neighbor].len())
                .max(1);
            if !budget.charge_by(comparison_work) {
                return None;
            }
            domains[face]
                .iter()
                .map(|choice| {
                    let mut mask = alloc_filled(
                        (domains[neighbor].len().saturating_add(63) / 64).max(1),
                        0u64,
                        "catia_endpoint_relation_support_mask",
                    )
                    .ok()?;
                    for other in &domains[neighbor] {
                        let compatible = edges.iter().all(|&edge| {
                            let left = choice
                                .edge_pairs
                                .iter()
                                .find_map(|&(candidate, pair)| (candidate == edge).then_some(pair));
                            let right = other
                                .edge_pairs
                                .iter()
                                .find_map(|&(candidate, pair)| (candidate == edge).then_some(pair));
                            left.zip(right)
                                .is_none_or(|(left, right)| same_unordered_pair(left, right))
                        });
                        if compatible {
                            mask[other.id / 64] |= 1u64 << (other.id % 64);
                        }
                    }
                    Some(mask)
                })
                .collect::<Option<Vec<_>>>()?
        };
        let arc_index = arcs[face].len();
        arcs[face].push(MeshEndpointRelationArc { neighbor, supports });
        incoming[neighbor].push((face, arc_index));
    }
    Some(MeshEndpointRelationConstraints {
        arcs,
        incoming,
        choice_counts,
    })
}

fn propagate_endpoint_relation_domains(
    domains: &mut [Vec<MeshEndpointRelationChoice>],
    assigned: &mut [Option<[usize; 2]>],
    constraints: &MeshEndpointRelationConstraints,
    budget: &WorkBudget<'_>,
) -> bool {
    let mut dirty_faces = domains
        .iter()
        .enumerate()
        .filter_map(|(face, choices)| {
            (choices.len() != constraints.choice_counts[face]).then_some(face)
        })
        .collect::<Vec<_>>();
    let mut first_pass = true;
    loop {
        let mut changed = false;
        for (face, choices) in domains.iter_mut().enumerate() {
            if !budget.charge_by(choices.len().max(1)) {
                return false;
            }
            let before = choices.len();
            choices.retain(|choice| {
                choice.edge_pairs.iter().all(|&(edge, pair)| {
                    assigned[edge].is_none_or(|selected| same_unordered_pair(selected, pair))
                })
            });
            if choices.is_empty() {
                return false;
            }
            if choices.len() != before {
                dirty_faces.push(face);
                changed = true;
            }
        }

        let Some(mut active) = constraints
            .choice_counts
            .iter()
            .map(|&choice_count| {
                alloc_filled(
                    (choice_count.saturating_add(63) / 64).max(1),
                    0u64,
                    "catia_endpoint_relation_active_mask",
                )
                .ok()
            })
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        for (face, choices) in domains.iter().enumerate() {
            for choice in choices {
                let Some(active_word) = active[face].get_mut(choice.id / 64) else {
                    return false;
                };
                *active_word |= 1u64 << (choice.id % 64);
            }
        }
        let mut queue = if first_pass && dirty_faces.is_empty() {
            constraints
                .arcs
                .iter()
                .enumerate()
                .flat_map(|(face, arcs)| (0..arcs.len()).map(move |arc| (face, arc)))
                .collect::<VecDeque<_>>()
        } else {
            dirty_faces
                .drain(..)
                .flat_map(|face| constraints.incoming[face].iter().copied())
                .collect::<VecDeque<_>>()
        };
        first_pass = false;
        while let Some((face, arc_index)) = queue.pop_front() {
            let arc = &constraints.arcs[face][arc_index];
            let neighbor_active = &active[arc.neighbor];
            let before = domains[face].len();
            domains[face].retain(|choice| {
                let Some(supports) = arc.supports.get(choice.id) else {
                    return false;
                };
                if !budget.charge_by(supports.len().max(1)) {
                    return false;
                }
                supports
                    .iter()
                    .zip(neighbor_active)
                    .any(|(supported, active)| supported & active != 0)
            });
            if budget.exhausted() || domains[face].is_empty() {
                return false;
            }
            if domains[face].len() == before {
                continue;
            }
            active[face].fill(0);
            for choice in &domains[face] {
                active[face][choice.id / 64] |= 1u64 << (choice.id % 64);
            }
            changed = true;
            queue.extend(
                constraints.incoming[face]
                    .iter()
                    .copied()
                    .filter(|&(source, _)| source != arc.neighbor),
            );
        }
        // A pair present with one value in every surviving choice of one face
        // is a forced edge relation, even when the face still has assignment
        // or boundary-direction alternatives. Record it before branching on
        // those independent alternatives.
        for choices in domains.iter() {
            if !budget.charge_by(choices.len().max(1)) {
                return false;
            }
            let choice_count = choices.len();
            let mut pairs = HashMap::<usize, HashSet<[usize; 2]>>::new();
            let mut counts = HashMap::<usize, usize>::new();
            for choice in choices {
                for &(edge, pair) in &choice.edge_pairs {
                    pairs.entry(edge).or_default().insert(pair);
                    *counts.entry(edge).or_default() += 1;
                }
            }
            for (edge, values) in pairs {
                if counts.get(&edge) != Some(&choice_count) || values.len() != 1 {
                    continue;
                }
                let Some(&pair) = values.iter().next() else {
                    continue;
                };
                match assigned[edge] {
                    Some(selected) if !same_unordered_pair(selected, pair) => return false,
                    Some(_) => {}
                    None => {
                        assigned[edge] = Some(pair);
                        changed = true;
                    }
                }
            }
        }
        for choices in domains.iter() {
            if choices.len() != 1 {
                continue;
            }
            for &(edge, pair) in &choices[0].edge_pairs {
                match assigned[edge] {
                    Some(selected) if !same_unordered_pair(selected, pair) => return false,
                    Some(_) => {}
                    None => {
                        assigned[edge] = Some(pair);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            return true;
        }
    }
}

// The recursive walk keeps branch-owned domains and shared memo state explicit;
// a context object would hide which values are cloned for each branch.
#[allow(clippy::too_many_arguments)]
fn walk_endpoint_relation_domains<F>(
    domains: Vec<Vec<MeshEndpointRelationChoice>>,
    face_assignments: &[Vec<MeshFaceBoundaryAssignment>],
    assigned: Vec<Option<[usize; 2]>>,
    constraints: &MeshEndpointRelationConstraints,
    point_count: usize,
    budget: &WorkBudget<'_>,
    state_memo: &mut HashSet<MeshEndpointRelationStateSignature>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
    priority_edges: Option<&[bool]>,
    partial_solution_valid: Option<&MeshEndpointSolutionPredicate<'_>>,
    coordinate_domains: Option<&MeshCoordinateRootDomains>,
    coordinate_budget: Option<&WorkBudget<'_>>,
    evaluate: &mut F,
) -> bool
where
    F: FnMut(MeshEndpointRelationSelections, Vec<[usize; 2]>) -> bool,
{
    if budget.exhausted() || !budget.charge() {
        return true;
    }
    let mut domains = domains;
    let mut assigned = assigned;
    let propagated = domains.iter().all(|choices| choices.len() == 1)
        || propagate_endpoint_relation_domains(&mut domains, &mut assigned, constraints, budget);
    if !propagated {
        return false;
    }
    // The partial predicate is monotone by contract: it can reject only
    // assignments that no completion can repair. Apply it as soon as
    // propagation fixes endpoint pairs, before branching over domains.
    if let Some(valid) = partial_solution_valid {
        if !valid(&assigned) {
            return false;
        }
    }
    // The final coordinate binding is surjective: every coordinate row must
    // occur in the selected endpoint pairs. When every surviving relation
    // choice has an explicit edge configuration, their point union is an
    // upper bound for every completion of this branch.
    if domains
        .iter()
        .flatten()
        .all(|choice| !choice.edge_pairs.is_empty())
    {
        let mut possible_points = assigned
            .iter()
            .flatten()
            .flatten()
            .copied()
            .collect::<HashSet<_>>();
        possible_points.extend(
            domains
                .iter()
                .flatten()
                .flat_map(|choice| choice.edge_pairs.iter().flat_map(|(_, pair)| pair))
                .copied(),
        );
        if possible_points.len() < point_count {
            return false;
        }
    }
    if let (Some(coordinate_domains), Some(coordinate_budget)) =
        (coordinate_domains, coordinate_budget)
    {
        if !coordinate_budget.exhausted() {
            let Some(candidates) = relation_coordinate_candidate_domains(
                &domains,
                &assigned,
                coordinate_domains.edge_candidates(),
            ) else {
                return false;
            };
            if coordinate_domains
                .refine_candidates(&candidates, Some(coordinate_budget))
                .is_none()
                && !coordinate_budget.exhausted()
            {
                return false;
            }
        }
    }
    if state_memo.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
        const MAX_GAUGE_STATE_ALTERNATIVES: usize = 512;
        let alternative_count = domains.iter().map(Vec::len).sum::<usize>();
        let signature =
            if candidate_gauge.is_some() && alternative_count <= MAX_GAUGE_STATE_ALTERNATIVES {
                endpoint_relation_state_signature(&domains, &assigned, candidate_gauge)
            } else {
                Some(raw_endpoint_relation_state_signature(&domains, &assigned))
            };
        if let Some(signature) = signature {
            if !state_memo.insert(signature) {
                return false;
            }
        }
    }
    // Visit a face touching a monotone preference-dependent edge before an
    // unrelated face. Within each tier use minimum remaining values first;
    // relation degree is only a deterministic tie breaker.
    let Some((face, choices)) = domains
        .iter()
        .enumerate()
        .filter(|(_, choices)| choices.len() > 1)
        .min_by_key(|(face, choices)| {
            let priority_count = priority_edges.map_or(0, |edges| {
                choices
                    .iter()
                    .flat_map(|choice| choice.edge_pairs.iter().map(|(edge, _)| *edge))
                    .filter(|edge| edges.get(*edge).copied().unwrap_or(false))
                    .collect::<HashSet<_>>()
                    .len()
            });
            (
                priority_count == 0,
                std::cmp::Reverse(priority_count),
                choices.len(),
                std::cmp::Reverse(constraints.arcs.get(*face).map_or(0, Vec::len)),
                *face,
            )
        })
    else {
        let mut edge_pairs = assigned;
        for choice in domains.iter().filter_map(|choices| choices.first()) {
            for &(edge, pair) in &choice.edge_pairs {
                match edge_pairs[edge] {
                    Some(selected) if !same_unordered_pair(selected, pair) => return false,
                    Some(_) => {}
                    None => edge_pairs[edge] = Some(pair),
                }
            }
        }
        let Some(edge_pairs) = edge_pairs.into_iter().collect::<Option<Vec<_>>>() else {
            return false;
        };
        if let Some(gauge) = candidate_gauge {
            let Some(canonical_pairs) = canonicalize_coordinate_endpoint_pairs(&edge_pairs, gauge)
            else {
                return false;
            };
            if canonical_pairs != edge_pairs {
                return false;
            }
        }
        let selections = domains
            .iter()
            .enumerate()
            .map(|(face, choices)| {
                let choice = choices.first()?;
                if choice.assignments != [usize::MAX] {
                    return Some(choice.assignments.clone());
                }
                let viable = face_assignments[face]
                    .iter()
                    .enumerate()
                    .filter_map(|(assignment, assignment_value)| {
                        let configuration =
                            endpoint_configuration_for_assignment(assignment_value, &edge_pairs)?;
                        endpoint_configuration_cycles_viable(assignment_value, &configuration)
                            .is_some_and(|viable| viable)
                            .then_some(assignment)
                    })
                    .collect::<Vec<_>>();
                (!viable.is_empty()).then_some(viable)
            })
            .collect::<Option<Vec<_>>>();
        let Some(selections) = selections else {
            return false;
        };
        return evaluate(selections, edge_pairs);
    };
    let assigned_points = assigned
        .iter()
        .flatten()
        .flatten()
        .copied()
        .collect::<HashSet<_>>();
    let mut point_support = HashMap::<usize, usize>::new();
    for choices in &domains {
        for choice in choices {
            let points = choice
                .edge_pairs
                .iter()
                .flat_map(|(_, pair)| pair)
                .copied()
                .collect::<HashSet<_>>();
            for point in points {
                *point_support.entry(point).or_default() += 1;
            }
        }
    }
    let mut branch_choices = choices.clone();
    branch_choices.sort_unstable_by(|left, right| {
        let score = |choice: &MeshEndpointRelationChoice| {
            choice
                .edge_pairs
                .iter()
                .flat_map(|(_, pair)| pair)
                .copied()
                .collect::<HashSet<_>>()
                .into_iter()
                .filter(|point| !assigned_points.contains(point))
                .map(|point| {
                    point_count
                        .saturating_sub(point_support.get(&point).copied().unwrap_or(0))
                        .saturating_add(1)
                })
                .sum::<usize>()
        };
        score(right)
            .cmp(&score(left))
            .then_with(|| left.id.cmp(&right.id))
    });
    for choice in branch_choices {
        if budget.exhausted() {
            return true;
        }
        let mut branch = domains.clone();
        branch[face] = vec![choice];
        if walk_endpoint_relation_domains(
            branch,
            face_assignments,
            assigned.clone(),
            constraints,
            point_count,
            budget,
            state_memo,
            candidate_gauge,
            priority_edges,
            partial_solution_valid,
            coordinate_domains,
            coordinate_budget,
            evaluate,
        ) {
            return true;
        }
    }
    false
}

/// Solve the unordered endpoint-configuration relation before selecting
/// intrinsic cycle orientations. A configuration records one candidate pair
/// for every edge in a face. Shared edges must agree on that pair, but their
/// row-port order is selected later by the quotient search.
// These independent inputs describe one bounded relation phase; keeping them
// separate preserves the ownership of parsed evidence and branch state.
#[allow(clippy::too_many_arguments)]
fn resolve_endpoint_configuration_relation_streaming(
    assignments: &[Vec<MeshFaceBoundaryAssignment>],
    endpoint_configurations: &[Vec<Option<MeshFaceEndpointConfigurations>>],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
    partial_solution_valid: Option<&MeshEndpointSolutionPredicate<'_>>,
    complete_solution_valid: Option<&MeshEndpointSolutionPredicate<'_>>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
    priority_edges: Option<&[bool]>,
    coordinate_domains: Option<&MeshCoordinateRootDomains>,
) -> Option<MeshEndpointResolve> {
    if assignments.len() != endpoint_configurations.len()
        || edge_candidates.iter().any(Vec::is_empty)
    {
        return None;
    }
    let mut domains = Vec::with_capacity(assignments.len());
    let mut covered = alloc_filled(
        edge_candidates.len(),
        false,
        "catia_endpoint_relation_covered",
    )
    .ok()?;
    for (face_assignments, face_configurations) in assignments.iter().zip(endpoint_configurations) {
        if face_assignments.len() != face_configurations.len() {
            return None;
        }
        let mut choices_by_configuration =
            HashMap::<MeshFaceEndpointConfiguration, Vec<usize>>::new();
        let mut unknown = false;
        for (assignment, configurations) in face_configurations.iter().enumerate() {
            let Some(configurations) = configurations else {
                unknown = true;
                continue;
            };
            for configuration in configurations {
                for &(edge, _) in configuration {
                    *covered.get_mut(edge)? = true;
                }
                if endpoint_configuration_cycles_viable(
                    face_assignments.get(assignment)?,
                    configuration,
                ) != Some(true)
                {
                    continue;
                }
                let mut relation_configuration = configuration.clone();
                for (_, pair) in &mut relation_configuration {
                    pair.sort_unstable();
                }
                relation_configuration.sort_unstable();
                choices_by_configuration
                    .entry(relation_configuration)
                    .or_default()
                    .push(assignment);
            }
        }
        let mut choices = choices_by_configuration
            .into_iter()
            .map(|(edge_pairs, mut assignments)| {
                assignments.sort_unstable();
                assignments.dedup();
                MeshEndpointRelationChoice {
                    id: 0,
                    assignments,
                    edge_pairs,
                }
            })
            .collect::<Vec<_>>();
        choices.sort_unstable_by(|left, right| {
            (&left.edge_pairs, &left.assignments).cmp(&(&right.edge_pairs, &right.assignments))
        });
        if choices.is_empty() {
            if !unknown {
                return Some(MeshEndpointResolve::Rejected);
            }
            choices.push(MeshEndpointRelationChoice {
                id: 0,
                assignments: vec![usize::MAX],
                edge_pairs: Vec::new(),
            });
        }
        for (id, choice) in choices.iter_mut().enumerate() {
            choice.id = id;
        }
        if !budget.charge_by(choices.len()) {
            return Some(MeshEndpointResolve::Exhausted);
        }
        domains.push(choices);
    }
    if covered.iter().any(|covered| !covered) {
        return None;
    }
    let Some(constraints) = build_endpoint_relation_constraints(&domains, budget) else {
        return Some(MeshEndpointResolve::Exhausted);
    };
    let mut resolved = None;
    let mut relation_state_memo =
        HashSet::<(MeshEndpointRelationSelections, Vec<[usize; 2]>)>::new();
    let mut relation_walk_state_memo = HashSet::<MeshEndpointRelationStateSignature>::new();
    let coordinate_budget =
        coordinate_domains.map(|_| budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS));
    let mut ambiguous = false;
    let mut exhausted = false;
    let mut evaluate = |selections: MeshEndpointRelationSelections,
                        edge_pairs: Vec<[usize; 2]>|
     -> bool {
        let point_set = edge_pairs.iter().flatten().copied().collect::<HashSet<_>>();
        if point_set.len() != vertex_points.len() {
            return false;
        }
        if let Some(valid) = partial_solution_valid {
            let candidate_pairs = edge_pairs.iter().copied().map(Some).collect::<Vec<_>>();
            if !valid(&candidate_pairs) {
                return false;
            }
        }
        if relation_state_memo.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let canonical_pairs = candidate_gauge.map_or_else(
                || Some(edge_pairs.clone()),
                |gauge| canonicalize_complete_endpoint_pairs(&edge_pairs, gauge),
            );
            let Some(canonical_pairs) = canonical_pairs else {
                return false;
            };
            if !relation_state_memo.insert((selections.clone(), canonical_pairs)) {
                return false;
            }
        }
        let assignment_domains = selections
            .iter()
            .enumerate()
            .map(|(face, options)| {
                options
                    .iter()
                    .filter_map(|assignment| assignments[face].get(*assignment).cloned())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if assignment_domains.iter().any(Vec::is_empty) {
            return false;
        }
        let candidates = edge_pairs
            .iter()
            .copied()
            .map(|pair| vec![pair])
            .collect::<Vec<_>>();
        let endpoint_resolution_budget = budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        let outcome = if assignment_domains.iter().all(|domain| domain.len() == 1) {
            let selected = assignment_domains
                .into_iter()
                .map(|mut domain| domain.pop())
                .collect::<Option<Vec<_>>>();
            selected.map_or(MeshEndpointResolve::Rejected, |selected| {
                resolve_fixed_mesh_endpoint_pairs(
                    edge_rows,
                    vertex_points,
                    &candidates,
                    &selected,
                    port_identities,
                    &endpoint_resolution_budget,
                    candidate_gauge,
                )
            })
        } else {
            resolve_standard_mesh_endpoint_candidates(
                edge_rows,
                vertex_points,
                &candidates,
                assignment_domains,
                port_identities,
                None,
                &endpoint_resolution_budget,
                partial_solution_valid,
                complete_solution_valid,
                candidate_gauge,
                priority_edges,
            )
        };
        match outcome {
            MeshEndpointResolve::Solved(topology, assignment) => {
                if let Some(valid) = complete_solution_valid {
                    let Some(candidate_pairs) = mesh_candidate_point_pairs(&topology, &assignment)
                    else {
                        return false;
                    };
                    if !valid(&candidate_pairs) {
                        return false;
                    }
                }
                let candidate = (topology, assignment);
                if let Some(previous) = &resolved {
                    let equivalent = mesh_candidates_equivalent_with_context(
                        previous,
                        &candidate,
                        candidate_gauge,
                    );
                    if !equivalent {
                        ambiguous = true;
                        return true;
                    }
                } else {
                    resolved = Some(candidate);
                }
            }
            MeshEndpointResolve::Ambiguous => {
                ambiguous = true;
                return true;
            }
            MeshEndpointResolve::Exhausted => {
                exhausted = true;
                return true;
            }
            MeshEndpointResolve::Rejected => {}
        }
        if budget.exhausted() {
            exhausted = true;
            return true;
        }
        false
    };
    walk_endpoint_relation_domains(
        domains,
        assignments,
        alloc_filled(
            edge_candidates.len(),
            None,
            "catia_endpoint_relation_assigned",
        )
        .ok()?,
        &constraints,
        vertex_points.len(),
        budget,
        &mut relation_walk_state_memo,
        candidate_gauge,
        priority_edges,
        partial_solution_valid,
        coordinate_domains,
        coordinate_budget.as_ref(),
        &mut evaluate,
    );
    if ambiguous {
        Some(MeshEndpointResolve::Ambiguous)
    } else if exhausted || budget.exhausted() {
        Some(MeshEndpointResolve::Exhausted)
    } else if let Some((topology, assignment)) = resolved {
        Some(MeshEndpointResolve::Solved(topology, assignment))
    } else {
        Some(MeshEndpointResolve::Rejected)
    }
}

fn mesh_candidate_point_pairs(
    topology: &StandardTopology,
    point_assignment: &[usize],
) -> Option<Vec<Option<[usize; 2]>>> {
    let pairs = topology
        .edge_vertices()?
        .into_iter()
        .map(|[start, end]| Some([*point_assignment.get(start)?, *point_assignment.get(end)?]))
        .collect::<Option<Vec<[usize; 2]>>>()?;
    Some(pairs.into_iter().map(Some).collect())
}

fn resolve_fixed_mesh_endpoint_pairs(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[MeshFaceBoundaryAssignment],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> MeshEndpointResolve {
    if selected.is_empty()
        || edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
    {
        return MeshEndpointResolve::Rejected;
    }
    let assignment_domains = selected
        .iter()
        .cloned()
        .map(|assignment| vec![assignment])
        .collect::<Vec<_>>();
    if edge_candidates
        .iter()
        .any(|candidates| candidates.len() != 1)
    {
        return MeshEndpointResolve::Rejected;
    }
    if let Some(resolved) = resolve_singleton_mesh_endpoint_candidates(
        edge_rows,
        vertex_points,
        edge_candidates,
        &assignment_domains,
        port_identities,
        budget,
        candidate_gauge,
    ) {
        if !matches!(&resolved, MeshEndpointResolve::Rejected) {
            return resolved;
        }
    }
    let edge_pairs = edge_candidates
        .iter()
        .map(|candidates| candidates.first().copied())
        .collect::<Option<Vec<_>>>()
        .ok_or(MeshEndpointResolve::Rejected);
    let Ok(edge_pairs) = edge_pairs else {
        return MeshEndpointResolve::Rejected;
    };
    let fixed_face_directions = selected
        .iter()
        .map(|assignment| {
            let configuration = endpoint_configuration_for_assignment(assignment, &edge_pairs)?;
            let directions = endpoint_configuration_directions(assignment, &configuration)?;
            (!directions.is_empty()).then_some(Some(directions))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(MeshEndpointResolve::Rejected);
    let Ok(fixed_face_directions) = fixed_face_directions else {
        return MeshEndpointResolve::Rejected;
    };
    let Ok(mut edge_has_fixed_direction) = alloc_filled(
        edge_candidates.len(),
        false,
        "catia_fixed_mesh_edge_directions",
    ) else {
        return MeshEndpointResolve::Rejected;
    };
    for use_ in selected
        .iter()
        .flat_map(|assignment| &assignment.boundaries)
        .flatten()
    {
        if use_.reversed.is_some() {
            let Some(fixed) = edge_has_fixed_direction.get_mut(use_.edge) else {
                return MeshEndpointResolve::Rejected;
            };
            *fixed = true;
        }
    }
    let Some(quotient) =
        initial_mesh_quotient(edge_candidates, vertex_points.len(), port_identities)
    else {
        return MeshEndpointResolve::Rejected;
    };
    let mut direct_quotient = quotient.clone();
    let mut direct_orientations = edge_has_fixed_direction
        .iter()
        .map(|fixed| (!fixed).then_some(false))
        .collect::<Vec<_>>();
    let mut direct_directions = Vec::with_capacity(selected.len());
    let mut direct_possible = true;
    for (assignment, direction_options) in selected.iter().zip(&fixed_face_directions) {
        let Some(direction_options) = direction_options.as_ref() else {
            direct_possible = false;
            break;
        };
        let Some(label_directions) = direction_options.first() else {
            direct_possible = false;
            break;
        };
        let mut next_orientations = direct_orientations.clone();
        let constrained = assignment
            .boundaries
            .iter()
            .zip(label_directions)
            .flat_map(|(boundary, directions)| boundary.iter().zip(directions))
            .all(|(use_, &label_direction)| {
                let Some(required) = use_.reversed else {
                    return true;
                };
                let Some(orientation) = next_orientations.get_mut(use_.edge) else {
                    return false;
                };
                let required_orientation = required ^ label_direction;
                match *orientation {
                    Some(existing) => existing == required_orientation,
                    None => {
                        *orientation = Some(required_orientation);
                        true
                    }
                }
            });
        if !constrained {
            direct_possible = false;
            break;
        }
        let Some(directions) = direct_quotient.merge_label_directions_in_place(
            assignment,
            label_directions,
            &next_orientations,
            Some(budget),
        ) else {
            direct_possible = false;
            break;
        };
        direct_orientations = next_orientations;
        direct_directions.push(directions);
    }
    if direct_possible {
        let outcome = reconstruct_mesh_selection(
            edge_rows.to_vec(),
            vertex_points.to_vec(),
            selected,
            &direct_directions,
        )
        .and_then(|topology| {
            resolve_mesh_selection_from_quotient(
                topology,
                direct_quotient,
                vertex_points,
                edge_candidates,
                port_identities,
                budget,
            )
        });
        if let Some(outcome) = outcome {
            match outcome {
                MeshEndpointResolve::Rejected => {}
                resolved => return resolved,
            }
        }
        if budget.exhausted() {
            return MeshEndpointResolve::Exhausted;
        }
    }
    let mut search = MeshSelectionSearch {
        assignments: &assignment_domains,
        #[cfg(test)]
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work: Vec::new(),
        edge_candidates,
        edge_rows,
        vertex_points,
        candidate_gauge,
        port_identities: Some(port_identities),
        fixed_face_directions,
        fixed_edge_orientations: edge_has_fixed_direction
            .iter()
            .map(|fixed| (!fixed).then_some(false))
            .collect(),
        edge_has_fixed_direction,
        selected: match alloc_filled(assignment_domains.len(), None, "catia_fixed_mesh_selection") {
            Ok(selected) => selected,
            Err(_) => return MeshEndpointResolve::Rejected,
        },
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    search.search_fixed_direction_with_budget(&quotient, budget);
    if search.exhausted {
        MeshEndpointResolve::Exhausted
    } else if search.ambiguous {
        MeshEndpointResolve::Ambiguous
    } else if let Some((topology, assignment)) = search.solution {
        MeshEndpointResolve::Solved(topology, assignment)
    } else {
        MeshEndpointResolve::Rejected
    }
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
    let budget = WorkBudget::new(limit);
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
            if budget.exhausted() {
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
            if budget.exhausted() {
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

impl MeshSelectionSearch<'_> {
    pub(crate) fn should_stop(&self) -> bool {
        self.ambiguous || self.exhausted
    }

    fn has_exact_singleton_endpoint_domains(&self) -> bool {
        self.port_identities.is_some()
            && self
                .edge_candidates
                .iter()
                .all(|candidates| candidates.len() == 1)
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
        let budget = WorkBudget::new(usize::MAX);
        self.propagate_forced_face_equations_from(quotient, None, &budget)
    }

    fn propagate_forced_face_equations_from(
        &self,
        quotient: &mut MeshQuotient,
        changed_edges: Option<&HashSet<usize>>,
        budget: &WorkBudget<'_>,
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
                        return budget.exhausted();
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
        let Ok(mut flips) = alloc_filled(constraints.len(), None, "catia_selection_flips") else {
            return false;
        };
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
        propagation_budget: &WorkBudget<'_>,
    ) -> Option<MeshQuotient> {
        let mut measured = quotient.clone();
        if !self.has_exact_singleton_endpoint_domains()
            && !self.propagate_forced_face_equations_from(
                &mut measured,
                Some(changed_edges),
                propagation_budget,
            )
        {
            return None;
        }
        if !measured.merge_singleton_coordinate_roots(self.edge_candidates) {
            return None;
        }
        let root_count = measured.root_count();
        if root_count < self.vertex_points.len() {
            return None;
        }
        if root_count == self.vertex_points.len()
            && !self.has_exact_singleton_endpoint_domains()
            && !measured.point_assignment_exists(
                self.vertex_points.len(),
                self.edge_candidates,
                Some(propagation_budget),
            )
        {
            return None;
        }
        (self.has_exact_singleton_endpoint_domains() || self.fixed_remaining_faces_are_orientable())
            .then_some(measured)
    }

    #[cfg(test)]
    pub(crate) fn search(&mut self, quotient: &MeshQuotient) {
        self.search_with_limit(quotient, MAX_MESH_CONSTRAINT_OPERATIONS);
    }

    pub(crate) fn search_with_budget(
        &mut self,
        quotient: &MeshQuotient,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) {
        self.search_from_state(quotient, false, budget, propagation_budget);
    }

    fn fixed_direction_options(
        &self,
        measured: &MeshQuotient,
        face: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Vec<MeshFixedDirectionOption> {
        let Some(direction_options) = self
            .fixed_face_directions
            .get(face)
            .and_then(Option::as_ref)
        else {
            return Vec::new();
        };
        let [assignment] = self.assignments[face].as_slice() else {
            return Vec::new();
        };
        let mut seen = HashSet::<(Vec<Vec<bool>>, Vec<Option<bool>>)>::new();
        let mut output = Vec::new();
        for label_directions in direction_options {
            let mut next_orientations = self.fixed_edge_orientations.clone();
            let constrained = assignment
                .boundaries
                .iter()
                .zip(label_directions)
                .flat_map(|(boundary, directions)| boundary.iter().zip(directions))
                .all(|(use_, &label_direction)| {
                    let Some(required) = use_.reversed else {
                        return true;
                    };
                    let Some(orientation) = next_orientations.get_mut(use_.edge) else {
                        return false;
                    };
                    let required_orientation = required ^ label_direction;
                    match *orientation {
                        Some(existing) => existing == required_orientation,
                        None => {
                            *orientation = Some(required_orientation);
                            true
                        }
                    }
                });
            if !constrained {
                continue;
            }
            let option = measured.assignment_option_for_label_directions(
                assignment,
                label_directions,
                &next_orientations,
                budget,
            );
            let Some((directions, quotient)) = option else {
                continue;
            };
            let signature = (
                canonical_mesh_boundary_directions(&directions),
                next_orientations.clone(),
            );
            if seen.insert(signature) {
                output.push((directions, quotient, next_orientations));
            }
        }
        output
    }

    fn search_fixed_direction_with_budget(
        &mut self,
        quotient: &MeshQuotient,
        budget: &WorkBudget<'_>,
    ) {
        if self.should_stop() {
            return;
        }
        if !budget.charge() {
            self.exhausted = true;
            return;
        }
        let mut measured = quotient.clone();
        if !measured.merge_singleton_coordinate_roots(self.edge_candidates) {
            return;
        }
        if measured.root_count() < self.vertex_points.len() {
            return;
        }
        if self.visited_states.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let signature = self.selection_state_signature(&measured, true);
            if !self.visited_states.insert(signature) {
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
        let mut impossible = false;
        let face = self
            .selected
            .iter()
            .enumerate()
            .filter(|(_, selected)| selected.is_none())
            .filter_map(|(face, _)| {
                let directions = self.fixed_face_directions.get(face)?.as_ref()?;
                let [assignment] = self.assignments[face].as_slice() else {
                    return None;
                };
                let ready = assignment.boundaries.iter().flatten().all(|use_| {
                    self.fixed_edge_orientations
                        .get(use_.edge)
                        .and_then(Option::as_ref)
                        .is_some()
                        || use_.reversed.is_some()
                        || !self
                            .edge_has_fixed_direction
                            .get(use_.edge)
                            .copied()
                            .unwrap_or(false)
                });
                if !ready {
                    return None;
                }
                let local_fixed = assignment
                    .boundaries
                    .iter()
                    .flatten()
                    .filter(|use_| use_.reversed.is_some())
                    .count();
                let adjacent = assignment
                    .boundaries
                    .iter()
                    .flatten()
                    .any(|use_| selected_edges.contains(&use_.edge));
                // The exact quotient options are generated once for the face
                // selected below. This count only orders the search; probing
                // every face here would construct and hash the same large
                // quotient states a second time.
                let viable_options = directions.len();
                if viable_options == 0 {
                    impossible = true;
                    return None;
                }
                let use_count = assignment.boundaries.iter().map(Vec::len).sum::<usize>();
                Some((
                    (
                        !adjacent,
                        viable_options,
                        local_fixed == 0,
                        usize::MAX.saturating_sub(use_count),
                        directions.len(),
                        face,
                    ),
                    face,
                ))
            })
            .min_by_key(|(key, _)| *key)
            .map(|(_, face)| face);
        if impossible {
            return;
        }
        let Some(face) = face else {
            if let Some(edge) =
                self.edge_has_fixed_direction
                    .iter()
                    .enumerate()
                    .find_map(|(edge, has_fixed)| {
                        (*has_fixed
                            && self
                                .fixed_edge_orientations
                                .get(edge)
                                .is_some_and(Option::is_none))
                        .then_some(edge)
                    })
            {
                for orientation in [false, true] {
                    if self.should_stop() {
                        return;
                    }
                    self.fixed_edge_orientations[edge] = Some(orientation);
                    self.search_fixed_direction_with_budget(&measured, budget);
                }
                self.fixed_edge_orientations[edge] = None;
                return;
            }
            let selected_assignments = self
                .selected
                .iter()
                .enumerate()
                .map(|(face, selected)| {
                    let (assignment, directions) = selected.as_ref()?;
                    if *assignment != 0 {
                        return None;
                    }
                    Some((self.assignments[face].get(*assignment)?.clone(), directions))
                })
                .collect::<Option<Vec<_>>>();
            let Some(selected_assignments) = selected_assignments else {
                return;
            };
            let (selected_assignments, directions): (Vec<_>, Vec<_>) = selected_assignments
                .into_iter()
                .map(|(assignment, directions)| (assignment, directions.clone()))
                .unzip();
            let Some(port_identities) = self.port_identities else {
                return;
            };
            let Some(outcome) = resolve_singleton_mesh_selection(
                self.edge_rows,
                self.vertex_points,
                self.edge_candidates,
                &selected_assignments,
                &directions,
                port_identities,
                budget,
                self.candidate_gauge,
            ) else {
                return;
            };
            match outcome {
                MeshEndpointResolve::Solved(topology, assignment) => {
                    let candidate = (topology, assignment);
                    match &self.solution {
                        Some(solution)
                            if *solution != candidate
                                && !mesh_candidates_equivalent_with_context(
                                    solution,
                                    &candidate,
                                    self.candidate_gauge,
                                ) =>
                        {
                            self.ambiguous = true;
                        }
                        None => self.solution = Some(candidate),
                        Some(_) => {}
                    }
                }
                MeshEndpointResolve::Ambiguous => self.ambiguous = true,
                MeshEndpointResolve::Exhausted => self.exhausted = true,
                MeshEndpointResolve::Rejected => {}
            }
            return;
        };
        let previous_orientations = self.fixed_edge_orientations.clone();
        let options = self.fixed_direction_options(&measured, face, Some(budget));
        if budget.exhausted() {
            self.exhausted = true;
            return;
        }
        for (directions, next_quotient, next_orientations) in options {
            if self.should_stop() {
                return;
            }
            self.fixed_edge_orientations = next_orientations;
            self.selected[face] = Some((0, directions));
            self.search_fixed_direction_with_budget(&next_quotient, budget);
            self.selected[face] = None;
            self.fixed_edge_orientations
                .clone_from(&previous_orientations);
        }
    }

    #[cfg(test)]
    pub(crate) fn search_with_limit(&mut self, quotient: &MeshQuotient, limit: usize) {
        let budget = WorkBudget::new(limit);
        let propagation_budget = WorkBudget::new(limit);
        self.search_from_state(quotient, false, &budget, &propagation_budget);
    }

    fn selection_state_signature(
        &self,
        quotient: &MeshQuotient,
        prepared: bool,
    ) -> MeshSelectionStateSignature {
        let mut quotient = quotient.clone();
        (
            prepared,
            self.selected.clone(),
            quotient.signature(),
            self.fixed_edge_orientations.clone(),
        )
    }

    pub(crate) fn search_from_state(
        &mut self,
        quotient: &MeshQuotient,
        prepared: bool,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) {
        if self.should_stop() {
            return;
        }
        if !budget.charge() {
            self.exhausted = true;
            return;
        }
        if self.visited_states.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let signature = self.selection_state_signature(quotient, prepared);
            if !self.visited_states.insert(signature) {
                return;
            }
        }
        self.search_state(quotient, prepared, budget, propagation_budget);
    }

    fn search_state(
        &mut self,
        quotient: &MeshQuotient,
        prepared: bool,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) {
        let mut measured = quotient.clone();
        if !prepared {
            if !self.has_exact_singleton_endpoint_domains()
                && !self.propagate_forced_face_equations_from(
                    &mut measured,
                    None,
                    propagation_budget,
                )
            {
                return;
            }
            if !measured.merge_singleton_coordinate_roots(self.edge_candidates) {
                return;
            }
            let root_count = measured.root_count();
            if root_count < self.vertex_points.len() {
                return;
            }
            if !self.has_exact_singleton_endpoint_domains()
                && root_count == self.vertex_points.len()
                && !measured.point_assignment_exists(
                    self.vertex_points.len(),
                    self.edge_candidates,
                    Some(propagation_budget),
                )
            {
                if propagation_budget.exhausted() {
                    self.exhausted = true;
                }
                return;
            }
            if !self.has_exact_singleton_endpoint_domains()
                && !self.fixed_remaining_faces_are_orientable()
            {
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
                let selected_incidence = assignments
                    .iter()
                    .map(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .filter(|use_| selected_edges.contains(&use_.edge))
                            .count()
                    })
                    .max()
                    .unwrap_or_default();
                let constrained = assignments
                    .iter()
                    .map(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .filter(|use_| {
                                let left = measured.union.find(use_.edge * 2);
                                let right = measured.union.find(use_.edge * 2 + 1);
                                measured.domains[left].len() < self.vertex_points.len()
                                    || measured.domains[right].len() < self.vertex_points.len()
                            })
                            .count()
                    })
                    .max()
                    .unwrap_or_default();
                Some((
                    if can_merge { 1 } else { 2 },
                    direction_work,
                    assignments.len(),
                    usize::MAX - selected_incidence,
                    usize::MAX - constrained,
                    face,
                ))
            })
            .min();
        if budget.exhausted() {
            self.exhausted = true;
            return;
        }
        let Some((_, supported, _, _, _, face)) = next else {
            let selected = self.selected.iter().cloned().collect::<Option<Vec<_>>>();
            let Some(selected) = selected else {
                return;
            };
            let assignment_indices = selected.iter().map(|(index, _)| *index).collect::<Vec<_>>();
            let directions = selected
                .iter()
                .map(|(_, directions)| directions.clone())
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
            if self
                .edge_candidates
                .iter()
                .all(|candidates| candidates.len() == 1)
            {
                if let Some(port_identities) = self.port_identities {
                    let outcome = resolve_singleton_mesh_selection(
                        self.edge_rows,
                        self.vertex_points,
                        self.edge_candidates,
                        &selected_assignments,
                        &directions,
                        port_identities,
                        budget,
                        self.candidate_gauge,
                    );
                    if let Some(outcome) = outcome {
                        match outcome {
                            MeshEndpointResolve::Solved(topology, assignment) => {
                                let candidate = (topology, assignment);
                                match &self.solution {
                                    Some(solution)
                                        if *solution != candidate
                                            && !mesh_candidates_equivalent_with_context(
                                                solution,
                                                &candidate,
                                                self.candidate_gauge,
                                            ) =>
                                    {
                                        self.ambiguous = true;
                                    }
                                    None => self.solution = Some(candidate),
                                    Some(_) => {}
                                }
                            }
                            MeshEndpointResolve::Ambiguous => self.ambiguous = true,
                            MeshEndpointResolve::Exhausted => self.exhausted = true,
                            MeshEndpointResolve::Rejected => {}
                        }
                        if self.should_stop() {
                            return;
                        }
                    }
                }
            }
            let mut quotient = measured.clone();
            let Some(root_points) = quotient.close_coordinate_roots(
                self.vertex_points.len(),
                self.edge_candidates,
                Some(budget),
            ) else {
                if budget.exhausted() {
                    self.exhausted = true;
                }
                return;
            };
            let candidate = reconstruct_mesh_selection(
                self.edge_rows.to_vec(),
                self.vertex_points.to_vec(),
                &selected_assignments,
                &directions,
            )
            .and_then(|mut topology| {
                let mut use_counts =
                    alloc_filled(topology.edge_rows.len(), 0usize, "catia_search_edge_uses")
                        .ok()?;
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
                let mut point_assignment = alloc_filled(
                    topology.logical_vertex_count,
                    None,
                    "catia_search_point_assignment",
                )
                .ok()?;
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
                            && !mesh_candidates_equivalent_with_context(
                                solution,
                                &candidate,
                                self.candidate_gauge,
                            ) =>
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
        let remaining_work = budget.remaining();
        if remaining_work == 0 {
            self.exhausted = true;
            return;
        }
        let mut options = Vec::new();
        for assignment_index in 0..self.assignments[face].len() {
            if !budget.charge() {
                self.exhausted = true;
                return;
            }
            let remaining = remaining_work.saturating_sub(options.len());
            if remaining == 0 {
                break;
            }
            let assignment = &self.assignments[face][assignment_index];
            let assignment_options = if let Some(direction_options) = self
                .fixed_face_directions
                .get(face)
                .and_then(Option::as_ref)
            {
                if assignment_index != 0 {
                    continue;
                }
                measured.assignment_options_for_directions(
                    assignment,
                    direction_options,
                    remaining,
                    Some(budget),
                )
            } else {
                measured.assignment_options_limited(
                    assignment,
                    self.edge_candidates,
                    &selected_edges,
                    remaining,
                    Some(budget),
                )
            };
            if budget.exhausted() {
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
        if options.len() == 1 {
            let (assignment_index, directions, next_quotient) =
                options.pop().expect("one mesh option");
            let changed_edges = changed_quotient_edges(&measured, &next_quotient);
            self.selected[face] = Some((assignment_index, directions));
            if self.selected_orientable() {
                if let Some(next_quotient) =
                    self.prepare_selected_branch(&next_quotient, &changed_edges, propagation_budget)
                {
                    // The branch preflight has already run. Continue the
                    // forced suffix without another memo entry or preflight.
                    self.search_state(&next_quotient, true, budget, propagation_budget);
                } else if budget.exhausted() || propagation_budget.exhausted() {
                    self.exhausted = true;
                }
            }
            self.selected[face] = None;
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
        for (assignment_index, directions, next_quotient) in options {
            let changed_edges = changed_quotient_edges(&measured, &next_quotient);
            self.selected[face] = Some((assignment_index, directions));
            if self.selected_orientable() {
                if let Some(next_quotient) =
                    self.prepare_selected_branch(&next_quotient, &changed_edges, propagation_budget)
                {
                    // `prepare_selected_branch` has already applied the recursive
                    // entry preflight to this quotient.
                    self.search_from_state(&next_quotient, true, budget, propagation_budget);
                } else if budget.exhausted() || propagation_budget.exhausted() {
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
#[cfg(test)]
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
    // Standard-row occurrence direction is a face-quotient choice. Complete
    // FBB tables retain their scoped handle equalities in these local ports.
    let port_identities = crate::solve::missing_edge::edge_port_identities(bytes)?;
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    resolve_standard_mesh_endpoint_candidates(
        &edge_rows,
        &vertex_points,
        edge_candidates,
        assignments,
        &port_identities,
        None,
        &budget,
        None,
        None,
        None,
        None,
    )
    .into_option()
}

fn singleton_mesh_boundary_directions(
    boundary: &[MeshBoundaryEdgeCandidate],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Option<Vec<bool>> {
    if boundary.is_empty() {
        return None;
    }
    let first = boundary[0];
    let first_pair = *edge_candidates.get(first.edge)?.first()?;
    let first_directions = first.reversed.map_or_else(
        || {
            if first_pair[0] == first_pair[1] {
                vec![false]
            } else {
                vec![false, true]
            }
        },
        |direction| vec![direction],
    );
    let mut solutions = Vec::new();
    for first_direction in first_directions {
        let first_start = if first_direction {
            first_pair[1]
        } else {
            first_pair[0]
        };
        let mut current = if first_direction {
            first_pair[0]
        } else {
            first_pair[1]
        };
        let mut directions = vec![first_direction];
        let mut valid = true;
        for use_ in &boundary[1..] {
            let pair = *edge_candidates.get(use_.edge)?.first()?;
            let mut choices = if pair[0] == pair[1] {
                (pair[0] == current).then(|| vec![use_.reversed.unwrap_or(false)])
            } else {
                Some(
                    [pair[0] == current, pair[1] == current]
                        .into_iter()
                        .enumerate()
                        .filter_map(|(direction, matches)| matches.then_some(direction == 1))
                        .collect::<Vec<_>>(),
                )
            }?;
            if let Some(required) = use_.reversed {
                choices.retain(|direction| *direction == required);
            }
            let [direction] = choices.as_slice() else {
                valid = false;
                break;
            };
            current = if *direction { pair[0] } else { pair[1] };
            directions.push(*direction);
        }
        if valid && current == first_start {
            solutions.push(directions);
        }
    }
    solutions.sort_unstable();
    solutions.dedup();
    if solutions.len() == 2 && boundary.iter().all(|use_| use_.reversed.is_none()) {
        solutions.truncate(1);
    }
    (solutions.len() == 1).then(|| solutions.remove(0))
}

fn resolve_mesh_selection_from_quotient(
    topology: StandardTopology,
    mut quotient: MeshQuotient,
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
) -> Option<MeshEndpointResolve> {
    if quotient.union.len() != edge_candidates.len().checked_mul(2)?
        || port_identities.len() != edge_candidates.len()
        || quotient.root_count() != vertex_points.len()
    {
        return None;
    }
    let edge_vertices = topology.edge_vertices()?;
    if edge_vertices.len() != edge_candidates.len() {
        return None;
    }
    let roots = (0..quotient.union.len())
        .filter(|node| quotient.union.find(*node) == *node)
        .collect::<Vec<_>>();
    let domains = roots
        .iter()
        .map(|root| {
            let mut domain = quotient.domains[*root].iter().copied().collect::<Vec<_>>();
            domain.sort_unstable();
            domain
        })
        .collect::<Vec<_>>();
    let root_assignment = distinct_domain_matching_with_budget(
        domains.iter().map(Vec::as_slice),
        vertex_points.len(),
        Some(budget),
        None,
    )?;
    let root_points = roots
        .into_iter()
        .zip(root_assignment)
        .collect::<HashMap<_, _>>();
    let mut point_assignment = alloc_filled(
        topology.logical_vertex_count,
        None,
        "catia_merged_mesh_point_assignment",
    )
    .ok()?;
    let mut points_by_identity = HashMap::<u32, usize>::new();
    for (edge, [start, end]) in edge_vertices.iter().copied().enumerate() {
        let points = [start, end]
            .into_iter()
            .enumerate()
            .map(|(port, vertex)| {
                let root = quotient.union.find(edge * 2 + port);
                let point = *root_points.get(&root)?;
                match point_assignment[vertex] {
                    Some(stored) if stored != point => return None,
                    Some(_) => {}
                    None => point_assignment[vertex] = Some(point),
                }
                Some(point)
            })
            .collect::<Option<Vec<_>>>()?;
        let points = <[usize; 2]>::try_from(points.as_slice()).ok()?;
        let closed_ports = quotient.union.find(edge * 2) == quotient.union.find(edge * 2 + 1);
        if !mesh_edge_points_compatible(closed_ports, &edge_candidates[edge], points) {
            return None;
        }
        for (identity, point) in port_identities[edge].into_iter().zip(points) {
            match points_by_identity.insert(identity, point) {
                Some(previous) if previous != point => return None,
                _ => {}
            }
        }
    }
    Some(MeshEndpointResolve::Solved(
        topology,
        point_assignment.into_iter().collect::<Option<Vec<_>>>()?,
    ))
}

fn reduced_distinct_matching(
    domains: &[Vec<usize>],
    point_count: usize,
    budget: &WorkBudget<'_>,
    excluded: Option<(usize, usize)>,
) -> Option<Vec<usize>> {
    let mut assignment = alloc_filled(domains.len(), None, "catia_reduced_matching").ok()?;
    let mut used = alloc_filled(point_count, false, "catia_reduced_matching_used").ok()?;
    let mut remaining = Vec::new();
    for (root, domain) in domains.iter().enumerate() {
        if domain.len() == 1 {
            let point = domain[0];
            if excluded.is_some_and(|(excluded_root, excluded_point)| {
                excluded_root == root && excluded_point == point
            }) || used[point]
            {
                return None;
            }
            used[point] = true;
            assignment[root] = Some(point);
            continue;
        }
        let values = domain
            .iter()
            .copied()
            .filter(|point| {
                !used[*point]
                    && excluded.is_none_or(|(excluded_root, excluded_point)| {
                        excluded_root != root || excluded_point != *point
                    })
            })
            .collect::<Vec<_>>();
        if values.is_empty() {
            return None;
        }
        remaining.push((root, values));
    }
    let remaining_domains = remaining
        .iter()
        .map(|(_, domain)| domain.as_slice())
        .collect::<Vec<_>>();
    let matching =
        distinct_domain_matching_with_budget(remaining_domains, point_count, Some(budget), None)?;
    for ((root, _), point) in remaining.into_iter().zip(matching) {
        assignment[root] = Some(point);
    }
    assignment.into_iter().collect()
}

// The selection owns the complete quotient inputs and the optional gauge. The
// explicit signature keeps the two bounded materialization paths symmetric.
#[allow(clippy::too_many_arguments)]
fn resolve_singleton_mesh_selection(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[MeshFaceBoundaryAssignment],
    directions: &[Vec<Vec<bool>>],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Option<MeshEndpointResolve> {
    if selected.len() != directions.len()
        || edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
    {
        return None;
    }
    let topology = reconstruct_mesh_selection(
        edge_rows.to_vec(),
        vertex_points.to_vec(),
        selected,
        directions,
    )?;
    let edge_vertices = topology.edge_vertices()?;
    let mut quotient =
        initial_mesh_quotient(edge_candidates, vertex_points.len(), port_identities)?;
    let mut port_by_vertex = HashMap::<usize, usize>::new();
    for (edge, [start, end]) in edge_vertices.iter().copied().enumerate() {
        for (port, vertex) in [(0, start), (1, end)] {
            let node = edge * 2 + port;
            if let Some(previous) = port_by_vertex.insert(vertex, node) {
                quotient.merge(previous, node)?;
            }
        }
    }
    for (edge, candidates) in edge_candidates.iter().enumerate() {
        let &[[left_point, right_point]] = candidates.as_slice() else {
            return None;
        };
        let left_root = quotient.union.find(edge * 2);
        let right_root = quotient.union.find(edge * 2 + 1);
        if left_root == right_root && left_point != right_point {
            return None;
        }
        let allowed = [left_point, right_point]
            .into_iter()
            .collect::<HashSet<_>>();
        for root in [left_root, right_root] {
            let mut domain = quotient.domains[root].as_ref().clone();
            domain.retain(|point| allowed.contains(point));
            if domain.is_empty() {
                return None;
            }
            quotient.domains[root] = Arc::new(domain);
        }
    }
    let roots = (0..quotient.union.len())
        .filter(|node| quotient.union.find(*node) == *node)
        .collect::<Vec<_>>();
    if roots.len() != vertex_points.len() {
        return None;
    }
    let root_indices = roots
        .iter()
        .enumerate()
        .map(|(index, root)| (*root, index))
        .collect::<HashMap<_, _>>();
    let domain_sets = roots
        .iter()
        .map(|root| quotient.domains[*root].as_ref().clone())
        .collect::<Vec<HashSet<_>>>();
    if domain_sets.iter().any(HashSet::is_empty) {
        return None;
    }
    let domain_values = domain_sets
        .iter()
        .map(|domain| {
            let mut values = domain.iter().copied().collect::<Vec<_>>();
            values.sort_unstable();
            values
        })
        .collect::<Vec<_>>();
    let first_assignment =
        reduced_distinct_matching(&domain_values, vertex_points.len(), budget, None);
    let Some(first_assignment) = first_assignment else {
        return budget.exhausted().then_some(MeshEndpointResolve::Exhausted);
    };
    let mut edge_use_counts =
        alloc_filled(edge_rows.len(), 0usize, "catia_mesh_edge_use_counts").ok()?;
    for use_ in selected
        .iter()
        .flat_map(|assignment| &assignment.boundaries)
        .flatten()
    {
        *edge_use_counts.get_mut(use_.edge)? += 1;
    }
    if edge_use_counts.iter().any(|count| *count > 2) {
        return None;
    }
    let mut materialize = |assignment: &[usize]| -> Option<(StandardTopology, Vec<usize>)> {
        if assignment.len() != roots.len() {
            return None;
        }
        let mut point_assignment = alloc_filled(
            topology.logical_vertex_count,
            None,
            "catia_selection_singleton_point_assignment",
        )
        .ok()?;
        let mut points_by_identity = HashMap::<u32, usize>::new();
        for (edge, [start, end]) in edge_vertices.iter().copied().enumerate() {
            let points = [start, end]
                .into_iter()
                .enumerate()
                .map(|(port, vertex)| {
                    let root = quotient.union.find(edge * 2 + port);
                    let root = *root_indices.get(&root)?;
                    let point = *assignment.get(root)?;
                    match point_assignment[vertex] {
                        Some(stored) if stored != point => return None,
                        Some(_) => {}
                        None => point_assignment[vertex] = Some(point),
                    }
                    Some(point)
                })
                .collect::<Option<Vec<_>>>()?;
            let points = <[usize; 2]>::try_from(points.as_slice()).ok()?;
            let closed_ports = quotient.union.find(edge * 2) == quotient.union.find(edge * 2 + 1);
            if !mesh_edge_points_compatible(closed_ports, &edge_candidates[edge], points) {
                return None;
            }
            for (identity, point) in port_identities[edge].into_iter().zip(points) {
                match points_by_identity.insert(identity, point) {
                    Some(previous) if previous != point => return None,
                    _ => {}
                }
            }
        }
        Some((
            topology.clone(),
            point_assignment.into_iter().collect::<Option<Vec<_>>>()?,
        ))
    };
    let first = materialize(&first_assignment)?;
    let ambiguous_roots = domain_values
        .iter()
        .enumerate()
        .filter(|(_, domain)| domain.len() > 1)
        .map(|(root, _)| root)
        .collect::<Vec<_>>();
    for root in ambiguous_roots {
        let Some(alternate) = reduced_distinct_matching(
            &domain_values,
            vertex_points.len(),
            budget,
            Some((root, first_assignment[root])),
        ) else {
            if budget.exhausted() {
                return Some(MeshEndpointResolve::Exhausted);
            }
            continue;
        };
        let Some(alternate) = materialize(&alternate) else {
            continue;
        };
        if !mesh_candidates_equivalent_with_context(&first, &alternate, candidate_gauge) {
            return Some(MeshEndpointResolve::Ambiguous);
        }
    }
    Some(MeshEndpointResolve::Solved(first.0, first.1))
}

fn resolve_singleton_mesh_endpoint_candidates(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    assignments: &[Vec<MeshFaceBoundaryAssignment>],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Option<MeshEndpointResolve> {
    if edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
        || edge_candidates
            .iter()
            .any(|candidates| candidates.len() != 1)
    {
        return None;
    }

    let selected = assignments
        .iter()
        .map(|face| {
            let mut viable = face.iter().filter_map(|assignment| {
                let directions = assignment
                    .boundaries
                    .iter()
                    .map(|boundary| singleton_mesh_boundary_directions(boundary, edge_candidates))
                    .collect::<Option<Vec<_>>>()?;
                Some((assignment.clone(), directions))
            });
            let first = viable.next()?;
            viable.next().is_none().then_some(first)
        })
        .collect::<Option<Vec<_>>>()?;
    let (selected, endpoint_labelled_directions): (Vec<_>, Vec<_>) = selected.into_iter().unzip();
    // An unresolved coedge direction does not select a point endpoint. It is
    // a row-orientation gauge. Let the exact coordinate binding prove the
    // resulting cycle, then try the endpoint-labelled gauge only when the
    // fixed false direction cannot bind.
    let fixed_directions = selected
        .iter()
        .map(|assignment| {
            assignment
                .boundaries
                .iter()
                .map(|boundary| {
                    (!boundary.is_empty()).then(|| {
                        boundary
                            .iter()
                            .map(|use_| use_.reversed.unwrap_or(false))
                            .collect::<Vec<_>>()
                    })
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    if let Some(resolved) = resolve_singleton_mesh_selection(
        edge_rows,
        vertex_points,
        edge_candidates,
        &selected,
        &fixed_directions,
        port_identities,
        budget,
        candidate_gauge,
    ) {
        match resolved {
            MeshEndpointResolve::Rejected => {}
            resolved => return Some(resolved),
        }
    }

    resolve_singleton_mesh_selection(
        edge_rows,
        vertex_points,
        edge_candidates,
        &selected,
        &endpoint_labelled_directions,
        port_identities,
        budget,
        candidate_gauge,
    )
}

// Endpoint materialization receives independent evidence, budgets, predicates,
// and gauge state so each fallback remains separately bounded and auditable.
#[allow(clippy::too_many_arguments)]
fn resolve_standard_mesh_endpoint_candidates(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    mut assignments: Vec<Vec<MeshFaceBoundaryAssignment>>,
    port_identities: &[[u32; 2]],
    prepared_quotient: Option<&MeshQuotient>,
    budget: &WorkBudget<'_>,
    partial_solution_valid: Option<&MeshEndpointSolutionPredicate<'_>>,
    complete_solution_valid: Option<&MeshEndpointSolutionPredicate<'_>>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
    priority_edges: Option<&[bool]>,
) -> MeshEndpointResolve {
    const MAX_SELECTION_WORK: usize = 100_000;
    let face_count = assignments.len();
    let mut edge_candidates = edge_candidates.to_vec();
    if !prune_mesh_endpoint_pair_support(&mut assignments, &mut edge_candidates) {
        return MeshEndpointResolve::Rejected;
    }
    let Some(quotient) = prepared_quotient
        .cloned()
        .or_else(|| initial_mesh_quotient(&edge_candidates, vertex_points.len(), port_identities))
    else {
        return MeshEndpointResolve::Rejected;
    };
    for face in &mut assignments {
        face.retain(|assignment| {
            quotient.assignment_has_option(assignment, &edge_candidates, Some(budget))
        });
        if budget.exhausted() {
            return MeshEndpointResolve::Exhausted;
        }
        if face.is_empty() {
            return MeshEndpointResolve::Rejected;
        }
    }
    if let Some(resolved) = resolve_singleton_mesh_endpoint_candidates(
        edge_rows,
        vertex_points,
        &edge_candidates,
        &assignments,
        port_identities,
        budget,
        candidate_gauge,
    ) {
        return resolved;
    }
    let coordinate_domains = (|| {
        let preparation_limit = quotient
            .clone()
            .coordinate_domain_preparation_limit(vertex_points.len(), &edge_candidates)?;
        let preparation_budget = budget.session_child_slice(preparation_limit);
        let mut coordinate_quotient = quotient.clone();
        coordinate_quotient.prepare_coordinate_root_domains(
            vertex_points.len(),
            &edge_candidates,
            Some(&preparation_budget),
        )
    })();
    let face_work = assignments
        .iter()
        .map(|assignments| Some(assignments.len()))
        .collect::<Vec<_>>();
    let Some(total_work) = face_work
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()
        .and_then(|work| work.into_iter().try_fold(0usize, usize::checked_add))
    else {
        return MeshEndpointResolve::Rejected;
    };
    if total_work > MAX_SELECTION_WORK {
        return MeshEndpointResolve::Exhausted;
    }
    let face_equations = possible_face_equations(&assignments);
    let Some(face_choices) = possible_face_choices_with_limit(
        &assignments,
        &face_equations,
        MAX_MESH_CONSTRAINT_OPERATIONS,
    ) else {
        return MeshEndpointResolve::Exhausted;
    };
    let Ok(unselected) = alloc_filled(
        edge_candidates.len(),
        None,
        "catia_endpoint_unselected_edges",
    ) else {
        return MeshEndpointResolve::Rejected;
    };
    let configuration_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let endpoint_configurations = assignments
        .iter()
        .map(|face| {
            face.iter()
                .map(|assignment| {
                    if configuration_budget.exhausted() {
                        return None;
                    }
                    let local_budget =
                        configuration_budget.child_slice(MAX_FACE_ENDPOINT_CONFIGURATION_WORK);
                    let configurations = mesh_face_endpoint_configurations(
                        std::slice::from_ref(assignment),
                        &edge_candidates,
                        &unselected,
                        &local_budget,
                    );
                    if !configuration_budget.charge_by(local_budget.consumed())
                        || local_budget.exhausted()
                    {
                        None
                    } else {
                        configurations
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let relation = resolve_endpoint_configuration_relation_streaming(
        &assignments,
        &endpoint_configurations,
        &edge_candidates,
        edge_rows,
        vertex_points,
        port_identities,
        budget,
        partial_solution_valid,
        complete_solution_valid,
        candidate_gauge,
        priority_edges,
        coordinate_domains.as_ref(),
    );
    if let Some(resolved) = relation {
        return resolved;
    }
    let mut search = MeshSelectionSearch {
        assignments: &assignments,
        #[cfg(test)]
        possible_face_equations: face_equations,
        possible_face_choices: face_choices,
        face_work,
        edge_candidates: &edge_candidates,
        edge_rows,
        vertex_points,
        candidate_gauge,
        port_identities: Some(port_identities),
        fixed_face_directions: match alloc_filled(
            face_count,
            None,
            "catia_mesh_fixed_face_directions",
        ) {
            Ok(fixed_face_directions) => fixed_face_directions,
            Err(_) => return MeshEndpointResolve::Rejected,
        },
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: match alloc_filled(face_count, None, "catia_mesh_selected_faces") {
            Ok(selected) => selected,
            Err(_) => return MeshEndpointResolve::Rejected,
        },
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    search.search_with_budget(&quotient, budget, budget);
    if search.exhausted {
        MeshEndpointResolve::Exhausted
    } else if search.ambiguous {
        MeshEndpointResolve::Ambiguous
    } else if let Some((topology, assignment)) = search.solution {
        MeshEndpointResolve::Solved(topology, assignment)
    } else {
        MeshEndpointResolve::Rejected
    }
}

/// Resolve geometric endpoint alternatives through face incidence before
/// applying the exact trim-mesh endpoint quotient. Parsed face domains are
/// shared with the exact ordered-domain fallback. Endpoint graphs must close
/// every face; all surviving graphs must produce one topology modulo logical
/// vertex labels, intrinsic edge direction, and boundary-cycle start.
/// `partial_solution_valid` receives partial assignments during search. It must
/// be monotone: once it rejects a selected subset, assigning more edges cannot
/// make that subset valid. `partial_constraint_edges` identifies every edge
/// whose assignment can affect that predicate and must be kept in the same
/// incidence component. `preferred_assignment_edges` identifies additional
/// variables that should be selected before unrelated incidence variables;
/// those variables do not require a single incidence component.
///
/// `complete_solution_valid` is evaluated only after every endpoint pair has
/// been assigned. Use it for global preferences whose result cannot be known
/// from a partial assignment.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub(crate) fn parse_standard_mesh_candidate_outcome<FP, FC>(
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_classes: &[usize],
    edge_identity_evidence: &[bool],
    partial_constraint_edges: &[bool],
    preferred_assignment_edges: &[bool],
    priority_edges: Option<&[bool]>,
    assignment_dependencies: Option<&[Vec<usize>]>,
    budget: &WorkBudget<'_>,
    partial_solution_valid: FP,
    complete_solution_valid: FC,
) -> MeshCandidateSolve
where
    FP: Fn(&[Option<[usize; 2]>]) -> bool,
    FC: Fn(&[Option<[usize; 2]>]) -> bool,
{
    let endpoint_budget = budget.session_child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
    let Some((
        face_count,
        edge_rows,
        vertex_points,
        boundary_context,
        mut mesh_domains,
        port_identities,
    )) = (|| {
        let (_, face_count, after_faces) = largest_fbb_run(bytes)?;
        let (edge_rows, vertex_header) = parse_edge_tables(bytes, after_faces)?;
        let vertex_points = parse_vertex_table(bytes, vertex_header)?;
        let boundary_context = StandardMeshBoundaryContext::parse(bytes, edge_faces)?;
        let mesh_domains = standard_mesh_boundary_domains_from_context(
            &boundary_context,
            Some(edge_candidates),
            true,
        )?;
        // Do not pre-apply raw trim-run direction: standard-row endpoints are
        // oriented only when a complete face-boundary quotient is selected.
        let port_identities = crate::solve::missing_edge::edge_port_identities(bytes)?;
        Some((
            face_count,
            edge_rows,
            vertex_points,
            boundary_context,
            mesh_domains,
            port_identities,
        ))
    })()
    else {
        return MeshCandidateSolve::Rejected(MeshCandidateRejection::InputStructure);
    };
    let coordinate_gauge = build_mesh_coordinate_gauge(
        vertex_points.len(),
        &edge_rows,
        edge_faces,
        edge_classes,
        edge_candidates,
        edge_identity_evidence,
    );
    let candidate_gauge = Some(MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces,
        edge_classes,
        edge_candidates,
        edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    });
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_candidates.len()
        || edge_rows.len() != edge_classes.len()
        || edge_rows.len() != partial_constraint_edges.len()
        || edge_rows.len() != preferred_assignment_edges.len()
        || priority_edges.is_some_and(|edges| edges.len() != edge_rows.len())
        || assignment_dependencies.is_some_and(|dependencies| {
            dependencies.len() != edge_rows.len()
                || dependencies
                    .iter()
                    .flatten()
                    .any(|edge| *edge >= edge_rows.len())
        })
        || edge_candidates
            .iter()
            .flatten()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
        return MeshCandidateSolve::Rejected(MeshCandidateRejection::InputCardinality);
    }
    if mesh_domains.len() != face_count {
        return MeshCandidateSolve::Rejected(MeshCandidateRejection::FaceBoundaryCardinality);
    }
    for domain in &mut mesh_domains {
        if let MeshFaceBoundaryDomain::Ordered(assignments) = domain {
            deduplicate_mesh_quotient_assignments(std::slice::from_mut(assignments));
        }
    }
    if port_identities.len() != edge_rows.len() {
        return MeshCandidateSolve::Rejected(MeshCandidateRejection::PortCardinality);
    }
    let Some((mesh_quotient, completed_edge_candidates)) = (|| {
        let mut mesh_quotient =
            initial_mesh_quotient(edge_candidates, vertex_points.len(), &port_identities)?;
        let mut propagated_quotient = mesh_quotient.clone();
        match propagate_common_ordered_face_quotients(
            &mesh_domains,
            edge_candidates,
            &mut propagated_quotient,
            budget,
        ) {
            Some(()) => mesh_quotient = propagated_quotient,
            None if budget.exhausted() => {}
            None => return None,
        }
        let completed_edge_candidates = if edge_candidates.iter().any(Vec::is_empty) {
            propagate_common_boundary_components(
                &mesh_domains,
                edge_candidates,
                &mut mesh_quotient,
            )?;
            edge_candidates.to_vec()
        } else {
            edge_candidates.to_vec()
        };
        if !mesh_quotient.edge_domains_viable(&completed_edge_candidates) {
            return None;
        }
        Some((mesh_quotient, completed_edge_candidates))
    })() else {
        return MeshCandidateSolve::Rejected(MeshCandidateRejection::QuotientPreparation);
    };
    let Some(class_constraint) =
        edge_class_search_constraint(edge_classes, &completed_edge_candidates)
    else {
        return MeshCandidateSolve::Rejected(MeshCandidateRejection::EdgeClassConstraint);
    };
    let constraint_edges = partial_constraint_edges
        .iter()
        .zip(preferred_assignment_edges)
        .zip(&class_constraint.active)
        .map(|((partial, preferred), class)| *partial || *preferred || *class)
        .collect::<Vec<_>>();
    let mut assignment_predecessors = vec![None; completed_edge_candidates.len()];
    for &(left, right) in &class_constraint.ordered {
        assignment_predecessors[right] = Some(
            assignment_predecessors[right].map_or(left, |predecessor: usize| predecessor.max(left)),
        );
    }
    let constrained_partial_solution_valid =
        |pairs: &[Option<[usize; 2]>]| partial_solution_valid(pairs);
    let complete_preference_rejected = Cell::new(false);
    let constrained_complete_solution_valid = |pairs: &[Option<[usize; 2]>]| {
        let valid = complete_solution_valid(pairs);
        if !valid {
            complete_preference_rejected.set(true);
        }
        valid
    };
    let mut incidence_solution = None;
    let mut incidence_ambiguity = None;
    let mut incidence_exhausted = false;
    let mut endpoint_resolution_memo = HashMap::<Vec<[usize; 2]>, MeshEndpointResolve>::new();
    let pair_solutions = visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy(
        &edge_rows,
        &vertex_points,
        edge_faces,
        &completed_edge_candidates,
        face_count,
        Some(&mesh_domains),
        Some(&mesh_quotient),
        CoordinateRootPolicy::DeferToVisitor,
        Some(MeshPartialEndpointConstraint {
            active_edges: &constraint_edges,
            coupled_edges: partial_constraint_edges,
            assignment_predecessors: Some(&assignment_predecessors),
            assignment_dependencies,
            valid: &constrained_partial_solution_valid,
        }),
        Some(&endpoint_budget),
        &|pairs| {
            constrained_complete_solution_valid(
                &pairs.iter().copied().map(Some).collect::<Vec<_>>(),
            )
        },
        &mut |pairs| {
            let endpoint_key = pairs.to_vec();
            let endpoint_resolution = if let Some(cached) =
                endpoint_resolution_memo.get(&endpoint_key).cloned()
            {
                cached
            } else {
                let singleton = pairs
                    .iter()
                    .copied()
                    .map(|pair| vec![pair])
                    .collect::<Vec<_>>();
                let Some(mut mesh_assignments) = standard_mesh_boundary_assignments_from_context(
                    &boundary_context,
                    Some(&singleton),
                ) else {
                    return ControlFlow::Continue(());
                };
                deduplicate_mesh_quotient_assignments(&mut mesh_assignments);
                // This child owns the incidence-to-endpoint relation phase. The
                // complete materialization invoked by that relation takes its
                // own MAX_MESH_CONSTRAINT_OPERATIONS child slice.
                let endpoint_resolution_budget =
                    endpoint_budget.session_child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
                let resolution = resolve_standard_mesh_endpoint_candidates(
                    &edge_rows,
                    &vertex_points,
                    &singleton,
                    mesh_assignments,
                    &port_identities,
                    None,
                    &endpoint_resolution_budget,
                    Some(&constrained_partial_solution_valid),
                    Some(&constrained_complete_solution_valid),
                    candidate_gauge,
                    priority_edges,
                );
                if !matches!(resolution, MeshEndpointResolve::Exhausted)
                    && endpoint_resolution_memo.len() < MAX_ENDPOINT_RESOLUTION_MEMO_ENTRIES
                {
                    endpoint_resolution_memo.insert(endpoint_key, resolution.clone());
                }
                resolution
            };
            let candidate = match endpoint_resolution {
                MeshEndpointResolve::Solved(topology, assignment) => (topology, assignment),
                MeshEndpointResolve::Rejected => return ControlFlow::Continue(()),
                MeshEndpointResolve::Ambiguous => {
                    incidence_ambiguity = Some(MeshCandidateAmbiguity::EndpointResolution);
                    return ControlFlow::Break(());
                }
                MeshEndpointResolve::Exhausted => {
                    incidence_exhausted = true;
                    return ControlFlow::Break(());
                }
            };
            match &incidence_solution {
                Some(stored)
                    if !mesh_candidates_equivalent_with_context(
                        stored,
                        &candidate,
                        candidate_gauge,
                    ) =>
                {
                    incidence_ambiguity = Some(MeshCandidateAmbiguity::DistinctTopologySolutions);
                    ControlFlow::Break(())
                }
                None => {
                    incidence_solution = Some(candidate);
                    ControlFlow::Continue(())
                }
                Some(_) => ControlFlow::Continue(()),
            }
        },
    );
    if let Some(ambiguity) = incidence_ambiguity {
        return MeshCandidateSolve::Ambiguous(ambiguity);
    }
    if matches!(pair_solutions, IncidenceSolve::Ambiguous) {
        return MeshCandidateSolve::Ambiguous(MeshCandidateAmbiguity::CoordinateRootClosure);
    }
    if incidence_exhausted || matches!(pair_solutions, IncidenceSolve::Exhausted) {
        return MeshCandidateSolve::Exhausted(if incidence_exhausted {
            if complete_preference_rejected.get() {
                MeshCandidateExhaustion::PreferredSolutionSearch
            } else {
                MeshCandidateExhaustion::EndpointResolution
            }
        } else if complete_preference_rejected.get() {
            MeshCandidateExhaustion::PreferredSolutionSearch
        } else {
            MeshCandidateExhaustion::IncidenceEnumeration
        });
    }
    if let Some((topology, assignment)) = incidence_solution {
        return MeshCandidateSolve::Solved(topology, assignment);
    }
    let incidence_rejection = match pair_solutions {
        IncidenceSolve::Rejected(rejection) => {
            MeshEndpointIncidenceRejection::NoAssignment(rejection)
        }
        IncidenceSolve::Solved(_) => MeshEndpointIncidenceRejection::BoundaryReconstruction,
        IncidenceSolve::Ambiguous => unreachable!("ambiguity returned before fallback"),
        IncidenceSolve::Exhausted => unreachable!("exhaustion returned before fallback"),
    };
    let fallback = (|| {
        let assignments = mesh_domains
            .into_iter()
            .map(|domain| match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => Some(assignments),
                MeshFaceBoundaryDomain::UnorderedFullCycle(_)
                | MeshFaceBoundaryDomain::DeferredValidation(_) => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let resolution = resolve_standard_mesh_endpoint_candidates(
            &edge_rows,
            &vertex_points,
            edge_candidates,
            assignments,
            &port_identities,
            Some(&mesh_quotient),
            &endpoint_budget,
            Some(&constrained_partial_solution_valid),
            Some(&constrained_complete_solution_valid),
            candidate_gauge,
            priority_edges,
        );
        Some(resolution)
    })();
    match fallback {
        Some(MeshEndpointResolve::Solved(topology, assignment)) => {
            MeshCandidateSolve::Solved(topology, assignment)
        }
        Some(MeshEndpointResolve::Ambiguous) => {
            MeshCandidateSolve::Ambiguous(MeshCandidateAmbiguity::EndpointResolution)
        }
        Some(MeshEndpointResolve::Exhausted) => {
            MeshCandidateSolve::Exhausted(if complete_preference_rejected.get() {
                MeshCandidateExhaustion::PreferredSolutionSearch
            } else {
                MeshCandidateExhaustion::EndpointResolution
            })
        }
        Some(MeshEndpointResolve::Rejected) | None => MeshCandidateSolve::Rejected(
            MeshCandidateRejection::EndpointIncidence(incidence_rejection),
        ),
    }
}

#[test]
fn relation_coordinate_candidates_keep_only_surviving_pair_values() {
    let base_candidates = vec![vec![[0, 1], [0, 2]], vec![[1, 2]]];
    let domains = vec![
        vec![MeshEndpointRelationChoice {
            id: 0,
            assignments: vec![0],
            edge_pairs: vec![(0, [0, 1]), (1, [1, 2])],
        }],
        vec![MeshEndpointRelationChoice {
            id: 1,
            assignments: vec![0],
            edge_pairs: vec![(0, [0, 1])],
        }],
    ];
    let assigned = vec![None, None];
    assert_eq!(
        relation_coordinate_candidate_domains(&domains, &assigned, &base_candidates),
        Some(vec![vec![[0, 1]], vec![[1, 2]]]),
    );

    let unknown_domains = vec![vec![MeshEndpointRelationChoice {
        id: 0,
        assignments: vec![usize::MAX],
        edge_pairs: Vec::new(),
    }]];
    assert_eq!(
        relation_coordinate_candidate_domains(&unknown_domains, &assigned, &base_candidates),
        Some(base_candidates.clone()),
    );
    assert!(relation_coordinate_candidate_domains(
        &domains,
        &[Some([2, 3]), None],
        &base_candidates,
    )
    .is_none());
}

#[test]
fn mesh_candidate_rejection_retains_the_failed_solver_stage() {
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(matches!(
        parse_standard_mesh_candidate_outcome(
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            None,
            None,
            &budget,
            |_| true,
            |_| true,
        ),
        MeshCandidateSolve::Rejected(MeshCandidateRejection::InputStructure)
    ));
}

#[test]
fn endpoint_configuration_relation_solves_cycle_orientation_globally() {
    let edge_rows = (0..3)
        .map(|_| EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let vertex_points = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let edge_candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
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
                end: 2,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 2,
                start: 2,
                end: 0,
                reversed: None,
            },
        ]],
    }]];
    let endpoint_configurations = vec![vec![Some(vec![vec![
        (0, [0, 1]),
        (1, [1, 2]),
        (2, [0, 2]),
    ]])]];
    let port_identities = vec![[0, 1], [2, 3], [4, 5]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    let Some(MeshEndpointResolve::Solved(topology, point_assignment)) =
        resolve_endpoint_configuration_relation_streaming(
            &assignments,
            &endpoint_configurations,
            &edge_candidates,
            &edge_rows,
            &vertex_points,
            &port_identities,
            &budget,
            None,
            None,
            None,
            None,
            None,
        )
    else {
        panic!("endpoint configuration relation did not solve the cycle");
    };

    assert_eq!(topology.faces.len(), 1);
    assert_eq!(point_assignment, vec![0, 1, 2]);
}

#[test]
fn raw_endpoint_relation_state_signature_ignores_local_order() {
    let left = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 7,
                assignments: vec![2, 0, 2],
                edge_pairs: vec![(1, [3, 2]), (0, [1, 0])],
            },
            MeshEndpointRelationChoice {
                id: 3,
                assignments: vec![4],
                edge_pairs: vec![(2, [5, 4])],
            },
        ],
        vec![MeshEndpointRelationChoice {
            id: 9,
            assignments: vec![3, 1],
            edge_pairs: vec![(0, [1, 0])],
        }],
    ];
    let right = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 30,
                assignments: vec![4],
                edge_pairs: vec![(2, [4, 5])],
            },
            MeshEndpointRelationChoice {
                id: 70,
                assignments: vec![0, 2],
                edge_pairs: vec![(0, [0, 1]), (1, [2, 3])],
            },
        ],
        vec![MeshEndpointRelationChoice {
            id: 90,
            assignments: vec![1, 3],
            edge_pairs: vec![(0, [0, 1])],
        }],
    ];
    let left_assigned = vec![Some([3, 2]), None, Some([5, 4])];
    let right_assigned = vec![Some([2, 3]), None, Some([4, 5])];

    assert_eq!(
        raw_endpoint_relation_state_signature(&left, &left_assigned),
        raw_endpoint_relation_state_signature(&right, &right_assigned),
    );
}

#[test]
fn endpoint_relation_requires_one_joint_support_for_all_shared_edges() {
    let mut domains = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                assignments: vec![0],
                edge_pairs: vec![(0, [0, 1]), (1, [2, 3])],
            },
            MeshEndpointRelationChoice {
                id: 1,
                assignments: vec![1],
                edge_pairs: vec![(0, [4, 5]), (1, [6, 7])],
            },
        ],
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                assignments: vec![0],
                edge_pairs: vec![(0, [0, 1]), (1, [6, 7])],
            },
            MeshEndpointRelationChoice {
                id: 1,
                assignments: vec![1],
                edge_pairs: vec![(0, [4, 5]), (1, [6, 7])],
            },
        ],
    ];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let constraints = build_endpoint_relation_constraints(&domains, &budget)
        .expect("shared-edge relation constraints should build");
    let mut assigned = vec![None; 2];

    assert!(propagate_endpoint_relation_domains(
        &mut domains,
        &mut assigned,
        &constraints,
        &budget,
    ));
    assert_eq!(
        domains[0]
            .iter()
            .map(|choice| choice.id)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert_eq!(
        domains[1]
            .iter()
            .map(|choice| choice.id)
            .collect::<Vec<_>>(),
        vec![1]
    );
}

#[test]
fn endpoint_relation_treats_optional_shared_edges_as_wildcards() {
    let mut domains = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                assignments: vec![0],
                edge_pairs: vec![(0, [0, 1])],
            },
            MeshEndpointRelationChoice {
                id: 1,
                assignments: vec![1],
                edge_pairs: vec![(0, [2, 3])],
            },
        ],
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                assignments: vec![0],
                edge_pairs: vec![(0, [4, 5])],
            },
            MeshEndpointRelationChoice {
                id: 1,
                assignments: vec![1],
                edge_pairs: Vec::new(),
            },
        ],
    ];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let constraints = build_endpoint_relation_constraints(&domains, &budget)
        .expect("shared-edge relation constraints should build");
    let mut assigned = vec![None];

    assert!(propagate_endpoint_relation_domains(
        &mut domains,
        &mut assigned,
        &constraints,
        &budget,
    ));
    assert_eq!(
        domains[0]
            .iter()
            .map(|choice| choice.id)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        domains[1]
            .iter()
            .map(|choice| choice.id)
            .collect::<Vec<_>>(),
        vec![1]
    );
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

    let configurations =
        mesh_face_endpoint_configurations(&[assignment], &candidates, &[None; 3], &budget)
            .expect("closed-point transitions should be deduplicated");

    assert_eq!(configurations.len(), 1);
    assert!(!budget.exhausted());
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
                            left: vec![0, 1],
                            right: vec![2, 3],
                            left_index: 0,
                            right_index: 0,
                            same_root: false,
                        },
                    },
                ))
            },
            |_, _| true,
        ),
        None
    );
    assert!(budget.exhausted());
}

#[test]
fn coordinate_root_closure_distinguishes_symmetric_assignments() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: vec![
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([0, 1])),
        ],
        members: vec![vec![0], vec![1]],
    };
    let outcome = quotient.coordinate_root_closure_outcome(2, &[vec![[0, 1]]], None, None);

    assert_eq!(outcome, CoordinateRootClosure::Ambiguous);
}

#[test]
fn coordinate_root_closure_rejects_a_single_prefix_after_budget_refusal() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: vec![
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([0, 1])),
        ],
        members: vec![vec![0], vec![1]],
    };
    let budget = WorkBudget::new(2);

    assert_eq!(
        quotient.coordinate_root_closure_outcome(2, &[vec![[0, 1]]], None, Some(&budget),),
        CoordinateRootClosure::Exhausted
    );
    assert!(budget.exhausted());
}

#[test]
fn coordinate_root_closure_rejects_a_refused_incidence_check() {
    let edge_candidates = vec![vec![[0, 1]], vec![[0, 1]]];
    let edge_faces = [[0, 1], [0, 1]];
    let assignment = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: None,
    };
    let boundary = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![assignment(0), assignment(1)]],
    };
    let boundary_domains = vec![
        MeshFaceBoundaryDomain::Ordered(vec![boundary.clone()]),
        MeshFaceBoundaryDomain::Ordered(vec![boundary]),
    ];
    let make_quotient = || {
        let mut quotient = MeshQuotient {
            union: UnionFind::new(4),
            domains: (0..4)
                .map(|node| Arc::new(HashSet::from([usize::from(node % 2 != 0)])))
                .collect(),
            members: (0..4).map(|node| vec![node]).collect(),
        };
        quotient.merge(0, 2).expect("shared left endpoint");
        quotient.merge(1, 3).expect("shared right endpoint");
        quotient
    };
    let refused_budget = WorkBudget::new(38);
    let refused = make_quotient().coordinate_root_closure_outcome(
        2,
        &edge_candidates,
        Some((&edge_faces, &boundary_domains)),
        Some(&refused_budget),
    );
    assert_eq!(refused, CoordinateRootClosure::Exhausted);
    assert!(refused_budget.exhausted());
    let complete_budget = WorkBudget::new(39);
    let complete = make_quotient().coordinate_root_closure_outcome(
        2,
        &edge_candidates,
        Some((&edge_faces, &boundary_domains)),
        Some(&complete_budget),
    );
    assert!(matches!(complete, CoordinateRootClosure::Solved(_)));
    assert!(!complete_budget.exhausted());
}

#[test]
fn coordinate_root_preparation_budgets_independent_components_separately() {
    const COMPONENT_COUNT: usize = 8;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(COMPONENT_COUNT * 6),
        domains: (0..COMPONENT_COUNT)
            .flat_map(|component| {
                let points = Arc::new((component * 3..component * 3 + 3).collect::<HashSet<_>>());
                std::iter::repeat_n(points, 6)
            })
            .collect(),
        members: (0..COMPONENT_COUNT * 6).map(|node| vec![node]).collect(),
    };
    let mut candidates = Vec::new();
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        let point = component * 3;
        quotient
            .merge(node + 1, node + 2)
            .expect("disjoint coordinate roots merge");
        quotient
            .merge(node + 3, node + 4)
            .expect("disjoint coordinate roots merge");
        candidates.extend([
            vec![[point, point + 1]],
            vec![[point + 1, point + 2]],
            vec![[point, point + 2]],
        ]);
    }
    let edge_faces = (0..COMPONENT_COUNT)
        .flat_map(|face| std::iter::repeat_n([face, face], 3))
        .collect::<Vec<_>>();
    let boundary_domains = (0..COMPONENT_COUNT)
        .map(|face| MeshFaceBoundaryDomain::UnorderedFullCycle((face * 3..face * 3 + 3).collect()))
        .collect::<Vec<_>>();
    let shared_budget = WorkBudget::new(1);
    let mut shared = quotient.clone();
    assert_eq!(
        shared.coordinate_root_closure_outcome(
            COMPONENT_COUNT * 3,
            &candidates,
            None,
            Some(&shared_budget),
        ),
        CoordinateRootClosure::Exhausted
    );

    let shared_incidence_budget = WorkBudget::new(100);
    let mut shared_incidence = quotient.clone();
    assert!(shared_incidence
        .close_coordinate_roots_for_incidence_with_budget(
            COMPONENT_COUNT * 3,
            &candidates,
            &edge_faces,
            COMPONENT_COUNT,
            &boundary_domains,
            Some(&shared_incidence_budget),
        )
        .is_none());
    assert!(shared_incidence_budget.exhausted());

    let preparation_budget = WorkBudget::new(100);
    let outcome = quotient.coordinate_root_closure_outcome_for_incidence(
        COMPONENT_COUNT * 3,
        &candidates,
        &edge_faces,
        COMPONENT_COUNT,
        &boundary_domains,
        Some(&preparation_budget),
    );
    assert!(
        matches!(outcome, CoordinateRootClosure::Solved(_)),
        "{outcome:?}"
    );
    assert!(!preparation_budget.exhausted());
}

#[test]
fn singleton_mesh_path_handles_many_independent_face_cycles() {
    const FACE_COUNT: usize = 128;
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut edge_rows = Vec::with_capacity(FACE_COUNT * 4);
    let mut edge_candidates = Vec::with_capacity(FACE_COUNT * 4);
    let mut port_identities = Vec::with_capacity(FACE_COUNT * 4);
    let mut assignments = Vec::with_capacity(FACE_COUNT);
    let mut vertex_points = Vec::with_capacity(FACE_COUNT * 4);

    for face in 0..FACE_COUNT {
        let edge = face * 4;
        let point = face * 4;
        vertex_points.extend([
            [point as f64, 0.0, 0.0],
            [(point + 1) as f64, 0.0, 0.0],
            [(point + 2) as f64, 0.0, 0.0],
            [(point + 3) as f64, 0.0, 0.0],
        ]);
        edge_rows.extend((0..4).map(|_| EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }));
        edge_candidates.extend([
            vec![[point, point + 1]],
            vec![[point + 1, point + 2]],
            vec![[point + 2, point + 3]],
            vec![[point, point + 3]],
        ]);
        let identity = (edge * 2) as u32;
        port_identities.extend([
            [identity, identity + 1],
            [identity + 2, identity + 3],
            [identity + 4, identity + 5],
            [identity + 6, identity + 7],
        ]);
        assignments.push(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![(0..4)
                .map(|offset| MeshBoundaryEdgeCandidate {
                    edge: edge + offset,
                    start: offset,
                    end: offset + 1,
                    reversed: None,
                })
                .collect()],
        }]);
    }

    let MeshEndpointResolve::Solved(topology, point_assignment) =
        resolve_singleton_mesh_endpoint_candidates(
            &edge_rows,
            &vertex_points,
            &edge_candidates,
            &assignments,
            &port_identities,
            &budget,
            None,
        )
        .expect("singleton path applies")
    else {
        panic!("singleton path did not solve");
    };
    assert_eq!(topology.faces.len(), FACE_COUNT);
    assert_eq!(point_assignment.len(), FACE_COUNT * 4);
}

#[test]
fn singleton_mesh_path_filters_endpoint_incompatible_face_assignments() {
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let edge_rows = (0..3)
        .map(|_| EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let vertex_points = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let edge_candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 0]]];
    let port_identities = vec![[0, 1], [2, 3], [4, 5]];
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let assignments = vec![vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1), use_(0)]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1), use_(2)]],
        },
    ]];

    let MeshEndpointResolve::Solved(topology, point_assignment) =
        resolve_singleton_mesh_endpoint_candidates(
            &edge_rows,
            &vertex_points,
            &edge_candidates,
            &assignments,
            &port_identities,
            &budget,
            None,
        )
        .expect("endpoint filtering should leave one face assignment")
    else {
        panic!("endpoint filtering did not solve");
    };
    assert_eq!(topology.faces.len(), 1);
    assert_eq!(point_assignment, vec![0, 1, 2]);
}

#[test]
fn singleton_mesh_path_handles_closed_endpoint_pairs() {
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let edge_rows = vec![EdgeRow {
        kind: 1,
        handles: Vec::new(),
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    let vertex_points = vec![[0.0, 0.0, 0.0]];
    let edge_candidates = vec![vec![[0, 0]]];
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    }]];
    let port_identities = vec![[0, 0]];

    let MeshEndpointResolve::Solved(topology, point_assignment) =
        resolve_singleton_mesh_endpoint_candidates(
            &edge_rows,
            &vertex_points,
            &edge_candidates,
            &assignments,
            &port_identities,
            &budget,
            None,
        )
        .expect("closed endpoint pair should use a direction gauge")
    else {
        panic!("closed endpoint pair did not solve");
    };
    assert_eq!(topology.logical_vertex_count, 1);
    assert_eq!(point_assignment, vec![0]);
}
