// SPDX-License-Identifier: Apache-2.0
//! Coordinate-root domains, arc consistency, and implicit edge candidates.

use crate::solve::matching::{
    distinct_domain_matching_with_budget, repair_distinct_domain_matching_with_budget,
    retain_distinct_matching_supports,
};
use crate::solve::missing_edge::MeshFaceBoundaryDomain;
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

pub(super) fn enforce_edge_arc_consistency(
    domains: &mut [Vec<usize>],
    edges: &[[usize; 2]],
    edge_ids: &[usize],
    root_edges: &[Vec<usize>],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&WorkBudget>,
) -> bool {
    let Some(support_work) = edge_ids.iter().try_fold(0usize, |work, edge| {
        work.checked_add(edge_candidates[*edge].len().checked_mul(2)?)
    }) else {
        return false;
    };
    if support_work > 0 && budget.is_some_and(|budget| !budget.charge_by(support_work)) {
        return false;
    }
    let mut supports = Vec::new();
    for &edge in edge_ids {
        let mut edge_supports = HashMap::<usize, HashSet<usize>>::new();
        for [left, right] in edge_candidates[edge].iter().copied() {
            for (point, neighbor) in [(left, right), (right, left)] {
                edge_supports.entry(point).or_default().insert(neighbor);
            }
        }
        supports.push(edge_supports);
    }
    let mut queued = vec![[true; 2]; edges.len()];
    let mut queue = VecDeque::new();
    for edge in 0..edges.len() {
        for side in 0..2 {
            queue.push_back((edge, side));
        }
    }
    while let Some((edge, side)) = queue.pop_front() {
        queued[edge][side] = false;
        if supports[edge].is_empty() {
            continue;
        }
        let root = edges[edge][side];
        let other = edges[edge][1 - side];
        let mut other_domain = HashSet::new();
        for &point in &domains[other] {
            other_domain.insert(point);
        }
        let before = domains[root].len();
        domains[root].retain(|point| {
            let Some(supported) = supports[edge].get(point) else {
                return false;
            };
            if budget.is_some_and(|budget| !budget.charge_by(work_units(supported.len()))) {
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

pub(super) fn enforce_edge_arc_consistency_from(
    domains: &mut [Vec<usize>],
    edges: &[[usize; 2]],
    root_edges: &[Vec<usize>],
    edge_candidates: &[Vec<[usize; 2]>],
    initial_edges: &[usize],
    budget: Option<&WorkBudget>,
) -> bool {
    let mut queued = vec![[true; 2]; edges.len()];
    let mut queue = VecDeque::new();
    for &edge in initial_edges {
        for side in 0..2 {
            queue.push_back((edge, side));
        }
    }
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
        let mut other_domain = HashSet::new();
        for &point in &domains[other] {
            other_domain.insert(point);
        }
        let before = domains[root].len();
        domains[root].retain(|point| {
            if budget.is_some_and(|budget| !budget.charge_by(work_units(candidates.len()))) {
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

pub(super) fn enforce_sparse_endpoint_membership(
    domains: &mut [Vec<usize>],
    edges: &[[usize; 2]],
    edge_ids: &[usize],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&WorkBudget>,
) -> bool {
    let mut ordered = Vec::new();
    for edge in 0..edges.len() {
        ordered.push(edge);
    }
    ordered.sort_unstable_by(|left, right| {
        edge_candidates[edge_ids[*left]]
            .len()
            .cmp(&edge_candidates[edge_ids[*right]].len())
    });
    for edge in ordered {
        let candidates = &edge_candidates[edge_ids[edge]];
        if candidates.is_empty() {
            continue;
        }
        let [left, right] = edges[edge];
        let Some(domain_work) = domains[left].len().checked_add(if right == left {
            0
        } else {
            domains[right].len()
        }) else {
            continue;
        };
        let Some(support_work) = candidates.len().checked_mul(2) else {
            continue;
        };
        if support_work >= domain_work {
            continue;
        }
        let Some(work) = support_work.checked_add(domain_work) else {
            continue;
        };
        if budget.is_some_and(|budget| work > budget.remaining()) {
            continue;
        }
        if budget.is_some_and(|budget| !budget.charge_by(work)) {
            return false;
        }
        let mut allowed = HashSet::new();
        for point in candidates.iter().flatten().copied() {
            allowed.insert(point);
        }
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
pub(in crate::solve) struct MeshCoordinateRootDomains {
    pub(super) domains: Arc<Vec<Vec<usize>>>,
    pub(super) edges: Arc<Vec<[usize; 2]>>,
    pub(super) root_edges: Arc<Vec<Vec<usize>>>,
    pub(super) edge_candidates: Arc<Vec<Vec<[usize; 2]>>>,
    pub(super) coverage_matching: Vec<usize>,
    pub(super) point_count: usize,
}

pub(super) struct RefinedCoordinateDomains {
    pub(super) domains: Vec<Vec<usize>>,
    pub(super) coverage_matching: Vec<usize>,
}

#[derive(Clone, Copy)]
pub(in crate::solve) struct MeshIncidenceBoundary<'a> {
    pub(in crate::solve) edge_faces: &'a [[usize; 2]],
    pub(in crate::solve) face_count: usize,
    pub(in crate::solve) domains: &'a [MeshFaceBoundaryDomain],
}

#[derive(Clone)]
pub(in crate::solve) struct MeshImplicitEdgeCandidates {
    pub(super) source: MeshImplicitEdgeCandidateSource,
}

#[derive(Clone)]
pub(super) enum MeshImplicitEdgeCandidateSource {
    Cartesian {
        domains: Arc<Vec<Vec<usize>>>,
        left_root: usize,
        right_root: usize,
        left_index: usize,
        right_index: usize,
        same_root: bool,
    },
    Required {
        domains: Arc<Vec<Vec<usize>>>,
        roots: [Option<usize>; 2],
        indexes: [usize; 2],
        required: usize,
        skip_required: bool,
        remaining: usize,
    },
}

impl MeshImplicitEdgeCandidates {
    pub(in crate::solve) fn width_upper_bound(&self) -> usize {
        match &self.source {
            MeshImplicitEdgeCandidateSource::Cartesian {
                domains,
                left_root,
                right_root,
                ..
            } => domains[*left_root].len() * domains[*right_root].len(),
            MeshImplicitEdgeCandidateSource::Required { remaining, .. } => *remaining,
        }
    }
}

impl Iterator for MeshImplicitEdgeCandidates {
    type Item = [usize; 2];

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.source {
            MeshImplicitEdgeCandidateSource::Required {
                domains,
                roots,
                indexes,
                required,
                skip_required,
                remaining,
            } => loop {
                let left = roots[0]
                    .and_then(|root| domains[root].get(indexes[0]))
                    .copied();
                let right = roots[1]
                    .and_then(|root| domains[root].get(indexes[1]))
                    .copied();
                let point = match (left, right) {
                    (Some(left), Some(right)) if left < right => {
                        indexes[0] += 1;
                        left
                    }
                    (Some(left), Some(right)) if right < left => {
                        indexes[1] += 1;
                        right
                    }
                    (Some(left), Some(_)) => {
                        indexes[0] += 1;
                        indexes[1] += 1;
                        left
                    }
                    (Some(left), None) => {
                        indexes[0] += 1;
                        left
                    }
                    (None, Some(right)) => {
                        indexes[1] += 1;
                        right
                    }
                    (None, None) => return None,
                };
                if *skip_required && point == *required {
                    continue;
                }
                if *remaining > 0 {
                    *remaining -= 1;
                }
                return Some(if *required <= point {
                    [*required, point]
                } else {
                    [point, *required]
                });
            },
            MeshImplicitEdgeCandidateSource::Cartesian {
                domains,
                left_root,
                right_root,
                left_index,
                right_index,
                same_root,
            } => {
                let left = &domains[*left_root];
                let right = &domains[*right_root];
                while *left_index < left.len() {
                    let left_point = left[*left_index];
                    let &right_point = right.get(*right_index)?;
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

pub(in crate::solve) enum MeshEndpointCandidates<'a> {
    Explicit(&'a [[usize; 2]]),
    Implicit(MeshImplicitEdgeCandidates),
    Selected([usize; 2]),
}

impl MeshCoordinateRootDomains {
    pub(in crate::solve) fn edge_candidates(&self) -> &[Vec<[usize; 2]>] {
        &self.edge_candidates
    }

    pub(in crate::solve) fn supports_edge_candidate(&self, edge: usize, pair: [usize; 2]) -> bool {
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

    pub(in crate::solve) fn edge_candidate_points(&self, edge: usize) -> Option<Vec<usize>> {
        let candidates = self.edge_candidates.get(edge)?;
        if !candidates.is_empty() {
            let mut points = Vec::new();
            for point in candidates.iter().flatten().copied() {
                points.push(point);
            }
            points.sort_unstable_by(Ord::cmp);
            points.dedup();
            return Some(points);
        }
        let &[left, right] = self.edges.get(edge)?;
        let mut points = self.domains[left].clone();
        if right != left {
            points.extend_from_slice(&self.domains[right]);
            points.sort_unstable_by(Ord::cmp);
            points.dedup();
        }
        Some(points)
    }

    pub(in crate::solve) fn implicit_edge_candidates(
        &self,
        edge: usize,
        required_point: Option<usize>,
    ) -> Option<MeshImplicitEdgeCandidates> {
        self.edge_candidates.get(edge)?.is_empty().then_some(())?;
        let &[left, right] = self.edges.get(edge)?;
        if let Some(required) = required_point {
            let required_in_left = self.domains[left].binary_search(&required).is_ok();
            let required_in_right = self.domains[right].binary_search(&required).is_ok();
            let (roots, skip_required) = match (required_in_left, required_in_right) {
                (true, false) => ([Some(right), None], false),
                (false, true) => ([Some(left), None], false),
                (false, false) => ([None, None], false),
                (true, true) if left == right => ([Some(left), None], false),
                (true, true) => ([Some(left), Some(right)], true),
            };
            let mut candidates = MeshImplicitEdgeCandidates {
                source: MeshImplicitEdgeCandidateSource::Required {
                    domains: Arc::clone(&self.domains),
                    roots,
                    indexes: [0, 0],
                    required,
                    skip_required,
                    remaining: 0,
                },
            };
            let remaining = candidates.clone().count();
            if let MeshImplicitEdgeCandidateSource::Required {
                remaining: count, ..
            } = &mut candidates.source
            {
                *count = remaining;
            }
            return Some(candidates);
        }
        Some(MeshImplicitEdgeCandidates {
            source: MeshImplicitEdgeCandidateSource::Cartesian {
                domains: Arc::clone(&self.domains),
                left_root: left,
                right_root: right,
                left_index: 0,
                right_index: 0,
                same_root: left == right,
            },
        })
    }

    pub(in crate::solve) fn implicit_edge_candidate_with_point(
        &self,
        edge: usize,
        required: usize,
        budget: Option<&WorkBudget>,
        mut valid: impl FnMut([usize; 2]) -> bool,
    ) -> Option<[usize; 2]> {
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
                    return Some(pair(point));
                }
            }
        }
        if self.domains[right].binary_search(&required).is_err() {
            return None;
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
                return Some(pair(point));
            }
        }
        None
    }

    pub(super) fn coverage_matching(
        ctx: &DecodeContext<'_>,
        domains: &[Vec<usize>],
        point_count: usize,
        budget: Option<&WorkBudget>,
    ) -> Result<Option<Vec<usize>>, CodecError> {
        let mut roots_by_point =
            ctx.alloc_filled(point_count, Vec::new(), "catia_quotient_roots_by_point")?;
        for (root, domain) in domains.iter().enumerate() {
            for &point in domain {
                roots_by_point[point].push(root);
            }
        }
        if roots_by_point.iter().any(Vec::is_empty) {
            return Ok(None);
        }
        distinct_domain_matching_with_budget(
            ctx,
            roots_by_point.iter().map(Vec::as_slice),
            domains.len(),
            budget,
            None,
        )
    }

    pub(super) fn refine_domains(
        &self,
        ctx: &DecodeContext<'_>,
        mut domains: Vec<Vec<usize>>,
        edge_candidates: &[Vec<[usize; 2]>],
        initial_edges: &[usize],
        mut propagate_all_different: bool,
        budget: Option<&WorkBudget>,
    ) -> Result<Option<RefinedCoordinateDomains>, CodecError> {
        let mut affected_edges = initial_edges.to_vec();
        let mut coverage_matching = self.coverage_matching.clone();
        let propagate_globally = propagate_all_different;
        loop {
            let mut domain_lengths = Vec::new();
            for domain in &domains {
                domain_lengths.push(domain.len());
            }
            if !enforce_edge_arc_consistency_from(
                &mut domains,
                &self.edges,
                &self.root_edges,
                edge_candidates,
                &affected_edges,
                budget,
            ) {
                return Ok(None);
            }
            let mut roots_by_point =
                ctx.alloc_filled(self.point_count, Vec::new(), "catia_quotient_refine_roots")?;
            for (root, domain) in domains.iter().enumerate() {
                for &point in domain {
                    roots_by_point[point].push(root);
                }
            }
            let repaired_matching = repair_distinct_domain_matching_with_budget(
                ctx,
                roots_by_point.iter().map(Vec::as_slice),
                domains.len(),
                &coverage_matching,
                budget,
            )?;
            let Some(repaired_matching) = repaired_matching else {
                return Ok(None);
            };
            propagate_all_different |= repaired_matching != coverage_matching;
            coverage_matching = repaired_matching;
            if !propagate_all_different {
                return Ok(Some(RefinedCoordinateDomains {
                    domains,
                    coverage_matching,
                }));
            }
            let mut changed_roots = Vec::new();
            for (root, (domain, before)) in domains.iter().zip(domain_lengths).enumerate() {
                if domain.len() != before {
                    changed_roots.push(root);
                }
            }
            let affected_points = if propagate_globally {
                let mut all_points = Vec::new();
                for point in 0..self.point_count {
                    all_points.push(point);
                }
                all_points
            } else {
                if changed_roots.is_empty() {
                    return Ok(Some(RefinedCoordinateDomains {
                        domains,
                        coverage_matching,
                    }));
                }
                let mut reached_roots = vec![false; domains.len()];
                let mut reached_points =
                    ctx.alloc_filled(self.point_count, false, "catia_quotient_reached_points")?;
                let mut root_queue = VecDeque::new();
                for root in changed_roots {
                    root_queue.push_back(root);
                }
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
                let mut points = Vec::new();
                for (point, reached) in reached_points.into_iter().enumerate() {
                    if reached {
                        points.push(point);
                    }
                }
                points
            };
            let mut affected_domains = Vec::new();
            let mut affected_matching = Vec::new();
            for &point in &affected_points {
                let domain = roots_by_point[point].clone();
                affected_domains.push(domain);
                affected_matching.push(coverage_matching[point]);
            }
            let support_count = affected_domains.iter().map(Vec::len).sum::<usize>();
            let Some(propagation_work) = support_count.checked_mul(4) else {
                return Ok(Some(RefinedCoordinateDomains {
                    domains,
                    coverage_matching,
                }));
            };
            if budget.is_some_and(|budget| propagation_work > budget.remaining()) {
                return Ok(Some(RefinedCoordinateDomains {
                    domains,
                    coverage_matching,
                }));
            }
            let Some(_) = retain_distinct_matching_supports(
                ctx,
                &mut affected_domains,
                domains.len(),
                &affected_matching,
                budget,
            )?
            else {
                return Ok(None);
            };
            for (point, supported) in affected_points.into_iter().zip(affected_domains) {
                roots_by_point[point] = supported;
            }
            let mut affected_roots = Vec::new();
            for (root, domain) in domains.iter_mut().enumerate() {
                let before = domain.len();
                domain.retain(|point| roots_by_point[*point].binary_search(&root).is_ok());
                if domain.is_empty() {
                    return Ok(None);
                }
                if domain.len() != before {
                    affected_roots.push(root);
                }
            }
            if affected_roots.is_empty() {
                return Ok(Some(RefinedCoordinateDomains {
                    domains,
                    coverage_matching,
                }));
            }
            affected_edges = Vec::new();
            for root in affected_roots {
                for &edge in &self.root_edges[root] {
                    affected_edges.push(edge);
                }
            }
            affected_edges.sort_unstable_by(Ord::cmp);
            affected_edges.dedup();
        }
    }

    pub(in crate::solve) fn refine_edge_candidate_arc(
        &self,
        ctx: &DecodeContext<'_>,
        edge: usize,
        pair: [usize; 2],
        budget: Option<&WorkBudget>,
    ) -> Result<Option<Self>, CodecError> {
        let Some(candidates) = self.edge_candidates.get(edge) else {
            return Ok(None);
        };
        if candidates.as_slice() == [pair] {
            return Ok(Some(self.clone()));
        }
        if !candidates.is_empty() && !candidates.contains(&pair) {
            return Ok(None);
        }
        if candidates.is_empty() && !self.supports_edge_candidate(edge, pair) {
            return Ok(None);
        }
        let mut edge_candidates = self.edge_candidates.as_ref().clone();
        edge_candidates[edge] = vec![pair];
        let Some(RefinedCoordinateDomains {
            domains,
            coverage_matching,
        }) = self.refine_domains(
            ctx,
            self.domains.to_vec(),
            &edge_candidates,
            &[edge],
            false,
            budget,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            domains: Arc::new(domains),
            edges: Arc::clone(&self.edges),
            root_edges: Arc::clone(&self.root_edges),
            edge_candidates: Arc::new(edge_candidates),
            coverage_matching,
            point_count: self.point_count,
        }))
    }

    pub(in crate::solve) fn refine_candidates(
        &self,
        ctx: &DecodeContext<'_>,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget>,
    ) -> Result<Option<Self>, CodecError> {
        if edge_candidates.len() != self.edge_candidates.len() {
            return Ok(None);
        }
        let mut changed = Vec::new();
        for (edge, (current, base)) in edge_candidates
            .iter()
            .zip(self.edge_candidates.iter())
            .enumerate()
        {
            if current != base {
                changed.push(edge);
            }
        }
        if changed.is_empty() {
            return Ok(Some(self.clone()));
        }
        let Some(RefinedCoordinateDomains {
            domains,
            coverage_matching,
        }) = self.refine_domains(
            ctx,
            self.domains.to_vec(),
            edge_candidates,
            &changed,
            false,
            budget,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            domains: Arc::new(domains),
            edges: Arc::clone(&self.edges),
            root_edges: Arc::clone(&self.root_edges),
            edge_candidates: Arc::new(edge_candidates.to_vec()),
            coverage_matching,
            point_count: self.point_count,
        }))
    }
}

#[test]
fn arc_consistency_rejects_a_root_without_supporting_pairs() {
    let mut domains = vec![vec![2, 3], vec![1, 2]];
    assert!(!enforce_edge_arc_consistency(
        &mut domains,
        &[[0, 1]],
        &[0],
        &[vec![0], vec![0]],
        &[vec![[0, 1]]],
        None,
    ));
}

#[test]
fn arc_consistency_from_initial_edges_rejects_unsupported_roots() {
    let mut domains = vec![vec![2, 3], vec![1, 2]];
    assert!(!enforce_edge_arc_consistency_from(
        &mut domains,
        &[[0, 1]],
        &[vec![0], vec![0]],
        &[vec![[0, 1]]],
        &[0],
        None,
    ));
}

#[test]
fn sparse_membership_rejects_points_outside_the_candidate_pairs() {
    let run = || {
        let mut domains = vec![vec![2, 3], vec![1, 2]];
        enforce_sparse_endpoint_membership(&mut domains, &[[0, 1]], &[0], &[vec![[0, 1]]], None)
    };
    assert!(!run());
}

#[test]
fn coverage_matching_fails_when_a_point_has_no_root() {
    use cadmpeg_core::decode::DecodeArena;
    use cadmpeg_core::decode::DecodePolicy;
    let domains = [vec![0]];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert!(
        MeshCoordinateRootDomains::coverage_matching(&ctx, &domains, 2, None)
            .expect("construct the mesh coordinate root domains")
            .is_none()
    );
}

#[test]
fn candidate_refinement_refuses_a_missing_coverage_matching() {
    use cadmpeg_core::decode::DecodeArena;
    use cadmpeg_core::decode::DecodePolicy;
    let domains = MeshCoordinateRootDomains {
        domains: Arc::new(vec![vec![0, 1], vec![1, 2], vec![0, 2]]),
        edges: Arc::new(vec![[0, 1], [1, 2]]),
        root_edges: Arc::new(vec![vec![0], vec![0, 1], vec![1]]),
        edge_candidates: Arc::new(vec![vec![[0, 1], [1, 2]], vec![[1, 2], [0, 2]]]),
        coverage_matching: Vec::new(),
        point_count: 3,
    };
    let candidates = [vec![[1, 2]], vec![[1, 2], [0, 2]]];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert!(domains
        .refine_candidates(&ctx, &candidates, None)
        .expect("refine the candidates")
        .is_none());
}

#[test]
fn all_different_refinement_narrows_the_third_root_to_point_two() {
    let domains = MeshCoordinateRootDomains {
        domains: Arc::new(vec![vec![0, 1], vec![0, 1], vec![0, 1, 2]]),
        edges: Arc::new(vec![[0, 2]]),
        root_edges: Arc::new(vec![vec![0], vec![], vec![0]]),
        edge_candidates: Arc::new(vec![vec![[0, 1], [0, 2], [1, 2]]]),
        coverage_matching: vec![0, 1, 2],
        point_count: 3,
    };
    let run = |ctx: &DecodeContext<'_>| {
        domains.refine_domains(
            ctx,
            domains.domains.as_ref().clone(),
            domains.edge_candidates.as_ref(),
            &[],
            true,
            None,
        )
    };
    let service = crate::test_support::with_service_context(run)
        .expect("refine domains")
        .expect("Hall refinement is feasible");
    assert_eq!(service.domains[2], vec![2]);
}

#[test]
fn candidate_refinement_keeps_domains_and_selects_an_edge_arc() {
    let domains = MeshCoordinateRootDomains {
        domains: Arc::new(vec![vec![0], vec![1]]),
        edges: Arc::new(vec![[0, 1]]),
        root_edges: Arc::new(vec![vec![0], vec![0]]),
        edge_candidates: Arc::new(vec![vec![[0, 1], [1, 0]]]),
        coverage_matching: vec![0, 1],
        point_count: 2,
    };
    let unchanged = |ctx: &DecodeContext<'_>| {
        domains.refine_candidates(ctx, domains.edge_candidates.as_ref(), None)
    };
    assert!(crate::test_support::with_service_context(unchanged)
        .expect("refine the candidates")
        .is_some());
    let selected =
        |ctx: &DecodeContext<'_>| domains.refine_edge_candidate_arc(ctx, 0, [0, 1], None);
    let selected_domains = crate::test_support::with_service_context(selected)
        .expect("refine edge candidate arc")
        .expect("selected coordinate domains");
    assert_eq!(selected_domains.edge_candidate_points(0), Some(vec![0, 1]));
    let changed = |ctx: &DecodeContext<'_>| domains.refine_candidates(ctx, &[vec![[0, 1]]], None);
    assert!(crate::test_support::with_service_context(changed)
        .expect("refine the candidates")
        .is_some());
}
