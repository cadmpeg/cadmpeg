// SPDX-License-Identifier: Apache-2.0
//! Coordinate-root assignment and incidence-constrained closure.

use super::{
    compact_boundary_domain_viable, deferred_boundary_cycle_matches,
    distinct_domain_matching_with_budget, domain_contains, domains_disjoint, incidence_cycles,
    same_unordered_pair, BTreeMap, BTreeSet, Cell, HashMap, HashSet, MeshBoundaryEdgeCandidate,
    MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain, MeshQuotient, PointAssignmentOutcome,
    ScopedValue, UnionFind, VecDeque, WorkBudget,
};

use super::{
    CoordinateRootClosure, MeshCandidateFailure, MeshSolve, PointDomain,
    MAX_MESH_CONSTRAINT_OPERATIONS,
};
use crate::solve::matching::{
    domains_have_distinct_matching, repair_distinct_domain_matching_with_budget,
    retain_distinct_matching_supports, MatchingEdgeConstraint,
};
use std::{ops::ControlFlow, rc::Rc};

use cadmpeg_core::decode::work_units;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
/// Both orientations of an edge's candidate pairs, ascending, so the
/// neighbors of one point form one contiguous run.
fn edge_point_supports(
    ctx: &DecodeContext<'_>,
    candidates: &[[usize; 2]],
    operation: &'static str,
) -> Result<Vec<[usize; 2]>, CodecError> {
    let capacity = candidates
        .len()
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    let mut supports = ctx.collection_vec(capacity, operation)?;
    for &[left, right] in ctx.admit_iter(candidates, operation)? {
        supports.push([left, right]);
        supports.push([right, left]);
    }
    ctx.sort_unstable_by(&mut supports, |pair| pair, Ord::cmp, operation)?;
    ctx.dedup_vec(&mut supports, operation)?;
    Ok(supports)
}

/// The run of `supports` whose first point is `point`.
pub(super) fn point_support_run<'supports>(
    ctx: &DecodeContext<'_>,
    supports: &'supports [[usize; 2]],
    point: usize,
    operation: &'static str,
) -> Result<&'supports [[usize; 2]], CodecError> {
    let start = ctx.partition_point(supports, |pair| Ok(pair[0] < point), operation)?;
    let tail = &supports[start..];
    let length = ctx.partition_point(tail, |pair| Ok(pair[0] == point), operation)?;
    Ok(&tail[..length])
}

/// Drops the points of `root`'s domain that no support pairs with a point of
/// `other`'s domain. `units` is the search allowance one kept or dropped point
/// consumes; a refused allowance drops the point and leaves the budget
/// exhausted. Returns whether the domain changed.
fn revise_root_domain(
    ctx: &DecodeContext<'_>,
    domains: &mut [Vec<usize>],
    [root, other]: [usize; 2],
    supports: &[[usize; 2]],
    budget: Option<&WorkBudget<'_>>,
    units: impl Fn(&[[usize; 2]]) -> Option<usize>,
    operation: &'static str,
) -> Result<bool, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_revise_root_domain_scratch")?;
    scratch.with_storage(|| {
        let mut domain = std::mem::take(&mut domains[root]);
        let snapshot = if root == other {
            Some(ctx.copy_slice(&domain, operation)?)
        } else {
            None
        };
        let other_domain = snapshot.as_deref().unwrap_or(&domains[other]);
        let before = domain.len();
        ctx.retain_vec(
            &mut domain,
            |point| {
                let neighbors = point_support_run(ctx, supports, *point, operation)?;
                let Some(units) = units(neighbors) else {
                    return Ok(false);
                };
                if budget.is_some_and(|budget| !budget.charge_by(units)) {
                    return Ok(false);
                }
                ctx.any_by(
                    neighbors,
                    |pair| domain_contains(ctx, other_domain, pair[1], operation),
                    operation,
                )
            },
            operation,
        )?;
        let changed = domain.len() != before;
        domains[root] = domain;
        Ok(changed)
    })
}

fn enforce_edge_arc_consistency(
    ctx: &DecodeContext<'_>,
    domains: &mut [Vec<usize>],
    edges: &[[usize; 2]],
    edge_ids: &[usize],
    root_edges: &[Vec<usize>],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&WorkBudget<'_>>,
) -> Result<bool, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_enforce_edge_arc_consistency_scratch")?;
    scratch.with_storage(|| {
        let Some(support_work) = ctx.fold(
            edge_ids,
            Some(0usize),
            |work, edge| {
                Ok(work.and_then(|work| {
                    work.checked_add(edge_candidates[*edge].len().checked_mul(2)?)
                }))
            },
            "catia_arc_support_work",
        )?
        else {
            return Ok(false);
        };
        if support_work > 0 && budget.is_some_and(|budget| !budget.charge_by(support_work)) {
            return Ok(false);
        }
        let mut supports = Vec::new();
        for &edge in ctx.admit_iter(edge_ids, "catia_arc_support_edges")? {
            let edge_supports =
                edge_point_supports(ctx, &edge_candidates[edge], "catia_arc_supports")?;
            ctx.push_vec(&mut supports, edge_supports, "catia_arc_support_edges")?;
        }
        let mut queued = ctx.alloc_filled(edges.len(), [true; 2], "catia_arc_queued")?;
        let mut queue = VecDeque::new();
        for edge in ctx.admit_iter(0..edges.len(), "catia_arc_queue")? {
            for side in 0..2 {
                ctx.push_back(&mut queue, (edge, side), "catia_arc_queue")?;
            }
        }
        while let Some((edge, side)) = ctx.next_charged(
            &mut std::iter::from_fn(|| queue.pop_front()),
            "catia_mesh_quotient_iteration",
        )? {
            queued[edge][side] = false;
            if supports[edge].is_empty() {
                continue;
            }
            let root = edges[edge][side];
            let other = edges[edge][1 - side];
            let changed = revise_root_domain(
                ctx,
                domains,
                [root, other],
                &supports[edge],
                budget,
                |neighbors| (!neighbors.is_empty()).then(|| work_units(neighbors.len())),
                "catia_arc_revision",
            )?;
            if budget.is_some_and(WorkBudget::exhausted) || domains[root].is_empty() {
                return Ok(false);
            }
            if !changed {
                continue;
            }
            for &neighbor in ctx.admit_iter(&root_edges[root], "catia_arc_requeue")? {
                let neighbor_side = usize::from(edges[neighbor][1] == root);
                let revised_side = 1 - neighbor_side;
                if !queued[neighbor][revised_side] {
                    queued[neighbor][revised_side] = true;
                    ctx.push_back(&mut queue, (neighbor, revised_side), "catia_arc_queue")?;
                }
            }
        }
        Ok(true)
    })
}

fn enforce_edge_arc_consistency_from(
    ctx: &DecodeContext<'_>,
    domains: &mut [Vec<usize>],
    edges: &[[usize; 2]],
    root_edges: &[Vec<usize>],
    edge_candidates: &[Vec<[usize; 2]>],
    initial_edges: &[usize],
    budget: Option<&WorkBudget<'_>>,
) -> Result<bool, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_enforce_edge_arc_consistency_from_scratch")?;
    scratch.with_storage(|| {
        let mut queued = ctx.alloc_filled(edges.len(), [false; 2], "catia_arc_from_queued")?;
        let mut queue = VecDeque::new();
        for &edge in ctx.admit_iter(initial_edges, "catia_arc_from_queue")? {
            queued[edge] = [true; 2];
            for side in 0..2 {
                ctx.push_back(&mut queue, (edge, side), "catia_arc_from_queue")?;
            }
        }
        // An edge's supports are indexed the first time one of its sides is revised.
        let mut supports =
            ctx.collect_indexed_vec(edges.len(), "catia_arc_from_support_edges", |_| Ok(None))?;
        while let Some((edge, side)) = ctx.next_charged(
            &mut std::iter::from_fn(|| queue.pop_front()),
            "catia_mesh_quotient_iteration",
        )? {
            queued[edge][side] = false;
            let candidates = &edge_candidates[edge];
            if candidates.is_empty() {
                continue;
            }
            let edge_supports = match &mut supports[edge] {
                Some(edge_supports) => edge_supports,
                slot @ None => slot.insert(edge_point_supports(
                    ctx,
                    candidates,
                    "catia_arc_from_supports",
                )?),
            };
            let root = edges[edge][side];
            let other = edges[edge][1 - side];
            let changed = revise_root_domain(
                ctx,
                domains,
                [root, other],
                edge_supports,
                budget,
                |_| Some(work_units(candidates.len())),
                "catia_arc_from_revision",
            )?;
            if budget.is_some_and(WorkBudget::exhausted) || domains[root].is_empty() {
                return Ok(false);
            }
            if !changed {
                continue;
            }
            for &neighbor in ctx.admit_iter(&root_edges[root], "catia_arc_from_requeue")? {
                let neighbor_side = usize::from(edges[neighbor][1] == root);
                let revised_side = 1 - neighbor_side;
                if !queued[neighbor][revised_side] {
                    queued[neighbor][revised_side] = true;
                    ctx.push_back(&mut queue, (neighbor, revised_side), "catia_arc_from_queue")?;
                }
            }
        }
        Ok(true)
    })
}

fn enforce_sparse_endpoint_membership(
    ctx: &DecodeContext<'_>,
    domains: &mut [Vec<usize>],
    edges: &[[usize; 2]],
    edge_ids: &[usize],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&WorkBudget<'_>>,
) -> Result<bool, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_enforce_sparse_endpoint_membership_scratch")?;
    scratch.with_storage(|| {
        let mut ordered = ctx.collect_vec(0..edges.len(), "catia_sparse_ordered_edges")?;
        ctx.sort_unstable_by_key(
            &mut ordered,
            |value| edge_candidates[edge_ids[*value]].len(),
            Ord::cmp,
            "catia_sparse_ordered_edges_sort",
        )?;
        for edge in ctx.admit_iter(ordered, "catia_sparse_ordered_edges")? {
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
                return Ok(false);
            }
            let (allowed, _allowed_storage) =
                ctx.with_scoped_storage("catia_sparse_allowed_points", || {
                    let mut allowed = ctx.collect_vec(
                        candidates.iter().flatten().copied(),
                        "catia_sparse_allowed_points",
                    )?;
                    ctx.sort_unstable_by(
                        &mut allowed,
                        |point| point,
                        Ord::cmp,
                        "catia_sparse_allowed_points",
                    )?;
                    Ok::<_, CodecError>(allowed)
                })?;
            let roots = if right == left {
                &[left][..]
            } else {
                &[left, right][..]
            };
            for &root in roots {
                ctx.retain_vec(
                    &mut domains[root],
                    |point| domain_contains(ctx, &allowed, *point, "catia_sparse_membership"),
                    "catia_sparse_membership",
                )?;
            }
            if domains[left].is_empty() || domains[right].is_empty() {
                return Ok(false);
            }
        }
        Ok(true)
    })
}

#[derive(Clone)]
pub(in crate::solve) struct MeshCoordinateRootDomains<'storage> {
    domains: Rc<ScopedValue<'storage, Vec<Vec<usize>>>>,
    edges: Rc<ScopedValue<'storage, Vec<[usize; 2]>>>,
    root_edges: Rc<ScopedValue<'storage, Vec<Vec<usize>>>>,
    edge_candidates: Rc<ScopedValue<'storage, Vec<Vec<[usize; 2]>>>>,
    coverage_matching: Rc<ScopedValue<'storage, Vec<usize>>>,
    point_count: usize,
}

struct RefinedCoordinateDomains<'storage> {
    domains: ScopedValue<'storage, Vec<Vec<usize>>>,
    coverage_matching: ScopedValue<'storage, Vec<usize>>,
}

#[derive(Clone, Copy)]
pub(in crate::solve) struct MeshIncidenceBoundary<'a> {
    pub(in crate::solve) edge_faces: &'a [[usize; 2]],
    pub(in crate::solve) face_count: usize,
    pub(in crate::solve) domains: &'a [MeshFaceBoundaryDomain],
}

#[derive(Clone)]
pub(in crate::solve) struct MeshImplicitEdgeCandidates<'storage> {
    pub(super) source: MeshImplicitEdgeCandidateSource<'storage>,
}

type RequiredCandidatePlan = ([Option<usize>; 2], bool);

#[derive(Clone)]
pub(super) enum MeshImplicitEdgeCandidateSource<'storage> {
    Cartesian {
        domains: Rc<ScopedValue<'storage, Vec<Vec<usize>>>>,
        left_root: usize,
        right_root: usize,
        left_index: usize,
        right_index: usize,
        same_root: bool,
    },
    Required {
        domains: Rc<ScopedValue<'storage, Vec<Vec<usize>>>>,
        roots: [usize; 2],
        plan: Cell<Option<RequiredCandidatePlan>>,
        indexes: [usize; 2],
        required: usize,
    },
}

/// Resolves which domains supply partners for the required point. The domains
/// are immutable, so a completed plan remains valid across candidate clones.
fn required_candidate_plan(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<usize>],
    [left, right]: [usize; 2],
    required: usize,
    plan: &Cell<Option<RequiredCandidatePlan>>,
) -> Result<([Option<usize>; 2], bool), CodecError> {
    if let Some(resolved) = plan.get() {
        return Ok(resolved);
    }
    let required_in_left = domain_contains(
        ctx,
        &domains[left],
        required,
        "catia_implicit_required_domain",
    )?;
    let required_in_right = if left == right {
        required_in_left
    } else {
        domain_contains(
            ctx,
            &domains[right],
            required,
            "catia_implicit_required_domain",
        )?
    };
    let resolved = match (required_in_left, required_in_right) {
        (true, false) => ([Some(right), None], false),
        (false, true) => ([Some(left), None], false),
        (false, false) => ([None, None], false),
        (true, true) if left == right => ([Some(left), None], false),
        (true, true) => ([Some(left), Some(right)], true),
    };
    plan.set(Some(resolved));
    Ok(resolved)
}

impl MeshImplicitEdgeCandidates<'_> {
    pub(in crate::solve) fn width_upper_bound(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<usize, CodecError> {
        match &self.source {
            MeshImplicitEdgeCandidateSource::Cartesian {
                domains,
                left_root,
                right_root,
                ..
            } => domains[*left_root]
                .len()
                .checked_mul(domains[*right_root].len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("catia_implicit_edge_width", u64::MAX, u64::MAX)
                }),
            MeshImplicitEdgeCandidateSource::Required {
                domains,
                roots,
                plan,
                required,
                ..
            } => {
                let (roots, skip_required) =
                    required_candidate_plan(ctx, domains, *roots, *required, plan)?;
                match roots {
                    [Some(left), Some(right)] => {
                        let (short, long) = if domains[left].len() <= domains[right].len() {
                            (&domains[left], &domains[right])
                        } else {
                            (&domains[right], &domains[left])
                        };
                        let shared = ctx.fold(
                            short,
                            0usize,
                            |shared, &point| {
                                Ok(shared
                                    + usize::from(domain_contains(
                                        ctx,
                                        long,
                                        point,
                                        "catia_implicit_edge_width",
                                    )?))
                            },
                            "catia_implicit_edge_width",
                        )?;
                        domains[left]
                            .len()
                            .checked_add(domains[right].len())
                            .and_then(|width| {
                                width.checked_sub(shared + usize::from(skip_required))
                            })
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(
                                    "catia_implicit_edge_width",
                                    u64::MAX,
                                    u64::MAX,
                                )
                            })
                    }
                    [Some(root), None] | [None, Some(root)] => Ok(domains[root].len()),
                    [None, None] => Ok(0),
                }
            }
        }
    }

    /// Examines raw pairs one at a time. Rejected pairs and duplicate checks
    /// consume work before the next pair is examined.
    pub(in crate::solve) fn next_with_context(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<[usize; 2]>, CodecError> {
        match &mut self.source {
            MeshImplicitEdgeCandidateSource::Required {
                domains,
                roots,
                plan,
                indexes,
                required,
            } => {
                let (roots, skip_required) =
                    required_candidate_plan(ctx, domains, *roots, *required, plan)?;
                let points = std::iter::from_fn(|| {
                    let left = roots[0]
                        .and_then(|root| domains[root].get(indexes[0]))
                        .copied();
                    let right = roots[1]
                        .and_then(|root| domains[root].get(indexes[1]))
                        .copied();
                    match (left, right) {
                        (Some(left), Some(right)) if left < right => {
                            indexes[0] += 1;
                            Some(left)
                        }
                        (Some(left), Some(right)) if right < left => {
                            indexes[1] += 1;
                            Some(right)
                        }
                        (Some(left), Some(_)) => {
                            indexes[0] += 1;
                            indexes[1] += 1;
                            Some(left)
                        }
                        (Some(left), None) => {
                            indexes[0] += 1;
                            Some(left)
                        }
                        (None, Some(right)) => {
                            indexes[1] += 1;
                            Some(right)
                        }
                        (None, None) => None,
                    }
                });
                ctx.find_map(
                    points,
                    |point| {
                        Ok((!skip_required || point != *required)
                            .then(|| [(*required).min(point), (*required).max(point)]))
                    },
                    "catia_implicit_candidate_scan",
                )
            }
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
                let pairs = std::iter::from_fn(|| {
                    let &left_point = left.get(*left_index)?;
                    let &right_point = right.get(*right_index)?;
                    *right_index += 1;
                    if *right_index == right.len() {
                        *left_index += 1;
                        *right_index = 0;
                    }
                    Some([left_point, right_point])
                });
                ctx.find_map(
                    pairs,
                    |[left_point, right_point]| {
                        if !*same_root && left_point == right_point {
                            return Ok(None);
                        }
                        if left_point > right_point
                            && domain_contains(
                                ctx,
                                left,
                                right_point,
                                "catia_implicit_candidate_duplicate",
                            )?
                            && domain_contains(
                                ctx,
                                right,
                                left_point,
                                "catia_implicit_candidate_duplicate",
                            )?
                        {
                            return Ok(None);
                        }
                        Ok(Some([
                            left_point.min(right_point),
                            left_point.max(right_point),
                        ]))
                    },
                    "catia_implicit_candidate_scan",
                )
            }
        }
    }
}

pub(in crate::solve) enum MeshEndpointCandidates<'a> {
    Explicit(&'a [[usize; 2]]),
    Implicit(MeshImplicitEdgeCandidates<'a>),
    Selected([usize; 2]),
}

impl<'storage> MeshCoordinateRootDomains<'storage> {
    pub(in crate::solve) fn edge_candidates(&self) -> &[Vec<[usize; 2]>] {
        &self.edge_candidates
    }

    pub(in crate::solve) fn supports_edge_candidate(
        &self,
        ctx: &DecodeContext<'_>,
        edge: usize,
        pair: [usize; 2],
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "catia_coordinate_candidate_support";
        let Some(&[left, right]) = self.edges.get(edge) else {
            return Ok(false);
        };
        if left != right && pair[0] == pair[1] {
            return Ok(false);
        }
        let contains =
            |root: usize, point: usize| domain_contains(ctx, &self.domains[root], point, OPERATION);
        Ok((contains(left, pair[0])? && contains(right, pair[1])?)
            || (contains(left, pair[1])? && contains(right, pair[0])?))
    }

    pub(in crate::solve) fn edge_candidate_points(
        &self,
        ctx: &DecodeContext<'_>,
        edge: usize,
    ) -> Result<Option<Vec<usize>>, CodecError> {
        let Some(candidates) = self.edge_candidates.get(edge) else {
            return Ok(None);
        };
        if !candidates.is_empty() {
            let mut points = ctx.collect_vec(
                candidates.iter().flatten().copied(),
                "catia_coordinate_edge_candidate_points",
            )?;
            ctx.sort_unstable_by(
                &mut points,
                |value| value,
                Ord::cmp,
                "catia_coordinate_edge_candidate_points_sort",
            )?;
            ctx.dedup_vec(&mut points, "catia_coordinate_edge_candidate_points_dedup")?;
            return Ok(Some(points));
        }
        let Some(&[left, right]) = self.edges.get(edge) else {
            return Ok(None);
        };
        let mut points =
            ctx.copy_slice(&self.domains[left], "catia_coordinate_edge_points_left")?;
        if right != left {
            ctx.extend_from_slice(
                &mut points,
                &self.domains[right],
                "catia_coordinate_edge_points_right",
            )?;
            ctx.sort_unstable_by(
                &mut points,
                |value| value,
                Ord::cmp,
                "catia_coordinate_edge_points_sort",
            )?;
            ctx.dedup_vec(&mut points, "catia_coordinate_edge_points_dedup")?;
        }
        Ok(Some(points))
    }

    pub(in crate::solve) fn implicit_edge_candidates(
        &self,
        edge: usize,
        required_point: Option<usize>,
    ) -> Option<MeshImplicitEdgeCandidates<'storage>> {
        self.edge_candidates.get(edge)?.is_empty().then_some(())?;
        let &[left, right] = self.edges.get(edge)?;
        if let Some(required) = required_point {
            return Some(MeshImplicitEdgeCandidates {
                source: MeshImplicitEdgeCandidateSource::Required {
                    domains: Rc::clone(&self.domains),
                    roots: [left, right],
                    plan: Cell::new(None),
                    indexes: [0, 0],
                    required,
                },
            });
        }
        Some(MeshImplicitEdgeCandidates {
            source: MeshImplicitEdgeCandidateSource::Cartesian {
                domains: Rc::clone(&self.domains),
                left_root: left,
                right_root: right,
                left_index: 0,
                right_index: 0,
                same_root: left == right,
            },
        })
    }

    /// Finds the first implicit pair of `edge` that contains `required` and
    /// passes `valid`. Each examined pair consumes one unit of `budget`; an
    /// exhausted budget ends the search without a pair.
    pub(in crate::solve) fn implicit_edge_candidate_with_point(
        &self,
        ctx: &DecodeContext<'_>,
        edge: usize,
        required: usize,
        budget: Option<&WorkBudget<'_>>,
        mut valid: impl FnMut([usize; 2]) -> bool,
    ) -> Result<Option<[usize; 2]>, CodecError> {
        const OPERATION: &str = "catia_coordinate_required_pair";
        if self
            .edge_candidates
            .get(edge)
            .is_none_or(|candidates| !candidates.is_empty())
        {
            return Ok(None);
        }
        let Some(&[left, right]) = self.edges.get(edge) else {
            return Ok(None);
        };
        let pair = |point| {
            if required <= point {
                [required, point]
            } else {
                [point, required]
            }
        };
        let required_in_left = domain_contains(ctx, &self.domains[left], required, OPERATION)?;
        let required_in_right = if left == right {
            required_in_left
        } else {
            domain_contains(ctx, &self.domains[right], required, OPERATION)?
        };
        // The right domain pairs with a required left endpoint; the left domain
        // then pairs with a required right endpoint, skipping the points the
        // first pass already paired.
        let passes = [
            (required_in_left, right, false),
            (required_in_right && left != right, left, required_in_left),
        ];
        for (active, root, skip_right_points) in passes {
            if !active {
                continue;
            }
            let found = ctx.find_map(
                self.domains[root].iter().copied(),
                |point| {
                    if (left != right && point == required)
                        || (skip_right_points
                            && domain_contains(ctx, &self.domains[right], point, OPERATION)?)
                    {
                        return Ok(None);
                    }
                    if budget.is_some_and(|budget| !budget.charge()) {
                        return Ok(Some(None));
                    }
                    Ok(valid(pair(point)).then_some(Some(pair(point))))
                },
                OPERATION,
            )?;
            if let Some(found) = found {
                return Ok(found);
            }
        }
        Ok(None)
    }

    /// The roots whose domain holds each point, ascending for every point.
    fn roots_by_point(
        ctx: &DecodeContext<'_>,
        domains: &[Vec<usize>],
        point_count: usize,
        rows_operation: &'static str,
        entries_operation: &'static str,
    ) -> Result<Vec<Vec<usize>>, CodecError> {
        let mut roots_by_point =
            ctx.collect_indexed_vec(point_count, rows_operation, |_| Ok(Vec::new()))?;
        for (root, domain) in ctx.admit_iter(domains, rows_operation)?.enumerate() {
            for &point in ctx.admit_iter(domain, entries_operation)? {
                ctx.push_vec(&mut roots_by_point[point], root, entries_operation)?;
            }
        }
        Ok(roots_by_point)
    }

    fn coverage_matching(
        ctx: &DecodeContext<'_>,
        domains: &[Vec<usize>],
        point_count: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<Vec<usize>>, CodecError> {
        let (roots_by_point, _roots_storage) =
            ctx.with_scoped_storage("catia_quotient_roots_by_point", || {
                Self::roots_by_point(
                    ctx,
                    domains,
                    point_count,
                    "catia_quotient_roots_by_point",
                    "catia_quotient_roots_by_point_entries",
                )
            })?;
        if ctx.any_by(
            &roots_by_point,
            |roots| Ok(roots.is_empty()),
            "catia_quotient_uncovered_points",
        )? {
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

    fn refine_domains<'next>(
        &self,
        ctx: &'next DecodeContext<'_>,
        domains: ScopedValue<'next, Vec<Vec<usize>>>,
        edge_candidates: &[Vec<[usize; 2]>],
        initial_edges: &[usize],
        propagate_all_different: bool,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<RefinedCoordinateDomains<'next>>, CodecError> {
        let (value, storage) =
            ctx.with_scoped_storage("catia_quotient_refine_coverage_matching", || {
                ctx.copy_slice(
                    &self.coverage_matching,
                    "catia_quotient_refine_coverage_matching",
                )
            })?;
        refine_coordinate_domains(
            ctx,
            domains,
            ScopedValue {
                value,
                storage: Some(storage),
            },
            CoordinateRefinementInputs {
                edges: &self.edges,
                root_edges: &self.root_edges,
                edge_candidates,
                point_count: self.point_count,
            },
            initial_edges,
            propagate_all_different,
            budget,
        )
    }

    pub(in crate::solve) fn refine_edge_candidate_arc<'next>(
        &self,
        ctx: &'next DecodeContext<'_>,
        edge: usize,
        pair: [usize; 2],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<MeshCoordinateRootDomains<'next>>, CodecError>
    where
        'storage: 'next,
    {
        let Some(candidates) = self.edge_candidates.get(edge) else {
            return Ok(None);
        };
        if candidates.as_slice() == [pair] {
            return Ok(Some(self.clone()));
        }
        if !candidates.is_empty()
            && !ctx.contains(
                candidates,
                &pair,
                "catia_coordinate_refine_candidate_lookup",
            )?
        {
            return Ok(None);
        }
        if candidates.is_empty() && !self.supports_edge_candidate(ctx, edge, pair)? {
            return Ok(None);
        }
        let (edge_candidates, candidate_storage) =
            ctx.with_scoped_storage("catia_coordinate_refine_candidate_rows", || {
                ctx.collect_indexed_vec(
                    self.edge_candidates.len(),
                    "catia_coordinate_refine_candidate_rows",
                    |index| {
                        if index == edge {
                            let mut selected =
                                ctx.collection_vec(1, "catia_coordinate_refine_selected_pair")?;
                            selected.push(pair);
                            Ok(selected)
                        } else {
                            ctx.copy_slice(
                                &self.edge_candidates[index],
                                "catia_coordinate_refine_candidate_pairs",
                            )
                        }
                    },
                )
            })?;
        let (value, storage) =
            ctx.with_scoped_storage("catia_coordinate_refine_domain_rows", || {
                ctx.copy_retained_rows(
                    &self.domains,
                    "catia_coordinate_refine_domain_rows",
                    "catia_coordinate_refine_domain_points",
                )
            })?;
        let input_domains = ScopedValue {
            value,
            storage: Some(storage),
        };
        let Some(RefinedCoordinateDomains {
            domains,
            coverage_matching,
        }) = self.refine_domains(ctx, input_domains, &edge_candidates, &[edge], false, budget)?
        else {
            return Ok(None);
        };
        Ok(Some(MeshCoordinateRootDomains {
            domains: Rc::new(domains),
            edges: Rc::clone(&self.edges),
            root_edges: Rc::clone(&self.root_edges),
            edge_candidates: Rc::new(ScopedValue {
                value: edge_candidates,
                storage: Some(candidate_storage),
            }),
            coverage_matching: Rc::new(coverage_matching),
            point_count: self.point_count,
        }))
    }

    pub(in crate::solve) fn refine_candidates<'next>(
        &self,
        ctx: &'next DecodeContext<'_>,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<MeshCoordinateRootDomains<'next>>, CodecError>
    where
        'storage: 'next,
    {
        if edge_candidates.len() != self.edge_candidates.len() {
            return Ok(None);
        }
        let mut scratch = ctx.reserve_scoped(0, "catia_coordinate_refine_changed_edges")?;
        let mut changed = Vec::new();
        scratch.with_storage(|| {
            for (edge, (current, base)) in ctx
                .admit_iter(edge_candidates, "catia_coordinate_refine_changed_edges")?
                .zip(self.edge_candidates.iter())
                .enumerate()
            {
                if current.len() != base.len()
                    || !ctx.equal(current, base, "catia_coordinate_refine_changed_edges")?
                {
                    ctx.push_vec(&mut changed, edge, "catia_coordinate_refine_changed_edges")?;
                }
            }
            Ok::<_, CodecError>(())
        })?;
        if changed.is_empty() {
            return Ok(Some(self.clone()));
        }
        let (value, storage) =
            ctx.with_scoped_storage("catia_coordinate_refine_domain_rows", || {
                ctx.copy_retained_rows(
                    &self.domains,
                    "catia_coordinate_refine_domain_rows",
                    "catia_coordinate_refine_domain_points",
                )
            })?;
        let input_domains = ScopedValue {
            value,
            storage: Some(storage),
        };
        let Some(RefinedCoordinateDomains {
            domains,
            coverage_matching,
        }) = self.refine_domains(ctx, input_domains, edge_candidates, &changed, false, budget)?
        else {
            return Ok(None);
        };
        let (value, storage) =
            ctx.with_scoped_storage("catia_coordinate_refine_candidate_rows", || {
                ctx.copy_retained_rows(
                    edge_candidates,
                    "catia_coordinate_refine_candidate_rows",
                    "catia_coordinate_refine_candidate_pairs",
                )
            })?;
        Ok(Some(MeshCoordinateRootDomains {
            domains: Rc::new(domains),
            edges: Rc::clone(&self.edges),
            root_edges: Rc::clone(&self.root_edges),
            edge_candidates: Rc::new(ScopedValue {
                value,
                storage: Some(storage),
            }),
            coverage_matching: Rc::new(coverage_matching),
            point_count: self.point_count,
        }))
    }
}

#[derive(Clone, Copy)]
struct CoordinateRefinementInputs<'input> {
    edges: &'input [[usize; 2]],
    root_edges: &'input [Vec<usize>],
    edge_candidates: &'input [Vec<[usize; 2]>],
    point_count: usize,
}

fn refine_coordinate_domains<'next>(
    ctx: &'next DecodeContext<'_>,
    mut domains: ScopedValue<'next, Vec<Vec<usize>>>,
    mut coverage_matching: ScopedValue<'next, Vec<usize>>,
    inputs: CoordinateRefinementInputs<'_>,
    initial_edges: &[usize],
    mut propagate_all_different: bool,
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<RefinedCoordinateDomains<'next>>, CodecError> {
    let CoordinateRefinementInputs {
        edges,
        root_edges,
        edge_candidates,
        point_count,
    } = inputs;
    let (value, storage) = ctx
        .with_scoped_storage("catia_quotient_refine_initial_edges", || {
            ctx.copy_slice(initial_edges, "catia_quotient_refine_initial_edges")
        })?;
    let mut affected_edges = ScopedValue {
        value,
        storage: Some(storage),
    };
    let propagate_globally = propagate_all_different;
    loop {
        let mut scratch = ctx.reserve_scoped(0, "catia_quotient_refine_scratch")?;
        let step = scratch.with_storage(|| -> Result<_, CodecError> {
            ctx.charge_work(1, "catia_mesh_quotient_iteration")?;
            let domain_lengths = ctx.collect_vec(
                domains.iter().map(Vec::len),
                "catia_quotient_refine_domain_lengths",
            )?;
            if !enforce_edge_arc_consistency_from(
                ctx,
                &mut domains,
                edges,
                root_edges,
                edge_candidates,
                &affected_edges,
                budget,
            )? {
                return Ok(ControlFlow::Break(None));
            }
            let mut roots_by_point = MeshCoordinateRootDomains::roots_by_point(
                ctx,
                &domains,
                point_count,
                "catia_quotient_refine_roots",
                "catia_quotient_refine_root_entries",
            )?;
            let (repaired_matching, matching_storage) =
                ctx.with_scoped_storage("catia_quotient_refine_matching", || {
                    repair_distinct_domain_matching_with_budget(
                        ctx,
                        roots_by_point.iter().map(Vec::as_slice),
                        domains.len(),
                        &coverage_matching,
                        budget,
                    )
                })?;
            let Some(repaired_matching) = repaired_matching else {
                return Ok(ControlFlow::Break(None));
            };
            propagate_all_different |= repaired_matching.len() != coverage_matching.len()
                || !ctx.equal(
                    &repaired_matching[..],
                    &coverage_matching[..],
                    "catia_quotient_refine_matching_change",
                )?;
            coverage_matching = ScopedValue {
                value: repaired_matching,
                storage: Some(matching_storage),
            };
            if !propagate_all_different {
                return Ok(ControlFlow::Break(Some(RefinedCoordinateDomains {
                    domains: std::mem::take(&mut domains),
                    coverage_matching: std::mem::take(&mut coverage_matching),
                })));
            }
            let mut changed_roots = Vec::new();
            for (root, (domain, before)) in ctx
                .admit_iter(domains.as_slice(), "catia_quotient_refine_changed_roots")?
                .zip(&domain_lengths)
                .enumerate()
            {
                if domain.len() != *before {
                    ctx.push_vec(
                        &mut changed_roots,
                        root,
                        "catia_quotient_refine_changed_roots",
                    )?;
                }
            }
            let affected_points = if propagate_globally {
                ctx.collect_vec(0..point_count, "catia_quotient_refine_all_points")?
            } else {
                if changed_roots.is_empty() {
                    return Ok(ControlFlow::Break(Some(RefinedCoordinateDomains {
                        domains: std::mem::take(&mut domains),
                        coverage_matching: std::mem::take(&mut coverage_matching),
                    })));
                }
                let mut reached_roots =
                    ctx.alloc_filled(domains.len(), false, "catia_quotient_reached_roots")?;
                let mut reached_points =
                    ctx.alloc_filled(point_count, false, "catia_quotient_reached_points")?;
                let mut root_queue = VecDeque::new();
                for root in ctx.admit_iter(changed_roots, "catia_quotient_refine_root_queue")? {
                    ctx.push_back(&mut root_queue, root, "catia_quotient_refine_root_queue")?;
                }
                while let Some(root) = ctx.next_charged(
                    &mut std::iter::from_fn(|| root_queue.pop_front()),
                    "catia_mesh_quotient_iteration",
                )? {
                    if reached_roots[root] {
                        continue;
                    }
                    reached_roots[root] = true;
                    for &point in ctx.admit_iter(&domains[root], "catia_quotient_refine_reach")? {
                        if reached_points[point] {
                            continue;
                        }
                        reached_points[point] = true;
                        for &neighbor in
                            ctx.admit_iter(&roots_by_point[point], "catia_quotient_refine_reach")?
                        {
                            if !reached_roots[neighbor] {
                                ctx.push_back(
                                    &mut root_queue,
                                    neighbor,
                                    "catia_quotient_refine_root_queue",
                                )?;
                            }
                        }
                    }
                }
                let mut points = Vec::new();
                for (point, reached) in ctx
                    .admit_iter(reached_points, "catia_quotient_refine_reached_points_list")?
                    .enumerate()
                {
                    if reached {
                        ctx.push_vec(
                            &mut points,
                            point,
                            "catia_quotient_refine_reached_points_list",
                        )?;
                    }
                }
                points
            };
            let mut affected_domains = Vec::new();
            let mut affected_matching = Vec::new();
            for &point in ctx.admit_iter(&affected_points, "catia_quotient_refine_affected")? {
                let domain = ctx.copy_slice(
                    &roots_by_point[point],
                    "catia_quotient_refine_affected_domain_roots",
                )?;
                ctx.push_vec(
                    &mut affected_domains,
                    domain,
                    "catia_quotient_refine_affected_domains",
                )?;
                ctx.push_vec(
                    &mut affected_matching,
                    coverage_matching[point],
                    "catia_quotient_refine_affected_matching",
                )?;
            }
            let support_count = ctx.fold(
                &affected_domains,
                Some(0usize),
                |total, roots| Ok(total.and_then(|total| total.checked_add(roots.len()))),
                "catia_quotient_refine_support_count",
            )?;
            let Some(propagation_work) = support_count.and_then(|count| count.checked_mul(4))
            else {
                return Ok(ControlFlow::Break(Some(RefinedCoordinateDomains {
                    domains: std::mem::take(&mut domains),
                    coverage_matching: std::mem::take(&mut coverage_matching),
                })));
            };
            if budget.is_some_and(|budget| propagation_work > budget.remaining()) {
                return Ok(ControlFlow::Break(Some(RefinedCoordinateDomains {
                    domains: std::mem::take(&mut domains),
                    coverage_matching: std::mem::take(&mut coverage_matching),
                })));
            }
            let Some(_) = retain_distinct_matching_supports(
                ctx,
                &mut affected_domains,
                domains.len(),
                &affected_matching,
                budget,
            )?
            else {
                return Ok(ControlFlow::Break(None));
            };
            for (point, supported) in ctx
                .admit_iter(affected_points, "catia_quotient_refine_supported_roots")?
                .zip(affected_domains)
            {
                roots_by_point[point] = supported;
            }
            let mut affected_roots = Vec::new();
            for (root, domain) in ctx
                .admit_iter(
                    domains.as_mut_slice(),
                    "catia_quotient_refine_root_supports",
                )?
                .enumerate()
            {
                let before = domain.len();
                ctx.retain_vec(
                    domain,
                    |point| {
                        Ok(ctx
                            .binary_search(
                                &roots_by_point[*point],
                                &root,
                                "catia_quotient_refine_root_supports",
                            )?
                            .is_ok())
                    },
                    "catia_quotient_refine_root_supports",
                )?;
                if domain.is_empty() {
                    return Ok(ControlFlow::Break(None));
                }
                if domain.len() != before {
                    ctx.push_vec(
                        &mut affected_roots,
                        root,
                        "catia_quotient_refine_affected_roots",
                    )?;
                }
            }
            if affected_roots.is_empty() {
                return Ok(ControlFlow::Break(Some(RefinedCoordinateDomains {
                    domains: std::mem::take(&mut domains),
                    coverage_matching: std::mem::take(&mut coverage_matching),
                })));
            }
            let (value, storage) =
                ctx.with_scoped_storage("catia_quotient_refine_affected_edges", || {
                    let mut affected_edges = Vec::new();
                    for root in
                        ctx.admit_iter(affected_roots, "catia_quotient_refine_affected_edges")?
                    {
                        for &edge in ctx
                            .admit_iter(&root_edges[root], "catia_quotient_refine_affected_edges")?
                        {
                            ctx.push_vec(
                                &mut affected_edges,
                                edge,
                                "catia_quotient_refine_affected_edges",
                            )?;
                        }
                    }
                    ctx.sort_unstable_by(
                        &mut affected_edges,
                        |value| value,
                        Ord::cmp,
                        "catia_quotient_refine_affected_edges_sort",
                    )?;
                    ctx.dedup_vec(
                        &mut affected_edges,
                        "catia_quotient_refine_affected_edges_dedup",
                    )?;
                    Ok::<_, CodecError>(affected_edges)
                })?;
            Ok(ControlFlow::Continue(ScopedValue {
                value,
                storage: Some(storage),
            }))
        })?;
        match step {
            ControlFlow::Break(result) => return Ok(result),
            ControlFlow::Continue(next_edges) => affected_edges = next_edges,
        }
    }
}

impl<'storage> MeshQuotient<'storage> {
    pub(in crate::solve) fn coordinate_domain_preparation_limit(
        &self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<Option<usize>, CodecError> {
        if Some(self.union.len()) != edge_candidates.len().checked_mul(2) {
            return Ok(None);
        }
        let mut root_count = 0usize;
        let mut root_supports = Some(0usize);
        for node in ctx.admit_iter(0..self.union.len(), "catia_quotient_preparation_roots")? {
            if self.union.root(ctx, node)? != node {
                continue;
            }
            root_count += 1;
            let supported = ctx.partition_point(
                &self.domains[node],
                |point| Ok(*point < point_count),
                "catia_quotient_preparation_supports",
            )?;
            root_supports = root_supports.and_then(|total| total.checked_add(supported));
        }
        let explicit_pair_supports = ctx.fold(
            edge_candidates,
            Some(0usize),
            |total, candidates| Ok(total.and_then(|total| total.checked_add(candidates.len()))),
            "catia_quotient_preparation_pairs",
        )?;
        Ok((|| {
            let matching_phase_bound = root_count
                .checked_add(point_count)?
                .isqrt()
                .checked_add(1)?;
            let traversal_bound = matching_phase_bound.checked_add(8)?;
            Some(
                root_supports?
                    .checked_add(explicit_pair_supports?)?
                    .checked_mul(traversal_bound)?
                    .max(MAX_MESH_CONSTRAINT_OPERATIONS),
            )
        })())
    }

    pub(in crate::solve) fn prepare_coordinate_root_domains(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<MeshCoordinateRootDomains<'storage>>, CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "catia_quotient_preparation_scratch")?;
        scratch.with_storage(|| {
            if edge_candidates.len().checked_mul(2) != Some(self.union.len()) {
                return Ok(None);
            }
            let mut roots = Vec::new();
            let mut root_indices =
                ctx.alloc_filled(self.union.len(), None, "catia_quotient_root_indices")?;
            for node in ctx.admit_iter(0..self.union.len(), "catia_quotient_roots")? {
                if self.union.find(ctx, node)? == node {
                    root_indices[node] = Some(roots.len());
                    ctx.push_vec(&mut roots, node, "catia_quotient_roots")?;
                }
            }
            if roots.len() < point_count {
                return Ok(None);
            }
            let (edges, edge_storage) = ctx.with_scoped_storage("catia_quotient_edges", || {
                let mut edges = Vec::new();
                for edge in ctx.admit_iter(0..edge_candidates.len(), "catia_quotient_edges")? {
                    let Some(left) = root_indices[self.union.find(ctx, edge * 2)?] else {
                        return Ok(None);
                    };
                    let Some(right) = root_indices[self.union.find(ctx, edge * 2 + 1)?] else {
                        return Ok(None);
                    };
                    ctx.push_vec(&mut edges, [left, right], "catia_quotient_edges")?;
                }
                Ok::<_, CodecError>(Some(edges))
            })?;
            let Some(edges) = edges else {
                return Ok(None);
            };
            let (domains, domain_storage) =
                ctx.with_scoped_storage("catia_quotient_domains", || {
                    let mut domains = Vec::new();
                    for &root in ctx.admit_iter(&roots, "catia_quotient_domains")? {
                        // Domains ascend, so the points below `point_count` are a prefix.
                        let supported = ctx.partition_point(
                            &self.domains[root],
                            |point| Ok(*point < point_count),
                            "catia_quotient_domain_points",
                        )?;
                        if supported == 0 {
                            return Ok(None);
                        }
                        let domain = ctx.copy_slice(
                            &self.domains[root][..supported],
                            "catia_quotient_domain_points",
                        )?;
                        ctx.push_vec(&mut domains, domain, "catia_quotient_domains")?;
                    }
                    Ok::<_, CodecError>(Some(domains))
                })?;
            let Some(mut domains) = domains else {
                return Ok(None);
            };
            let edge_ids = ctx.collect_vec(0..edges.len(), "catia_quotient_edge_ids")?;
            let (root_edges, root_edge_storage) =
                ctx.with_scoped_storage("catia_quotient_root_edges", || {
                    let mut root_edges =
                        ctx.collect_indexed_vec(roots.len(), "catia_quotient_root_edges", |_| {
                            Ok(Vec::new())
                        })?;
                    for (edge, &[left, right]) in ctx
                        .admit_iter(&edges, "catia_quotient_root_edge_entries")?
                        .enumerate()
                    {
                        ctx.push_vec(
                            &mut root_edges[left],
                            edge,
                            "catia_quotient_root_edge_entries",
                        )?;
                        if right != left {
                            ctx.push_vec(
                                &mut root_edges[right],
                                edge,
                                "catia_quotient_root_edge_entries",
                            )?;
                        }
                    }
                    Ok::<_, CodecError>(root_edges)
                })?;
            if !enforce_sparse_endpoint_membership(
                ctx,
                &mut domains,
                &edges,
                &edge_ids,
                edge_candidates,
                budget,
            )? {
                return Ok(None);
            }
            if !enforce_edge_arc_consistency(
                ctx,
                &mut domains,
                &edges,
                &edge_ids,
                &root_edges,
                edge_candidates,
                budget,
            )? {
                return Ok(None);
            }
            let (mut supported_candidates, candidate_storage) =
                ctx.with_scoped_storage("catia_quotient_supported_candidate_rows", || {
                    ctx.copy_retained_rows(
                        edge_candidates,
                        "catia_quotient_supported_candidate_rows",
                        "catia_quotient_supported_candidate_pairs",
                    )
                })?;
            loop {
                ctx.charge_work(1, "catia_mesh_quotient_iteration")?;
                let mut changed_storage = ctx.reserve_scoped(0, "catia_quotient_changed_edges")?;
                let mut changed = Vec::new();
                for (edge, candidates) in ctx
                    .admit_iter(
                        &mut supported_candidates,
                        "catia_quotient_supported_candidates",
                    )?
                    .enumerate()
                {
                    if candidates.is_empty() {
                        continue;
                    }
                    let [left, right] = edges[edge];
                    let before = candidates.len();
                    if budget.is_some_and(|budget| !budget.charge_by(before)) {
                        ctx.charge_work(0, "catia quotient domain preparation")?;
                        return Err(ctx.refuse_codec_limit(
                            "catia quotient domain preparation",
                            0,
                            1,
                        ));
                    }
                    let contains = |root: usize, point: usize| {
                        domain_contains(
                            ctx,
                            &domains[root],
                            point,
                            "catia_quotient_supported_candidates",
                        )
                    };
                    ctx.retain_vec(
                        candidates,
                        |pair| {
                            Ok((contains(left, pair[0])? && contains(right, pair[1])?)
                                || (contains(left, pair[1])? && contains(right, pair[0])?))
                        },
                        "catia_quotient_supported_candidates",
                    )?;
                    if candidates.is_empty() {
                        return Ok(None);
                    }
                    if candidates.len() != before {
                        changed_storage.with_storage(|| {
                            ctx.push_vec(&mut changed, edge, "catia_quotient_changed_edges")
                        })?;
                    }
                }
                if changed.is_empty() {
                    break;
                }
                if !enforce_edge_arc_consistency_from(
                    ctx,
                    &mut domains,
                    &edges,
                    &root_edges,
                    &supported_candidates,
                    &changed,
                    budget,
                )? {
                    return Ok(None);
                }
            }
            let (coverage_matching, matching_storage) = ctx
                .with_scoped_storage("catia_quotient_coverage_matching", || {
                    MeshCoordinateRootDomains::coverage_matching(ctx, &domains, point_count, budget)
                })?;
            let Some(coverage_matching) = coverage_matching else {
                return Ok(None);
            };
            let Some(RefinedCoordinateDomains {
                domains,
                coverage_matching,
            }) = refine_coordinate_domains(
                ctx,
                ScopedValue {
                    value: domains,
                    storage: Some(domain_storage),
                },
                ScopedValue {
                    value: coverage_matching,
                    storage: Some(matching_storage),
                },
                CoordinateRefinementInputs {
                    edges: &edges,
                    root_edges: &root_edges,
                    edge_candidates: &supported_candidates,
                    point_count,
                },
                // Edge supports are already consistent; start with Hall support.
                &[],
                true,
                budget,
            )?
            else {
                return Ok(None);
            };
            Ok(Some(MeshCoordinateRootDomains {
                domains: Rc::new(domains),
                edges: Rc::new(ScopedValue {
                    value: edges,
                    storage: Some(edge_storage),
                }),
                root_edges: Rc::new(ScopedValue {
                    value: root_edges,
                    storage: Some(root_edge_storage),
                }),
                edge_candidates: Rc::new(ScopedValue {
                    value: supported_candidates,
                    storage: Some(candidate_storage),
                }),
                coverage_matching: Rc::new(coverage_matching),
                point_count,
            }))
        })
    }

    pub(in crate::solve) fn merge_singleton_coordinate_roots(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<bool, CodecError> {
        loop {
            ctx.charge_work(1, "catia_mesh_quotient_iteration")?;
            let mut storage = ctx.reserve_scoped(0, "catia_singleton_points")?;
            // Singleton roots, keyed by their point; ascending nodes keep each
            // point's roots ascending.
            let mut singletons = Vec::new();
            for node in ctx.admit_iter(0..self.union.len(), "catia_singleton_points")? {
                let root = self.union.find(ctx, node)?;
                if root != node || self.domains[root].len() != 1 {
                    continue;
                }
                let point = self.domains[root][0];
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut singletons,
                        (point, root),
                        "catia_singleton_point_roots",
                    )
                })?;
            }
            ctx.sort_unstable_by(
                &mut singletons,
                |entry| entry,
                Ord::cmp,
                "catia_singleton_points_sort",
            )?;
            let mut changed = false;
            let mut affected_edges = Vec::new();
            for (index, &(point, root)) in ctx
                .admit_iter(&singletons, "catia_singleton_merges")?
                .enumerate()
            {
                let Some(&(first_point, first)) = index
                    .checked_sub(1)
                    .and_then(|previous| singletons.get(previous))
                else {
                    continue;
                };
                if first_point != point {
                    continue;
                }
                // Earlier roots of this point already merged into the first
                // root of the run, whose class keeps that root.
                let first = self.union.find(ctx, first)?;
                storage.with_storage(|| {
                    self.push_component_edges(
                        ctx,
                        first,
                        edge_candidates,
                        &mut affected_edges,
                        "catia_singleton_affected_edges",
                    )?;
                    self.push_component_edges(
                        ctx,
                        root,
                        edge_candidates,
                        &mut affected_edges,
                        "catia_singleton_affected_edges",
                    )
                })?;
                if self.merge_charged(ctx, first, root)?.is_none() {
                    return Ok(false);
                }
                changed = true;
            }
            if !changed {
                return Ok(true);
            }
            if !self.propagate_edge_domains(ctx, &affected_edges, edge_candidates, None)? {
                return Ok(false);
            }
        }
    }

    pub(in crate::solve) fn close_coordinate_roots(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<HashMap<usize, usize>>, CodecError> {
        Ok(self
            .coordinate_root_closure_outcome(ctx, point_count, edge_candidates, None, budget)?
            .into_option())
    }

    #[cfg(test)]
    pub(in crate::solve) fn close_coordinate_roots_for_incidence_with_budget(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: MeshIncidenceBoundary<'_>,
        budget: Option<&WorkBudget<'_>>,
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
        ctx: &'storage DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: MeshIncidenceBoundary<'_>,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<CoordinateRootClosure, CodecError> {
        let MeshIncidenceBoundary {
            edge_faces,
            face_count,
            domains: boundary_domains,
        } = incidence;
        if edge_faces.len() != edge_candidates.len()
            || ctx.any_by(
                edge_faces.iter().flatten(),
                |face| Ok(*face >= face_count),
                "catia_coordinate_incidence_faces",
            )?
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
        ctx: &'storage DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
        budget: Option<&WorkBudget<'_>>,
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

    fn coordinate_root_closure_outcome_with_component_budget(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
        budget: Option<&WorkBudget<'_>>,
        component_search_budget: Option<usize>,
    ) -> Result<CoordinateRootClosure, CodecError> {
        let ambiguous = Cell::new(false);
        let exhausted = Cell::new(false);
        let result = close_coordinate_roots_with_incidence(
            ctx,
            CloseCoordinateRootsWithIncidenceInputs {
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

    pub(crate) fn point_assignment(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
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
        ctx: &'storage DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<bool, CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "catia_point_assignment_exists_scratch")?;
        scratch.with_storage(|| {
            Ok(matches!(
                self.point_assignments_with_budget(ctx, point_count, edge_candidates, 1, budget)?,
                PointAssignmentOutcome::Complete(solutions) if !solutions.is_empty()
            ))
        })
    }

    pub(super) fn point_assignments_with_budget(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        solution_limit: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<PointAssignmentOutcome, CodecError> {
        const OPERATION: &str = "catia_point_assignment";
        let (prepared, _search_storage) =
            ctx.with_scoped_storage("catia_point_assignment_search_storage", || {
                let mut roots = Vec::new();
                let mut root_indices = ctx.alloc_filled(
                    self.union.len(),
                    None,
                    "catia_point_assignment_root_indices",
                )?;
                for node in ctx.admit_iter(0..self.union.len(), "catia_point_assignment_roots")? {
                    let root = self.union.find(ctx, node)?;
                    if root == node {
                        root_indices[node] = Some(roots.len());
                        ctx.push_vec(&mut roots, root, "catia_point_assignment_roots")?;
                    }
                }
                if roots.len() != point_count {
                    return Ok(ControlFlow::Break(PointAssignmentOutcome::Complete(
                        Vec::new(),
                    )));
                }
                let domains = ctx.collect_vec(
                    roots.iter().map(|&root| Rc::clone(&self.domains[root])),
                    "catia_point_assignment_domains",
                )?;
                let mut edge_roots =
                    ctx.collection_vec(edge_candidates.len(), "catia_point_assignment_edge_roots")?;
                for edge in ctx.admit_iter(
                    0..edge_candidates.len(),
                    "catia_point_assignment_edge_roots",
                )? {
                    let (Some(left), Some(right)) = (
                        root_indices[self.union.find(ctx, edge * 2)?],
                        root_indices[self.union.find(ctx, edge * 2 + 1)?],
                    ) else {
                        return Ok(ControlFlow::Break(PointAssignmentOutcome::Complete(
                            Vec::new(),
                        )));
                    };
                    edge_roots.push([left, right]);
                }
                let mut root_edges = ctx.collect_indexed_vec(
                    roots.len(),
                    "catia point assignment root edges",
                    |_| Ok(Vec::new()),
                )?;
                for (edge_index, edge) in ctx
                    .admit_iter(&edge_roots, "catia_point_root_edge_entries")?
                    .enumerate()
                {
                    ctx.push_vec(
                        &mut root_edges[edge[0]],
                        edge_index,
                        "catia_point_root_edge_entries",
                    )?;
                    if edge[1] != edge[0] {
                        ctx.push_vec(
                            &mut root_edges[edge[1]],
                            edge_index,
                            "catia_point_root_edge_entries",
                        )?;
                    }
                }
                let mut edge_supports = ctx.collection_vec(
                    edge_candidates.len(),
                    "catia_point_assignment_neighbor_rows",
                )?;
                for candidates in
                    ctx.admit_iter(edge_candidates, "catia_point_assignment_neighbor_rows")?
                {
                    edge_supports.push(edge_point_supports(
                        ctx,
                        candidates,
                        "catia_point_assignment_neighbor_points",
                    )?);
                }
                let search = PointAssignmentSearch {
                    domains: &domains,
                    edge_roots: &edge_roots,
                    root_edges: &root_edges,
                    edge_candidates,
                    edge_supports: &edge_supports,
                    solution_limit,
                    budget,
                };
                let mut solutions = Vec::new();
                let mut assigned =
                    ctx.alloc_filled(domains.len(), None, "catia point assignment slots")?;
                let mut used = HashSet::new();
                search.walk(ctx, &mut assigned, &mut used, &mut solutions)?;
                if budget.is_some_and(WorkBudget::exhausted) {
                    return Ok(ControlFlow::Break(PointAssignmentOutcome::Exhausted));
                }
                Ok::<_, CodecError>(ControlFlow::Continue((roots, solutions)))
            })?;
        let (roots, solutions) = match prepared {
            ControlFlow::Break(outcome) => return Ok(outcome),
            ControlFlow::Continue(prepared) => prepared,
        };
        let mut completed =
            ctx.collection_vec(solutions.len(), "catia_point_assignment_completed")?;
        for solution in ctx.admit_iter(solutions, OPERATION)? {
            let mut pairs = HashMap::new();
            for (root, point) in ctx.admit_iter(&roots, OPERATION)?.copied().zip(solution) {
                ctx.insert_hash_map(
                    &mut pairs,
                    root,
                    point,
                    "catia_point_assignment_completed_pairs",
                )?;
            }
            completed.push(pairs);
        }
        Ok(PointAssignmentOutcome::Complete(completed))
    }
}

/// Backtracking search for distinct coordinate points, one per quotient root,
/// that every edge's candidate pairs admit.
struct PointAssignmentSearch<'a> {
    domains: &'a [PointDomain<'a>],
    edge_roots: &'a [[usize; 2]],
    root_edges: &'a [Vec<usize>],
    edge_candidates: &'a [Vec<[usize; 2]>],
    /// Both orientations of each edge's candidate pairs, ascending.
    edge_supports: &'a [Vec<[usize; 2]>],
    solution_limit: usize,
    budget: Option<&'a WorkBudget<'a>>,
}

impl PointAssignmentSearch<'_> {
    const OPERATION: &'static str = "catia_point_assignment";

    /// Tests whether `edge` has a candidate pair joining `point` to `other`.
    fn pair_supported(
        &self,
        ctx: &DecodeContext<'_>,
        edge: usize,
        point: usize,
        other: usize,
    ) -> Result<bool, CodecError> {
        let run = point_support_run(ctx, &self.edge_supports[edge], point, Self::OPERATION)?;
        Ok(ctx
            .binary_search_by(run, |pair| Ok(pair[1].cmp(&other)), Self::OPERATION)?
            .is_ok())
    }

    /// Tests whether assigning `point` to `root` leaves every incident edge
    /// satisfiable by the current partial assignment.
    fn value_viable(
        &self,
        ctx: &DecodeContext<'_>,
        root: usize,
        point: usize,
        assigned: &[Option<usize>],
        used: &HashSet<usize>,
    ) -> Result<bool, CodecError> {
        let unused = |other_point: usize| -> Result<bool, CodecError> {
            Ok(other_point != point
                && !ctx.contains_hash_set(used, &other_point, Self::OPERATION)?)
        };
        ctx.all_by(
            &self.root_edges[root],
            |&edge_index| {
                let edge = self.edge_roots[edge_index];
                let unconstrained = self.edge_candidates[edge_index].is_empty();
                let other = if edge[0] == root {
                    edge[1]
                } else if edge[1] == root {
                    edge[0]
                } else {
                    return Ok(true);
                };
                if other == root {
                    return Ok(unconstrained || self.pair_supported(ctx, edge_index, point, point)?);
                }
                if let Some(other_point) = assigned[other] {
                    return Ok(unconstrained
                        || self.pair_supported(ctx, edge_index, point, other_point)?);
                }
                if unconstrained {
                    return ctx.any_by(
                        &self.domains[other][..],
                        |&other_point| unused(other_point),
                        Self::OPERATION,
                    );
                }
                let run = point_support_run(
                    ctx,
                    &self.edge_supports[edge_index],
                    point,
                    Self::OPERATION,
                )?;
                ctx.any_by(
                    run,
                    |pair| {
                        Ok(unused(pair[1])?
                            && domain_contains(
                                ctx,
                                &self.domains[other],
                                pair[1],
                                Self::OPERATION,
                            )?)
                    },
                    Self::OPERATION,
                )
            },
            Self::OPERATION,
        )
    }

    /// The unused points of `root`'s domain that keep its edges satisfiable.
    fn values_for(
        &self,
        ctx: &DecodeContext<'_>,
        root: usize,
        assigned: &[Option<usize>],
        used: &HashSet<usize>,
    ) -> Result<Vec<usize>, CodecError> {
        let mut values = Vec::new();
        for &point in ctx.admit_iter(&self.domains[root][..], Self::OPERATION)? {
            if !ctx.contains_hash_set(used, &point, Self::OPERATION)?
                && self.value_viable(ctx, root, point, assigned, used)?
            {
                ctx.push_vec(&mut values, point, "catia_point_assignment_values")?;
            }
        }
        Ok(values)
    }

    fn rollback(
        ctx: &DecodeContext<'_>,
        assigned: &mut [Option<usize>],
        used: &mut HashSet<usize>,
        propagated: Vec<(usize, usize)>,
    ) -> Result<(), CodecError> {
        for (root, point) in ctx.admit_iter(propagated, Self::OPERATION)?.rev() {
            assigned[root] = None;
            ctx.remove_hash_set(used, &point, Self::OPERATION)?;
        }
        Ok(())
    }

    /// Assigns forced roots, then branches on the root with the fewest
    /// viable points, recording complete assignments up to the limit.
    fn walk(
        &self,
        ctx: &DecodeContext<'_>,
        assigned: &mut [Option<usize>],
        used: &mut HashSet<usize>,
        solutions: &mut Vec<Vec<usize>>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("catia_point_assignment_walk")?;
        if solutions.len() >= self.solution_limit {
            return Ok(());
        }
        if self.budget.is_some_and(|budget| !budget.charge()) {
            return Ok(());
        }
        let mut propagated_storage = ctx.reserve_scoped(0, "catia_point_assignment_propagated")?;
        let mut propagated = Vec::new();
        let branch = loop {
            ctx.charge_work(1, "catia_mesh_quotient_iteration")?;
            let (mut values, _row_storage) =
                ctx.with_scoped_storage("catia_point_assignment_values", || {
                    let mut values = Vec::new();
                    for (root, slot) in ctx.admit_iter(&*assigned, Self::OPERATION)?.enumerate() {
                        if slot.is_none() {
                            let (value, storage) = ctx
                                .with_scoped_storage("catia_point_assignment_values", || {
                                    self.values_for(ctx, root, assigned, used)
                                })?;
                            let candidates = ScopedValue {
                                value,
                                storage: Some(storage),
                            };
                            ctx.push_vec(
                                &mut values,
                                (root, candidates),
                                "catia_point_assignment_value_rows",
                            )?;
                        }
                    }
                    Ok::<_, CodecError>(values)
                })?;
            if values.is_empty() {
                break Some(None);
            }
            if ctx.any_by(
                &values,
                |(_, values)| Ok(values.is_empty()),
                Self::OPERATION,
            )? || !domains_have_distinct_matching(
                ctx,
                values.iter().map(|(_, values)| values.as_slice()),
                assigned.len(),
            )? {
                break None;
            }
            let mut dead = false;
            let mut progress = false;
            let mut rows = values.iter_mut();
            while let Some((root, candidates)) = ctx.next_charged(&mut rows, Self::OPERATION)? {
                let root = *root;
                if progress {
                    ctx.retain_vec(
                        &mut candidates.value,
                        |point| {
                            Ok(!ctx.contains_hash_set(used, point, Self::OPERATION)?
                                && self.value_viable(ctx, root, *point, assigned, used)?)
                        },
                        Self::OPERATION,
                    )?;
                }
                let Some(&point) = candidates.first() else {
                    dead = true;
                    break;
                };
                if candidates.len() != 1 {
                    continue;
                }
                if !ctx.insert_hash_set(used, point, "catia_point_assignment_used")? {
                    dead = true;
                    break;
                }
                assigned[root] = Some(point);
                propagated_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut propagated,
                        (root, point),
                        "catia_point_assignment_propagated",
                    )
                })?;
                progress = true;
            }
            if dead {
                break None;
            }
            if !progress {
                let Some(&(root, _)) = ctx.min_by_key(
                    &values,
                    |(root, values)| Ok((values.len(), *root)),
                    |left, right| Ok(left.cmp(right)),
                    Self::OPERATION,
                )?
                else {
                    break Some(None);
                };
                let index = ctx
                    .binary_search_by_key(&values, &root, |(root, _)| Ok(*root), Self::OPERATION)?
                    .map_err(|_| CodecError::malformed("point assignment branch row is missing"))?;
                break Some(Some(values.swap_remove(index)));
            }
        };
        let Some(branch) = branch else {
            return Self::rollback(ctx, assigned, used, propagated);
        };
        let Some((root, values)) = branch else {
            if ctx.all_by(&*assigned, |slot| Ok(slot.is_some()), Self::OPERATION)? {
                let solution = ctx.collect_vec(
                    assigned.iter().flatten().copied(),
                    "catia_point_assignment_solution",
                )?;
                ctx.push_vec(solutions, solution, "catia_point_assignment_solutions")?;
            }
            return Self::rollback(ctx, assigned, used, propagated);
        };
        let _value_storage = values.storage;
        let mut points = values.value.into_iter();
        while let Some(point) = ctx.next_charged(&mut points, Self::OPERATION)? {
            assigned[root] = Some(point);
            ctx.insert_hash_set(used, point, "catia_point_assignment_used")?;
            self.walk(ctx, assigned, used, solutions)?;
            ctx.remove_hash_set(used, &point, Self::OPERATION)?;
            assigned[root] = None;
            if solutions.len() >= self.solution_limit {
                break;
            }
        }
        Self::rollback(ctx, assigned, used, propagated)
    }
}

#[derive(Clone, Copy)]
struct PartialCompactAssignmentViableInputs<'input0, 'input1, 'input2, 'input3, 'input4, 'input5> {
    domain: &'input0 MeshFaceBoundaryDomain,
    local_edge_by_id: &'input1 HashMap<usize, usize>,
    edges: &'input2 [[usize; 2]],
    global_edge_count: usize,
    assigned: &'input3 [Option<usize>],
    candidate: (usize, usize),
    budget: Option<&'input5 WorkBudget<'input4>>,
}

fn partial_compact_assignment_viable(
    ctx: &DecodeContext<'_>,
    inputs: PartialCompactAssignmentViableInputs<'_, '_, '_, '_, '_, '_>,
) -> Result<bool, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_partial_compact_assignment_viable_scratch")?;
    scratch.with_storage(|| {
        fn augment(
            ctx: &DecodeContext<'_>,
            component: usize,
            compatible: &[Vec<bool>],
            seen: &mut [bool],
            matched: &mut [Option<usize>],
        ) -> Result<bool, CodecError> {
            let _depth = ctx.enter_nested("catia deferred boundary augmentation")?;
            ctx.any_by(
                0..matched.len(),
                |cycle| {
                    if !compatible[component][cycle] || seen[cycle] {
                        return Ok(false);
                    }
                    seen[cycle] = true;
                    let available = match matched[cycle] {
                        Some(previous) => augment(ctx, previous, compatible, seen, matched)?,
                        None => true,
                    };
                    if available {
                        matched[cycle] = Some(component);
                    }
                    Ok(available)
                },
                "catia deferred boundary augmentation",
            )
        }

        const OPERATION: &str = "catia coordinate selected edges";
        let PartialCompactAssignmentViableInputs {
            domain,
            local_edge_by_id,
            edges,
            global_edge_count,
            assigned,
            candidate,
            budget,
        } = inputs;

        let relevant = match domain {
            MeshFaceBoundaryDomain::Ordered(_) => return Ok(true),
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                ctx.copy_slice(edges, "catia coordinate relevant edges")?
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                let mut edges =
                    ctx.copy_slice(&domain.missing_edges, "catia coordinate relevant edges")?;
                for cycle in ctx.admit_iter(&domain.cycles, "catia coordinate relevant edges")? {
                    for (use_, _) in
                        ctx.admit_iter(&cycle.exact_uses, "catia coordinate relevant edges")?
                    {
                        ctx.push_vec(&mut edges, use_.edge, "catia coordinate relevant edges")?;
                    }
                }
                ctx.sort_unstable_by(
                    &mut edges,
                    |value| value,
                    Ord::cmp,
                    "catia coordinate relevant edges sort",
                )?;
                ctx.dedup_vec(&mut edges, "catia coordinate relevant edges")?;
                edges
            }
        };
        if budget.is_some_and(|budget| !budget.charge_by(work_units(relevant.len()))) {
            return Ok(false);
        }
        let value = |root| {
            if root == candidate.0 {
                Some(candidate.1)
            } else {
                assigned[root]
            }
        };
        // The selected edges, their points, and each edge's position among them.
        let mut selected_index = HashMap::new();
        let mut selected_edges = Vec::new();
        let mut selected_points = Vec::new();
        let mut adjacency = HashMap::<usize, Vec<usize>>::new();
        for &edge in ctx.admit_iter(&relevant, OPERATION)? {
            let Some(&local) = ctx.get_hash_map(local_edge_by_id, &edge, OPERATION)? else {
                return Ok(false);
            };
            let [left, right] = edges[local];
            let [Some(left), Some(right)] = [value(left), value(right)] else {
                continue;
            };
            ctx.insert_hash_map(
                &mut selected_index,
                edge,
                selected_edges.len(),
                "catia coordinate selected edges",
            )?;
            ctx.push_vec(
                &mut selected_edges,
                edge,
                "catia coordinate selected edge list",
            )?;
            ctx.push_vec(
                &mut selected_points,
                [left, right],
                "catia coordinate selected edge list",
            )?;
            for point in [left, right] {
                ctx.push_vec(
                    ctx.entry_hash_map(&mut adjacency, point, "catia coordinate adjacency points")?
                        .or_default(),
                    edge,
                    "catia coordinate adjacent edges",
                )?;
            }
        }
        let adjacent = |point: usize| -> Result<&[usize], CodecError> {
            Ok(ctx
                .get_hash_map(&adjacency, &point, "catia coordinate adjacency points")?
                .map_or(&[][..], Vec::as_slice))
        };
        let mut closed_components = Vec::new();
        let mut seen_edges = HashSet::new();
        for &first in ctx.admit_iter(&selected_edges, "catia coordinate component walk")? {
            if ctx.contains_hash_set(&seen_edges, &first, "catia coordinate seen edges")? {
                continue;
            }
            let mut component_storage =
                ctx.reserve_scoped(0, "catia coordinate component edges")?;
            let mut component = Vec::new();
            let closed = ctx
                .with_scoped_storage("catia coordinate component scratch", || {
                    let mut stack = Vec::new();
                    ctx.push_vec(&mut stack, first, "catia coordinate component stack")?;
                    let mut vertices = BTreeSet::new();
                    while let Some(edge) = ctx.next_charged(
                        &mut std::iter::from_fn(|| stack.pop()),
                        "catia coordinate component walk",
                    )? {
                        if !ctx.insert_hash_set(
                            &mut seen_edges,
                            edge,
                            "catia coordinate seen edges",
                        )? {
                            continue;
                        }
                        component_storage.with_storage(|| {
                            ctx.push_vec(&mut component, edge, "catia coordinate component edges")
                        })?;
                        let Some(&index) = ctx.get_hash_map(
                            &selected_index,
                            &edge,
                            "catia coordinate selected edges",
                        )?
                        else {
                            continue;
                        };
                        for point in selected_points[index] {
                            ctx.insert_btree_set(
                                &mut vertices,
                                point,
                                "catia coordinate component points",
                            )?;
                            ctx.extend_from_slice(
                                &mut stack,
                                adjacent(point)?,
                                "catia coordinate component stack",
                            )?;
                        }
                    }
                    ctx.all_by(
                        &vertices,
                        |point| Ok(adjacent(*point)?.len() == 2),
                        "catia coordinate component points",
                    )
                })?
                .0;
            if closed {
                ctx.push_vec(
                    &mut closed_components,
                    ScopedValue {
                        value: component,
                        storage: Some(component_storage),
                    },
                    "catia coordinate closed components",
                )?;
            }
        }
        match domain {
            MeshFaceBoundaryDomain::Ordered(_) => Ok(true),
            MeshFaceBoundaryDomain::UnorderedFullCycle(_) => Ok(closed_components.is_empty()
                || (selected_edges.len() == relevant.len() && closed_components.len() == 1)),
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                if closed_components.len() > domain.cycles.len() {
                    return Ok(false);
                }
                let mut missing = HashSet::new();
                for &edge in
                    ctx.admit_iter(&domain.missing_edges, "catia deferred missing edges")?
                {
                    ctx.insert_hash_set(&mut missing, edge, "catia deferred missing edges")?;
                }
                let mut edge_points = ctx.alloc_filled(
                    global_edge_count,
                    [0; 2],
                    "catia coordinate assignment edge points",
                )?;
                for (&edge, &points) in ctx
                    .admit_iter(&selected_edges, "catia coordinate assignment edge points")?
                    .zip(&selected_points)
                {
                    edge_points[edge] = points;
                }
                let mut compatible = ctx.collect_indexed_vec(
                    closed_components.len(),
                    "catia_deferred_compatible_rows",
                    |_| Ok(Vec::new()),
                )?;
                for (row, component) in ctx
                    .admit_iter(&mut compatible, "catia_deferred_compatible_rows")?
                    .zip(&closed_components)
                {
                    let (incidence, _incidence_storage) = ctx
                        .with_scoped_storage("catia coordinate incidence scratch", || {
                            incidence_cycles(ctx, component, &edge_points)
                        })?;
                    let Some([incidence]) = incidence.as_deref() else {
                        return Ok(false);
                    };
                    *row = ctx.alloc_filled(
                        domain.cycles.len(),
                        false,
                        "catia_deferred_compatible_cycles",
                    )?;
                    for (slot, cycle) in ctx
                        .admit_iter(&mut *row, "catia_deferred_compatible_cycles")?
                        .zip(&domain.cycles)
                    {
                        *slot = deferred_boundary_cycle_matches(
                            ctx,
                            cycle,
                            incidence.as_slice(),
                            &missing,
                        )?;
                    }
                }
                let mut matched =
                    ctx.alloc_filled(domain.cycles.len(), None, "catia_deferred_matched")?;
                for component in
                    ctx.admit_iter(0..closed_components.len(), "catia_deferred_matched")?
                {
                    let (augmented, _visit_storage) =
                        ctx.with_scoped_storage("catia deferred augmentation scratch", || {
                            let mut visited = ctx.alloc_filled(
                                domain.cycles.len(),
                                false,
                                "catia_deferred_augment_visit",
                            )?;
                            augment(ctx, component, &compatible, &mut visited, &mut matched)
                        })?;
                    if !augmented {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    })
}

pub(super) struct CloseCoordinateRootsWithIncidenceInputs<
    'storage,
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
    'input6,
    'input7,
> {
    pub(super) quotient: &'input0 mut MeshQuotient<'storage>,
    pub(super) point_count: usize,
    pub(super) edge_candidates: &'input1 [Vec<[usize; 2]>],
    pub(super) incidence: Option<(&'input2 [[usize; 2]], &'input3 [MeshFaceBoundaryDomain])>,
    pub(super) budget: Option<&'input5 WorkBudget<'input4>>,
    pub(super) component_search_budget: Option<usize>,
    pub(super) ambiguous: &'input6 Cell<bool>,
    pub(super) exhausted: &'input7 Cell<bool>,
}

pub(super) fn close_coordinate_roots_with_incidence<'storage>(
    ctx: &'storage DecodeContext<'_>,
    inputs: CloseCoordinateRootsWithIncidenceInputs<'storage, '_, '_, '_, '_, '_, '_, '_, '_>,
) -> Result<Option<HashMap<usize, usize>>, CodecError> {
    const MAX_COORDINATE_CLOSURE_STATES: usize = 256;
    const OPERATION: &str = "catia_coordinate_closure_search";
    type FaceDegrees = BTreeMap<(usize, usize), usize>;
    struct LocalIncidence<'a> {
        edge_faces: Vec<[usize; 2]>,
        face_edges: Vec<Vec<usize>>,
        closed_faces: Vec<bool>,
        boundary_domains: &'a [MeshFaceBoundaryDomain],
    }
    fn pair_supported(
        ctx: &DecodeContext<'_>,
        candidates: &[[usize; 2]],
        left: usize,
        right: usize,
    ) -> Result<bool, CodecError> {
        Ok(candidates.is_empty()
            || ctx.any_by(
                candidates,
                |pair| Ok(same_unordered_pair(*pair, [left, right])),
                "catia_coordinate_pair_support",
            )?)
    }

    /// The directions a boundary use can take: its fixed one or both.
    fn use_directions(use_: MeshBoundaryEdgeCandidate) -> [Option<bool>; 2] {
        match use_.reversed {
            Some(reversed) => [Some(reversed), None],
            None => [Some(false), Some(true)],
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn partial_ordered_assignment_viable(
        ctx: &DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        local_edge_by_id: &HashMap<usize, usize>,
        edges: &[[usize; 2]],
        domains: &[Vec<usize>],
        assigned: &[Option<usize>],
        candidate: Option<(usize, usize)>,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "catia_coordinate_ordered_viability";
        let port_root =
            |use_: MeshBoundaryEdgeCandidate, reversed: bool, end: bool| -> Result<_, CodecError> {
                let Some(&local) = ctx.get_hash_map(local_edge_by_id, &use_.edge, OPERATION)?
                else {
                    return Ok(None);
                };
                Ok(Some(
                    edges[local][usize::from(if end { !reversed } else { reversed })],
                ))
            };
        let value = |root| {
            candidate
                .filter(|(candidate_root, _)| *candidate_root == root)
                .map(|(_, point)| point)
                .or_else(|| assigned.get(root).copied().flatten())
        };
        let compatible = |left: usize, right: usize, charge: bool| -> Result<bool, CodecError> {
            if charge && budget.is_some_and(|budget| !budget.charge()) {
                return Ok(false);
            }
            Ok(match (value(left), value(right)) {
                (Some(left), Some(right)) => left == right,
                (Some(point), None) => domain_contains(ctx, &domains[right], point, OPERATION)?,
                (None, Some(point)) => domain_contains(ctx, &domains[left], point, OPERATION)?,
                (None, None) => !domains_disjoint(ctx, &domains[left], &domains[right], OPERATION)?,
            })
        };
        // A corner joins two consecutive uses when some direction pair maps
        // its ports to compatible roots. Each use has at most two directions.
        let joined = |left: (MeshBoundaryEdgeCandidate, bool),
                      right: (MeshBoundaryEdgeCandidate, bool),
                      charge: bool|
         -> Result<bool, CodecError> {
            let (Some(left), Some(right)) = (
                port_root(left.0, left.1, true)?,
                port_root(right.0, right.1, false)?,
            ) else {
                return Ok(false);
            };
            compatible(left, right, charge)
        };
        ctx.all_by(
            &assignment.boundaries,
            |boundary| {
                let Some((first, tail)) = boundary.split_first() else {
                    return Ok(false);
                };
                let last = tail.last().copied().unwrap_or(*first);
                for first_direction in use_directions(*first).into_iter().flatten() {
                    let mut previous = [Some(first_direction), None];
                    let mut open = true;
                    let mut indices = 1..boundary.len();
                    while let Some(index) = ctx.next_charged(&mut indices, OPERATION)? {
                        let mut next = [None; 2];
                        let mut next_count = 0;
                        for direction in use_directions(boundary[index]).into_iter().flatten() {
                            let mut reached = false;
                            for previous_direction in previous.into_iter().flatten() {
                                if joined(
                                    (boundary[index - 1], previous_direction),
                                    (boundary[index], direction),
                                    true,
                                )? {
                                    reached = true;
                                    break;
                                }
                            }
                            if reached {
                                next[next_count] = Some(direction);
                                next_count += 1;
                            }
                        }
                        if next_count == 0 {
                            open = false;
                            break;
                        }
                        previous = next;
                    }
                    if !open {
                        continue;
                    }
                    for previous_direction in previous.into_iter().flatten() {
                        if joined((last, previous_direction), (*first, first_direction), false)? {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            },
            OPERATION,
        )
    }
    fn complete_ordered_assignment_viable(
        ctx: &DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        edge_points: &[Option<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "catia_coordinate_complete_viability";
        ctx.all_by(
            &assignment.boundaries,
            |boundary| {
                let Some(first) = boundary.first().copied() else {
                    return Ok(false);
                };
                'directions: for first_reversed in use_directions(first).into_iter().flatten() {
                    let Some(first_points) = edge_points.get(first.edge).copied().flatten() else {
                        continue;
                    };
                    let first_start = first_points[usize::from(first_reversed)];
                    let mut ends = [Some(first_points[usize::from(!first_reversed)]), None];
                    let mut uses = boundary[1..].iter();
                    while let Some(use_) = ctx.next_charged(&mut uses, OPERATION)? {
                        let Some(points) = edge_points.get(use_.edge).copied().flatten() else {
                            continue 'directions;
                        };
                        let mut next = [None; 2];
                        for current in ends.into_iter().flatten() {
                            for reversed in use_directions(*use_).into_iter().flatten() {
                                if budget.is_some_and(|budget| !budget.charge()) {
                                    continue 'directions;
                                }
                                if points[usize::from(reversed)] == current {
                                    let endpoint = points[usize::from(!reversed)];
                                    if !next.contains(&Some(endpoint)) {
                                        if next[0].is_none() {
                                            next[0] = Some(endpoint);
                                        } else {
                                            next[1] = Some(endpoint);
                                        }
                                    }
                                }
                            }
                        }
                        if next[0].is_none() {
                            continue 'directions;
                        }
                        ends = next;
                    }
                    if ends.contains(&Some(first_start)) {
                        return Ok(true);
                    }
                }
                Ok(false)
            },
            OPERATION,
        )
    }
    struct CoordinateClosureSearch<
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
        'input10,
        'input11,
        'input12,
        'input13,
        'input14,
        'input15,
        'input16,
    > {
        domains: &'input0 [Vec<usize>],
        edges: &'input1 [[usize; 2]],
        edge_ids: &'input2 [usize],
        local_edge_by_id: &'input3 HashMap<usize, usize>,
        root_edges: &'input4 [Vec<usize>],
        edge_candidates: &'input5 [Vec<[usize; 2]>],
        incidence: Option<&'input7 LocalIncidence<'input6>>,
        /// The component's points, ascending.
        component_points: &'input8 [usize],
        assigned: &'input9 mut [Option<usize>],
        point_uses: &'input10 mut [usize],
        solutions: &'input11 mut Vec<Vec<usize>>,
        states: &'input12 mut usize,
        state_limit: usize,
        exhausted: &'input13 mut bool,
        base_degrees: &'input14 mut FaceDegrees,
        budget: Option<&'input16 WorkBudget<'input15>>,
    }
    fn walk(
        ctx: &DecodeContext<'_>,
        inputs: CoordinateClosureSearch<
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
            '_,
        >,
    ) -> Result<(), CodecError> {
        enum CoordinateBranch<'storage> {
            Search {
                root: usize,
                values: ScopedValue<'storage, Vec<usize>>,
            },
            Complete,
        }
        struct DegreeUndo<'storage> {
            entries: Vec<((usize, usize), Option<usize>)>,
            storage: ScopedReservation<'storage>,
        }
        /// The faces an edge bounds, each once.
        fn distinct_faces(faces: [usize; 2]) -> impl Iterator<Item = usize> {
            faces
                .into_iter()
                .enumerate()
                .filter(move |(rank, face)| *rank == 0 || *face != faces[0])
                .map(|(_, face)| face)
        }
        fn adjust_assignment_degrees<'storage>(
            ctx: &'storage DecodeContext<'_>,
            root: usize,
            assigned: &[Option<usize>],
            edges: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            incidence: Option<&LocalIncidence<'_>>,
            degrees: &mut FaceDegrees,
        ) -> Result<DegreeUndo<'storage>, CodecError> {
            let mut undo = DegreeUndo {
                entries: Vec::new(),
                storage: ctx.reserve_scoped(0, "catia_coordinate_closure_degree_undo")?,
            };
            let Some(incidence) = incidence else {
                return Ok(undo);
            };
            let edge_faces = &incidence.edge_faces;
            for &edge in
                ctx.admit_iter(&root_edges[root], "catia_coordinate_closure_degree_entries")?
            {
                let [left, right] = edges[edge];
                let [Some(left), Some(right)] = [assigned[left], assigned[right]] else {
                    continue;
                };
                for face in distinct_faces(edge_faces[edge]) {
                    for point in [left, right] {
                        let key = (face, point);
                        let entry = ctx.entry_btree_map(
                            degrees,
                            key,
                            "catia_coordinate_closure_degree_entries",
                        )?;
                        let previous = match &entry {
                            std::collections::btree_map::Entry::Occupied(entry) => {
                                Some(*entry.get())
                            }
                            std::collections::btree_map::Entry::Vacant(_) => None,
                        };
                        let degree = entry.or_default();
                        *degree = degree.checked_add(1).ok_or_else(|| {
                            CodecError::malformed("CATIA coordinate closure degree exceeds usize")
                        })?;
                        undo.storage.with_storage(|| {
                            ctx.push_vec(
                                &mut undo.entries,
                                (key, previous),
                                "catia_coordinate_closure_degree_undo",
                            )
                        })?;
                    }
                }
            }
            Ok(undo)
        }
        fn restore_assignment_degrees(
            ctx: &DecodeContext<'_>,
            degrees: &mut FaceDegrees,
            undo: DegreeUndo<'_>,
        ) -> Result<(), CodecError> {
            let DegreeUndo {
                entries,
                storage: _storage,
            } = undo;
            for (key, previous) in ctx
                .admit_iter(entries, "catia_coordinate_closure_degree_undo")?
                .rev()
            {
                match previous {
                    Some(degree) => {
                        if let Some(stored) = ctx.get_mut_btree_map(
                            degrees,
                            &key,
                            "catia_coordinate_closure_degree_undo",
                        )? {
                            *stored = degree;
                        }
                    }
                    None => {
                        ctx.remove_btree_map(
                            degrees,
                            &key,
                            "catia_coordinate_closure_degree_undo",
                        )?;
                    }
                }
            }
            Ok(())
        }
        struct CoordinateAssignment<
            'input0,
            'input1,
            'input2,
            'input3,
            'input4,
            'input5,
            'input6,
            'input7,
        > {
            root: usize,
            point: usize,
            assigned: &'input0 mut [Option<usize>],
            edges: &'input1 [[usize; 2]],
            root_edges: &'input2 [Vec<usize>],
            incidence: Option<&'input4 LocalIncidence<'input3>>,
            degrees: &'input5 mut FaceDegrees,
            budget: Option<&'input7 WorkBudget<'input6>>,
        }
        fn assign<'storage>(
            ctx: &'storage DecodeContext<'_>,
            inputs: CoordinateAssignment<'_, '_, '_, '_, '_, '_, '_, '_>,
        ) -> Result<Option<DegreeUndo<'storage>>, CodecError> {
            let CoordinateAssignment {
                root,
                point,
                assigned,
                edges,
                root_edges,
                incidence,
                degrees,
                budget,
            } = inputs;

            if budget.is_some_and(|budget| !budget.charge_by(root_edges[root].len())) {
                return Ok(None);
            }
            assigned[root] = Some(point);
            Ok(Some(adjust_assignment_degrees(
                ctx, root, assigned, edges, root_edges, incidence, degrees,
            )?))
        }
        fn unassign(
            ctx: &DecodeContext<'_>,
            root: usize,
            assigned: &mut [Option<usize>],
            degrees: &mut FaceDegrees,
            undo: DegreeUndo<'_>,
        ) -> Result<(), CodecError> {
            restore_assignment_degrees(ctx, degrees, undo)?;
            assigned[root] = None;
            Ok(())
        }
        fn rollback(
            ctx: &DecodeContext<'_>,
            assigned: &mut [Option<usize>],
            point_uses: &mut [usize],
            propagated: Vec<(usize, usize, DegreeUndo<'_>)>,
            degrees: &mut FaceDegrees,
        ) -> Result<(), CodecError> {
            for (root, point, undo) in ctx
                .admit_iter(propagated, "catia_coordinate_closure_propagated")?
                .rev()
            {
                point_uses[point] -= 1;
                unassign(ctx, root, assigned, degrees, undo)?;
            }
            Ok(())
        }
        /// The roots, other than `root`, on an edge of `root` or on an edge
        /// of a face such an edge bounds; ascending.
        fn affected_roots(
            ctx: &DecodeContext<'_>,
            root: usize,
            root_edges: &[Vec<usize>],
            edges: &[[usize; 2]],
            incidence: Option<&LocalIncidence<'_>>,
        ) -> Result<Vec<usize>, CodecError> {
            const AFFECTED: &str = "catia_coordinate_closure_affected_roots";
            let mut affected = Vec::new();
            for &edge in ctx.admit_iter(&root_edges[root], AFFECTED)? {
                ctx.reserve_vec(&mut affected, 2, AFFECTED)?;
                affected.extend_from_slice(&edges[edge]);
                let Some(incidence) = incidence else {
                    continue;
                };
                for face in distinct_faces(incidence.edge_faces[edge]) {
                    for &face_edge in ctx.admit_iter(&incidence.face_edges[face], AFFECTED)? {
                        ctx.reserve_vec(&mut affected, 2, AFFECTED)?;
                        affected.extend_from_slice(&edges[face_edge]);
                    }
                }
            }
            ctx.sort_unstable_by(&mut affected, |root| root, Ord::cmp, AFFECTED)?;
            ctx.dedup_vec(&mut affected, AFFECTED)?;
            ctx.retain_vec(&mut affected, |other| Ok(*other != root), AFFECTED)?;
            Ok(affected)
        }
        /// The position of the first least item by `key`.
        fn least_position<T, K: Ord>(
            ctx: &DecodeContext<'_>,
            values: &[T],
            key: impl Fn(&T) -> K,
        ) -> Result<Option<usize>, CodecError> {
            let mut position = 0usize;
            Ok(ctx
                .fold(
                    values,
                    None,
                    |least: Option<(K, usize)>, value| {
                        let index = position;
                        position += 1;
                        let current = key(value);
                        Ok(match least {
                            Some(least) if least.0 <= current => Some(least),
                            _ => Some((current, index)),
                        })
                    },
                    OPERATION,
                )?
                .map(|(_, index)| index))
        }

        let CoordinateClosureSearch {
            domains,
            edges,
            edge_ids,
            local_edge_by_id,
            root_edges,
            edge_candidates,
            incidence,
            component_points,
            assigned,
            point_uses,
            solutions,
            states,
            state_limit,
            exhausted,
            base_degrees,
            budget,
        } = inputs;

        let _depth = ctx.enter_nested("catia_coordinate_closure_walk")?;
        if solutions.len() > 1 || *exhausted {
            return Ok(());
        }
        if budget.is_some_and(|budget| !budget.charge()) {
            *exhausted = true;
            return Ok(());
        }
        // Whether `point` can take `root` given the current assignment: every
        // edge of the root stays supported, no face degree passes two, a
        // degree-one point on an affected face keeps a supporting edge, and
        // every affected closed face keeps a viable boundary.
        let point_viable = |root: usize,
                            point: usize,
                            assigned: &[Option<usize>],
                            base_degrees: &FaceDegrees,
                            work_budget: Option<&WorkBudget<'_>>|
         -> Result<bool, CodecError> {
            let pair_viable = ctx.all_by(
                &root_edges[root],
                |edge| {
                    let [left, right] = edges[*edge];
                    let other = if left == root { right } else { left };
                    match assigned[other] {
                        Some(other_point) => pair_supported(
                            ctx,
                            &edge_candidates[edge_ids[*edge]],
                            point,
                            other_point,
                        ),
                        None => Ok(true),
                    }
                },
                OPERATION,
            )?;
            if !pair_viable {
                return Ok(false);
            }
            let Some(incidence) = incidence else {
                return Ok(true);
            };
            if work_budget
                .is_some_and(|budget| !budget.charge_by(work_units(root_edges[root].len())))
            {
                return Ok(false);
            }
            let value = |endpoint| {
                if endpoint == root {
                    Some(point)
                } else {
                    assigned[endpoint]
                }
            };
            // The face degrees this assignment adds, and the faces it touches.
            let mut storage = ctx.reserve_scoped(0, "catia_coordinate_closure_probe_degrees")?;
            let (increments, affected_faces) = storage.with_storage(|| {
                let mut increments = Vec::new();
                let mut affected_faces = Vec::new();
                for &edge in
                    ctx.admit_iter(&root_edges[root], "catia_coordinate_closure_affected_faces")?
                {
                    let [left, right] = edges[edge];
                    let (Some(left), Some(right)) = (value(left), value(right)) else {
                        continue;
                    };
                    for face in distinct_faces(incidence.edge_faces[edge]) {
                        ctx.push_vec(
                            &mut affected_faces,
                            face,
                            "catia_coordinate_closure_affected_faces",
                        )?;
                        for endpoint in [left, right] {
                            ctx.push_vec(
                                &mut increments,
                                (face, endpoint),
                                "catia_coordinate_closure_probe_degrees",
                            )?;
                        }
                    }
                }
                ctx.sort_unstable_by(
                    &mut affected_faces,
                    |face| face,
                    Ord::cmp,
                    "catia_coordinate_closure_affected_faces",
                )?;
                ctx.dedup_vec(
                    &mut affected_faces,
                    "catia_coordinate_closure_affected_faces",
                )?;
                Ok::<_, CodecError>((increments, affected_faces))
            })?;
            // The probe holds every degree of the affected faces after the
            // assignment.
            let mut probe = FaceDegrees::new();
            storage.with_storage(|| {
                for &face in
                    ctx.admit_iter(&affected_faces, "catia_coordinate_closure_probe_degrees")?
                {
                    let mut entries = base_degrees.range((face, 0)..=(face, usize::MAX));
                    while let Some((&key, &degree)) =
                        ctx.next_charged(&mut entries, "catia_coordinate_closure_probe_degrees")?
                    {
                        ctx.insert_btree_map(
                            &mut probe,
                            key,
                            degree,
                            "catia_coordinate_closure_probe_degrees",
                        )?;
                    }
                }
                Ok::<_, CodecError>(())
            })?;
            let within_degree = storage.with_storage(|| {
                for &key in ctx.admit_iter(&increments, "catia_coordinate_closure_probe_degrees")? {
                    let degree = ctx
                        .entry_btree_map(&mut probe, key, "catia_coordinate_closure_probe_degrees")?
                        .or_default();
                    let Some(next_degree) = degree.checked_add(1) else {
                        return Ok(false);
                    };
                    *degree = next_degree;
                    if *degree > 2 {
                        return Ok(false);
                    }
                }
                Ok::<_, CodecError>(true)
            })?;
            if !within_degree {
                return Ok(false);
            }
            for (&(face, point), &degree) in
                ctx.admit_iter(&probe, "catia_coordinate_closure_degree_support")?
            {
                if degree != 1 {
                    continue;
                }
                if work_budget.is_some_and(|budget| {
                    !budget.charge_by(work_units(incidence.face_edges[face].len()))
                }) {
                    return Ok(false);
                }
                let supported = ctx.any_by(
                    &incidence.face_edges[face],
                    |&edge| {
                        let [left, right] = edges[edge];
                        if value(left).is_some() && value(right).is_some() {
                            return Ok(false);
                        }
                        let supports = |endpoint: usize| -> Result<bool, CodecError> {
                            Ok(match value(endpoint) {
                                Some(value) => value == point,
                                None => domain_contains(
                                    ctx,
                                    &domains[endpoint],
                                    point,
                                    "catia_coordinate_closure_degree_support",
                                )?,
                            })
                        };
                        Ok(supports(left)? || supports(right)?)
                    },
                    "catia_coordinate_closure_degree_support",
                )?;
                if !supported {
                    return Ok(false);
                }
            }
            ctx.all_by(
                &affected_faces,
                |&face| {
                    if !incidence.closed_faces[face] {
                        return Ok(true);
                    }
                    let domain = &incidence.boundary_domains[face];
                    match domain {
                        MeshFaceBoundaryDomain::Ordered(assignments) => ctx.any_by(
                            assignments,
                            |assignment| {
                                partial_ordered_assignment_viable(
                                    ctx,
                                    assignment,
                                    local_edge_by_id,
                                    edges,
                                    domains,
                                    assigned,
                                    Some((root, point)),
                                    work_budget,
                                )
                            },
                            "catia_coordinate_closure_boundaries",
                        ),
                        _ => partial_compact_assignment_viable(
                            ctx,
                            PartialCompactAssignmentViableInputs {
                                domain,
                                local_edge_by_id,
                                edges,
                                global_edge_count: edge_candidates.len(),
                                assigned,
                                candidate: (root, point),
                                budget: work_budget,
                            },
                        ),
                    }
                },
                "catia_coordinate_closure_boundaries",
            )
        };
        let viable_values = |root: usize,
                             assigned: &[Option<usize>],
                             base_degrees: &FaceDegrees,
                             work_budget: Option<&WorkBudget<'_>>|
         -> Result<ScopedValue<'_, Vec<usize>>, CodecError> {
            let (values, storage) =
                ctx.with_scoped_storage("catia_coordinate_viable_values", || {
                    let mut values = Vec::new();
                    for &point in
                        ctx.admit_iter(&domains[root], "catia_coordinate_viable_values")?
                    {
                        if point_viable(root, point, assigned, base_degrees, work_budget)? {
                            ctx.push_vec(&mut values, point, "catia_coordinate_viable_values")?;
                        }
                    }
                    Ok::<_, CodecError>(values)
                })?;
            Ok(ScopedValue {
                value: values,
                storage: Some(storage),
            })
        };

        let mut propagated_storage =
            ctx.reserve_scoped(0, "catia_coordinate_closure_propagated")?;
        let mut propagated = Vec::new();
        let mut pending_roots = None::<ScopedValue<'_, Vec<usize>>>;
        let branch: Option<CoordinateBranch<'_>> = loop {
            let mut iteration_storage =
                ctx.reserve_scoped(0, "catia_coordinate_closure_iteration_storage")?;
            ctx.charge_work(1, "catia_coordinate_assignment_iteration")?;
            let mut scanned_roots = match pending_roots.take() {
                Some(roots) => roots,
                None => {
                    let (value, storage) =
                        ctx.with_scoped_storage("catia_coordinate_closure_scanned_roots", || {
                            ctx.collect_vec(
                                0..domains.len(),
                                "catia_coordinate_closure_scanned_roots",
                            )
                        })?;
                    ScopedValue {
                        value,
                        storage: Some(storage),
                    }
                }
            };
            ctx.sort_unstable_by_key(
                &mut scanned_roots,
                |value| (domains[*value].len(), *value),
                Ord::cmp,
                "catia_coordinate_closure_scanned_roots_sort",
            )?;
            let partial_scan = scanned_roots.len() < domains.len();
            let bounded_scan = if partial_scan {
                false
            } else if let Some(budget) = budget {
                let mut position = 0usize;
                let scan_work = ctx.fold(
                    &*assigned,
                    Some(0usize),
                    |work, point| {
                        let root = position;
                        position += 1;
                        if point.is_some() {
                            return Ok(work);
                        }
                        Ok(work
                            .and_then(|work| work.checked_add(domains[root].len().checked_add(1)?)))
                    },
                    OPERATION,
                )?;
                scan_work.is_none_or(|work| work > budget.remaining())
            } else {
                false
            };
            let remaining = ctx.fold(
                &*assigned,
                0usize,
                |count, point| Ok(count + usize::from(point.is_none())),
                OPERATION,
            )?;
            let unused = ctx.fold(
                component_points,
                0usize,
                |count, point| Ok(count + usize::from(point_uses[*point] == 0)),
                OPERATION,
            )?;
            if remaining < unused {
                break None;
            }
            let mut viable_domains = Vec::new();
            let mut dead = false;
            let mut progress = false;
            let mut scan_truncated = false;
            let mut scan_deferred = false;
            let mut supported_unused = HashSet::new();
            let mut unused_point_roots = BTreeMap::<usize, Vec<usize>>::new();
            let ScopedValue {
                value: scanned_roots,
                storage: _scanned_storage,
            } = scanned_roots;
            let mut roots = scanned_roots.into_iter();
            while let Some(root) = ctx.next_charged(&mut roots, OPERATION)? {
                if assigned[root].is_some() {
                    continue;
                }
                // The scan of one root runs under a child slice; a slice that
                // ends defers the root without charging this budget.
                let work_budget =
                    budget.map(|budget| budget.session_child_slice(budget.remaining()));
                let Some(scan_work) = domains[root].len().checked_add(1) else {
                    scan_deferred = true;
                    continue;
                };
                if work_budget
                    .as_ref()
                    .is_some_and(|budget| !budget.charge_by(scan_work))
                {
                    scan_deferred = true;
                    continue;
                }
                let values = viable_values(root, assigned, base_degrees, work_budget.as_ref())?;
                if work_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                    scan_deferred = true;
                    continue;
                }
                if let (Some(budget), Some(work_budget)) = (budget, work_budget.as_ref()) {
                    if budget.consume_child(work_budget).is_err() {
                        *exhausted = true;
                        break;
                    }
                }
                if values.is_empty() {
                    dead = true;
                    break;
                }
                for &point in
                    ctx.admit_iter(&*values, "catia_coordinate_closure_supported_unused")?
                {
                    if point_uses[point] == 0 {
                        iteration_storage.with_storage(|| {
                            ctx.insert_hash_set(
                                &mut supported_unused,
                                point,
                                "catia_coordinate_closure_supported_unused",
                            )
                        })?;
                        iteration_storage.with_storage(|| {
                            ctx.push_btree_group(
                                &mut unused_point_roots,
                                point,
                                root,
                                "catia_coordinate_closure_unused_point_keys",
                                "catia_coordinate_closure_unused_point_roots",
                            )
                        })?;
                    }
                }
                if let [point] = values.as_slice() {
                    let Some(undo) = assign(
                        ctx,
                        CoordinateAssignment {
                            root,
                            point: *point,
                            assigned,
                            edges,
                            root_edges,
                            incidence,
                            degrees: base_degrees,
                            budget,
                        },
                    )?
                    else {
                        *exhausted = true;
                        break;
                    };
                    point_uses[*point] += 1;
                    propagated_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut propagated,
                            (root, *point, undo),
                            "catia_coordinate_closure_propagated",
                        )
                    })?;
                    progress = true;
                    if incidence.is_some() || bounded_scan {
                        pending_roots = Some({
                            let (value, storage) = ctx.with_scoped_storage(
                                "catia_coordinate_closure_affected_roots",
                                || affected_roots(ctx, root, root_edges, edges, incidence),
                            )?;
                            ScopedValue {
                                value,
                                storage: Some(storage),
                            }
                        });
                        break;
                    }
                } else {
                    iteration_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut viable_domains,
                            (root, values),
                            "catia_coordinate_closure_viable_domains",
                        )
                    })?;
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
                if let Some(best) =
                    least_position(ctx, &viable_domains, |(_, values)| values.len())?
                {
                    let (root, values) = viable_domains.swap_remove(best);
                    break Some(CoordinateBranch::Search { root, values });
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
            if ctx.any_by(
                component_points,
                |point| {
                    Ok(point_uses[*point] == 0
                        && !ctx.contains_hash_set(
                            &supported_unused,
                            point,
                            "catia_coordinate_closure_supported_unused",
                        )?)
                },
                OPERATION,
            )? {
                break None;
            }
            // Ascending by point, as the ordered map yields them.
            let point_supports = iteration_storage.with_storage(|| {
                ctx.collect_vec(
                    unused_point_roots,
                    "catia_coordinate_closure_point_supports",
                )
            })?;
            let mut uniquely_required = Vec::new();
            for (point, roots) in
                ctx.admit_iter(&point_supports, "catia_coordinate_closure_unique_supports")?
            {
                if let [root] = roots.as_slice() {
                    iteration_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut uniquely_required,
                            (*point, *root),
                            "catia_coordinate_closure_unique_supports",
                        )
                    })?;
                }
            }
            if let Some(&(point, root)) = uniquely_required.first() {
                if ctx.any_by(
                    &uniquely_required,
                    |&(other_point, other_root)| Ok(other_root == root && other_point != point),
                    "catia_coordinate_closure_unique_supports",
                )? {
                    break None;
                }
                let Some(undo) = assign(
                    ctx,
                    CoordinateAssignment {
                        root,
                        point,
                        assigned,
                        edges,
                        root_edges,
                        incidence,
                        degrees: base_degrees,
                        budget,
                    },
                )?
                else {
                    *exhausted = true;
                    break None;
                };
                point_uses[point] += 1;
                propagated_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut propagated,
                        (root, point, undo),
                        "catia_coordinate_closure_propagated",
                    )
                })?;
                pending_roots = Some({
                    let (value, storage) = ctx
                        .with_scoped_storage("catia_coordinate_closure_affected_roots", || {
                            affected_roots(ctx, root, root_edges, edges, incidence)
                        })?;
                    ScopedValue {
                        value,
                        storage: Some(storage),
                    }
                });
                continue;
            }
            let matching_budget =
                budget.map(|budget| budget.session_child_slice(budget.remaining()));
            let support_domains = iteration_storage.with_storage(|| {
                ctx.collect_vec(
                    point_supports.iter().map(|(_, roots)| roots.as_slice()),
                    "catia_coordinate_closure_support_domains",
                )
            })?;
            let (coverage_matching, _matching_storage) =
                ctx.with_scoped_storage("catia_coordinate_coverage_matching", || {
                    distinct_domain_matching_with_budget(
                        ctx,
                        support_domains.iter().copied(),
                        assigned.len(),
                        matching_budget.as_ref(),
                        None,
                    )
                })?;
            let mut matching_forced = None;
            let mut unsupported_matches = HashSet::new();
            if let Some(matching) = &coverage_matching {
                let mut matches = matching.iter().enumerate();
                while let Some((support, &root)) =
                    ctx.next_charged(&mut matches, "catia_coordinate_closure_forced_matches")?
                {
                    let excluded_match_impossible = {
                        let mut scratch =
                            ctx.reserve_scoped(0, "catia_coordinate_matching_probe")?;
                        scratch.with_storage(|| {
                            Ok::<_, CodecError>(
                                distinct_domain_matching_with_budget(
                                    ctx,
                                    support_domains.iter().copied(),
                                    assigned.len(),
                                    matching_budget.as_ref(),
                                    Some(MatchingEdgeConstraint::Exclude(support, root)),
                                )?
                                .is_none(),
                            )
                        })?
                    };
                    if excluded_match_impossible {
                        if matching_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                            break;
                        }
                        matching_forced = Some((point_supports[support].0, root));
                        break;
                    }
                }
                if matching_forced.is_none() {
                    let mut support_rows = point_supports.iter().enumerate();
                    'supports: while let Some((support, (_, roots))) = ctx.next_charged(
                        &mut support_rows,
                        "catia_coordinate_closure_unsupported_matches",
                    )? {
                        let mut roots = roots.iter();
                        while let Some(&root) = ctx.next_charged(
                            &mut roots,
                            "catia_coordinate_closure_unsupported_matches",
                        )? {
                            if matching[support] == root {
                                continue;
                            }
                            let required_match_impossible = {
                                let mut scratch =
                                    ctx.reserve_scoped(0, "catia_coordinate_matching_probe")?;
                                scratch.with_storage(|| {
                                    Ok::<_, CodecError>(
                                        distinct_domain_matching_with_budget(
                                            ctx,
                                            support_domains.iter().copied(),
                                            assigned.len(),
                                            matching_budget.as_ref(),
                                            Some(MatchingEdgeConstraint::Require(support, root)),
                                        )?
                                        .is_none(),
                                    )
                                })?
                            };
                            if required_match_impossible {
                                if matching_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                                    break 'supports;
                                }
                                iteration_storage.with_storage(|| {
                                    ctx.insert_hash_set(
                                        &mut unsupported_matches,
                                        (root, point_supports[support].0),
                                        "catia_coordinate_closure_unsupported_matches",
                                    )
                                })?;
                            }
                        }
                    }
                }
            }
            if matching_budget
                .as_ref()
                .is_none_or(|budget| !budget.exhausted())
            {
                if let (Some(budget), Some(matching_budget)) = (budget, matching_budget.as_ref()) {
                    if budget.consume_child(matching_budget).is_err() {
                        *exhausted = true;
                        break None;
                    }
                }
                if coverage_matching.is_none() {
                    break None;
                }
                if let Some((point, root)) = matching_forced {
                    let Some(undo) = assign(
                        ctx,
                        CoordinateAssignment {
                            root,
                            point,
                            assigned,
                            edges,
                            root_edges,
                            incidence,
                            degrees: base_degrees,
                            budget,
                        },
                    )?
                    else {
                        *exhausted = true;
                        break None;
                    };
                    point_uses[point] += 1;
                    propagated_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut propagated,
                            (root, point, undo),
                            "catia_coordinate_closure_propagated",
                        )
                    })?;
                    pending_roots = Some({
                        let (value, storage) = ctx.with_scoped_storage(
                            "catia_coordinate_closure_affected_roots",
                            || affected_roots(ctx, root, root_edges, edges, incidence),
                        )?;
                        ScopedValue {
                            value,
                            storage: Some(storage),
                        }
                    });
                    continue;
                }
                let mut rows = viable_domains.iter_mut();
                while let Some((root, values)) =
                    ctx.next_charged(&mut rows, "catia_coordinate_closure_unsupported_matches")?
                {
                    let root = *root;
                    ctx.retain_vec(
                        values,
                        |point| {
                            Ok(!ctx.contains_hash_set(
                                &unsupported_matches,
                                &(root, *point),
                                "catia_coordinate_closure_unsupported_matches",
                            )?)
                        },
                        "catia_coordinate_closure_unsupported_matches",
                    )?;
                    if values.is_empty() {
                        break;
                    }
                }
                if ctx.any_by(
                    &viable_domains,
                    |(_, values)| Ok(values.is_empty()),
                    OPERATION,
                )? {
                    break None;
                }
                if let Some(&(root, ref values)) = ctx.find_by(
                    &viable_domains,
                    |(_, values)| Ok(values.len() == 1),
                    OPERATION,
                )? {
                    let point = values[0];
                    let Some(undo) = assign(
                        ctx,
                        CoordinateAssignment {
                            root,
                            point,
                            assigned,
                            edges,
                            root_edges,
                            incidence,
                            degrees: base_degrees,
                            budget,
                        },
                    )?
                    else {
                        *exhausted = true;
                        break None;
                    };
                    point_uses[point] += 1;
                    propagated_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut propagated,
                            (root, point, undo),
                            "catia_coordinate_closure_propagated",
                        )
                    })?;
                    pending_roots = Some({
                        let (value, storage) = ctx.with_scoped_storage(
                            "catia_coordinate_closure_affected_roots",
                            || affected_roots(ctx, root, root_edges, edges, incidence),
                        )?;
                        ScopedValue {
                            value,
                            storage: Some(storage),
                        }
                    });
                    continue;
                }
            }
            if let Some(best) = least_position(ctx, &viable_domains, |(_, values)| values.len())? {
                let (root, values) = viable_domains.swap_remove(best);
                break Some(CoordinateBranch::Search { root, values });
            }
            if ctx.any_by(&*assigned, |point| Ok(point.is_none()), OPERATION)? {
                break None;
            }
            break Some(CoordinateBranch::Complete);
        };
        let Some(branch) = branch else {
            return rollback(ctx, assigned, point_uses, propagated, base_degrees);
        };
        let (root, values) = match branch {
            CoordinateBranch::Complete => {
                let solution = ctx.collect_vec(
                    assigned.iter().flatten().copied(),
                    "catia_coordinate_closure_complete_branch",
                )?;
                let mut incidence_storage =
                    ctx.reserve_scoped(0, "catia_coordinate_completed_incidence")?;
                let incidence_closed =
                    incidence_storage.with_storage(|| -> Result<bool, CodecError> {
                        let Some(incidence) = incidence else {
                            return Ok(true);
                        };
                        if budget.is_some_and(|budget| !budget.charge_by(edges.len())) {
                            return Ok(false);
                        }
                        let mut degrees = FaceDegrees::new();
                        for (edge, &[left, right]) in ctx
                            .admit_iter(edges, "catia_coordinate_closure_completed_degrees")?
                            .enumerate()
                        {
                            let [left, right] = [solution[left], solution[right]];
                            for face in distinct_faces(incidence.edge_faces[edge]) {
                                for point in [left, right] {
                                    let degree = ctx
                                        .entry_btree_map(
                                            &mut degrees,
                                            (face, point),
                                            "catia_coordinate_closure_completed_degrees",
                                        )?
                                        .or_default();
                                    let Some(next_degree) = degree.checked_add(1) else {
                                        return Ok(false);
                                    };
                                    *degree = next_degree;
                                }
                            }
                        }
                        ctx.all_by(
                        &degrees,
                        |(&(face, _), &degree)| Ok(!incidence.closed_faces[face] || degree == 2),
                        "catia_coordinate_closure_completed_degrees",
                    )
                    })?;
                let mut boundary_storage =
                    ctx.reserve_scoped(0, "catia_coordinate_completed_boundaries")?;
                let boundaries_close = boundary_storage.with_storage(|| {
                    incidence.map_or(Ok(true), |incidence| -> Result<bool, CodecError> {
                        let closed_face_count = ctx.fold(
                            &incidence.closed_faces,
                            0usize,
                            |count, closed| Ok(count + usize::from(*closed)),
                            OPERATION,
                        )?;
                        let Some(work) = edge_ids.len().checked_add(closed_face_count) else {
                            return Ok(false);
                        };
                        if budget.is_some_and(|budget| !budget.charge_by(work)) {
                            return Ok(false);
                        }
                        let mut selected = ctx.alloc_filled(
                            edge_candidates.len(),
                            None,
                            "catia coordinate closure selected edges",
                        )?;
                        for (local_edge, &edge) in ctx
                            .admit_iter(edge_ids, "catia coordinate closure selected edges")?
                            .enumerate()
                        {
                            let [left, right] = edges[local_edge].map(|root| solution[root]);
                            selected[edge] = Some([left, right]);
                        }
                        ctx.all_by(
                            incidence.closed_faces.iter().enumerate(),
                            |(face, closed)| {
                                if !*closed {
                                    return Ok(true);
                                }
                                match &incidence.boundary_domains[face] {
                                    MeshFaceBoundaryDomain::Ordered(assignments) => ctx.any_by(
                                        assignments,
                                        |assignment| {
                                            complete_ordered_assignment_viable(
                                                ctx, assignment, &selected, budget,
                                            )
                                        },
                                        OPERATION,
                                    ),
                                    domain => {
                                        compact_boundary_domain_viable(ctx, domain, &selected, None)
                                    }
                                }
                            },
                            OPERATION,
                        )
                    })
                })?;
                if budget.is_some_and(WorkBudget::exhausted) {
                    *exhausted = true;
                }
                if !*exhausted
                    && incidence_closed
                    && boundaries_close
                    && ctx.all_by(
                        component_points,
                        |point| Ok(point_uses[*point] > 0),
                        OPERATION,
                    )?
                {
                    ctx.push_vec(solutions, solution, "catia_coordinate_closure_solutions")?;
                }
                return rollback(ctx, assigned, point_uses, propagated, base_degrees);
            }
            CoordinateBranch::Search { root, values } => (root, values),
        };
        if *states >= state_limit {
            if let Some(budget) = budget {
                budget.exhaust();
            }
            *exhausted = true;
            return rollback(ctx, assigned, point_uses, propagated, base_degrees);
        }
        *states += 1;
        let ScopedValue {
            value: values,
            storage: _value_storage,
        } = values;
        let mut points = values.into_iter();
        while let Some(point) = ctx.next_charged(&mut points, OPERATION)? {
            let Some(undo) = assign(
                ctx,
                CoordinateAssignment {
                    root,
                    point,
                    assigned,
                    edges,
                    root_edges,
                    incidence,
                    degrees: base_degrees,
                    budget,
                },
            )?
            else {
                *exhausted = true;
                break;
            };
            point_uses[point] += 1;
            walk(
                ctx,
                CoordinateClosureSearch {
                    domains,
                    edges,
                    edge_ids,
                    local_edge_by_id,
                    root_edges,
                    edge_candidates,
                    incidence,
                    component_points,
                    assigned,
                    point_uses,
                    solutions,
                    states,
                    state_limit,
                    exhausted,
                    base_degrees,
                    budget,
                },
            )?;
            point_uses[point] -= 1;
            unassign(ctx, root, assigned, base_degrees, undo)?;
            if solutions.len() > 1 || *exhausted {
                break;
            }
        }
        rollback(ctx, assigned, point_uses, propagated, base_degrees)
    }

    let CloseCoordinateRootsWithIncidenceInputs {
        quotient,
        point_count,
        edge_candidates,
        incidence,
        budget,
        component_search_budget,
        ambiguous,
        exhausted,
    } = inputs;

    let ((roots, root_indices), _root_storage) =
        ctx.with_scoped_storage("catia_coordinate_closure_roots", || {
            let mut roots = Vec::new();
            let mut root_indices = ctx.alloc_filled(
                quotient.union.len(),
                None,
                "catia_coordinate_closure_root_indices",
            )?;
            for node in ctx.admit_iter(0..quotient.union.len(), "catia_coordinate_closure_roots")? {
                if quotient.union.find(ctx, node)? == node {
                    root_indices[node] = Some(roots.len());
                    ctx.push_vec(&mut roots, node, "catia_coordinate_closure_roots")?;
                }
            }
            Ok::<_, CodecError>((roots, root_indices))
        })?;
    if roots.len() < point_count {
        return Ok(None);
    }
    if roots.len() == point_count && incidence.is_none() {
        return Ok(
            match quotient.point_assignments_with_budget(
                ctx,
                point_count,
                edge_candidates,
                2,
                budget,
            )? {
                PointAssignmentOutcome::Complete(mut assignments) => match assignments.len() {
                    1 => Some(assignments.remove(0)),
                    length if length > 1 => {
                        ambiguous.set(true);
                        None
                    }
                    _ => None,
                },
                PointAssignmentOutcome::Exhausted => None,
            },
        );
    }
    let mut scratch = ctx.reserve_scoped(0, "catia_coordinate_closure_scratch")?;
    let valid = scratch.with_storage(|| -> Result<bool, CodecError> {
        let mut edges = Vec::new();
        for edge in ctx.admit_iter(0..edge_candidates.len(), "catia_coordinate_closure_edges")? {
            let Some(left) = root_indices[quotient.union.find(ctx, edge * 2)?] else {
                return Ok(false);
            };
            let Some(right) = root_indices[quotient.union.find(ctx, edge * 2 + 1)?] else {
                return Ok(false);
            };
            ctx.push_vec(&mut edges, [left, right], "catia_coordinate_closure_edges")?;
        }
        let mut domains = Vec::new();
        for &root in ctx.admit_iter(&roots, "catia_coordinate_closure_domains")? {
            // Domains ascend, so the points below `point_count` are a prefix.
            let supported = ctx.partition_point(
                &quotient.domains[root],
                |point| Ok(*point < point_count),
                "catia_coordinate_closure_domain_points",
            )?;
            if supported == 0 {
                return Ok(false);
            }
            let domain = ctx.copy_slice(
                &quotient.domains[root][..supported],
                "catia_coordinate_closure_domain_points",
            )?;
            ctx.push_vec(&mut domains, domain, "catia_coordinate_closure_domains")?;
        }
        let mut covered_points = HashSet::new();
        for domain in ctx.admit_iter(&domains, "catia_coordinate_closure_covered_points")? {
            for &point in ctx.admit_iter(domain, "catia_coordinate_closure_covered_points")? {
                ctx.insert_hash_set(
                    &mut covered_points,
                    point,
                    "catia_coordinate_closure_covered_points",
                )?;
            }
        }
        if covered_points.len() != point_count {
            return Ok(false);
        }
        let mut dependency =
            UnionFind::charged(ctx, roots.len(), "catia_coordinate_closure_dependency")?;
        for [left, right] in ctx.admit_iter(&edges, "catia_coordinate_closure_dependency")? {
            dependency.union(ctx, *left, *right)?;
        }
        let mut root_by_point = HashMap::new();
        for (root, domain) in ctx
            .admit_iter(&domains, "catia_coordinate_closure_point_roots")?
            .enumerate()
        {
            for &point in ctx.admit_iter(domain, "catia_coordinate_closure_point_roots")? {
                if let Some(previous) = ctx.insert_hash_map(
                    &mut root_by_point,
                    point,
                    root,
                    "catia_coordinate_closure_point_roots",
                )? {
                    dependency.union(ctx, previous, root)?;
                }
            }
        }
        // Ascending roots open each component's group at its least root, so the
        // groups are already ordered by their first root. Each edge belongs to
        // its left root's component.
        let mut group_by_component =
            ctx.alloc_filled(roots.len(), None, "catia_coordinate_closure_component_keys")?;
        let mut group_of_root = ctx.alloc_filled(
            roots.len(),
            0usize,
            "catia_coordinate_closure_component_keys",
        )?;
        let mut components = Vec::new();
        for root in ctx.admit_iter(0..roots.len(), "catia_coordinate_closure_component_members")? {
            let component = dependency.find(ctx, root)?;
            let group = match group_by_component[component] {
                Some(group) => group,
                None => {
                    group_by_component[component] = Some(components.len());
                    ctx.push_vec(
                        &mut components,
                        Vec::new(),
                        "catia_coordinate_closure_components",
                    )?;
                    components.len() - 1
                }
            };
            group_of_root[root] = group;
            let members: &mut Vec<usize> = &mut components[group];
            ctx.push_vec(members, root, "catia_coordinate_closure_component_members")?;
        }
        let mut component_edges = ctx.collect_indexed_vec(
            components.len(),
            "catia_coordinate_closure_edge_ids",
            |_| Ok(Vec::new()),
        )?;
        for (edge, [left, _]) in ctx
            .admit_iter(&edges, "catia_coordinate_closure_edge_ids")?
            .enumerate()
        {
            let rows: &mut Vec<usize> = &mut component_edges[group_of_root[*left]];
            ctx.push_vec(rows, edge, "catia_coordinate_closure_edge_ids")?;
        }
        let incidence = if let Some((edge_faces, boundary_domains)) = incidence {
            if budget.is_some_and(|budget| !budget.charge_by(edge_faces.len())) {
                exhausted.set(true);
                return Ok(false);
            }
            let mut counts = ctx.alloc_filled(
                boundary_domains.len(),
                0usize,
                "catia_coordinate_closure_face_counts",
            )?;
            for faces in ctx.admit_iter(edge_faces, "catia_coordinate_closure_face_counts")? {
                for (rank, face) in faces.iter().copied().enumerate() {
                    if rank == 0 || face != faces[0] {
                        counts[face] += 1;
                    }
                }
            }
            Some((edge_faces, boundary_domains, counts))
        } else {
            None
        };
        let mut assignment =
            ctx.alloc_filled(roots.len(), None, "catia coordinate root assignment")?;
        let shared_budget = budget;
        for (component, edge_ids) in ctx
            .admit_iter(components, "catia_coordinate_closure_components")?
            .zip(component_edges)
        {
            let mut component_storage =
                ctx.reserve_scoped(0, "catia_coordinate_closure_component_scratch")?;
            let valid = component_storage.with_storage(|| -> Result<bool, CodecError> {
                let Some(support_count) = ctx.fold(
                    &component,
                    Some(0usize),
                    |total, root| {
                        Ok(total.and_then(|total| total.checked_add(domains[*root].len())))
                    },
                    "catia_coordinate_closure_support_count",
                )?
                else {
                    return Ok(false);
                };
                let mut local_index = HashMap::new();
                for (local, &global) in ctx
                    .admit_iter(&component, "catia_coordinate_closure_local_index")?
                    .enumerate()
                {
                    ctx.insert_hash_map(
                        &mut local_index,
                        global,
                        local,
                        "catia_coordinate_closure_local_index",
                    )?;
                }
                let mut component_points = Vec::new();
                for &root in
                    ctx.admit_iter(&component, "catia_coordinate_closure_component_points")?
                {
                    ctx.extend_from_slice(
                        &mut component_points,
                        &domains[root],
                        "catia_coordinate_closure_component_points",
                    )?;
                }
                ctx.sort_unstable_by(
                    &mut component_points,
                    |point| point,
                    Ord::cmp,
                    "catia_coordinate_closure_component_points",
                )?;
                ctx.dedup_vec(
                    &mut component_points,
                    "catia_coordinate_closure_component_points",
                )?;
                let Some(explicit_pair_supports) = ctx.fold(
                    &edge_ids,
                    Some(0usize),
                    |total, edge| {
                        Ok(total.and_then(|total| total.checked_add(edge_candidates[*edge].len())))
                    },
                    "catia_coordinate_closure_support_count",
                )?
                else {
                    return Ok(false);
                };
                let Some(traversal_bound) = component
                    .len()
                    .checked_add(component_points.len())
                    .and_then(|size| size.isqrt().checked_add(9))
                else {
                    return Ok(false);
                };
                // A component may require one branch state for every explicit
                // root-point support before propagation distinguishes a solution.
                let state_limit = MAX_COORDINATE_CLOSURE_STATES.max(support_count);
                let component_limit = component_search_budget.map(|base| {
                    // Reserve the same graph-traversal allowance used by coordinate
                    // preparation plus face-incidence scans for every permitted
                    // branch state.
                    let incidence_work = if incidence.is_some() {
                        support_count.checked_mul(edge_ids.len())?
                    } else {
                        0
                    };
                    support_count
                        .checked_add(explicit_pair_supports)?
                        .checked_mul(traversal_bound)?
                        .checked_add(incidence_work)?
                        .checked_mul(state_limit)?
                        .checked_add(base)
                });
                let component_budget = match component_limit {
                    Some(Some(limit)) => Some(WorkBudget::new(limit)),
                    Some(None) => return Ok(false),
                    None => None,
                };
                let budget = component_budget.as_ref().or(shared_budget);
                let mut local_edges = Vec::new();
                let mut local_edge_by_id = HashMap::new();
                for (local, &edge) in ctx
                    .admit_iter(&edge_ids, "catia_coordinate_closure_local_edges")?
                    .enumerate()
                {
                    let [left, right] = edges[edge];
                    let (Some(&left), Some(&right)) = (
                        ctx.get_hash_map(
                            &local_index,
                            &left,
                            "catia_coordinate_closure_local_index",
                        )?,
                        ctx.get_hash_map(
                            &local_index,
                            &right,
                            "catia_coordinate_closure_local_index",
                        )?,
                    ) else {
                        return Ok(false);
                    };
                    ctx.push_vec(
                        &mut local_edges,
                        [left, right],
                        "catia_coordinate_closure_local_edges",
                    )?;
                    ctx.insert_hash_map(
                        &mut local_edge_by_id,
                        edge,
                        local,
                        "catia_coordinate_closure_local_edge_index",
                    )?;
                }
                let local_incidence = match incidence.as_ref() {
                    Some((edge_faces, boundary_domains, counts)) => {
                        if budget
                            .is_some_and(|budget| !budget.charge_by(work_units(edge_ids.len())))
                        {
                            exhausted.set(true);
                            return Ok(false);
                        }
                        let local_edge_faces = ctx.collect_vec(
                            edge_ids.iter().map(|edge| edge_faces[*edge]),
                            "catia_coordinate_closure_local_edge_faces",
                        )?;
                        let mut face_edges = ctx.collect_indexed_vec(
                            boundary_domains.len(),
                            "catia_coordinate_closure_face_edges",
                            |_| Ok(Vec::new()),
                        )?;
                        for (edge, faces) in ctx
                            .admit_iter(
                                &local_edge_faces,
                                "catia_coordinate_closure_face_edge_entries",
                            )?
                            .enumerate()
                        {
                            for (rank, face) in faces.iter().copied().enumerate() {
                                if rank == 0 || face != faces[0] {
                                    ctx.push_vec(
                                        &mut face_edges[face],
                                        edge,
                                        "catia_coordinate_closure_face_edge_entries",
                                    )?;
                                }
                            }
                        }
                        let closed_faces = ctx.collect_vec(
                            face_edges
                                .iter()
                                .zip(counts.iter())
                                .map(|(local, total)| local.len() == *total),
                            "catia_coordinate_closure_closed_faces",
                        )?;
                        Some(LocalIncidence {
                            edge_faces: local_edge_faces,
                            face_edges,
                            closed_faces,
                            boundary_domains,
                        })
                    }
                    None => None,
                };
                let mut local_domains = Vec::new();
                for &root in ctx.admit_iter(&component, "catia_coordinate_closure_local_domains")? {
                    let domain = ctx.copy_slice(
                        &domains[root],
                        "catia_coordinate_closure_local_domain_points",
                    )?;
                    ctx.push_vec(
                        &mut local_domains,
                        domain,
                        "catia_coordinate_closure_local_domains",
                    )?;
                }
                let mut root_edges = ctx.collect_indexed_vec(
                    component.len(),
                    "catia_coordinate_closure_root_edges",
                    |_| Ok(Vec::new()),
                )?;
                for (edge, &[left, right]) in ctx
                    .admit_iter(&local_edges, "catia_coordinate_closure_root_edge_entries")?
                    .enumerate()
                {
                    ctx.push_vec(
                        &mut root_edges[left],
                        edge,
                        "catia_coordinate_closure_root_edge_entries",
                    )?;
                    if right != left {
                        ctx.push_vec(
                            &mut root_edges[right],
                            edge,
                            "catia_coordinate_closure_root_edge_entries",
                        )?;
                    }
                }
                if !enforce_sparse_endpoint_membership(
                    ctx,
                    &mut local_domains,
                    &local_edges,
                    &edge_ids,
                    edge_candidates,
                    budget,
                )? {
                    if budget.is_some_and(WorkBudget::exhausted) {
                        exhausted.set(true);
                    }
                    return Ok(false);
                }
                let mut arc_domains = ctx.copy_retained_rows(
                    &local_domains,
                    "catia_coordinate_closure_arc_domains",
                    "catia_coordinate_closure_arc_domain_points",
                )?;
                let arc_budget =
                    budget.map(|budget| budget.session_child_slice(budget.remaining()));
                let arc_consistent = enforce_edge_arc_consistency(
                    ctx,
                    &mut arc_domains,
                    &local_edges,
                    &edge_ids,
                    &root_edges,
                    edge_candidates,
                    arc_budget.as_ref(),
                )?;
                if arc_consistent {
                    if let (Some(budget), Some(arc_budget)) = (budget, arc_budget.as_ref()) {
                        if budget.consume_child(arc_budget).is_err() {
                            exhausted.set(true);
                            return Ok(false);
                        }
                    }
                    local_domains = arc_domains;
                } else {
                    if arc_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                        exhausted.set(true);
                    }
                    return Ok(false);
                }
                let mut remaining_points = Vec::new();
                for domain in
                    ctx.admit_iter(&local_domains, "catia_coordinate_closure_remaining_points")?
                {
                    ctx.extend_from_slice(
                        &mut remaining_points,
                        domain,
                        "catia_coordinate_closure_remaining_points",
                    )?;
                }
                ctx.sort_unstable_by(
                    &mut remaining_points,
                    |point| point,
                    Ord::cmp,
                    "catia_coordinate_closure_remaining_points",
                )?;
                ctx.dedup_vec(
                    &mut remaining_points,
                    "catia_coordinate_closure_remaining_points",
                )?;
                if remaining_points.len() != component_points.len()
                    || !ctx.equal(
                        &remaining_points,
                        &component_points,
                        "catia_coordinate_closure_remaining_points",
                    )?
                {
                    return Ok(false);
                }
                let mut solutions = Vec::new();
                let mut states = 0;
                let mut exhausted = false;
                let mut base_degrees = FaceDegrees::new();
                let mut local_assignment = ctx.alloc_filled(
                    component.len(),
                    None,
                    "catia coordinate component assignment",
                )?;
                let mut point_degrees =
                    ctx.alloc_filled(point_count, 0, "catia coordinate point degrees")?;
                walk(
                    ctx,
                    CoordinateClosureSearch {
                        domains: &local_domains,
                        edges: &local_edges,
                        edge_ids: &edge_ids,
                        local_edge_by_id: &local_edge_by_id,
                        root_edges: &root_edges,
                        edge_candidates,
                        incidence: local_incidence.as_ref(),
                        component_points: &component_points,
                        assigned: &mut local_assignment,
                        point_uses: &mut point_degrees,
                        solutions: &mut solutions,
                        states: &mut states,
                        state_limit,
                        exhausted: &mut exhausted,
                        base_degrees: &mut base_degrees,
                        budget,
                    },
                )?;
                if exhausted {
                    if component_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                        if let Some(shared_budget) = shared_budget {
                            shared_budget.exhaust();
                        }
                    }
                    return Ok(false);
                }
                let [local_assignment] = solutions.as_slice() else {
                    if solutions.len() > 1 {
                        ambiguous.set(true);
                    }
                    return Ok(false);
                };
                for (&root, &point) in ctx
                    .admit_iter(&component, "catia coordinate root assignment")?
                    .zip(local_assignment)
                {
                    assignment[root] = Some(point);
                }
                Ok(true)
            })?;
            if !valid {
                return Ok(false);
            }
        }
        let mut completed_assignment = Vec::new();
        for point in ctx.admit_iter(assignment, "catia_coordinate_closure_completed_assignment")? {
            let Some(point) = point else {
                return Ok(false);
            };
            ctx.push_vec(
                &mut completed_assignment,
                point,
                "catia_coordinate_closure_completed_assignment",
            )?;
        }
        let assignment = completed_assignment;
        for (&root, &point) in ctx
            .admit_iter(&roots, "catia_coordinate_closure_fixed_domain_point")?
            .zip(&assignment)
        {
            let fixed_domain =
                quotient.new_domain("catia_coordinate_closure_fixed_domain_point", || {
                    let mut domain =
                        ctx.collection_vec(1, "catia_coordinate_closure_fixed_domain_point")?;
                    domain.push(point);
                    Ok(domain)
                })?;
            quotient.domains[root] = fixed_domain;
        }
        let mut root_by_point = HashMap::new();
        for (&root, &point) in ctx
            .admit_iter(&roots, "catia_coordinate_closure_assigned_point_roots")?
            .zip(&assignment)
        {
            match ctx.entry_hash_map(
                &mut root_by_point,
                point,
                "catia_coordinate_closure_assigned_point_roots",
            )? {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(root);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let Some(merged) = quotient.merge_charged(ctx, *entry.get(), root)? else {
                        return Ok(false);
                    };
                    entry.insert(merged);
                }
            }
        }
        if !quotient.edge_domains_viable(ctx, edge_candidates)? {
            return Ok(false);
        }
        Ok(true)
    })?;
    if !valid {
        return Ok(None);
    }
    quotient.point_assignment(ctx, point_count, edge_candidates, None)
}

#[cfg(test)]
mod tests {
    use super::{
        enforce_edge_arc_consistency, enforce_edge_arc_consistency_from,
        enforce_sparse_endpoint_membership, HashSet, MeshCandidateFailure,
        MeshCoordinateRootDomains, MeshFaceBoundaryAssignment, MeshIncidenceBoundary, MeshQuotient,
        MeshSolve, Rc, ScopedValue, WorkBudget,
    };
    use crate::solve::mesh_quotient::point_domain;
    use std::sync::Arc;

    use super::{partial_compact_assignment_viable, MeshFaceBoundaryDomain};
    use crate::solve::missing_edge::{
        MeshBoundaryEdgeCandidate, MeshDeferredBoundaryCycle, MeshDeferredFaceBoundary,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::HashMap;

    #[test]
    fn deferred_coordinate_boundary_refuses_edge_point_collection_limit() {
        let domain = MeshFaceBoundaryDomain::DeferredValidation(MeshDeferredFaceBoundary {
            cycles: vec![MeshDeferredBoundaryCycle {
                length: 2,
                exact_uses: vec![(
                    MeshBoundaryEdgeCandidate {
                        edge: 0,
                        start: 0,
                        end: 1,
                        reversed: Some(false),
                    },
                    1,
                )],
            }],
            missing_edges: vec![1],
        });
        let edge_by_id = HashMap::from([(0, 0), (1, 1)]);
        let edges = [[0, 1], [2, 3]];
        let assigned = [None, Some(1), Some(1), Some(0)];

        let arena = DecodeArena::new();
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &service)
            .expect("fixture fits the service profile");
        assert!(partial_compact_assignment_viable(&ctx, crate::solve::mesh_quotient::coordinate_assignment::PartialCompactAssignmentViableInputs { domain: &domain, local_edge_by_id: &edge_by_id, edges: &edges, global_edge_count: 2, assigned: &assigned, candidate: (0, 0), budget: None })
        .expect("service budget"));

        let mut refused = std::collections::BTreeSet::new();
        for cap in 0..=128 {
            let mut limited = DecodePolicy::service();
            limited.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &limited)
                .expect("fixture fits the input limit");
            match partial_compact_assignment_viable(&ctx, crate::solve::mesh_quotient::coordinate_assignment::PartialCompactAssignmentViableInputs { domain: &domain, local_edge_by_id: &edge_by_id, edges: &edges, global_edge_count: 2, assigned: &assigned, candidate: (0, 0), budget: None }) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    refused.insert(limit.operation);
                }
                Ok(_) => break,
                Err(error) => panic!("unexpected deferred boundary refusal: {error}"),
            }
        }
        assert!(refused.contains("catia coordinate assignment edge points"));
    }

    #[test]
    fn deferred_coordinate_boundary_charges_cycle_matching_arrays() {
        use std::collections::BTreeSet;

        let domain = MeshFaceBoundaryDomain::DeferredValidation(MeshDeferredFaceBoundary {
            cycles: vec![MeshDeferredBoundaryCycle {
                length: 3,
                exact_uses: Vec::new(),
            }],
            missing_edges: vec![0, 1, 2],
        });
        let edge_by_id = HashMap::from([(0, 0), (1, 1), (2, 2)]);
        let edges = [[0, 1], [2, 3], [4, 5]];
        let assigned = [Some(0), Some(1), Some(1), Some(2), Some(2), Some(0)];
        let mut operations = BTreeSet::new();
        let mut completed = false;
        for limit in 0..=128 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            match partial_compact_assignment_viable(&ctx, crate::solve::mesh_quotient::coordinate_assignment::PartialCompactAssignmentViableInputs { domain: &domain, local_edge_by_id: &edge_by_id, edges: &edges, global_edge_count: 3, assigned: &assigned, candidate: (0, 0), budget: None }) {
                Err(CodecError::ResourceLimit(error)) => {
                    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                    operations.insert(error.operation);
                }
                Ok(true) => {
                    completed = true;
                    break;
                }
                Ok(false) => panic!("closed deferred cycle must remain viable"),
                Err(error) => panic!("unexpected deferred boundary refusal: {error}"),
            }
        }
        assert!(
            completed,
            "collection limit 128 must admit the deferred cycle"
        );
        assert!(operations.contains("catia_deferred_matched"));
        assert!(operations.contains("catia_deferred_augment_visit"));
    }
    #[test]
    fn arc_consistency_refuses_before_incompatible_domains() {
        let run = |ctx: &DecodeContext<'_>| {
            let mut domains = vec![vec![2, 3], vec![1, 2]];
            enforce_edge_arc_consistency(
                ctx,
                &mut domains,
                &[[0, 1]],
                &[0],
                &[vec![0], vec![0]],
                &[vec![[0, 1]]],
                None,
            )
        };
        assert!(!crate::test_support::with_service_context(run).expect("service resource budget"));
        let mut refused = HashSet::new();
        for cap in 0..64 {
            match crate::test_support::with_collection_limit(cap, run) {
                Err(CodecError::ResourceLimit(limit)) => {
                    refused.insert(limit.operation);
                }
                Ok(false) => break,
                _ => panic!("unexpected arc consistency result"),
            }
        }
        for operation in [
            "catia_arc_supports",
            "catia_arc_support_edges",
            "catia_arc_queued",
            "catia_arc_queue",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn arc_consistency_from_refuses_before_incompatible_domains() {
        let run = |ctx: &DecodeContext<'_>| {
            let mut domains = vec![vec![2, 3], vec![1, 2]];
            enforce_edge_arc_consistency_from(
                ctx,
                &mut domains,
                &[[0, 1]],
                &[vec![0], vec![0]],
                &[vec![[0, 1]]],
                &[0],
                None,
            )
        };
        assert!(!crate::test_support::with_service_context(run).expect("service resource budget"));
        let mut refused = HashSet::new();
        for cap in 0..64 {
            match crate::test_support::with_collection_limit(cap, run) {
                Err(CodecError::ResourceLimit(limit)) => {
                    refused.insert(limit.operation);
                }
                Ok(false) => break,
                _ => panic!("unexpected incremental arc result"),
            }
        }
        for operation in [
            "catia_arc_from_queued",
            "catia_arc_from_queue",
            "catia_arc_from_support_edges",
            "catia_arc_from_supports",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn sparse_membership_refuses_before_incompatible_domains() {
        let run = |ctx: &DecodeContext<'_>| {
            let mut domains = vec![vec![2, 3], vec![1, 2]];
            enforce_sparse_endpoint_membership(
                ctx,
                &mut domains,
                &[[0, 1]],
                &[0],
                &[vec![[0, 1]]],
                None,
            )
        };
        assert!(!crate::test_support::with_service_context(run).expect("service resource budget"));
        let mut refused = HashSet::new();
        for cap in 0..64 {
            match crate::test_support::with_collection_limit(cap, run) {
                Err(CodecError::ResourceLimit(limit)) => {
                    refused.insert(limit.operation);
                }
                Ok(false) => break,
                _ => panic!("unexpected sparse membership result"),
            }
        }
        for operation in ["catia_sparse_ordered_edges", "catia_sparse_allowed_points"] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
    }

    #[cfg(test)]
    #[test]
    fn point_assignment_predicate_uses_temporary_storage_and_output_uses_retained_storage() {
        use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let domains = [
            point_domain(&ctx, [0usize], "fixture left domain").expect("left domain"),
            point_domain(&ctx, [1usize], "fixture right domain").expect("right domain"),
        ];
        let mut quotient = MeshQuotient::new_charged(&ctx, 2, |node| Ok(Rc::clone(&domains[node])))
            .expect("temporary quotient");
        let candidates = [vec![[0, 1]]];
        assert!(quotient
            .point_assignment_exists(&ctx, 2, &candidates, None)
            .expect("temporary predicate"));
        let Err(CodecError::ResourceLimit(refusal)) =
            quotient.point_assignment(&ctx, 2, &candidates, None)
        else {
            panic!("retained output must refuse a zero-byte limit");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "catia_point_assignment_completed");
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
            .expect("service resource budget");

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
                .expect("service resource budget"),
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
            crate::test_support::with_service_context(|ctx| quotient.merge_charged(ctx, 0, 2))
                .expect("service merge")
                .expect("shared left endpoint");
            crate::test_support::with_service_context(|ctx| quotient.merge_charged(ctx, 1, 3))
                .expect("service merge")
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
            .expect("service resource budget");
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
            .expect("service resource budget");
        assert!(matches!(complete, MeshSolve::Solved(_)));
        assert!(!complete_budget.exhausted());
    }

    #[test]
    fn coordinate_root_closure_refuses_selected_edge_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

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
            crate::test_support::with_service_context(|ctx| quotient.merge_charged(ctx, 0, 2))
                .expect("service merge")
                .expect("shared left endpoint");
            crate::test_support::with_service_context(|ctx| quotient.merge_charged(ctx, 1, 3))
                .expect("service merge")
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
        catia_test_context!(service_ctx);
        assert!(matches!(
            run(&service_ctx).expect("service resource budget"),
            MeshSolve::Solved(_)
        ));
        let mut refused = HashSet::new();
        for limit in 0..512 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            match run(&ctx) {
                Err(CodecError::ResourceLimit(error)) => {
                    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                    refused.insert(error.operation.to_owned());
                }
                Ok(MeshSolve::Solved(_)) => break,
                Ok(_) => panic!("closed incidence must solve"),
                Err(error) => panic!("unexpected refusal: {error}"),
            }
        }
        assert!(refused.contains("catia coordinate closure selected edges"));
        assert!(refused.contains("catia coordinate component assignment"));
        assert!(refused.contains("catia coordinate point degrees"));
        for operation in [
            "catia_coordinate_closure_roots",
            "catia_coordinate_closure_root_indices",
            "catia_coordinate_closure_edges",
            "catia_coordinate_closure_domain_points",
            "catia_coordinate_closure_domains",
            "catia_coordinate_closure_covered_points",
            "catia_coordinate_closure_dependency",
            "catia_coordinate_closure_point_roots",
            "catia_coordinate_closure_component_keys",
            "catia_coordinate_closure_component_members",
            "catia_coordinate_closure_components",
            "catia_coordinate_closure_face_counts",
            "catia_coordinate_closure_local_index",
            "catia_coordinate_closure_edge_ids",
            "catia_coordinate_closure_component_points",
            "catia_coordinate_closure_local_edges",
            "catia_coordinate_closure_local_edge_index",
            "catia_coordinate_closure_local_edge_faces",
            "catia_coordinate_closure_face_edges",
            "catia_coordinate_closure_face_edge_entries",
            "catia_coordinate_closure_closed_faces",
            "catia_coordinate_closure_local_domain_points",
            "catia_coordinate_closure_local_domains",
            "catia_coordinate_closure_root_edges",
            "catia_coordinate_closure_root_edge_entries",
            "catia_coordinate_closure_arc_domain_points",
            "catia_coordinate_closure_arc_domains",
            "catia_coordinate_closure_remaining_points",
            "catia_coordinate_closure_completed_assignment",
            "catia_coordinate_closure_fixed_domain_point",
            "catia_coordinate_closure_assigned_point_roots",
            "catia_coordinate_closure_scanned_roots",
            "catia_coordinate_closure_supported_unused",
            "catia_coordinate_closure_unused_point_keys",
            "catia_coordinate_closure_unused_point_roots",
            "catia_coordinate_closure_propagated",
            "catia_coordinate_closure_affected_roots",
            "catia_coordinate_closure_degree_entries",
            "catia_coordinate_closure_degree_undo",
            "catia_coordinate_closure_probe_degrees",
            "catia_coordinate_closure_affected_faces",
            "catia_coordinate_closure_complete_branch",
            "catia_coordinate_closure_completed_degrees",
            "catia_coordinate_closure_solutions",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn coordinate_root_closure_refuses_recursive_walk_depth() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

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
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits input limit");
        let mut quotient = MeshQuotient::new(
            (0..4)
                .map(|node| Arc::new(HashSet::from([usize::from(node % 2 != 0)])))
                .collect(),
        );
        crate::test_support::with_service_context(|ctx| quotient.merge_charged(ctx, 0, 2))
            .expect("service merge")
            .expect("shared left endpoint");
        crate::test_support::with_service_context(|ctx| quotient.merge_charged(ctx, 1, 3))
            .expect("service merge")
            .expect("shared right endpoint");
        let result = quotient.coordinate_root_closure_outcome(
            &ctx,
            2,
            &edge_candidates,
            Some((&edge_faces, &boundary_domains)),
            None,
        );
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RecursionDepth
                    && limit.operation == "catia_coordinate_closure_walk"
        ));
    }

    #[test]
    fn coordinate_root_closure_charges_matching_support_rows() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

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
            let mut quotient =
                MeshQuotient::new((0..4).map(|_| Arc::new(HashSet::from([0, 1]))).collect());
            crate::test_support::with_service_context(|ctx| quotient.merge_charged(ctx, 0, 2))
                .expect("service merge")
                .expect("shared left endpoint");
            crate::test_support::with_service_context(|ctx| quotient.merge_charged(ctx, 1, 3))
                .expect("service merge")
                .expect("shared right endpoint");
            quotient.coordinate_root_closure_outcome(
                ctx,
                2,
                &edge_candidates,
                Some((&edge_faces, &boundary_domains)),
                None,
            )
        };
        catia_test_context!(service_ctx);
        assert_eq!(
            run(&service_ctx).expect("service resource budget"),
            MeshSolve::Failed(MeshCandidateFailure::Ambiguous(()))
        );
        let mut refused = HashSet::new();
        for cap in 0..1024 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits input limit");
            match run(&ctx) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    refused.insert(limit.operation);
                }
                Ok(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(()))) => break,
                other => panic!("unexpected closure outcome: {other:?}"),
            }
        }
        for operation in [
            "catia_coordinate_closure_viable_domains",
            "catia_coordinate_closure_point_supports",
            "catia_coordinate_closure_support_domains",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn coordinate_closure_refuses_before_empty_domain_rejection() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

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
                .expect("service resource budget"),
            MeshSolve::Failed(MeshCandidateFailure::Rejected(()))
        ));

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        assert!(matches!(
            make_quotient().coordinate_root_closure_outcome(&ctx, 2, &candidates, None, None),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "catia_coordinate_closure_root_indices"
        ));
    }

    #[test]
    fn coordinate_coverage_matching_charges_inner_root_entries() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let domains = [vec![0]];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        assert!(
            MeshCoordinateRootDomains::coverage_matching(&ctx, &domains, 2, None)
                .expect("service resource budget")
                .is_none()
        );

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let result = MeshCoordinateRootDomains::coverage_matching(&ctx, &domains, 2, None);
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia_quotient_roots_by_point_entries"));
    }

    #[test]
    fn coordinate_root_preparation_charges_root_edge_and_matching_arrays() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let candidates = [vec![[0, 1]]];
        let make_quotient = || {
            let domain = Arc::new(HashSet::from([0, 1]));
            MeshQuotient::new(vec![domain.clone(), domain])
        };
        catia_test_context!(service_ctx);
        assert!(make_quotient()
            .prepare_coordinate_root_domains(&service_ctx, 2, &candidates, None)
            .expect("service resource budget")
            .is_some());

        let mut refused = HashSet::new();
        for limit in 0..=128 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            match make_quotient().prepare_coordinate_root_domains(&ctx, 2, &candidates, None) {
                Err(CodecError::ResourceLimit(error)) => {
                    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                    refused.insert(error.operation);
                }
                Ok(Some(_)) => break,
                Ok(None) => panic!("two endpoint roots must retain a coordinate matching"),
                Err(error) => panic!("unexpected coordinate preparation refusal: {error}"),
            };
        }
        assert!(refused.contains("catia_quotient_root_edges"));
        assert!(refused.contains("catia_quotient_roots"));
        assert!(refused.contains("catia_quotient_root_indices"));
        assert!(refused.contains("catia_quotient_edges"));
        assert!(refused.contains("catia_quotient_domain_points"));
        assert!(refused.contains("catia_quotient_domains"));
        assert!(refused.contains("catia_quotient_edge_ids"));
        assert!(refused.contains("catia_quotient_root_edge_entries"));
        assert!(refused.contains("catia_quotient_supported_candidate_rows"));
        assert!(refused.contains("catia_quotient_supported_candidate_pairs"));
        assert!(refused.contains("catia_quotient_roots_by_point"));
        assert!(refused.contains("catia_quotient_refine_roots"));
        assert!(refused.contains("catia_quotient_refine_all_points"));
    }

    #[test]
    fn local_coordinate_refinement_charges_inner_root_entries() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let domains = MeshCoordinateRootDomains {
            domains: Rc::new(ScopedValue {
                value: vec![vec![0, 1], vec![1, 2], vec![0, 2]],
                storage: None,
            }),
            edges: Rc::new(ScopedValue {
                value: vec![[0, 1], [1, 2]],
                storage: None,
            }),
            root_edges: Rc::new(ScopedValue {
                value: vec![vec![0], vec![0, 1], vec![1]],
                storage: None,
            }),
            edge_candidates: Rc::new(ScopedValue {
                value: vec![vec![[0, 1], [1, 2]], vec![[1, 2], [0, 2]]],
                storage: None,
            }),
            coverage_matching: Rc::new(ScopedValue {
                value: Vec::new(),
                storage: None,
            }),
            point_count: 3,
        };
        let candidates = [vec![[1, 2]], vec![[1, 2], [0, 2]]];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        assert!(domains
            .refine_candidates(&ctx, &candidates, None)
            .expect("service resource budget")
            .is_none());

        let mut refused = HashSet::new();
        for limit in 0..=128 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            match domains.refine_candidates(&ctx, &candidates, None) {
                Err(CodecError::ResourceLimit(error)) => {
                    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                    refused.insert(error.operation);
                }
                Ok(None) => break,
                _ => panic!("unexpected local refinement result"),
            };
        }
        assert!(refused.contains("catia_quotient_refine_root_entries"));
    }

    #[test]
    fn local_coordinate_refinement_charges_reached_root_and_point_arrays() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let domains = MeshCoordinateRootDomains {
            domains: Rc::new(ScopedValue {
                value: vec![vec![0, 1], vec![1, 2], vec![0, 2]],
                storage: None,
            }),
            edges: Rc::new(ScopedValue {
                value: vec![[0, 1], [1, 2]],
                storage: None,
            }),
            root_edges: Rc::new(ScopedValue {
                value: vec![vec![0], vec![0, 1], vec![1]],
                storage: None,
            }),
            edge_candidates: Rc::new(ScopedValue {
                value: vec![vec![[0, 1], [1, 2]], vec![[1, 2], [0, 2]]],
                storage: None,
            }),
            coverage_matching: Rc::new(ScopedValue {
                value: vec![0, 1, 2],
                storage: None,
            }),
            point_count: 3,
        };
        let refined_candidates = [vec![[1, 2]], vec![[1, 2], [0, 2]]];
        catia_test_context!(service_ctx);
        assert!(domains
            .refine_candidates(&service_ctx, &refined_candidates, None)
            .expect("service resource budget")
            .is_some());

        let mut refused = HashSet::new();
        for limit in 0..=256 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            match domains.refine_candidates(&ctx, &refined_candidates, None) {
                Err(CodecError::ResourceLimit(error)) => {
                    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                    refused.insert(error.operation);
                }
                Ok(Some(_)) => break,
                Ok(None) => panic!("local refinement must preserve the three-point matching"),
                Err(error) => panic!("unexpected coordinate refinement refusal: {error}"),
            };
        }
        assert!(refused.contains("catia_quotient_refine_roots"));
        assert!(refused.contains("catia_quotient_reached_roots"));
        assert!(refused.contains("catia_quotient_reached_points"));
        for operation in [
            "catia_quotient_refine_initial_edges",
            "catia_quotient_refine_coverage_matching",
            "catia_quotient_refine_domain_lengths",
            "catia_quotient_refine_changed_roots",
            "catia_quotient_refine_root_queue",
            "catia_quotient_refine_reached_points_list",
            "catia_quotient_refine_affected_domain_roots",
            "catia_quotient_refine_affected_domains",
            "catia_quotient_refine_affected_matching",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn coordinate_refinement_charges_hall_changed_roots_and_edges() {
        let domains = MeshCoordinateRootDomains {
            domains: Rc::new(ScopedValue {
                value: vec![vec![0, 1], vec![0, 1], vec![0, 1, 2]],
                storage: None,
            }),
            edges: Rc::new(ScopedValue {
                value: vec![[0, 2]],
                storage: None,
            }),
            root_edges: Rc::new(ScopedValue {
                value: vec![vec![0], vec![], vec![0]],
                storage: None,
            }),
            edge_candidates: Rc::new(ScopedValue {
                value: vec![vec![[0, 1], [0, 2], [1, 2]]],
                storage: None,
            }),
            coverage_matching: Rc::new(ScopedValue {
                value: vec![0, 1, 2],
                storage: None,
            }),
            point_count: 3,
        };
        let run = |ctx: &DecodeContext<'_>| {
            domains
                .refine_domains(
                    ctx,
                    ScopedValue {
                        value: domains.domains.value.clone(),
                        storage: None,
                    },
                    domains.edge_candidates.as_slice(),
                    &[],
                    true,
                    None,
                )
                .map(|result| {
                    result.map(|service| {
                        assert_eq!(service.domains[2], vec![2]);
                    })
                })
        };
        crate::test_support::with_service_context(run)
            .expect("service resource budget")
            .expect("Hall refinement is feasible");
        let mut refused = HashSet::new();
        for cap in 0..256 {
            match crate::test_support::with_collection_limit(cap, run) {
                Err(CodecError::ResourceLimit(limit)) => {
                    refused.insert(limit.operation);
                }
                Ok(Some(())) => break,
                _ => panic!("unexpected Hall refinement result"),
            }
        }
        for operation in [
            "catia_quotient_refine_affected_roots",
            "catia_quotient_refine_affected_edges",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn coordinate_root_refinement_shares_unchanged_and_reserves_changed_storage() {
        let domains = MeshCoordinateRootDomains {
            domains: Rc::new(ScopedValue {
                value: vec![vec![0], vec![1]],
                storage: None,
            }),
            edges: Rc::new(ScopedValue {
                value: vec![[0, 1]],
                storage: None,
            }),
            root_edges: Rc::new(ScopedValue {
                value: vec![vec![0], vec![0]],
                storage: None,
            }),
            edge_candidates: Rc::new(ScopedValue {
                value: vec![vec![[0, 1], [1, 0]]],
                storage: None,
            }),
            coverage_matching: Rc::new(ScopedValue {
                value: vec![0, 1],
                storage: None,
            }),
            point_count: 2,
        };
        let unchanged = |ctx: &DecodeContext<'_>| {
            domains
                .refine_candidates(ctx, domains.edge_candidates.as_slice(), None)
                .map(|result| result.is_some())
        };
        assert!(
            crate::test_support::with_service_context(unchanged).expect("service resource budget")
        );
        assert!(crate::test_support::with_collection_limit(0, unchanged)
            .expect("unchanged domains share allocations"));
        assert!(crate::test_support::with_retained_limit(0, unchanged)
            .expect("unchanged domains allocate no retained storage"));
        let shared = domains.clone();
        assert!(Rc::ptr_eq(&shared.domains, &domains.domains));
        assert!(Rc::ptr_eq(
            &shared.coverage_matching,
            &domains.coverage_matching
        ));
        let selected = |ctx: &DecodeContext<'_>| {
            domains
                .refine_edge_candidate_arc(ctx, 0, [0, 1], None)
                .map(|result| result.is_some())
        };
        catia_test_context!(selected_ctx);
        let selected_domains = domains
            .refine_edge_candidate_arc(&selected_ctx, 0, [0, 1], None)
            .expect("service resource budget")
            .expect("selected coordinate domains");
        let mut selected_refusals = HashSet::new();
        for cap in 0..256 {
            match crate::test_support::with_collection_limit(cap, selected) {
                Err(CodecError::ResourceLimit(limit)) => {
                    selected_refusals.insert(limit.operation);
                }
                Ok(true) => break,
                _ => panic!("unexpected selected coordinate domains"),
            }
        }
        for operation in [
            "catia_coordinate_refine_candidate_rows",
            "catia_coordinate_refine_selected_pair",
            "catia_coordinate_refine_domain_rows",
            "catia_coordinate_refine_domain_points",
        ] {
            assert!(
                selected_refusals.contains(operation),
                "no refusal at {operation}"
            );
        }
        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                selected_domains.edge_candidate_points(ctx, 0)
            })
            .expect("service resource budget"),
            Some(vec![0, 1])
        );
        assert!(matches!(
            crate::test_support::with_collection_limit(0, |ctx| {
                selected_domains.edge_candidate_points(ctx, 0)
            }),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "catia_coordinate_edge_candidate_points"
        ));

        let changed = |ctx: &DecodeContext<'_>| {
            domains
                .refine_candidates(ctx, &[vec![[0, 1]]], None)
                .map(|result| result.is_some())
        };
        assert!(
            crate::test_support::with_service_context(changed).expect("service resource budget")
        );
        let mut changed_refusals = HashSet::new();
        for cap in 0..256 {
            match crate::test_support::with_collection_limit(cap, changed) {
                Err(CodecError::ResourceLimit(limit)) => {
                    changed_refusals.insert(limit.operation);
                }
                Ok(true) => break,
                _ => panic!("unexpected changed coordinate domains"),
            }
        }
        assert!(changed_refusals.contains("catia_coordinate_refine_changed_edges"));

        assert!(crate::test_support::with_retained_limit(0, selected)
            .expect("selected domains use temporary storage"));
    }

    #[test]
    fn coordinate_root_preparation_budgets_independent_components_separately() {
        const COMPONENT_COUNT: usize = 8;
        catia_test_context!(ctx);
        let mut quotient = MeshQuotient::new(
            (0..COMPONENT_COUNT)
                .flat_map(|component| {
                    let points =
                        Arc::new((component * 3..component * 3 + 3).collect::<HashSet<_>>());
                    std::iter::repeat_n(points, 6)
                })
                .collect(),
        );
        let mut candidates = Vec::new();
        for component in 0..COMPONENT_COUNT {
            let node = component * 6;
            let point = component * 3;
            crate::test_support::with_service_context(|ctx| {
                quotient.merge_charged(ctx, node + 1, node + 2)
            })
            .expect("service merge")
            .expect("disjoint coordinate roots merge");
            crate::test_support::with_service_context(|ctx| {
                quotient.merge_charged(ctx, node + 3, node + 4)
            })
            .expect("service merge")
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
            .map(|face| {
                MeshFaceBoundaryDomain::UnorderedFullCycle((face * 3..face * 3 + 3).collect())
            })
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
                .expect("service resource budget"),
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
            .expect("service resource budget")
            .is_none());
        assert!(shared_incidence_budget.exhausted());

        let preparation_budget = WorkBudget::new(100);
        let outcome = quotient
            .coordinate_root_closure_outcome_for_incidence(
                &ctx,
                COMPONENT_COUNT * 3,
                &candidates,
                MeshIncidenceBoundary {
                    edge_faces: &edge_faces,
                    face_count: COMPONENT_COUNT,
                    domains: &boundary_domains,
                },
                Some(&preparation_budget),
            )
            .expect("service resource budget");
        assert!(matches!(outcome, MeshSolve::Solved(_)), "{outcome:?}");
        assert!(!preparation_budget.exhausted());
    }
}
