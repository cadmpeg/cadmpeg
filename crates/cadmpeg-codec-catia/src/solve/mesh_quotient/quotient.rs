// SPDX-License-Identifier: Apache-2.0
//! The mesh vertex-coordinate quotient and its closure.

use super::root_domains::{
    enforce_edge_arc_consistency, enforce_edge_arc_consistency_from,
    enforce_sparse_endpoint_membership, MeshCoordinateRootDomains, MeshIncidenceBoundary,
    RefinedCoordinateDomains,
};
use super::selection_state::admit_orientation_option;
use super::{
    edge_end, edge_start, CoordinateRootClosure, MeshCandidateFailure, MeshFaceDirectionOptions,
    MeshOrientationOption, MeshQuotientSignature, MeshSolve, PointAssignmentOutcome,
    MAX_MESH_CONSTRAINT_OPERATIONS,
};
use crate::solve::matching::domains_have_distinct_matching;
use crate::solve::missing_edge::{
    MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
};
use crate::solve::union_find::UnionFind;
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct MeshQuotient {
    pub(super) union: UnionFind,
    pub(super) domains: Vec<Arc<HashSet<usize>>>,
    pub(super) members: Vec<Option<Arc<Vec<usize>>>>,
}

pub(in crate::solve) fn initial_mesh_quotient(
    edge_candidates: &[Vec<[usize; 2]>],
    point_count: usize,
    port_identities: &[[u32; 2]],
) -> Result<Option<MeshQuotient>, CodecError> {
    if port_identities.len() != edge_candidates.len() {
        return Ok(None);
    }
    let mut all_points = HashSet::new();
    for point in 0..point_count {
        all_points.insert(point);
    }
    let all_points = Arc::new(all_points);
    let mut domains = Vec::new();
    for candidates in edge_candidates {
        let domain = if candidates.is_empty() {
            Arc::clone(&all_points)
        } else {
            let mut points = HashSet::new();
            for &point in candidates.iter().flatten() {
                points.insert(point);
            }
            Arc::new(points)
        };
        if domain.is_empty() || domain.iter().any(|point| *point >= point_count) {
            return Ok(None);
        }
        domains.push(Arc::clone(&domain));
        domains.push(domain);
    }
    let mut quotient = MeshQuotient::new(domains);
    let mut node_by_identity = HashMap::new();
    for (edge, ports) in port_identities.iter().enumerate() {
        for (port, identity) in ports.iter().copied().enumerate() {
            let node = edge * 2 + port;
            if let Some(&previous) = node_by_identity.get(&identity) {
                if quotient.merge(previous, node)?.is_none() {
                    return Ok(None);
                }
            } else {
                node_by_identity.insert(identity, node);
            }
        }
    }
    Ok(quotient
        .edge_domains_viable(edge_candidates)?
        .then_some(quotient))
}

#[cfg(test)]
pub(super) fn complete_mesh_endpoint_candidates_from_quotient(
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
            let left = quotient
                .union
                .find(edge * 2)
                .expect("find the component root");
            let right = quotient
                .union
                .find(edge * 2 + 1)
                .expect("find the component root");
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
    pub(crate) fn new(domains: Vec<Arc<HashSet<usize>>>) -> Self {
        Self {
            union: UnionFind::new(domains.len()),
            members: (0..domains.len())
                .map(|node| Some(Arc::new(vec![node])))
                .collect(),
            domains,
        }
    }

    pub(in crate::solve) fn len(&self) -> usize {
        self.domains.len()
    }

    pub(crate) fn find(&mut self, node: usize) -> Result<usize, CodecError> {
        self.union.find(node)
    }

    pub(crate) fn root(&self, node: usize) -> Result<usize, CodecError> {
        self.union.root(node)
    }

    pub(crate) fn domains(&self) -> &[Arc<HashSet<usize>>] {
        &self.domains
    }

    pub(super) fn members(&self, root: usize) -> &[usize] {
        self.members[root].as_deref().map_or(&[], Vec::as_slice)
    }

    pub(in crate::solve) fn coordinate_domain_preparation_limit(
        &mut self,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<Option<usize>, CodecError> {
        (|| -> Option<Result<usize, CodecError>> {
            if self.union.len() != edge_candidates.len().checked_mul(2)? {
                return None;
            }
            let mut root_count = 0usize;
            let mut root_supports = 0usize;
            for node in 0..self.union.len() {
                if match self.union.find(node) {
                    Ok(root) => root,
                    Err(error) => return Some(Err(error)),
                } != node
                {
                    continue;
                }
                root_count += 1;
                root_supports = root_supports.checked_add(
                    self.domains[node]
                        .iter()
                        .filter(|point| **point < point_count)
                        .count(),
                )?;
            }
            let explicit_pair_supports = edge_candidates.iter().map(Vec::len).sum::<usize>();
            let matching_phase_bound = root_count
                .checked_add(point_count)?
                .isqrt()
                .checked_add(1)?;
            let traversal_bound = matching_phase_bound.checked_add(8)?;
            Some(Ok(root_supports
                .checked_add(explicit_pair_supports)?
                .checked_mul(traversal_bound)?
                .max(MAX_MESH_CONSTRAINT_OPERATIONS)))
        })()
        .transpose()
    }

    pub(in crate::solve) fn signature_work(&mut self) -> Result<Option<usize>, CodecError> {
        (|| -> Option<Result<usize, CodecError>> {
            let mut work = 0usize;
            for node in 0..self.union.len() {
                if match self.union.find(node) {
                    Ok(root) => root,
                    Err(error) => return Some(Err(error)),
                } == node
                {
                    work = work
                        .checked_add(self.members(node).len())?
                        .checked_add(self.domains[node].len())?;
                }
            }
            Some(Ok(work_units(work)))
        })()
        .transpose()
    }

    pub(super) fn monotone_measure(&mut self) -> Result<Option<(usize, usize)>, CodecError> {
        (|| -> Option<Result<(usize, usize), CodecError>> {
            let mut root_count = 0usize;
            let mut domain_cardinality = 0usize;
            for node in 0..self.union.len() {
                if match self.union.find(node) {
                    Ok(root) => root,
                    Err(error) => return Some(Err(error)),
                } == node
                {
                    root_count += 1;
                    domain_cardinality =
                        domain_cardinality.checked_add(self.domains[node].len())?;
                }
            }
            Some(Ok((root_count, domain_cardinality)))
        })()
        .transpose()
    }

    pub(in crate::solve) fn signature(&mut self) -> Result<MeshQuotientSignature, CodecError> {
        let mut components = Vec::new();
        for node in 0..self.union.len() {
            if self.union.find(node)? != node {
                continue;
            }
            let mut members = self.members(node).to_vec();
            members.sort_unstable_by(Ord::cmp);
            let mut domain = Vec::new();
            for &point in self.domains[node].iter() {
                domain.push(point);
            }
            domain.sort_unstable_by(Ord::cmp);
            components.push((members, domain));
        }
        components.sort_unstable_by(Ord::cmp);
        Ok(components)
    }

    pub(in crate::solve) fn root_count(&mut self) -> Result<usize, CodecError> {
        let mut count = 0;
        for node in 0..self.union.len() {
            if self.union.find(node)? == node {
                count += 1;
            }
        }
        Ok(count)
    }

    pub(crate) fn merge(&mut self, left: usize, right: usize) -> Result<Option<usize>, CodecError> {
        let left = self.union.find(left)?;
        let right = self.union.find(right)?;
        if left == right {
            return Ok(Some(left));
        }
        let mut intersection = HashSet::new();
        for &point in self.domains[left].intersection(&self.domains[right]) {
            intersection.insert(point);
        }
        if intersection.is_empty() {
            return Ok(None);
        }
        let right_members = self.members[right]
            .as_ref()
            .map(Arc::clone)
            .ok_or_else(|| CodecError::malformed("quotient root has no member row"))?;
        let replacement = if self.members[left].as_mut().and_then(Arc::get_mut).is_some() {
            None
        } else {
            let mut members = Vec::new();

            members.extend_from_slice(self.members(left));
            members.extend_from_slice(&right_members);

            Some(Arc::new(members))
        };
        self.union.union(left, right)?;
        let root = self.union.find(left)?;
        self.domains[root] = Arc::new(intersection);
        if let Some(members) = replacement {
            self.members[root] = Some(members);
        } else {
            self.members[root]
                .as_mut()
                .and_then(Arc::get_mut)
                .ok_or_else(|| {
                    CodecError::malformed("quotient member row became shared during merge")
                })?
                .extend_from_slice(&right_members);
        }
        self.members[right] = None;
        Ok(Some(root))
    }

    pub(crate) fn edge_domains_viable(
        &mut self,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<bool, CodecError> {
        self.propagate_edge_domains(
            edge_candidates
                .iter()
                .enumerate()
                .filter_map(|(edge, candidates)| (!candidates.is_empty()).then_some(edge)),
            edge_candidates,
            None,
        )
    }

    pub(in crate::solve) fn prepare_coordinate_root_domains(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget>,
    ) -> Result<Option<MeshCoordinateRootDomains>, CodecError> {
        if edge_candidates.len().checked_mul(2) != Some(self.union.len()) {
            return Ok(None);
        }
        let mut roots = Vec::new();
        for node in 0..self.union.len() {
            if self.union.find(node)? == node {
                roots.push(node);
            }
        }
        if roots.len() < point_count {
            return Ok(None);
        }
        let mut root_indices = HashMap::new();
        for (index, root) in roots.iter().copied().enumerate() {
            root_indices.insert(root, index);
        }
        let mut edges = Vec::new();
        for edge in 0..edge_candidates.len() {
            let Some(&left) = root_indices.get(&self.union.find(edge * 2)?) else {
                return Ok(None);
            };
            let Some(&right) = root_indices.get(&self.union.find(edge * 2 + 1)?) else {
                return Ok(None);
            };
            edges.push([left, right]);
        }
        let mut domains = Vec::new();
        for root in roots.iter().copied() {
            let mut domain = Vec::new();
            for point in self.domains[root]
                .iter()
                .copied()
                .filter(|point| *point < point_count)
            {
                domain.push(point);
            }
            domain.sort_unstable_by(Ord::cmp);
            domains.push(domain);
        }
        if domains.iter().any(Vec::is_empty) {
            return Ok(None);
        }
        let mut edge_ids = Vec::new();
        for edge in 0..edges.len() {
            edge_ids.push(edge);
        }
        let mut root_edges = vec![Vec::new(); roots.len()];
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
            return Ok(None);
        }
        if !enforce_edge_arc_consistency(
            &mut domains,
            &edges,
            &edge_ids,
            &root_edges,
            edge_candidates,
            budget,
        ) {
            return Ok(None);
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
                    // An exhausted local slice leaves the quotient unprepared.
                    return Ok(None);
                }
                candidates.retain(|pair| {
                    (domains[left].binary_search(&pair[0]).is_ok()
                        && domains[right].binary_search(&pair[1]).is_ok())
                        || (domains[left].binary_search(&pair[1]).is_ok()
                            && domains[right].binary_search(&pair[0]).is_ok())
                });
                if candidates.is_empty() {
                    return Ok(None);
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
                return Ok(None);
            }
        }
        let Some(coverage_matching) =
            MeshCoordinateRootDomains::coverage_matching(ctx, &domains, point_count, budget)?
        else {
            return Ok(None);
        };
        let coordinate_domains = MeshCoordinateRootDomains {
            domains: Arc::new(domains),
            edges: Arc::new(edges),
            root_edges: Arc::new(root_edges),
            edge_candidates: Arc::new(supported_candidates),
            coverage_matching,
            point_count,
        };
        let Some(RefinedCoordinateDomains {
            domains,
            coverage_matching,
        }) = coordinate_domains.refine_domains(
            ctx,
            coordinate_domains.domains.to_vec(),
            &coordinate_domains.edge_candidates,
            // The full edge set already reached arc consistency above. This pass
            // starts with Hall support and only revisits edges narrowed by it.
            &[],
            true,
            budget,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(MeshCoordinateRootDomains {
            domains: Arc::new(domains),
            coverage_matching,
            ..coordinate_domains
        }))
    }

    pub(super) fn propagate_component_edge_domains(
        &mut self,
        root: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget>,
    ) -> Result<bool, CodecError> {
        let mut edges = HashSet::new();
        for edge in self.members(root).iter().map(|node| node / 2) {
            if !edge_candidates[edge].is_empty() {
                edges.insert(edge);
            }
        }
        self.propagate_edge_domains(edges, edge_candidates, budget)
    }

    pub(super) fn affected_edges_for_nodes(
        &mut self,
        nodes: &[usize],
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<HashSet<usize>, CodecError> {
        let mut affected = HashSet::new();
        for &node in nodes {
            let root = self.union.find(node)?;
            for edge in self.members(root).iter().map(|member| member / 2) {
                if !edge_candidates[edge].is_empty() {
                    affected.insert(edge);
                }
            }
        }
        Ok(affected)
    }

    pub(super) fn propagate_edge_domains(
        &mut self,
        edges: impl IntoIterator<Item = usize>,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget>,
    ) -> Result<bool, CodecError> {
        fn enqueue_component_edges(
            members: &[usize],
            edge_candidates: &[Vec<[usize; 2]>],
            queue: &mut VecDeque<usize>,
            queued: &mut HashSet<usize>,
        ) {
            for edge in members.iter().map(|node| node / 2) {
                if !edge_candidates[edge].is_empty() && !queued.contains(&edge) {
                    queued.insert(edge);
                    queue.push_back(edge);
                }
            }
        }

        let mut queue = VecDeque::new();
        let mut queued = HashSet::new();
        for edge in edges {
            if !queued.contains(&edge) {
                queued.insert(edge);
                queue.push_back(edge);
            }
        }
        while let Some(edge) = queue.pop_front() {
            queued.remove(&edge);
            let candidates = &edge_candidates[edge];
            if budget.is_some_and(|budget| !budget.charge_by(work_units(candidates.len()))) {
                return Ok(false);
            }
            if candidates.is_empty() {
                continue;
            }
            let start = self.union.find(edge * 2)?;
            let end = self.union.find(edge * 2 + 1)?;
            if start == end {
                let mut supported = HashSet::new();
                for pair in candidates {
                    if pair[0] == pair[1] && self.domains[start].contains(&pair[0]) {
                        supported.insert(pair[0]);
                    }
                }
                if supported.is_empty() {
                    return Ok(false);
                }
                if supported != *self.domains[start] {
                    self.domains[start] = Arc::new(supported);
                    enqueue_component_edges(
                        self.members(start),
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
                return Ok(false);
            }
            if supported_starts != *self.domains[start] {
                self.domains[start] = Arc::new(supported_starts);
                enqueue_component_edges(
                    self.members(start),
                    edge_candidates,
                    &mut queue,
                    &mut queued,
                );
            }
            if supported_ends != *self.domains[end] {
                self.domains[end] = Arc::new(supported_ends);
                enqueue_component_edges(
                    self.members(end),
                    edge_candidates,
                    &mut queue,
                    &mut queued,
                );
            }
        }
        Ok(true)
    }

    pub(in crate::solve) fn merge_singleton_coordinate_roots(
        &mut self,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<bool, CodecError> {
        loop {
            let mut roots_by_point = HashMap::<usize, Vec<usize>>::new();
            for node in 0..self.union.len() {
                let root = self.union.find(node)?;
                if root != node || self.domains[root].len() != 1 {
                    continue;
                }
                let Some(&point) = self.domains[root].iter().next() else {
                    return Ok(false);
                };
                if let Some(roots) = roots_by_point.get_mut(&point) {
                    roots.push(root);
                } else {
                    let roots = vec![root];
                    roots_by_point.insert(point, roots);
                }
            }
            let mut changed = false;
            let mut affected_edges = HashSet::new();
            for roots in roots_by_point.into_values() {
                let Some((&first, rest)) = roots.split_first() else {
                    continue;
                };
                if rest.is_empty() {
                    continue;
                }
                for edge in roots
                    .iter()
                    .flat_map(|&root| self.members(root))
                    .map(|node| node / 2)
                {
                    if !edge_candidates[edge].is_empty() {
                        affected_edges.insert(edge);
                    }
                }
                for &root in rest {
                    if self.merge(first, root)?.is_none() {
                        return Ok(false);
                    }
                    changed = true;
                }
            }
            if !changed {
                return Ok(true);
            }
            if !self.propagate_edge_domains(affected_edges, edge_candidates, None)? {
                return Ok(false);
            }
        }
    }

    pub(in crate::solve) fn close_coordinate_roots(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget>,
    ) -> Result<Option<HashMap<usize, usize>>, CodecError> {
        Ok(self
            .coordinate_root_closure_outcome(ctx, point_count, edge_candidates, None, budget)?
            .into_option())
    }

    #[cfg(test)]
    pub(in crate::solve) fn close_coordinate_roots_for_incidence_with_budget(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: MeshIncidenceBoundary<'_>,
        budget: Option<&WorkBudget>,
    ) -> Result<Option<HashMap<usize, usize>>, CodecError> {
        let MeshIncidenceBoundary {
            edge_faces,
            face_count,
            domains: boundary_domains,
        } = incidence;
        if !(edge_faces.len() == edge_candidates.len()
            && edge_faces.iter().flatten().all(|face| *face < face_count)
            && boundary_domains.len() == face_count)
        {
            return Ok(None);
        }
        Ok(self
            .coordinate_root_closure_outcome(
                ctx,
                point_count,
                edge_candidates,
                Some((edge_faces, boundary_domains)),
                budget,
            )?
            .into_option())
    }

    pub(in crate::solve) fn coordinate_root_closure_outcome_for_incidence(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: MeshIncidenceBoundary<'_>,
        budget: Option<&WorkBudget>,
    ) -> Result<CoordinateRootClosure, CodecError> {
        let MeshIncidenceBoundary {
            edge_faces,
            face_count,
            domains: boundary_domains,
        } = incidence;
        if edge_faces.len() != edge_candidates.len()
            || edge_faces.iter().flatten().any(|face| *face >= face_count)
            || boundary_domains.len() != face_count
        {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
        }
        self.coordinate_root_closure_outcome_with_component_budget(
            ctx,
            point_count,
            edge_candidates,
            Some((edge_faces, boundary_domains)),
            budget,
            Some(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
    }

    pub(super) fn coordinate_root_closure_outcome(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
        budget: Option<&WorkBudget>,
    ) -> Result<CoordinateRootClosure, CodecError> {
        self.coordinate_root_closure_outcome_with_component_budget(
            ctx,
            point_count,
            edge_candidates,
            incidence,
            budget,
            None,
        )
    }

    pub(super) fn coordinate_root_closure_outcome_with_component_budget(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
        budget: Option<&WorkBudget>,
        component_search_budget: Option<usize>,
    ) -> Result<CoordinateRootClosure, CodecError> {
        let ambiguous = Cell::new(false);
        let exhausted = Cell::new(false);
        let result = super::coordinate_assignment::close_coordinate_roots_with_incidence(
            ctx,
            super::coordinate_assignment::CloseCoordinateRootsWithIncidenceInputs {
                quotient: self,
                point_count,
                edge_candidates,
                incidence,
                budget,
                component_search_budget,
                ambiguous: &ambiguous,
                exhausted: &exhausted,
            },
        )?;
        Ok(match result {
            Some(assignment) => MeshSolve::Solved(assignment),
            None if exhausted.get() || budget.is_some_and(WorkBudget::exhausted) => {
                MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))
            }
            None if ambiguous.get() => MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())),
            None => MeshSolve::Failed(MeshCandidateFailure::Rejected(())),
        })
    }

    pub(super) fn assignment_has_option(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget>,
    ) -> Result<bool, CodecError> {
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
        ) -> Result<bool, CodecError> {
            if state.at > 0 {
                let Some(previous_end) =
                    edge_end(boundary[state.at - 1], state.directions[state.at - 1])
                else {
                    return Ok(false);
                };
                let Some(current_start) = edge_start(boundary[state.at], reversed) else {
                    return Ok(false);
                };
                if state.quotient.merge(previous_end, current_start)?.is_none() {
                    return Ok(false);
                }
            }
            state.directions.push(reversed);
            state.at += 1;
            Ok(true)
        }

        let mut states = Vec::new();
        states.push(State {
            boundary_index: 0,
            at: 0,
            directions: Vec::new(),
            quotient: self.clone(),
        });
        while let Some(mut state) = states.pop() {
            loop {
                if budget.is_some_and(|budget| !budget.charge()) {
                    return Ok(false);
                }
                if state.boundary_index == assignment.boundaries.len() {
                    return Ok(true);
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
                    if state.quotient.merge(last_end, first_start)?.is_none() {
                        break;
                    }
                    if !state.quotient.edge_domains_viable(edge_candidates)? {
                        break;
                    }
                    state.boundary_index += 1;
                    state.at = 0;
                    state.directions.clear();
                    continue;
                }
                if let Some(reversed) = boundary[state.at].reversed {
                    if !advance(&mut state, boundary, reversed)? {
                        break;
                    }
                    continue;
                }
                for reversed in [true, false] {
                    if budget.is_some_and(|budget| !budget.charge()) {
                        return Ok(false);
                    }
                    let mut next = State {
                        boundary_index: state.boundary_index,
                        at: state.at,
                        directions: state.directions.clone(),
                        quotient: state.quotient.clone(),
                    };
                    if advance(&mut next, boundary, reversed)?
                        && next.quotient.edge_domains_viable(edge_candidates)?
                    {
                        states.push(next);
                    }
                }
                break;
            }
        }
        Ok(false)
    }

    #[cfg(test)]
    pub(super) fn assignment_options(
        &self,
        ctx: &DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Vec<(Vec<Vec<bool>>, Self)> {
        const MAX_ORIENTED_OPTIONS: usize = 4_096;

        fn boundary_options(
            ctx: &DecodeContext<'_>,
            quotient: MeshQuotient,
            boundary: &[MeshBoundaryEdgeCandidate],
            edge_candidates: &[Vec<[usize; 2]>],
        ) -> Vec<(Vec<bool>, MeshQuotient)> {
            fn advance(
                resources: (&DecodeContext<'_>, &[Vec<[usize; 2]>]),
                boundary: &[MeshBoundaryEdgeCandidate],
                at: usize,
                reversed: bool,
                directions: &mut Vec<bool>,
                mut quotient: MeshQuotient,
                output: &mut Vec<(Vec<bool>, MeshQuotient)>,
            ) {
                let (ctx, edge_candidates) = resources;
                if at > 0 {
                    let Some(previous_end) = edge_end(boundary[at - 1], directions[at - 1]) else {
                        return;
                    };
                    let Some(current_start) = edge_start(boundary[at], reversed) else {
                        return;
                    };
                    let Some(root) = quotient
                        .merge(previous_end, current_start)
                        .expect("merge the components")
                    else {
                        return;
                    };
                    if !quotient
                        .propagate_component_edge_domains(root, edge_candidates, None)
                        .expect("propagate component edge domains")
                    {
                        return;
                    }
                }
                directions.push(reversed);
                walk(
                    ctx,
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
                ctx: &DecodeContext<'_>,
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
                    let Some(root) = quotient
                        .merge(last_end, first_start)
                        .expect("merge the components")
                    else {
                        return;
                    };
                    if quotient
                        .propagate_component_edge_domains(root, edge_candidates, None)
                        .expect("propagate component edge domains")
                    {
                        output.push((directions.clone(), quotient));
                    }
                    return;
                }
                if let Some(reversed) = boundary[at].reversed {
                    advance(
                        (ctx, edge_candidates),
                        boundary,
                        at,
                        reversed,
                        directions,
                        quotient,
                        output,
                    );
                } else {
                    advance(
                        (ctx, edge_candidates),
                        boundary,
                        at,
                        false,
                        directions,
                        quotient.clone(),
                        output,
                    );
                    advance(
                        (ctx, edge_candidates),
                        boundary,
                        at,
                        true,
                        directions,
                        quotient,
                        output,
                    );
                }
            }

            if boundary.is_empty() {
                return Vec::new();
            }
            let mut output = Vec::new();
            walk(
                ctx,
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
                    boundary_options(ctx, quotient, boundary, edge_candidates)
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

    pub(in crate::solve) fn assignment_options_limited(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        oriented_edges: &HashSet<usize>,
        limit: usize,
        budget: &WorkBudget,
    ) -> Result<Vec<MeshOrientationOption>, CodecError> {
        struct BoundaryOrientationSearch<
            'input0,
            'input1,
            'input2,
            'input3,
            'input4,
            'input5,
            'input6,
            'input7,
            'input9,
        > {
            boundaries: &'input0 [Vec<MeshBoundaryEdgeCandidate>],
            boundary_index: usize,
            at: usize,
            boundary_directions: &'input1 mut Vec<bool>,
            directions: &'input2 mut Vec<Vec<bool>>,
            quotient: MeshQuotient,
            edge_candidates: &'input3 [Vec<[usize; 2]>],
            output: &'input4 mut Vec<(Vec<Vec<bool>>, MeshQuotient)>,
            seen: &'input5 mut HashMap<u64, Vec<usize>>,
            oriented: &'input6 mut HashSet<usize>,
            gaugeable_edges: &'input7 HashSet<usize>,
            limit: usize,
            budget: &'input9 WorkBudget,
        }

        fn walk(
            inputs: BoundaryOrientationSearch<'_, '_, '_, '_, '_, '_, '_, '_, '_>,
        ) -> Result<(), CodecError> {
            let BoundaryOrientationSearch {
                boundaries,
                boundary_index,
                at,
                boundary_directions,
                directions,
                mut quotient,
                edge_candidates,
                output,
                seen,
                oriented,
                gaugeable_edges,
                limit,
                budget,
            } = inputs;

            let Ok(_depth) = budget.recursion_guard() else {
                return Ok(());
            };
            if output.len() >= limit {
                return Ok(());
            }
            if !budget.charge() {
                return Ok(());
            }
            if boundary_index == boundaries.len() {
                if admit_orientation_option(seen, output, directions, &quotient)? {
                    output.push((directions.clone(), quotient));
                }
                return Ok(());
            }
            let boundary = &boundaries[boundary_index];
            if boundary.is_empty() {
                return Ok(());
            }
            if at == boundary.len() {
                let Some(last_end) = edge_end(boundary[at - 1], boundary_directions[at - 1]) else {
                    return Ok(());
                };
                let Some(first_start) = edge_start(boundary[0], boundary_directions[0]) else {
                    return Ok(());
                };
                let Some(root) = quotient.merge(last_end, first_start)? else {
                    return Ok(());
                };
                if !quotient.propagate_component_edge_domains(
                    root,
                    edge_candidates,
                    Some(budget),
                )? {
                    return Ok(());
                }
                directions.push(std::mem::take(boundary_directions));
                walk(BoundaryOrientationSearch {
                    boundaries,
                    boundary_index: boundary_index + 1,
                    at: 0,
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
                })?;
                *boundary_directions = directions.pop().unwrap_or_default();
                return Ok(());
            }
            let edge = boundary[at].edge;
            let first = oriented.insert(edge);
            let mut advance =
                |reversed: bool, mut quotient: MeshQuotient| -> Result<(), CodecError> {
                    if at > 0 {
                        let Some(previous_end) =
                            edge_end(boundary[at - 1], boundary_directions[at - 1])
                        else {
                            return Ok(());
                        };
                        let Some(current_start) = edge_start(boundary[at], reversed) else {
                            return Ok(());
                        };
                        let Some(root) = quotient.merge(previous_end, current_start)? else {
                            return Ok(());
                        };
                        if !quotient.propagate_component_edge_domains(
                            root,
                            edge_candidates,
                            Some(budget),
                        )? {
                            return Ok(());
                        }
                    }
                    boundary_directions.push(reversed);
                    walk(BoundaryOrientationSearch {
                        boundaries,
                        boundary_index,
                        at: at + 1,
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
                    })?;
                    boundary_directions.pop();
                    Ok(())
                };
            match (boundary[at].reversed, first) {
                (Some(reversed), _) => advance(reversed, quotient)?,
                (None, true) if gaugeable_edges.contains(&edge) => advance(false, quotient)?,
                (None, _) => {
                    advance(false, quotient.clone())?;
                    advance(true, quotient)?;
                }
            }
            if first {
                oriented.remove(&edge);
            }
            Ok(())
        }

        if limit == 0 {
            return Ok(Vec::new());
        }
        if assignment.boundaries.iter().any(Vec::is_empty) {
            return Ok(Vec::new());
        }
        // Fix a new edge's direction only while its two endpoint labels remain
        // exchangeable. Distinct domains or prior quotient merges make the
        // direction observable and require both orientations.
        let mut direction_union = self.union.clone();
        let mut gaugeable_edges = HashSet::new();
        for edge in assignment.boundaries.iter().flatten().map(|use_| use_.edge) {
            let Some(right_node) = edge.checked_mul(2).and_then(|node| node.checked_add(1)) else {
                continue;
            };
            if right_node >= self.domains.len() {
                continue;
            }
            let left_node = right_node - 1;
            let left_root = direction_union.find(left_node)?;
            let right_root = direction_union.find(right_node)?;
            if left_root == right_root
                || (self.domains[left_root] == self.domains[right_root]
                    && self.members(left_root) == [left_node]
                    && self.members(right_root) == [right_node])
            {
                gaugeable_edges.insert(edge);
            }
        }
        let mut oriented = HashSet::new();

        oriented.extend(oriented_edges.iter().copied());
        let mut variable_count = 0usize;
        let mut orientation_plan = Vec::new();

        for boundary in &assignment.boundaries {
            let mut row = Vec::new();

            for use_ in boundary {
                row.push(match use_.reversed {
                    Some(reversed) => (reversed, None),
                    None if oriented.insert(use_.edge) && gaugeable_edges.contains(&use_.edge) => {
                        (false, None)
                    }
                    None => {
                        let variable = variable_count;
                        variable_count += 1;
                        (false, Some(variable))
                    }
                });
            }
            orientation_plan.push(row);
        }
        if variable_count <= 8 {
            let mut output = Vec::new();
            let mut seen = HashMap::new();
            let combinations = 1usize << variable_count;
            let orientation_work =
                work_units(assignment.boundaries.iter().map(Vec::len).sum::<usize>());
            for mask in 0..combinations {
                if output.len() >= limit {
                    break;
                }
                if !budget.charge_by(orientation_work) {
                    break;
                }
                let mut directions = Vec::new();

                for boundary in &orientation_plan {
                    let mut row = Vec::new();

                    for (fixed, variable) in boundary {
                        row.push(variable.map_or(*fixed, |variable| {
                            let shift = variable_count - variable - 1;
                            mask & (1usize << shift) != 0
                        }));
                    }
                    directions.push(row);
                }
                let mut oriented = HashSet::new();

                oriented.extend(oriented_edges.iter().copied());
                let mut uses_gauge = true;
                for (boundary, row) in assignment.boundaries.iter().zip(&directions) {
                    for (use_, direction) in boundary.iter().zip(row) {
                        let first = oriented.insert(use_.edge);
                        if first
                            && use_.reversed.is_none()
                            && gaugeable_edges.contains(&use_.edge)
                            && *direction
                        {
                            uses_gauge = false;
                        }
                    }
                }
                if !uses_gauge {
                    continue;
                }
                let mut quotient = self.clone();
                let mut merged_nodes = Vec::new();
                let mut merged = true;
                'merge: for (boundary, row) in assignment.boundaries.iter().zip(&directions) {
                    for index in 0..boundary.len() {
                        let next = (index + 1) % boundary.len();
                        let Some(left_end) = edge_end(boundary[index], row[index]) else {
                            merged = false;
                            break 'merge;
                        };
                        let Some(right_start) = edge_start(boundary[next], row[next]) else {
                            merged = false;
                            break 'merge;
                        };
                        let Some(root) = quotient.merge(left_end, right_start)? else {
                            merged = false;
                            break 'merge;
                        };
                        merged_nodes.push(root);
                    }
                }
                if !merged {
                    continue;
                }
                let affected_edges =
                    quotient.affected_edges_for_nodes(&merged_nodes, edge_candidates)?;
                if !quotient.propagate_edge_domains(
                    affected_edges,
                    edge_candidates,
                    Some(budget),
                )? {
                    continue;
                }
                if admit_orientation_option(&mut seen, &output, &directions, &quotient)? {
                    output.push((directions, quotient));
                }
            }
            return Ok(output);
        }
        let mut output = Vec::new();
        let mut seen = HashMap::<u64, Vec<usize>>::new();
        let mut oriented = HashSet::new();

        oriented.extend(oriented_edges.iter().copied());
        walk(BoundaryOrientationSearch {
            boundaries: &assignment.boundaries,
            boundary_index: 0,
            at: 0,
            boundary_directions: &mut Vec::new(),
            directions: &mut Vec::new(),
            quotient: self.clone(),
            edge_candidates,
            output: &mut output,
            seen: &mut seen,
            oriented: &mut oriented,
            gaugeable_edges: &gaugeable_edges,
            limit,
            budget,
        })?;
        Ok(output)
    }

    pub(super) fn assignment_options_for_directions(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        direction_options: &MeshFaceDirectionOptions,
        limit: usize,
        budget: Option<&WorkBudget>,
    ) -> Result<Vec<MeshOrientationOption>, CodecError> {
        if limit == 0
            || assignment.boundaries.len() != direction_options.first().map_or(0, Vec::len)
        {
            return Ok(Vec::new());
        }
        let work = work_units(assignment.boundaries.iter().map(Vec::len).sum::<usize>());
        let mut output = Vec::new();
        let mut seen = HashMap::<u64, Vec<usize>>::new();
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
            let mut merged = true;
            'boundaries: for (boundary, boundary_directions) in
                assignment.boundaries.iter().zip(directions)
            {
                if boundary.is_empty()
                    || boundary
                        .iter()
                        .zip(boundary_directions)
                        .any(|(use_, direction)| {
                            use_.reversed.is_some_and(|required| required != *direction)
                        })
                {
                    merged = false;
                    break;
                }
                for index in 0..boundary.len() {
                    let next = (index + 1) % boundary.len();
                    let Some(left_end) = edge_end(boundary[index], boundary_directions[index])
                    else {
                        merged = false;
                        break 'boundaries;
                    };
                    let Some(right_start) = edge_start(boundary[next], boundary_directions[next])
                    else {
                        merged = false;
                        break 'boundaries;
                    };
                    if quotient.merge(left_end, right_start)?.is_none() {
                        merged = false;
                        break 'boundaries;
                    }
                }
            }
            if !merged {
                continue;
            }
            if admit_orientation_option(&mut seen, &output, directions, &quotient)? {
                output.push((directions.clone(), quotient));
            }
        }
        Ok(output)
    }

    pub(super) fn merge_label_directions_in_place(
        &mut self,
        assignment: &MeshFaceBoundaryAssignment,
        label_directions: &[Vec<bool>],
        edge_orientations: &[Option<bool>],
        budget: Option<&WorkBudget>,
    ) -> Result<Option<Vec<Vec<bool>>>, CodecError> {
        if assignment.boundaries.len() != label_directions.len()
            || label_directions
                .iter()
                .zip(&assignment.boundaries)
                .any(|(directions, boundary)| directions.len() != boundary.len())
        {
            return Ok(None);
        }
        let work = work_units(assignment.boundaries.iter().map(Vec::len).sum::<usize>());
        if budget.is_some_and(|budget| !budget.charge_by(work)) {
            return Ok(None);
        }
        let mut directions = Vec::new();

        for (boundary, labels) in assignment.boundaries.iter().zip(label_directions) {
            let mut row = Vec::new();

            for (use_, &label_direction) in boundary.iter().zip(labels) {
                let Some(orientation) = edge_orientations.get(use_.edge).copied().flatten() else {
                    return Ok(None);
                };
                let direction = orientation ^ label_direction;
                if use_.reversed.is_some_and(|required| required != direction) {
                    return Ok(None);
                }
                row.push(direction);
            }
            directions.push(row);
        }
        let mut merged = true;
        'boundaries: for (boundary, boundary_directions) in
            assignment.boundaries.iter().zip(&directions)
        {
            if boundary.is_empty() {
                merged = false;
                break;
            }
            for index in 0..boundary.len() {
                let next = (index + 1) % boundary.len();
                let Some(left_end) = edge_end(boundary[index], boundary_directions[index]) else {
                    merged = false;
                    break 'boundaries;
                };
                let Some(right_start) = edge_start(boundary[next], boundary_directions[next])
                else {
                    merged = false;
                    break 'boundaries;
                };
                if self.merge(left_end, right_start)?.is_none() {
                    merged = false;
                    break 'boundaries;
                }
            }
        }
        if !merged {
            return Ok(None);
        }
        Ok(Some(directions))
    }

    pub(super) fn assignment_option_for_label_directions(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        label_directions: &[Vec<bool>],
        edge_orientations: &[Option<bool>],
        budget: Option<&WorkBudget>,
    ) -> Result<Option<MeshOrientationOption>, CodecError> {
        let mut quotient = self.clone();
        let directions = quotient.merge_label_directions_in_place(
            assignment,
            label_directions,
            edge_orientations,
            budget,
        )?;
        Ok(directions.map(|directions| (directions, quotient)))
    }

    pub(crate) fn point_assignment(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget>,
    ) -> Result<Option<HashMap<usize, usize>>, CodecError> {
        Ok(
            match self.point_assignments_with_budget(
                ctx,
                point_count,
                edge_candidates,
                2,
                budget,
            )? {
                PointAssignmentOutcome::Complete(mut solutions) => {
                    (solutions.len() == 1).then(|| solutions.remove(0))
                }
                PointAssignmentOutcome::Exhausted => None,
            },
        )
    }

    pub(in crate::solve) fn point_assignment_exists(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget>,
    ) -> Result<bool, CodecError> {
        Ok(matches!(
            self.point_assignments_with_budget(ctx, point_count, edge_candidates, 1, budget)?,
            PointAssignmentOutcome::Complete(solutions) if !solutions.is_empty()
        ))
    }

    pub(super) fn point_assignments_with_budget(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        solution_limit: usize,
        budget: Option<&WorkBudget>,
    ) -> Result<PointAssignmentOutcome, CodecError> {
        type PointNeighbors = HashMap<usize, HashSet<usize>>;

        fn remaining_domains_match(
            ctx: &DecodeContext<'_>,
            values: &[(usize, Vec<usize>)],
            point_count: usize,
        ) -> Result<bool, CodecError> {
            domains_have_distinct_matching(
                ctx,
                values.iter().map(|(_, values)| values.as_slice()),
                point_count,
            )
        }
        #[derive(Clone, Copy)]
        struct CoordinateValueSupport<'input0, 'input1, 'input2, 'input3, 'input4, 'input5, 'input6> {
            root: usize,
            point: usize,
            domains: &'input0 [Arc<HashSet<usize>>],
            edge_roots: &'input1 [[usize; 2]],
            root_edges: &'input2 [Vec<usize>],
            edge_candidates: &'input3 [Vec<[usize; 2]>],
            edge_neighbors: &'input4 [PointNeighbors],
            assigned: &'input5 [Option<usize>],
            used: &'input6 HashSet<usize>,
        }

        fn value_viable(inputs: CoordinateValueSupport<'_, '_, '_, '_, '_, '_, '_>) -> bool {
            let CoordinateValueSupport {
                root,
                point,
                domains,
                edge_roots,
                root_edges,
                edge_candidates,
                edge_neighbors,
                assigned,
                used,
            } = inputs;

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

        struct CoordinateAssignmentSearch<
            'input0,
            'input1,
            'input2,
            'input3,
            'input4,
            'input5,
            'input6,
            'input7,
            'input8,
            'input9,
        > {
            domains: &'input0 [Arc<HashSet<usize>>],
            edge_roots: &'input1 [[usize; 2]],
            root_edges: &'input2 [Vec<usize>],
            edge_candidates: &'input3 [Vec<[usize; 2]>],
            edge_neighbors: &'input4 [PointNeighbors],
            assigned: &'input5 mut [Option<usize>],
            used: &'input6 mut HashSet<usize>,
            solutions: &'input7 mut Vec<Vec<usize>>,
            solution_limit: usize,
            budget: Option<&'input9 WorkBudget>,
            exhausted: &'input8 mut bool,
            depth: usize,
        }

        fn walk(
            ctx: &DecodeContext<'_>,
            inputs: CoordinateAssignmentSearch<'_, '_, '_, '_, '_, '_, '_, '_, '_, '_>,
        ) -> Result<(), CodecError> {
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

            let CoordinateAssignmentSearch {
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
                exhausted,
                depth,
            } = inputs;

            if solutions.len() >= solution_limit || *exhausted {
                return Ok(());
            }
            if depth >= WorkBudget::RECURSION_DEPTH {
                if let Some(budget) = budget {
                    budget.exhaust();
                }
                *exhausted = true;
                return Ok(());
            }
            if budget.is_some_and(|budget| !budget.charge()) {
                return Ok(());
            }
            let values_for = |root: usize,
                              assigned: &[Option<usize>],
                              used: &HashSet<usize>|
             -> Result<Vec<usize>, CodecError> {
                let mut values = Vec::new();
                for point in domains[root]
                    .iter()
                    .copied()
                    .filter(|point| !used.contains(point))
                {
                    if value_viable(CoordinateValueSupport {
                        root,
                        point,
                        domains,
                        edge_roots,
                        root_edges,
                        edge_candidates,
                        edge_neighbors,
                        assigned,
                        used,
                    }) {
                        values.push(point);
                    }
                }
                Ok(values)
            };
            let mut propagated = Vec::new();
            let branch = loop {
                let mut values = Vec::new();
                for (root, _) in assigned
                    .iter()
                    .enumerate()
                    .filter(|(_, point)| point.is_none())
                {
                    let candidates = values_for(root, assigned, used)?;
                    values.push((root, candidates));
                }
                if values.is_empty() {
                    break Some(None);
                }
                if values.iter().any(|(_, values)| values.is_empty())
                    || !remaining_domains_match(ctx, &values, assigned.len())?
                {
                    break None;
                }
                let mut dead = false;
                let mut progress = false;
                for root in 0..assigned.len() {
                    if assigned[root].is_some() {
                        continue;
                    }
                    let values = values_for(root, assigned, used)?;
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
                return Ok(());
            };
            let Some((root, values)) = branch else {
                if assigned.iter().all(Option::is_some) {
                    let mut solution = Vec::new();

                    for value in assigned.iter().flatten() {
                        solution.push(*value);
                    }
                    solutions.push(solution);
                }
                rollback(assigned, used, propagated);
                return Ok(());
            };
            for point in values {
                assigned[root] = Some(point);
                used.insert(point);
                walk(
                    ctx,
                    CoordinateAssignmentSearch {
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
                        exhausted,
                        depth: depth + 1,
                    },
                )?;
                used.remove(&point);
                assigned[root] = None;
                if solutions.len() >= solution_limit {
                    break;
                }
            }
            rollback(assigned, used, propagated);
            Ok(())
        }

        let mut roots = Vec::new();
        for node in 0..self.union.len() {
            let root = self.union.find(node)?;
            if root == node {
                roots.push(root);
            }
        }
        if roots.len() != point_count {
            return Ok(PointAssignmentOutcome::Complete(Vec::new()));
        }
        let mut domains = Vec::new();

        let mut root_indices = HashMap::new();
        for (index, root) in roots.iter().copied().enumerate() {
            domains.push(Arc::clone(&self.domains[root]));
            root_indices.insert(root, index);
        }
        let mut edge_roots = Vec::new();

        for edge in 0..edge_candidates.len() {
            let (Some(left), Some(right)) = (
                root_indices.get(&self.union.find(edge * 2)?),
                root_indices.get(&self.union.find(edge * 2 + 1)?),
            ) else {
                return Ok(PointAssignmentOutcome::Complete(Vec::new()));
            };
            edge_roots.push([*left, *right]);
        }
        let mut root_edges = vec![Vec::new(); roots.len()];
        for (edge_index, edge) in edge_roots.iter().enumerate() {
            root_edges[edge[0]].push(edge_index);
            if edge[1] != edge[0] {
                root_edges[edge[1]].push(edge_index);
            }
        }
        let mut edge_neighbors = Vec::new();

        for candidates in edge_candidates {
            let mut neighbors = PointNeighbors::new();
            for [left, right] in candidates {
                for (from, to) in [(*left, *right), (*right, *left)] {
                    neighbors.entry(from).or_default().insert(to);
                }
            }
            edge_neighbors.push(neighbors);
        }

        let mut solutions = Vec::new();
        let mut assigned = vec![None; domains.len()];
        let mut used = HashSet::new();
        let mut exhausted = false;
        walk(
            ctx,
            CoordinateAssignmentSearch {
                domains: &domains,
                edge_roots: &edge_roots,
                root_edges: &root_edges,
                edge_candidates,
                edge_neighbors: &edge_neighbors,
                assigned: &mut assigned,
                used: &mut used,
                solutions: &mut solutions,
                solution_limit,
                budget,
                exhausted: &mut exhausted,
                depth: 0,
            },
        )?;
        if exhausted || budget.is_some_and(WorkBudget::exhausted) {
            Ok(PointAssignmentOutcome::Exhausted)
        } else {
            let mut completed = Vec::new();

            for solution in solutions {
                let mut pairs = HashMap::new();
                for (root, point) in roots.iter().copied().zip(solution) {
                    pairs.insert(root, point);
                }
                completed.push(pairs);
            }
            Ok(PointAssignmentOutcome::Complete(completed))
        }
    }
}

#[cfg(test)]
#[test]
fn quotient_merge_preserves_shared_snapshot_members() {
    let domain = Arc::new(HashSet::from([0usize]));
    let quotient = MeshQuotient::new(vec![domain; 3]);
    let mut copy = quotient.clone();
    assert_eq!(copy.merge(0, 1).expect("merge storage"), Some(0));
    assert_eq!(copy.members(0), &[0, 1]);
    assert_eq!(quotient.members(0), &[0]);
    assert_eq!(quotient.members(1), &[1]);
    assert!(copy.members(1).is_empty());
    assert!(Arc::ptr_eq(
        quotient.members[2]
            .as_ref()
            .expect("source root member row"),
        copy.members[2].as_ref().expect("snapshot root member row")
    ));
}

#[test]
fn coordinate_root_closure_distinguishes_symmetric_assignments() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(vec![
        Arc::new(HashSet::from([0, 1])),
        Arc::new(HashSet::from([0, 1])),
    ]);
    let outcome = quotient
        .coordinate_root_closure_outcome(&ctx, 2, &[vec![[0, 1]]], None, None)
        .expect("resolve the coordinate root closure");

    assert_eq!(
        outcome,
        MeshSolve::Failed(MeshCandidateFailure::Ambiguous(()))
    );
}

#[test]
fn coordinate_root_closure_rejects_a_single_prefix_after_budget_refusal() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(vec![
        Arc::new(HashSet::from([0, 1])),
        Arc::new(HashSet::from([0, 1])),
    ]);
    let budget = WorkBudget::new(2);

    assert_eq!(
        quotient
            .coordinate_root_closure_outcome(&ctx, 2, &[vec![[0, 1]]], None, Some(&budget),)
            .expect("resolve the coordinate root closure"),
        MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))
    );
    assert!(budget.exhausted());
}

#[test]
fn coordinate_root_closure_rejects_a_refused_incidence_check() {
    catia_test_context!(ctx);
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
        let mut quotient = MeshQuotient::new(
            (0..4)
                .map(|node| Arc::new(HashSet::from([usize::from(node % 2 != 0)])))
                .collect(),
        );
        quotient
            .merge(0, 2)
            .expect("merge the components")
            .expect("shared left endpoint");
        quotient
            .merge(1, 3)
            .expect("merge the components")
            .expect("shared right endpoint");
        quotient
    };
    let refused_budget = WorkBudget::new(38);
    let refused = make_quotient()
        .coordinate_root_closure_outcome(
            &ctx,
            2,
            &edge_candidates,
            Some((&edge_faces, &boundary_domains)),
            Some(&refused_budget),
        )
        .expect("resolve the coordinate root closure");
    assert_eq!(
        refused,
        MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))
    );
    assert!(refused_budget.exhausted());
    let complete_budget = WorkBudget::new(39);
    let complete = make_quotient()
        .coordinate_root_closure_outcome(
            &ctx,
            2,
            &edge_candidates,
            Some((&edge_faces, &boundary_domains)),
            Some(&complete_budget),
        )
        .expect("resolve the coordinate root closure");
    assert!(matches!(complete, MeshSolve::Solved(_)));
    assert!(!complete_budget.exhausted());
}

#[test]
fn coordinate_root_closure_walk_does_not_charge_the_session_recursion_ceiling() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let edge_candidates = vec![vec![[0, 1]], vec![[0, 1]]];
    let edge_faces = [[0, 1], [0, 1]];
    let boundary = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 0,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 0,
                end: 0,
                reversed: None,
            },
        ]],
    };
    let boundary_domains = vec![
        MeshFaceBoundaryDomain::Ordered(vec![boundary.clone()]),
        MeshFaceBoundaryDomain::Ordered(vec![boundary]),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture fits input limit");
    let mut quotient = MeshQuotient::new(
        (0..4)
            .map(|node| Arc::new(HashSet::from([usize::from(node % 2 != 0)])))
            .collect(),
    );
    quotient
        .merge(0, 2)
        .expect("merge the components")
        .expect("shared left endpoint");
    quotient
        .merge(1, 3)
        .expect("merge the components")
        .expect("shared right endpoint");
    let result = quotient.coordinate_root_closure_outcome(
        &ctx,
        2,
        &edge_candidates,
        Some((&edge_faces, &boundary_domains)),
        None,
    );
    assert!(result.is_ok());
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn coordinate_root_component_closures_are_charged_to_their_caller() {
    const COMPONENT_COUNT: usize = 8;
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(
        (0..COMPONENT_COUNT)
            .flat_map(|component| {
                let points = Arc::new((component * 3..component * 3 + 3).collect::<HashSet<_>>());
                std::iter::repeat_n(points, 6)
            })
            .collect(),
    );
    let mut candidates = Vec::new();
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        let point = component * 3;
        quotient
            .merge(node + 1, node + 2)
            .expect("merge the components")
            .expect("disjoint coordinate roots merge");
        quotient
            .merge(node + 3, node + 4)
            .expect("merge the components")
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
        shared
            .coordinate_root_closure_outcome(
                &ctx,
                COMPONENT_COUNT * 3,
                &candidates,
                None,
                Some(&shared_budget),
            )
            .expect("resolve the coordinate root closure"),
        MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))
    );

    let shared_incidence_budget = WorkBudget::new(100);
    let mut shared_incidence = quotient.clone();
    assert!(shared_incidence
        .close_coordinate_roots_for_incidence_with_budget(
            &ctx,
            COMPONENT_COUNT * 3,
            &candidates,
            MeshIncidenceBoundary {
                edge_faces: &edge_faces,
                face_count: COMPONENT_COUNT,
                domains: &boundary_domains,
            },
            Some(&shared_incidence_budget),
        )
        .expect("close the coordinate roots for incidence")
        .is_none());
    assert!(shared_incidence_budget.exhausted());

    let closure_for_incidence = |budget: &WorkBudget| {
        let mut quotient = quotient.clone();
        cadmpeg_core::decode::test_support::work_charged(|| {
            quotient
                .coordinate_root_closure_outcome_for_incidence(
                    &ctx,
                    COMPONENT_COUNT * 3,
                    &candidates,
                    MeshIncidenceBoundary {
                        edge_faces: &edge_faces,
                        face_count: COMPONENT_COUNT,
                        domains: &boundary_domains,
                    },
                    Some(budget),
                )
                .expect("compute the coordinate root closure outcome for incidence")
        })
    };
    let small_budget = WorkBudget::new(100);
    let (outcome, charged) = closure_for_incidence(&small_budget);
    assert_eq!(
        outcome,
        MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))
    );
    assert!(
        charged <= 100,
        "component closures charged {charged} of 100"
    );

    let preparation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let (outcome, charged) = closure_for_incidence(&preparation_budget);
    assert!(matches!(outcome, MeshSolve::Solved(_)), "{outcome:?}");
    assert!(!preparation_budget.exhausted());
    assert_eq!(
        charged,
        u64::try_from(preparation_budget.consumed()).expect("consumed fits u64")
    );
}

#[cfg(test)]
mod direct_matching_tests {
    use super::{initial_mesh_quotient, MAX_MESH_CONSTRAINT_OPERATIONS};
    use crate::families::standard::topology::EdgeBoundaryLayout;
    use crate::families::standard::topology::EdgeRow;
    use crate::families::standard::topology::StandardTopologyDraft;
    use crate::solve::mesh_quotient::selection_search::resolve_mesh_selection_from_quotient;
    use cadmpeg_core::decode::WorkBudget;

    #[test]
    fn direct_mesh_quotient_defers_non_unique_matching() {
        use cadmpeg_core::decode::DecodeContext;

        catia_test_context!(ctx);
        let edge_rows = (0..3)
            .map(|edge| {
                assert!(EdgeRow::new(
                    1,
                    vec![u32::try_from(edge).expect("fixture value fits u32")],
                    EdgeBoundaryLayout::CompleteBoundaryRun
                )
                .is_none());
                EdgeRow::new(
                    1,
                    vec![
                        u32::try_from(edge).expect("fixture value fits u32"),
                        u32::try_from(edge).expect("fixture value fits u32"),
                    ],
                    EdgeBoundaryLayout::CompleteBoundaryRun,
                )
                .expect("admitted edge row")
            })
            .collect::<Vec<_>>();
        let topology = StandardTopologyDraft {
            faces: vec![crate::families::standard::topology::FaceTopologyDraft {
                boundaries: vec![
                    crate::families::standard::topology::BoundaryDraft::new(vec![
                        crate::families::standard::topology::CoedgeUse {
                            edge_row: 0,
                            reversed: false,
                            start_vertex: 0,
                            end_vertex: 1,
                        },
                        crate::families::standard::topology::CoedgeUse {
                            edge_row: 1,
                            reversed: false,
                            start_vertex: 1,
                            end_vertex: 2,
                        },
                        crate::families::standard::topology::CoedgeUse {
                            edge_row: 2,
                            reversed: false,
                            start_vertex: 2,
                            end_vertex: 0,
                        },
                    ])
                    .expect("nonempty topology boundary"),
                ],
            }],
            edge_rows,
            vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            logical_vertex_count: 3,
        };
        let edge_candidates = vec![
            vec![[0, 1], [0, 2], [1, 2]],
            vec![[0, 1], [0, 2], [1, 2]],
            vec![[0, 1], [0, 2], [1, 2]],
        ];
        let port_identities = vec![[0, 1], [1, 2], [2, 0]];
        let quotient = initial_mesh_quotient(&edge_candidates, 3, &port_identities)
            .expect("build the initial mesh quotient")
            .expect("triangle quotient");
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

        assert!(resolve_mesh_selection_from_quotient(
            &ctx,
            topology.clone(),
            quotient,
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            &edge_candidates,
            &port_identities,
            &budget,
        )
        .expect("resolve mesh selection from quotient")
        .is_none());

        let singleton_candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];
        let singleton_quotient = initial_mesh_quotient(&singleton_candidates, 3, &port_identities)
            .expect("build the initial mesh quotient")
            .expect("three singleton edge pairs form a triangle quotient");
        let run = |ctx: &DecodeContext<'_>| {
            let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
            resolve_mesh_selection_from_quotient(
                ctx,
                topology.clone(),
                singleton_quotient.clone(),
                &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                &singleton_candidates,
                &port_identities,
                &budget,
            )
        };
        assert!(matches!(
            run(&ctx).expect("resolve mesh selection from quotient"),
            Some(super::MeshSolve::Solved(_))
        ));
    }
}

#[cfg(test)]
#[test]
fn initial_quotient_requires_candidate_points_below_point_count() {
    let candidates = [vec![[0, 1]]];
    let identities = [[10, 11]];
    assert!(initial_mesh_quotient(&candidates, 1, &identities)
        .expect("build the initial mesh quotient")
        .is_none());
    assert!(initial_mesh_quotient(&candidates, 2, &identities)
        .expect("build the initial mesh quotient")
        .is_some());
}

#[cfg(test)]
#[test]
fn initial_quotient_merges_ports_with_a_shared_identity() {
    let candidates = [vec![[0, 1]], vec![[0, 1]]];
    let identities = [[10, 11], [10, 12]];
    assert!(initial_mesh_quotient(&candidates, 2, &identities)
        .expect("build the initial mesh quotient")
        .is_some());
}

#[test]
fn coordinate_root_closure_solves_a_merged_two_point_quotient() {
    let edge_candidates = vec![vec![[0, 1]], vec![[0, 1]]];
    let edge_faces = [[0, 1], [0, 1]];
    let boundary = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 0,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 0,
                end: 0,
                reversed: None,
            },
        ]],
    };
    let boundary_domains = vec![
        MeshFaceBoundaryDomain::Ordered(vec![boundary.clone()]),
        MeshFaceBoundaryDomain::Ordered(vec![boundary]),
    ];
    let run = |ctx: &DecodeContext<'_>| {
        let mut quotient = MeshQuotient::new(
            (0..4)
                .map(|node| Arc::new(HashSet::from([usize::from(node % 2 != 0)])))
                .collect(),
        );
        quotient
            .merge(0, 2)
            .expect("merge the components")
            .expect("shared left endpoint");
        quotient
            .merge(1, 3)
            .expect("merge the components")
            .expect("shared right endpoint");
        let budget = WorkBudget::new(1_000);
        quotient.coordinate_root_closure_outcome(
            ctx,
            2,
            &edge_candidates,
            Some((&edge_faces, &boundary_domains)),
            Some(&budget),
        )
    };
    assert!(matches!(
        crate::test_support::with_service_context(run)
            .expect("resolve the coordinate root closure"),
        MeshSolve::Solved(_)
    ));
}

#[test]
fn coordinate_root_closure_rejects_out_of_range_root_points() {
    let make_quotient = || {
        MeshQuotient::new(vec![
            Arc::new(HashSet::from([9])),
            Arc::new(HashSet::from([0])),
        ])
    };
    let candidates = [vec![[0, 1]]];
    catia_test_context!(service_ctx);
    assert!(matches!(
        make_quotient()
            .coordinate_root_closure_outcome(&service_ctx, 2, &candidates, None, None)
            .expect("resolve the coordinate root closure"),
        MeshSolve::Failed(MeshCandidateFailure::Rejected(()))
    ));
}

#[test]
fn coordinate_root_preparation_admits_two_roots_on_one_edge() {
    let candidates = [vec![[0, 1]]];
    let make_quotient = || {
        let domain = Arc::new(HashSet::from([0, 1]));
        MeshQuotient::new(vec![domain.clone(), domain])
    };
    catia_test_context!(service_ctx);
    assert!(make_quotient()
        .prepare_coordinate_root_domains(&service_ctx, 2, &candidates, None)
        .expect("prepare the coordinate root domains")
        .is_some());
}
