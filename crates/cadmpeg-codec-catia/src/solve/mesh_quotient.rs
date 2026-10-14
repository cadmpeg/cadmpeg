//! Mesh-quotient constraint solver for standard nested B-rep topology.
//!
//! Closes vertex-coordinate quotients and enumerates face endpoint configurations.

use cadmpeg_core::decode::index_from_u32;

#[cfg(test)]
use std::num::NonZeroUsize;

use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
#[cfg(test)]
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_core::CodecError;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Words in a zeroed bitset over `bits` positions.
///
/// A domain of no choices states no words. Every reader of these masks indexes
/// by a choice identifier below `bits`, and one that cannot prove the index
/// reads through `get`/`get_mut` or `zip`, so no reader reads a word an empty
/// domain does not have and no minimum is stated here.
fn bitset_words(bits: usize) -> usize {
    bits.div_ceil(64)
}

/// The roots a component still requires once the face choices merge what they
/// can. Read by `remaining_equation_merge_capacity`, which is itself
/// `#[cfg(test)]`.
///
/// A component exists, so it owns at least one root: `roots` is a
/// [`NonZeroUsize`] and a component of no roots is a state the argument cannot
/// hold. `merge_capacity` counts the roots those choices can fuse away; a
/// capacity that reaches or passes the component's root count fuses the
/// component onto that one remaining root. The relation
/// `roots > merge_capacity` is not proven - `merge_capacity` sums a per-face
/// maximum reduction over every face - so both cases are stated.
#[cfg(test)]
fn required_component_roots(roots: NonZeroUsize, merge_capacity: usize) -> usize {
    match roots.get().checked_sub(merge_capacity) {
        Some(0) | None => 1,
        Some(required) => required,
    }
}

use super::mesh_gauge::{
    build_mesh_coordinate_gauge, canonicalize_complete_endpoint_pairs,
    canonicalize_endpoint_relation_state, canonicalize_mesh_candidate_for_output,
    mesh_candidates_equivalent_with_context, MeshCandidateGauge, MeshEdgeGeometry,
};
use crate::families::standard::fbb::{largest_fbb_run, parse_edge_tables, parse_vertex_table};
#[cfg(test)]
use crate::families::standard::topology::EdgeBoundaryLayout;
use crate::families::standard::topology::{
    incidence_cycles, orient_face_cycles, reconstruct_mesh_selection, BoundaryDraft, CoedgeUse,
    EdgeRow, FaceTopologyDraft, StandardTopologyDraft,
};
use crate::solve::incidence::{
    compact_boundary_domain_viable, deferred_boundary_assignment, deferred_boundary_cycle_matches,
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
    same_unordered_pair, standard_mesh_boundary_domains_from_context,
    visit_duplicate_face_assignments, DuplicateFaceAssignmentVisit, MeshBoundaryEdgeCandidate,
    MeshDeferredFaceBoundary, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
    StandardMeshBoundaryContext,
};
use crate::solve::union_find::UnionFind;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::ops::ControlFlow;
use std::sync::Arc;

mod coordinate_assignment;
pub(in crate::solve) mod selection_search;
use selection_search::{
    canonical_singleton_coordinate_cycles, reconstruct_singleton_coordinate_topology,
    resolve_mesh_selection_from_quotient, resolve_singleton_mesh_selection,
    singleton_mesh_boundary_directions,
};

fn edge_start(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
    use_.edge.checked_mul(2)?.checked_add(usize::from(reversed))
}

fn edge_end(use_: MeshBoundaryEdgeCandidate, reversed: bool) -> Option<usize> {
    use_.edge
        .checked_mul(2)?
        .checked_add(usize::from(!reversed))
}

fn port(use_: MeshBoundaryEdgeCandidate, reversed: bool, end: bool) -> Option<usize> {
    use_.edge
        .checked_mul(2)?
        .checked_add(usize::from(if end { !reversed } else { reversed }))
}

const MAX_FACE_EQUATION_CACHE_ENTRIES: usize = 4_096;
/// Caps the optional exact-state memo without turning it into a search refusal.
const MAX_SELECTION_STATE_MEMO_ENTRIES: usize = 4_096;
/// Bounds reuse of deterministic endpoint-resolution results across incidence
/// assignments without retaining an unbounded set of complete topologies.
const MAX_ENDPOINT_RESOLUTION_MEMO_ENTRIES: usize = 256;
pub(super) const MAX_FACE_ENDPOINT_CONFIGURATION_WORK: usize = 4_096;
const MAX_FACE_DOMAIN_ASSIGNMENTS: usize = 4_096;
/// Bounds one complete mesh-constraint phase, including exhaustive endpoint
/// orientation selection.
pub(crate) const MAX_MESH_CONSTRAINT_OPERATIONS: usize = 1_000_000;
/// The relation walk and its endpoint materialization proof are independent
/// bounded phases and each uses the complete mesh-constraint allowance.
pub(crate) const MAX_MESH_TOPOLOGY_OPERATIONS: usize = MAX_MESH_CONSTRAINT_OPERATIONS * 2;
pub(super) type MeshQuotientGaugeState = (MeshQuotient, HashSet<usize>);

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

/// Coordinate root closure with unclassified failures.
type CoordinateRootClosure = MeshSolve<HashMap<usize, usize>, MeshCandidateFailure<(), (), ()>>;

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
    IncidenceEnumeration,
    EndpointResolution,
    PreferredSolutionSearch,
    FaceDomainEnumeration,
}

/// Solved mesh payload or its failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MeshSolve<T, F = MeshCandidateFailure> {
    Solved(T),
    Failed(F),
}

impl<T, F> MeshSolve<T, F> {
    fn into_option(self) -> Option<T> {
        match self {
            Self::Solved(value) => Some(value),
            Self::Failed(_) => None,
        }
    }
}

/// Mesh candidate topology and endpoint assignment.
pub(crate) type MeshCandidateSolve = MeshSolve<(StandardTopologyDraft, Vec<usize>)>;

/// Non-solved mesh candidate outcomes stored on topology diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeshCandidateFailure<
    R = MeshCandidateRejection,
    A = MeshCandidateAmbiguity,
    E = MeshCandidateExhaustion,
> {
    Rejected(R),
    Ambiguous(A),
    Exhausted(E),
}

/// Exclusive search status: a solved candidate cannot also be ambiguous or exhausted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SearchOutcome<T> {
    Open,
    Solved(T),
    Ambiguous,
    Exhausted,
}

impl<T> SearchOutcome<T> {
    pub(super) fn is_closed(&self) -> bool {
        matches!(self, Self::Ambiguous | Self::Exhausted)
    }

    pub(super) fn exhaust(&mut self) {
        if !self.is_closed() {
            *self = Self::Exhausted;
        }
    }

    fn mark_ambiguous(&mut self) {
        if !self.is_closed() {
            *self = Self::Ambiguous;
        }
    }

    pub(super) fn record_solved(&mut self, candidate: T, equivalent: impl FnOnce(&T, &T) -> bool) {
        match self {
            Self::Solved(previous) if !equivalent(previous, &candidate) => {
                *self = Self::Ambiguous;
            }
            Self::Open => *self = Self::Solved(candidate),
            Self::Solved(_) | Self::Ambiguous | Self::Exhausted => {}
        }
    }
}

/// Selected face domains, topology, and endpoint assignment.
type MeshFaceDomainCandidateSolve = MeshSolve<(Vec<[usize; 2]>, StandardTopologyDraft, Vec<usize>)>;

#[derive(Clone, Copy)]
pub(crate) enum MeshFaceAssignmentCandidates<'a> {
    Domains {
        edge_faces: &'a [[usize; 2]],
        allowed_faces: &'a [Vec<usize>],
        face_count: usize,
    },
    Concrete {
        assignments: &'a [Vec<[usize; 2]>],
        face_count: usize,
    },
}

type MeshEndpointResolve =
    MeshSolve<(StandardTopologyDraft, Vec<usize>), MeshCandidateFailure<(), (), ()>>;

impl<T> From<SearchOutcome<T>> for MeshSolve<T, MeshCandidateFailure<(), (), ()>> {
    fn from(outcome: SearchOutcome<T>) -> Self {
        match outcome {
            SearchOutcome::Open => Self::Failed(MeshCandidateFailure::Rejected(())),
            SearchOutcome::Solved(value) => Self::Solved(value),
            SearchOutcome::Ambiguous => Self::Failed(MeshCandidateFailure::Ambiguous(())),
            SearchOutcome::Exhausted => Self::Failed(MeshCandidateFailure::Exhausted(())),
        }
    }
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
    let Some(support_work) = edge_ids.iter().try_fold(0usize, |work, edge| {
        work.checked_add(edge_candidates[*edge].len().checked_mul(2)?)
    }) else {
        return Ok(false);
    };
    if support_work > 0 && budget.is_some_and(|budget| !budget.charge_by(support_work)) {
        return Ok(false);
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
    let mut queued = ctx.alloc_filled(edges.len(), [true; 2], "catia_arc_queued")?;
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
            return Ok(false);
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
    Ok(true)
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
    let mut queued = ctx.alloc_filled(edges.len(), [true; 2], "catia_arc_from_queued")?;
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
            return Ok(false);
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
    Ok(true)
}

fn enforce_sparse_endpoint_membership(
    domains: &mut [Vec<usize>],
    edges: &[[usize; 2]],
    edge_ids: &[usize],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&WorkBudget<'_>>,
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
pub(crate) struct MeshQuotient {
    union: UnionFind,
    domains: Vec<Arc<HashSet<usize>>>,
    members: Vec<Option<Arc<Vec<usize>>>>,
}

#[derive(Clone)]
pub(super) struct MeshCoordinateRootDomains {
    domains: Arc<Vec<Vec<usize>>>,
    edges: Arc<Vec<[usize; 2]>>,
    root_edges: Arc<Vec<Vec<usize>>>,
    edge_candidates: Arc<Vec<Vec<[usize; 2]>>>,
    coverage_matching: Vec<usize>,
    point_count: usize,
}

struct RefinedCoordinateDomains {
    domains: Vec<Vec<usize>>,
    coverage_matching: Vec<usize>,
}

#[derive(Clone, Copy)]
pub(super) struct MeshIncidenceBoundary<'a> {
    pub(super) edge_faces: &'a [[usize; 2]],
    pub(super) face_count: usize,
    pub(super) domains: &'a [MeshFaceBoundaryDomain],
}

#[derive(Clone)]
pub(super) struct MeshImplicitEdgeCandidates {
    source: MeshImplicitEdgeCandidateSource,
}

#[derive(Clone)]
enum MeshImplicitEdgeCandidateSource {
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
    pub(super) fn width_upper_bound(&self) -> usize {
        match &self.source {
            MeshImplicitEdgeCandidateSource::Cartesian {
                domains,
                left_root,
                right_root,
                ..
            } => domains[*left_root]
                .len()
                .saturating_mul(domains[*right_root].len()),
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

pub(super) enum MeshEndpointCandidates<'a> {
    Explicit(&'a [[usize; 2]]),
    Implicit(MeshImplicitEdgeCandidates),
    Selected([usize; 2]),
}

impl MeshCoordinateRootDomains {
    pub(super) fn edge_candidates(&self) -> &[Vec<[usize; 2]>] {
        &self.edge_candidates
    }

    pub(super) fn supports_edge_candidate(&self, edge: usize, pair: [usize; 2]) -> bool {
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

    pub(super) fn edge_candidate_points(&self, edge: usize) -> Option<Vec<usize>> {
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

    pub(super) fn implicit_edge_candidates(
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

    pub(super) fn implicit_edge_candidate_with_point(
        &self,
        edge: usize,
        required: usize,
        budget: Option<&WorkBudget<'_>>,
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

    fn coverage_matching(
        ctx: &DecodeContext<'_>,
        domains: &[Vec<usize>],
        point_count: usize,
        budget: Option<&WorkBudget<'_>>,
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

    fn refine_domains(
        &self,
        ctx: &DecodeContext<'_>,
        mut domains: Vec<Vec<usize>>,
        edge_candidates: &[Vec<[usize; 2]>],
        initial_edges: &[usize],
        mut propagate_all_different: bool,
        budget: Option<&WorkBudget<'_>>,
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
                ctx,
                &mut domains,
                &self.edges,
                &self.root_edges,
                edge_candidates,
                &affected_edges,
                budget,
            )? {
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
                let mut reached_roots =
                    ctx.alloc_filled(domains.len(), false, "catia_quotient_reached_roots")?;
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

    pub(super) fn refine_edge_candidate_arc(
        &self,
        ctx: &DecodeContext<'_>,
        edge: usize,
        pair: [usize; 2],
        budget: Option<&WorkBudget<'_>>,
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

    pub(super) fn refine_candidates(
        &self,
        ctx: &DecodeContext<'_>,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
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

pub(super) fn initial_mesh_quotient(
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
fn complete_mesh_endpoint_candidates_from_quotient(
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
            let left =
                crate::test_support::with_service_context(|ctx| quotient.union.find(edge * 2))
                    .expect("service forest traversal");
            let right =
                crate::test_support::with_service_context(|ctx| quotient.union.find(edge * 2 + 1))
                    .expect("service forest traversal");
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

    pub(super) fn len(&self) -> usize {
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

    fn members(&self, root: usize) -> &[usize] {
        self.members[root].as_deref().map_or(&[], Vec::as_slice)
    }

    pub(super) fn coordinate_domain_preparation_limit(
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

    pub(super) fn signature_work(&mut self) -> Result<Option<usize>, CodecError> {
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

    fn monotone_measure(&mut self) -> Result<Option<(usize, usize)>, CodecError> {
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

    pub(super) fn signature(&mut self) -> Result<MeshQuotientSignature, CodecError> {
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

    pub(super) fn root_count(&mut self) -> Result<usize, CodecError> {
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

    pub(super) fn prepare_coordinate_root_domains(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
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
        let mut root_edges =
            ctx.alloc_filled(roots.len(), Vec::new(), "catia_quotient_root_edges")?;
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

    fn propagate_component_edge_domains(
        &mut self,
        root: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<bool, CodecError> {
        let mut edges = HashSet::new();
        for edge in self.members(root).iter().map(|node| node / 2) {
            if !edge_candidates[edge].is_empty() {
                edges.insert(edge);
            }
        }
        self.propagate_edge_domains(edges, edge_candidates, budget)
    }

    fn affected_edges_for_nodes(
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

    fn propagate_edge_domains(
        &mut self,
        edges: impl IntoIterator<Item = usize>,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
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

    pub(super) fn merge_singleton_coordinate_roots(
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
                for &root in rest {
                    for edge in self
                        .members(first)
                        .iter()
                        .chain(self.members(root))
                        .map(|node| node / 2)
                    {
                        if !edge_candidates[edge].is_empty() {
                            affected_edges.insert(edge);
                        }
                    }
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

    pub(super) fn close_coordinate_roots(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<HashMap<usize, usize>>, CodecError> {
        Ok(self
            .coordinate_root_closure_outcome(ctx, point_count, edge_candidates, None, budget)?
            .into_option())
    }

    #[cfg(test)]
    pub(super) fn close_coordinate_roots_for_incidence_with_budget(
        &mut self,
        ctx: &DecodeContext<'_>,
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

    pub(super) fn coordinate_root_closure_outcome_for_incidence(
        &mut self,
        ctx: &DecodeContext<'_>,
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

    fn coordinate_root_closure_outcome(
        &mut self,
        ctx: &DecodeContext<'_>,
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
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
        budget: Option<&WorkBudget<'_>>,
        component_search_budget: Option<usize>,
    ) -> Result<CoordinateRootClosure, CodecError> {
        let ambiguous = Cell::new(false);
        let exhausted = Cell::new(false);
        let result = coordinate_assignment::close_coordinate_roots_with_incidence(ctx, crate::solve::mesh_quotient::coordinate_assignment::CloseCoordinateRootsWithIncidenceInputs { quotient: self, point_count, edge_candidates, incidence, budget, component_search_budget, ambiguous: &ambiguous, exhausted: &exhausted })?;
        Ok(match result {
            Some(assignment) => MeshSolve::Solved(assignment),
            None if exhausted.get() || budget.is_some_and(WorkBudget::exhausted) => {
                MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))
            }
            None if ambiguous.get() => MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())),
            None => MeshSolve::Failed(MeshCandidateFailure::Rejected(())),
        })
    }

    fn assignment_has_option(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
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
    fn assignment_options(
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
                        .expect("service merge")
                    else {
                        return;
                    };
                    if !quotient
                        .propagate_component_edge_domains(root, edge_candidates, None)
                        .expect("service resource budget")
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
                        .expect("service merge")
                    else {
                        return;
                    };
                    if quotient
                        .propagate_component_edge_domains(root, edge_candidates, None)
                        .expect("service resource budget")
                    {
                        output.push((directions.clone(), quotient));
                    }
                    return;
                }
                if let Some(reversed) = boundary[at].reversed {
                    advance(boundary, at, reversed, directions, quotient, output);
                } else {
                    advance(boundary, at, false, directions, quotient.clone(), output);
                    advance(boundary, at, true, directions, quotient, output);
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

    pub(super) fn assignment_options_limited(
        &self,
        ctx: &DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        oriented_edges: &HashSet<usize>,
        limit: usize,
        budget: Option<&WorkBudget<'_>>,
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
            'input8,
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
            budget: Option<&'input9 WorkBudget<'input8>>,
        }

        fn walk(
            ctx: &DecodeContext<'_>,
            inputs: BoundaryOrientationSearch<'_, '_, '_, '_, '_, '_, '_, '_, '_, '_>,
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

            let _depth = ctx.enter_nested("catia orientation search")?;

            if output.len() >= limit {
                return Ok(());
            }
            if budget.is_some_and(|budget| !budget.charge()) {
                return Ok(());
            }
            if boundary_index == boundaries.len() {
                if admit_orientation_option(seen, output, directions, &quotient)? {
                    output.push((directions.to_vec(), quotient));
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
                if !quotient.propagate_component_edge_domains(root, edge_candidates, budget)? {
                    return Ok(());
                }
                directions.push(std::mem::take(boundary_directions));
                walk(
                    ctx,
                    BoundaryOrientationSearch {
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
                    },
                )?;
                *boundary_directions = directions.pop().unwrap_or_default();
                return Ok(());
            }
            let edge = boundary[at].edge;
            let first = oriented.insert(edge);
            let mut advance = |reversed: bool,
                               mut quotient: MeshQuotient|
             -> Result<(), CodecError> {
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
                    if !quotient.propagate_component_edge_domains(root, edge_candidates, budget)? {
                        return Ok(());
                    }
                }
                boundary_directions.push(reversed);
                walk(
                    ctx,
                    BoundaryOrientationSearch {
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
                    },
                )?;
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
                if budget.is_some_and(|budget| !budget.charge_by(orientation_work)) {
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
                if !quotient.propagate_edge_domains(affected_edges, edge_candidates, budget)? {
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
        walk(
            ctx,
            BoundaryOrientationSearch {
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
            },
        )?;
        Ok(output)
    }

    fn assignment_options_for_directions(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        direction_options: &MeshFaceDirectionOptions,
        limit: usize,
        budget: Option<&WorkBudget<'_>>,
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
                output.push((directions.to_vec(), quotient));
            }
        }
        Ok(output)
    }

    fn merge_label_directions_in_place(
        &mut self,
        assignment: &MeshFaceBoundaryAssignment,
        label_directions: &[Vec<bool>],
        edge_orientations: &[Option<bool>],
        budget: Option<&WorkBudget<'_>>,
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

    fn assignment_option_for_label_directions(
        &self,
        assignment: &MeshFaceBoundaryAssignment,
        label_directions: &[Vec<bool>],
        edge_orientations: &[Option<bool>],
        budget: Option<&WorkBudget<'_>>,
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

    pub(super) fn point_assignment_exists(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<bool, CodecError> {
        Ok(matches!(
            self.point_assignments_with_budget(ctx, point_count, edge_candidates, 1, budget)?,
            PointAssignmentOutcome::Complete(solutions) if !solutions.is_empty()
        ))
    }

    fn point_assignments_with_budget(
        &mut self,
        ctx: &DecodeContext<'_>,
        point_count: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        solution_limit: usize,
        budget: Option<&WorkBudget<'_>>,
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
            budget: Option<&'input9 WorkBudget<'input8>>,
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
            } = inputs;

            let _depth = ctx.enter_nested("catia_point_assignment_walk")?;

            if solutions.len() >= solution_limit {
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
        let mut root_edges =
            ctx.alloc_filled(roots.len(), Vec::new(), "catia point assignment root edges")?;
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
        let mut assigned = ctx.alloc_filled(domains.len(), None, "catia point assignment slots")?;
        let mut used = HashSet::new();
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
            },
        )?;
        if budget.is_some_and(WorkBudget::exhausted) {
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
    catia_test_context!(ctx);
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
) -> Result<Option<MeshQuotient>, CodecError> {
    let mut materialized = base.clone();
    for local_node in 0..base_nodes.len() {
        let local_root = local.union.root(local_node)?;
        if local_root != local_node
            && materialized
                .merge(base_nodes[local_root], base_nodes[local_node])?
                .is_none()
        {
            return Ok(None);
        }
    }
    Ok(materialized
        .propagate_edge_domains(affected_edges, edge_candidates, Some(budget))?
        .then_some(materialized))
}

fn deferred_face_quotient_options_limited(
    ctx: &DecodeContext<'_>,
    domain: &MeshDeferredFaceBoundary,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &MeshQuotient,
    limit: usize,
    budget: &WorkBudget<'_>,
) -> Result<Option<DeferredFaceQuotientOptions>, CodecError> {
    #[derive(Clone, Copy)]
    struct Gap {
        left_end: usize,
        right_start: usize,
        capacity: usize,
    }

    struct DeferredGapFill<
        'input0,
        'input1,
        'input2,
        'input3,
        'input4,
        'input5,
        'input6,
        'input7,
        'input8,
    > {
        gaps: &'input0 [Gap],
        gap: usize,
        at: usize,
        target: usize,
        used: u64,
        previous_end: usize,
        missing_edges: &'input1 [usize],
        missing_nodes: &'input2 [[usize; 2]],
        edge_candidates: &'input3 [Vec<[usize; 2]>],
        quotient: MeshQuotient,
        base_quotient: &'input4 MeshQuotient,
        base_nodes: &'input5 [usize],
        output: &'input6 mut Vec<MeshQuotient>,
        limit: usize,
        budget: &'input8 WorkBudget<'input7>,
    }

    fn fill_gap(
        ctx: &DecodeContext<'_>,
        inputs: DeferredGapFill<'_, '_, '_, '_, '_, '_, '_, '_, '_>,
    ) -> Result<(), CodecError> {
        let DeferredGapFill {
            gaps,
            gap,
            at,
            target,
            used,
            previous_end,
            missing_edges,
            missing_nodes,
            edge_candidates,
            quotient,
            base_quotient,
            base_nodes,
            output,
            limit,
            budget,
        } = inputs;

        let _depth = ctx.enter_nested("catia deferred gap search")?;

        if output.len() >= limit || budget.exhausted() {
            return Ok(());
        }
        if at == target {
            let mut quotient = quotient;
            if quotient
                .merge(previous_end, gaps[gap].right_start)?
                .is_none()
            {
                return Ok(());
            }
            walk_gaps(
                ctx,
                DeferredGapWalk {
                    gaps,
                    gap: gap + 1,
                    used,
                    missing_edges,
                    missing_nodes,
                    edge_candidates,
                    quotient: &quotient,
                    base_quotient,
                    base_nodes,
                    output,
                    limit,
                    budget,
                },
            )?;
            return Ok(());
        }
        let Some(options) =
            (missing_edges.len() - index_from_u32(used.count_ones())).checked_mul(2)
        else {
            return Ok(());
        };
        if options > 1 && !budget.charge_by(options) {
            return Ok(());
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
                if next.merge(previous_end, start)?.is_none() {
                    continue;
                }
                let end_root = next.union.find(end)?;
                if !seen.insert((rank, end_root, next.signature()?)) {
                    continue;
                }
                fill_gap(
                    ctx,
                    DeferredGapFill {
                        gaps,
                        gap,
                        at: at + 1,
                        target,
                        used: used | (1 << rank),
                        previous_end: end,
                        missing_edges,
                        missing_nodes,
                        edge_candidates,
                        quotient: next,
                        base_quotient,
                        base_nodes,
                        output,
                        limit,
                        budget,
                    },
                )?;
                if output.len() >= limit || budget.exhausted() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    struct DeferredGapWalk<
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
        gaps: &'input0 [Gap],
        gap: usize,
        used: u64,
        missing_edges: &'input1 [usize],
        missing_nodes: &'input2 [[usize; 2]],
        edge_candidates: &'input3 [Vec<[usize; 2]>],
        quotient: &'input4 MeshQuotient,
        base_quotient: &'input5 MeshQuotient,
        base_nodes: &'input6 [usize],
        output: &'input7 mut Vec<MeshQuotient>,
        limit: usize,
        budget: &'input9 WorkBudget<'input8>,
    }

    fn walk_gaps(
        ctx: &DecodeContext<'_>,
        inputs: DeferredGapWalk<'_, '_, '_, '_, '_, '_, '_, '_, '_, '_>,
    ) -> Result<(), CodecError> {
        let DeferredGapWalk {
            gaps,
            gap,
            used,
            missing_edges,
            missing_nodes,
            edge_candidates,
            quotient,
            base_quotient,
            base_nodes,
            output,
            limit,
            budget,
        } = inputs;

        let _depth = ctx.enter_nested("catia deferred gap walk")?;

        if output.len() >= limit || budget.exhausted() {
            return Ok(());
        }
        if gap == gaps.len() {
            if index_from_u32(used.count_ones()) != missing_edges.len() {
                return Ok(());
            }
            let mut affected_edges = HashSet::new();
            for &edge in missing_edges {
                if !edge_candidates[edge].is_empty() {
                    affected_edges.insert(edge);
                }
            }
            if materialize_deferred_quotient_option(
                base_quotient,
                quotient,
                base_nodes,
                affected_edges,
                edge_candidates,
                budget,
            )?
            .is_some()
            {
                output.push(quotient.clone());
            }
            return Ok(());
        }
        let remaining_edges = missing_edges.len() - index_from_u32(used.count_ones());
        let remaining_gaps = gaps.len() - gap - 1;
        let minimum = 1;
        let Some(available_edges) = remaining_edges.checked_sub(remaining_gaps) else {
            return Ok(());
        };
        let maximum = gaps[gap].capacity.min(available_edges);
        if maximum < minimum {
            return Ok(());
        }
        if maximum > minimum && !budget.charge_by(maximum - minimum + 1) {
            return Ok(());
        }
        for target in minimum..=maximum {
            fill_gap(
                ctx,
                DeferredGapFill {
                    gaps,
                    gap,
                    at: 0,
                    target,
                    used,
                    previous_end: gaps[gap].left_end,
                    missing_edges,
                    missing_nodes,
                    edge_candidates,
                    quotient: quotient.clone(),
                    base_quotient,
                    base_nodes,
                    output,
                    limit,
                    budget,
                },
            )?;
            if output.len() >= limit || budget.exhausted() {
                return Ok(());
            }
        }
        Ok(())
    }

    if domain.missing_edges.len() > index_from_u32(u64::BITS) {
        return Ok(None);
    }
    let mut gaps = Vec::new();
    for cycle in &domain.cycles {
        if cycle.exact_uses.is_empty() {
            return Ok(None);
        }
        for index in 0..cycle.exact_uses.len() {
            let (left, left_span) = cycle.exact_uses[index];
            let right = cycle.exact_uses[(index + 1) % cycle.exact_uses.len()].0;
            let left_end_position = (left.start + left_span) % cycle.length;
            let capacity = (right.start + cycle.length - left_end_position) % cycle.length;
            if capacity == 0 {
                continue;
            }
            let (Some(left_reversed), Some(right_reversed)) = (left.reversed, right.reversed)
            else {
                return Ok(None);
            };
            let Some(left_end) = left
                .edge
                .checked_mul(2)
                .and_then(|node| node.checked_add(usize::from(!left_reversed)))
            else {
                return Ok(None);
            };
            let Some(right_start) = right
                .edge
                .checked_mul(2)
                .and_then(|node| node.checked_add(usize::from(right_reversed)))
            else {
                return Ok(None);
            };
            gaps.push(Gap {
                left_end,
                right_start,
                capacity,
            });
        }
    }
    if gaps.is_empty() {
        return Ok(domain
            .missing_edges
            .is_empty()
            .then(|| DeferredFaceQuotientOptions {
                alternatives: Vec::new(),
                base_nodes: Vec::new(),
            }));
    }
    if domain.missing_edges.len() < gaps.len() {
        return Ok(Some(DeferredFaceQuotientOptions {
            alternatives: Vec::new(),
            base_nodes: Vec::new(),
        }));
    }
    let mut base_nodes = Vec::new();
    for gap in &gaps {
        for node in [gap.left_end, gap.right_start] {
            base_nodes.push(quotient.union.root(node)?);
        }
    }
    for &edge in &domain.missing_edges {
        for node in [edge * 2, edge * 2 + 1] {
            base_nodes.push(quotient.union.root(node)?);
        }
    }
    base_nodes.sort_unstable_by(Ord::cmp);
    base_nodes.dedup();
    let mut local_by_base = HashMap::new();
    for (local, &base) in base_nodes.iter().enumerate() {
        local_by_base.insert(base, local);
    }
    for gap in &mut gaps {
        gap.left_end = local_by_base[&quotient.union.root(gap.left_end)?];
        gap.right_start = local_by_base[&quotient.union.root(gap.right_start)?];
    }
    let mut missing_nodes = Vec::new();
    for &edge in &domain.missing_edges {
        missing_nodes.push([
            local_by_base[&quotient.union.root(edge * 2)?],
            local_by_base[&quotient.union.root(edge * 2 + 1)?],
        ]);
    }
    let mut local_domains = Vec::new();
    for &root in &base_nodes {
        local_domains.push(quotient.domains[root].clone());
    }
    let local_quotient = MeshQuotient::new(local_domains);
    let mut ranked_gaps = Vec::new();
    for gap in gaps {
        let single_edge_options = if gap.capacity == 1 {
            let mut count = 0usize;
            for (rank, _) in domain.missing_edges.iter().enumerate() {
                for reversed in [false, true] {
                    let start = missing_nodes[rank][usize::from(reversed)];
                    let end = missing_nodes[rank][usize::from(!reversed)];
                    let mut trial = local_quotient.clone();
                    if trial.merge(gap.left_end, start)?.is_some()
                        && trial.merge(end, gap.right_start)?.is_some()
                    {
                        count += 1;
                    }
                }
            }
            count
        } else {
            usize::MAX
        };
        ranked_gaps.push((gap, (gap.capacity, single_edge_options)));
    }
    ranked_gaps.sort_unstable_by_key(|(_, left)| *left);
    let mut gaps = Vec::new();
    for (gap, _) in ranked_gaps {
        gaps.push(gap);
    }
    let mut output = Vec::new();
    walk_gaps(
        ctx,
        DeferredGapWalk {
            gaps: &gaps,
            gap: 0,
            used: 0,
            missing_edges: &domain.missing_edges,
            missing_nodes: &missing_nodes,
            edge_candidates,
            quotient: &local_quotient,
            base_quotient: quotient,
            base_nodes: &base_nodes,
            output: &mut output,
            limit,
            budget,
        },
    )?;
    Ok(
        (!budget.exhausted()).then_some(DeferredFaceQuotientOptions {
            alternatives: output,
            base_nodes,
        }),
    )
}

fn propagate_common_deferred_quotients(
    mut options: DeferredFaceQuotientOptions,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
    budget: &WorkBudget<'_>,
) -> Result<Option<()>, CodecError> {
    let node_count = options.base_nodes.len();
    let mut equivalence_classes = HashMap::<Vec<usize>, Vec<usize>>::new();
    for node in 0..node_count {
        let mut signature = Vec::new();
        for alternative in &mut options.alternatives {
            signature.push(alternative.union.find(node)?);
        }
        if let Some(members) = equivalence_classes.get_mut(&signature) {
            members.push(node);
        } else {
            let members = vec![node];
            equivalence_classes.insert(signature, members);
        }
    }
    for nodes in equivalence_classes.into_values() {
        let Some((&representative, rest)) = nodes.split_first() else {
            continue;
        };
        for &node in rest {
            if quotient
                .merge(options.base_nodes[representative], options.base_nodes[node])?
                .is_none()
            {
                return Ok(None);
            }
        }
    }
    for local in 0..node_count {
        let mut allowed = HashSet::new();
        for alternative in &mut options.alternatives {
            let root = alternative.union.find(local)?;
            for &point in alternative.domains[root].iter() {
                allowed.insert(point);
            }
        }
        let root = quotient.union.find(options.base_nodes[local])?;
        let mut narrowed = HashSet::new();
        for &point in quotient.domains[root].intersection(&allowed) {
            narrowed.insert(point);
        }
        if narrowed.is_empty() {
            return Ok(None);
        }
        quotient.domains[root] = Arc::new(narrowed);
    }
    let mut affected_edges = HashSet::new();
    for node in options.base_nodes {
        let root = quotient.union.find(node)?;
        for edge in quotient.members(root).iter().map(|node| node / 2) {
            if !edge_candidates[edge].is_empty() {
                affected_edges.insert(edge);
            }
        }
    }
    Ok(quotient
        .propagate_edge_domains(affected_edges, edge_candidates, Some(budget))?
        .then_some(()))
}

fn common_supported_corner_equations(
    ctx: &DecodeContext<'_>,
    quotient: &mut MeshQuotient,
    assignments: &[MeshFaceBoundaryAssignment],
    budget: &WorkBudget<'_>,
) -> Result<Option<HashSet<[usize; 2]>>, CodecError> {
    fn compatible(quotient: &MeshQuotient, left: usize, right: usize) -> Result<bool, CodecError> {
        let left = quotient.union.root(left)?;
        let right = quotient.union.root(right)?;
        Ok(left == right || !quotient.domains[left].is_disjoint(&quotient.domains[right]))
    }

    (|| -> Option<Result<HashSet<[usize; 2]>, CodecError>> {
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
                    .map(|use_| match use_.reversed {
                        Some(reversed) => vec![reversed],
                        None => vec![false, true],
                    })
                    .collect::<Vec<_>>();
                let mut supported = match (|| -> Result<Vec<Vec<Vec<bool>>>, CodecError> {
                    let mut supported = Vec::new();
                    for index in 0..boundary.len() {
                        let width = directions[(index + 1) % boundary.len()].len();
                        let height = directions[index].len();
                        let mut grid = Vec::new();
                        for _ in 0..height {
                            let row = ctx.alloc_filled(width, false, "catia_boundary_dir_row")?;
                            grid.push(row);
                        }
                        supported.push(grid);
                    }
                    Ok(supported)
                })() {
                    Ok(supported) => supported,
                    Err(error) => return Some(Err(error)),
                };
                for first in 0..directions[0].len() {
                    let mut forward = match (|| -> Result<Vec<Vec<bool>>, CodecError> {
                        let mut forward = Vec::new();
                        for states in &directions {
                            let row =
                                ctx.alloc_filled(states.len(), false, "catia_boundary_forward")?;
                            forward.push(row);
                        }
                        Ok(forward)
                    })() {
                        Ok(forward) => forward,
                        Err(error) => return Some(Err(error)),
                    };
                    forward[0][first] = true;
                    for index in 0..boundary.len() - 1 {
                        for left in 0..directions[index].len() {
                            if !forward[index][left] {
                                continue;
                            }
                            for right in 0..directions[index + 1].len() {
                                let left_node =
                                    port(boundary[index], directions[index][left], true)?;
                                let right_node =
                                    port(boundary[index + 1], directions[index + 1][right], false)?;
                                if match compatible(quotient, left_node, right_node) {
                                    Ok(value) => value,
                                    Err(error) => return Some(Err(error)),
                                } {
                                    forward[index + 1][right] = true;
                                }
                            }
                        }
                    }
                    let last = boundary.len() - 1;
                    let mut backward = match (|| -> Result<Vec<Vec<bool>>, CodecError> {
                        let mut backward = Vec::new();
                        for states in &directions {
                            let row =
                                ctx.alloc_filled(states.len(), false, "catia_boundary_backward")?;
                            backward.push(row);
                        }
                        Ok(backward)
                    })() {
                        Ok(backward) => backward,
                        Err(error) => return Some(Err(error)),
                    };
                    for state in 0..directions[last].len() {
                        let left_node = port(boundary[last], directions[last][state], true)?;
                        let right_node = port(boundary[0], directions[0][first], false)?;
                        backward[last][state] = forward[last][state]
                            && match compatible(quotient, left_node, right_node) {
                                Ok(value) => value,
                                Err(error) => return Some(Err(error)),
                            };
                    }
                    for index in (0..last).rev() {
                        for left in 0..directions[index].len() {
                            if !forward[index][left] {
                                continue;
                            }
                            for right in 0..directions[index + 1].len() {
                                if !backward[index + 1][right] {
                                    continue;
                                }
                                let left_node =
                                    port(boundary[index], directions[index][left], true)?;
                                let right_node =
                                    port(boundary[index + 1], directions[index + 1][right], false)?;
                                match compatible(quotient, left_node, right_node) {
                                    Ok(true) => {
                                        backward[index][left] = true;
                                        break;
                                    }
                                    Ok(false) => {}
                                    Err(error) => return Some(Err(error)),
                                }
                            }
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
                                    let right_node = port(
                                        boundary[index + 1],
                                        directions[index + 1][right],
                                        false,
                                    )?;
                                    if match compatible(quotient, left_node, right_node) {
                                        Ok(value) => value,
                                        Err(error) => return Some(Err(error)),
                                    } {
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
                                let left = match quotient.union.find(port(
                                    boundary[index],
                                    directions[index][left],
                                    true,
                                )?) {
                                    Ok(root) => root,
                                    Err(error) => return Some(Err(error)),
                                };
                                let right = match quotient.union.find(port(
                                    boundary[next],
                                    directions[next][right],
                                    false,
                                )?) {
                                    Ok(root) => root,
                                    Err(error) => return Some(Err(error)),
                                };
                                let equation = if left <= right {
                                    [left, right]
                                } else {
                                    [right, left]
                                };
                                equations.insert(equation);
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
        common.map(Ok)
    })()
    .transpose()
}

fn propagate_common_full_quotients(
    mut alternatives: Vec<MeshQuotient>,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
) -> Result<Option<()>, CodecError> {
    let node_count = quotient.union.len();
    let mut equivalence_classes = HashMap::<Vec<usize>, Vec<usize>>::new();
    for node in 0..node_count {
        let mut signature = Vec::new();
        for alternative in &mut alternatives {
            signature.push(alternative.union.find(node)?);
        }
        if let Some(members) = equivalence_classes.get_mut(&signature) {
            members.push(node);
        } else {
            let members = vec![node];
            equivalence_classes.insert(signature, members);
        }
    }
    for nodes in equivalence_classes.into_values() {
        let Some((&representative, rest)) = nodes.split_first() else {
            continue;
        };
        for &node in rest {
            if quotient.merge(representative, node)?.is_none() {
                return Ok(None);
            }
        }
    }

    let mut roots = Vec::new();
    for node in 0..node_count {
        if quotient.union.find(node)? == node {
            roots.push(node);
        }
    }
    for root in roots {
        let representative = quotient.members(root)[0];
        let mut allowed = HashSet::new();
        for alternative in &mut alternatives {
            let alternative_root = alternative.union.find(representative)?;
            for &point in alternative.domains[alternative_root].iter() {
                allowed.insert(point);
            }
        }
        let mut narrowed = HashSet::new();
        for &point in quotient.domains[root].intersection(&allowed) {
            narrowed.insert(point);
        }
        if narrowed.is_empty() {
            return Ok(None);
        }
        quotient.domains[root] = Arc::new(narrowed);
    }
    Ok(quotient.edge_domains_viable(edge_candidates)?.then_some(()))
}
pub(super) fn propagate_common_ordered_face_quotients(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
    budget: &WorkBudget<'_>,
) -> Result<Option<()>, CodecError> {
    (|| -> Option<Result<(), CodecError>> {
        const MAX_FACE_OPTIONS: usize = 4_096;
        const MAX_ORDERED_FACE_CONSTRAINT_OPERATIONS: usize = 64;
        const MAX_DEFERRED_FACE_CONSTRAINT_OPERATIONS: usize = 512;
        let mut face_order =
            match ctx.alloc_filled(domains.len(), 0usize, "catia_ordered_face_order") {
                Ok(order) => order,
                Err(error) => return Some(Err(error)),
            };
        for (face, slot) in face_order.iter_mut().enumerate() {
            *slot = face;
        }
        let order_key = |face: &usize| match &domains[*face] {
            MeshFaceBoundaryDomain::DeferredValidation(_) => (0, 0),
            MeshFaceBoundaryDomain::Ordered(assignments) => (1, assignments.len()),
            MeshFaceBoundaryDomain::UnorderedFullCycle(_) => (2, 0),
        };
        face_order.sort_unstable_by_key(|left| order_key(left));
        loop {
            let before = match quotient.monotone_measure() {
                Ok(measure) => measure?,
                Err(error) => return Some(Err(error)),
            };
            for &face in &face_order {
                let domain = &domains[face];
                let face_limit = match domain {
                    MeshFaceBoundaryDomain::DeferredValidation(_) => {
                        MAX_DEFERRED_FACE_CONSTRAINT_OPERATIONS
                    }
                    MeshFaceBoundaryDomain::Ordered(_)
                    | MeshFaceBoundaryDomain::UnorderedFullCycle(_) => {
                        MAX_ORDERED_FACE_CONSTRAINT_OPERATIONS
                    }
                };
                let face_budget = WorkBudget::new(face_limit);
                // An exhausted face slice skips this face.
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
                            if left.reversed.is_none() || right.reversed.is_none() {
                                continue;
                            }
                            let (left_reversed, right_reversed) = (left.reversed?, right.reversed?);
                            let left_node = left
                                .edge
                                .checked_mul(2)?
                                .checked_add(usize::from(!left_reversed))?;
                            let right_node = right
                                .edge
                                .checked_mul(2)?
                                .checked_add(usize::from(right_reversed))?;
                            let merged = match quotient.merge(left_node, right_node) {
                                Ok(Some(root)) => root,
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            };
                            merged_nodes.push(merged);
                        }
                    }
                    let affected_edges =
                        match quotient.affected_edges_for_nodes(&merged_nodes, edge_candidates) {
                            Ok(edges) => edges,
                            Err(error) => return Some(Err(error)),
                        };
                    match quotient.propagate_edge_domains(
                        affected_edges,
                        edge_candidates,
                        Some(budget),
                    ) {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(error) => return Some(Err(error)),
                    }
                    let options = match deferred_face_quotient_options_limited(
                        ctx,
                        domain,
                        edge_candidates,
                        quotient,
                        MAX_FACE_OPTIONS + 1,
                        &face_budget,
                    ) {
                        Ok(Some(options)) => options,
                        Ok(None) => continue,
                        Err(error) => return Some(Err(error)),
                    };
                    if options.alternatives.len() <= MAX_FACE_OPTIONS
                        && !options.alternatives.is_empty()
                    {
                        match propagate_common_deferred_quotients(
                            options,
                            edge_candidates,
                            quotient,
                            budget,
                        ) {
                            Ok(Some(())) => {}
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        }
                    }
                    continue;
                }
                let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                    continue;
                };
                let equations = match common_supported_corner_equations(
                    ctx,
                    quotient,
                    assignments,
                    &face_budget,
                ) {
                    Ok(equations) => equations,
                    Err(error) => return Some(Err(error)),
                };
                if let Some(equations) = equations {
                    let mut merged_nodes = Vec::new();
                    for [left, right] in equations {
                        let merged = match quotient.merge(left, right) {
                            Ok(Some(root)) => root,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                        merged_nodes.push(merged);
                    }
                    let affected_edges =
                        match quotient.affected_edges_for_nodes(&merged_nodes, edge_candidates) {
                            Ok(edges) => edges,
                            Err(error) => return Some(Err(error)),
                        };
                    match quotient.propagate_edge_domains(
                        affected_edges,
                        edge_candidates,
                        Some(budget),
                    ) {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(error) => return Some(Err(error)),
                    }
                }
                // This preparation pass propagates local equations only. The
                // complete incidence and endpoint searches below retain every
                // face domain and validate their correlated assignments. Signing
                // whole-mesh alternatives here repeats that search per face.
            }
            if match quotient.monotone_measure() {
                Ok(measure) => measure?,
                Err(error) => return Some(Err(error)),
            } == before
            {
                return Some(Ok(()));
            }
        }
    })()
    .transpose()
}

fn mesh_boundary_domain_edges(domain: &MeshFaceBoundaryDomain) -> Vec<usize> {
    let mut edges = Vec::new();
    match domain {
        MeshFaceBoundaryDomain::Ordered(assignments) => {
            for assignment in assignments {
                for use_ in assignment.boundaries.iter().flatten() {
                    edges.push(use_.edge);
                }
            }
        }
        MeshFaceBoundaryDomain::UnorderedFullCycle(ordered_edges) => {
            edges.clone_from(ordered_edges);
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            edges.clone_from(&domain.missing_edges);
            for cycle in &domain.cycles {
                for (use_, _) in &cycle.exact_uses {
                    edges.push(use_.edge);
                }
            }
        }
    }
    edges.sort_unstable_by(Ord::cmp);
    edges.dedup();
    edges
}

pub(super) fn bounded_unordered_cycle_assignments(
    ctx: &DecodeContext<'_>,
    edges: &[usize],
    quotient: &MeshQuotient,
    limit: usize,
    budget: &WorkBudget<'_>,
) -> Result<Option<Vec<MeshFaceBoundaryAssignment>>, CodecError> {
    struct Search<'a, 'b, 'c> {
        edges: &'a [usize],
        compatible: &'a HashSet<(usize, usize)>,
        ctx: &'a DecodeContext<'b>,
        limit: usize,
        budget: &'a WorkBudget<'c>,
        assignments: Vec<MeshFaceBoundaryAssignment>,
    }

    impl Search<'_, '_, '_> {
        fn walk(
            &mut self,
            first_start: usize,
            previous_end: usize,
            used: u64,
            boundary: &mut Vec<MeshBoundaryEdgeCandidate>,
        ) -> Result<bool, CodecError> {
            let _depth = self.ctx.enter_nested("catia unordered boundary search")?;

            if !self.budget.charge() {
                return Ok(false);
            }
            if boundary.len() == self.edges.len() {
                if self.compatible.contains(&(previous_end, first_start)) {
                    let completed = boundary.clone();
                    let boundaries = vec![completed];
                    self.assignments
                        .push(MeshFaceBoundaryAssignment { boundaries });
                }
                return Ok(self.assignments.len() <= self.limit);
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
                    )? {
                        return Ok(false);
                    }
                    boundary.pop();
                }
            }
            Ok(true)
        }
    }

    if edges.is_empty() || edges.len() > index_from_u32(u64::BITS) {
        return Ok(None);
    }
    let edge_count = edges.len();
    let mut edges = edges.to_vec();
    edges.sort_unstable_by(Ord::cmp);
    edges.dedup();
    if edges.len() != edge_count {
        return Ok(None);
    }
    let mut quotient = quotient.clone();
    let mut nodes = Vec::new();
    for &edge in &edges {
        nodes.push(edge * 2);
        nodes.push(edge * 2 + 1);
    }
    let mut compatible = HashSet::new();
    for &left in &nodes {
        let left_root = quotient.union.find(left)?;
        for &right in &nodes {
            let right_root = quotient.union.find(right)?;
            if left_root == right_root
                || !quotient.domains[left_root].is_disjoint(&quotient.domains[right_root])
            {
                compatible.insert((left, right));
            }
        }
    }
    let first = edges[0];
    let first_start = first * 2;
    let mut boundary = Vec::new();
    boundary.push(MeshBoundaryEdgeCandidate {
        edge: first,
        start: 0,
        end: 0,
        reversed: Some(false),
    });
    let mut search = Search {
        edges: &edges,
        compatible: &compatible,
        ctx,
        limit,
        budget,
        assignments: Vec::new(),
    };
    Ok(search
        .walk(first_start, first * 2 + 1, 1, &mut boundary)?
        .then_some(search.assignments))
}

fn advance_boundary_component_states(
    ctx: &DecodeContext<'_>,
    domain: &MeshFaceBoundaryDomain,
    states: &[MeshQuotientGaugeState],
    edge_candidates: &[Vec<[usize; 2]>],
    limit: usize,
    budget: &WorkBudget<'_>,
) -> Result<Option<Vec<MeshQuotientGaugeState>>, CodecError> {
    let outcome = (|| -> Result<Option<Vec<MeshQuotientGaugeState>>, CodecError> {
        let mut next = Vec::new();
        let mut signatures = HashSet::new();
        let domain_edges = mesh_boundary_domain_edges(domain);
        for (state, oriented_edges) in states {
            // A full state set ends this pass without a result.
            let Some(remaining) = limit
                .checked_add(1)
                .and_then(|end| end.checked_sub(next.len()))
                .filter(|remaining| *remaining != 0)
            else {
                return Ok(None);
            };
            let candidates = match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    let mut candidates = Vec::new();
                    for assignment in assignments {
                        for (_, quotient) in state.assignment_options_limited(
                            ctx,
                            assignment,
                            edge_candidates,
                            oriented_edges,
                            remaining,
                            Some(budget),
                        )? {
                            candidates.push(quotient);
                        }
                    }
                    candidates
                }
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    let Some(options) = deferred_face_quotient_options_limited(
                        ctx,
                        domain,
                        edge_candidates,
                        state,
                        remaining,
                        budget,
                    )?
                    else {
                        return Ok(None);
                    };
                    if options.alternatives.is_empty() && domain.missing_edges.is_empty() {
                        vec![state.clone()]
                    } else {
                        let mut affected_edges = HashSet::new();
                        for &edge in domain_edges
                            .iter()
                            .filter(|edge| !edge_candidates[**edge].is_empty())
                        {
                            affected_edges.insert(edge);
                        }
                        let mut candidates = Vec::new();
                        for local in &options.alternatives {
                            if let Some(candidate) = materialize_deferred_quotient_option(
                                state,
                                local,
                                &options.base_nodes,
                                affected_edges.iter().copied(),
                                edge_candidates,
                                budget,
                            )? {
                                candidates.push(candidate);
                            }
                        }
                        candidates
                    }
                }
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                    let Some(assignments) =
                        bounded_unordered_cycle_assignments(ctx, edges, state, remaining, budget)?
                    else {
                        return Ok(None);
                    };
                    let mut candidates = Vec::new();
                    for assignment in &assignments {
                        for (_, quotient) in state.assignment_options_limited(
                            ctx,
                            assignment,
                            edge_candidates,
                            oriented_edges,
                            remaining,
                            Some(budget),
                        )? {
                            candidates.push(quotient);
                        }
                    }
                    candidates
                }
            };
            for mut candidate in candidates {
                let mut next_oriented = HashSet::new();
                for &edge in oriented_edges.iter().chain(domain_edges.iter()) {
                    next_oriented.insert(edge);
                }
                let Some(work) = candidate
                    .signature_work()?
                    .and_then(|work| work.checked_add(work_units(next_oriented.len())))
                else {
                    return Ok(None);
                };
                if !budget.charge_by(work) {
                    return Ok(None);
                }
                let mut oriented_signature = Vec::new();
                for &edge in &next_oriented {
                    oriented_signature.push(edge);
                }
                oriented_signature.sort_unstable_by(Ord::cmp);
                if signatures.insert((candidate.signature()?, oriented_signature)) {
                    next.push((candidate, next_oriented));
                }
                if next.len() > limit {
                    return Ok(None);
                }
            }
            if budget.exhausted() {
                return Ok(None);
            }
        }
        Ok((!next.is_empty()).then_some(next))
    })()?;
    // Local exhaustion ends this component pass without a state set.
    if budget.exhausted() {
        return Ok(None);
    }
    Ok(outcome)
}

pub(super) fn propagate_common_boundary_components(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
) -> Result<Option<()>, CodecError> {
    const MAX_COMPONENT_STATES: usize = 128;
    const MAX_COMPONENT_OPERATIONS: usize = 8_192;
    const MAX_COMPONENT_ROUNDS: usize = 8;

    let mut domain_edges = Vec::new();
    let mut active_faces = Vec::new();
    for (face, domain) in domains.iter().enumerate() {
        let edges = mesh_boundary_domain_edges(domain);
        if edges.iter().any(|edge| edge_candidates[*edge].is_empty()) {
            active_faces.push(face);
        }
        domain_edges.push(edges);
    }
    let mut active_index = HashMap::new();
    for (index, &face) in active_faces.iter().enumerate() {
        active_index.insert(face, index);
    }
    let mut components = UnionFind::new(active_faces.len());
    let mut edge_owner = HashMap::<usize, usize>::new();
    for &face in &active_faces {
        let index = active_index[&face];
        for &edge in &domain_edges[face] {
            if let Some(previous) = edge_owner.insert(edge, index) {
                components.union(previous, index)?;
            }
        }
    }
    let mut faces_by_component = HashMap::<usize, (usize, Vec<usize>)>::new();
    for face in active_faces {
        let root = components.find(active_index[&face])?;
        // `active_faces` is built by an ascending enumeration, so the face that
        // creates a component's entry is that component's smallest face.
        if let Some((_, faces)) = faces_by_component.get_mut(&root) {
            faces.push(face);
        } else {
            let faces = vec![face];
            faces_by_component.insert(root, (face, faces));
        }
    }
    let mut face_components = Vec::new();
    for group in faces_by_component.into_values() {
        face_components.push(group);
    }
    face_components.sort_by_key(|(left, _)| *left);

    let face_keys = domains
        .iter()
        .enumerate()
        .map(|(face, domain)| {
            Ok::<_, CodecError>(match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    let mut direction_work = 0usize;
                    for assignment in assignments {
                        for boundary in &assignment.boundaries {
                            let unresolved = boundary
                                .iter()
                                .filter(|use_| use_.reversed.is_none())
                                .count();
                            direction_work += unresolved;
                        }
                    }
                    (0, assignments.len(), direction_work, face)
                }
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    (1, domain.missing_edges.len(), 0, face)
                }
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => (2, edges.len(), 0, face),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (_, mut faces) in face_components {
        let mut ordered_faces = Vec::new();
        let mut selected_edges = HashSet::new();
        while !faces.is_empty() {
            let next = faces
                .iter()
                .enumerate()
                .min_by_key(|(_, face)| {
                    let shared = domain_edges[**face]
                        .iter()
                        .filter(|edge| selected_edges.contains(*edge))
                        .count();
                    let key = face_keys[**face];
                    (key.0, usize::MAX - shared, key)
                })
                .map(|(index, _)| index);
            let Some(next) = next else {
                return Ok(None);
            };
            let face = faces.swap_remove(next);
            for &edge in &domain_edges[face] {
                selected_edges.insert(edge);
            }
            ordered_faces.push(face);
        }
        let budget = WorkBudget::new(MAX_COMPONENT_OPERATIONS);
        for _ in 0..MAX_COMPONENT_ROUNDS {
            let Some(before) = quotient.monotone_measure()? else {
                return Ok(None);
            };
            let mut cursor = 0usize;
            while cursor < ordered_faces.len() {
                let mut states = vec![(quotient.clone(), HashSet::<usize>::new())];
                let mut processed = 0usize;
                while let Some(&face) = ordered_faces.get(cursor + processed) {
                    let Some(next) = advance_boundary_component_states(
                        ctx,
                        &domains[face],
                        &states,
                        edge_candidates,
                        MAX_COMPONENT_STATES,
                        &budget,
                    )?
                    else {
                        break;
                    };
                    states = next;
                    processed += 1;
                }
                if processed == 0 {
                    cursor += 1;
                    continue;
                }
                let mut alternatives = Vec::new();
                for (state, _) in states {
                    alternatives.push(state);
                }
                if propagate_common_full_quotients(alternatives, edge_candidates, quotient)?
                    .is_none()
                {
                    return Ok(None);
                }
                cursor += processed;
            }
            let Some(after) = quotient.monotone_measure()? else {
                return Ok(None);
            };
            if after == before {
                break;
            }
        }
    }
    Ok(Some(()))
}

type MeshFaceSelection = Option<(usize, Vec<Vec<bool>>)>;
type MeshFaceDirectionOptions = Vec<Vec<Vec<bool>>>;
pub(super) type MeshEndpointPair = (usize, [usize; 2]);
pub(super) type MeshEndpointSolutionFilter<'a> =
    &'a dyn Fn(&[MeshEndpointPair]) -> Result<bool, CodecError>;
type MeshPartialEndpointSolutionFilter<'a> = &'a dyn Fn(&[Option<[usize; 2]>]) -> bool;

/// Evaluation-order constraints on mesh endpoint assignment.
///
/// At least one of `predecessors` or `dependencies` is present.
#[derive(Clone, Copy)]
pub(super) struct AssignmentOrder<'a> {
    predecessors: Option<&'a [Option<usize>]>,
    dependencies: Option<&'a [Vec<usize>]>,
}

impl<'a> AssignmentOrder<'a> {
    /// Build an assignment order from independently optional predecessor and
    /// dependency tables. Both absent is `None`.
    pub(super) fn new(
        predecessors: Option<&'a [Option<usize>]>,
        dependencies: Option<&'a [Vec<usize>]>,
    ) -> Option<Self> {
        match (predecessors, dependencies) {
            (None, None) => None,
            (predecessors, dependencies) => Some(Self {
                predecessors,
                dependencies,
            }),
        }
    }

    pub(super) fn predecessors(self) -> Option<&'a [Option<usize>]> {
        self.predecessors
    }

    pub(super) fn dependencies(self) -> Option<&'a [Vec<usize>]> {
        self.dependencies
    }
}

#[derive(Clone, Copy)]
pub(super) struct MeshPartialEndpointConstraint<'a> {
    pub(crate) active_edges: &'a [bool],
    pub(crate) coupled_edges: &'a [bool],
    pub(crate) assignment_order: Option<AssignmentOrder<'a>>,
    pub(crate) valid: MeshPartialEndpointSolutionFilter<'a>,
}
type MeshFaceEndpointConfiguration = Vec<MeshEndpointPair>;
pub(super) type MeshFaceEndpointConfigurations = Vec<MeshFaceEndpointConfiguration>;
type MeshQuotientSignature = Vec<(Vec<usize>, Vec<usize>)>;
type MeshSelectionStateSignature = (
    bool,
    Vec<MeshFaceSelection>,
    MeshQuotientSignature,
    Vec<Option<bool>>,
);
type MeshOrientationOption = (Vec<Vec<bool>>, MeshQuotient);
type MeshFaceEquationCache = RefCell<HashMap<(usize, MeshQuotientSignature), Vec<[usize; 2]>>>;

fn canonical_direction_bit(row: &[bool], index: usize) -> bool {
    row[index] ^ row.first().copied().unwrap_or(false)
}

fn orientation_fingerprint(
    quotient: &MeshQuotient,
    directions: &[Vec<bool>],
) -> Result<u64, CodecError> {
    let mut hasher = DefaultHasher::new();
    quotient.union.len().hash(&mut hasher);
    for node in 0..quotient.union.len() {
        let root = quotient.union.root(node)?;
        quotient.members(root).iter().min().hash(&mut hasher);
        let domain = &quotient.domains[root];
        domain.len().hash(&mut hasher);
        let mut xor = 0u64;
        let mut sum = 0u128;
        for point in domain.iter() {
            let mut point_hasher = DefaultHasher::new();
            point.hash(&mut point_hasher);
            let value = point_hasher.finish();
            xor ^= value;
            sum += u128::from(value);
        }
        xor.hash(&mut hasher);
        sum.hash(&mut hasher);
    }
    directions.len().hash(&mut hasher);
    for row in directions {
        row.len().hash(&mut hasher);
        for index in 0..row.len() {
            canonical_direction_bit(row, index).hash(&mut hasher);
        }
    }
    Ok(hasher.finish())
}

fn orientation_options_equivalent(
    left_quotient: &MeshQuotient,
    left_directions: &[Vec<bool>],
    right_quotient: &MeshQuotient,
    right_directions: &[Vec<bool>],
) -> Result<bool, CodecError> {
    if left_quotient.union.len() != right_quotient.union.len()
        || left_directions.len() != right_directions.len()
    {
        return Ok(false);
    }
    for node in 0..left_quotient.union.len() {
        let left_root = left_quotient.union.root(node)?;
        let right_root = right_quotient.union.root(node)?;
        if left_quotient.members(left_root).iter().min()
            != right_quotient.members(right_root).iter().min()
            || left_quotient.domains[left_root] != right_quotient.domains[right_root]
        {
            return Ok(false);
        }
    }
    Ok(left_directions
        .iter()
        .zip(right_directions)
        .all(|(left, right)| {
            left.len() == right.len()
                && (0..left.len()).all(|index| {
                    canonical_direction_bit(left, index) == canonical_direction_bit(right, index)
                })
        }))
}

fn admit_orientation_option(
    seen: &mut HashMap<u64, Vec<usize>>,
    output: &[MeshOrientationOption],
    directions: &[Vec<bool>],
    quotient: &MeshQuotient,
) -> Result<bool, CodecError> {
    let fingerprint = orientation_fingerprint(quotient, directions)?;
    if let Some(indices) = seen.get(&fingerprint) {
        for &index in indices {
            let (prior_directions, prior_quotient) = &output[index];
            if orientation_options_equivalent(
                quotient,
                directions,
                prior_quotient,
                prior_directions,
            )? {
                return Ok(false);
            }
        }
    }

    seen.entry(fingerprint).or_default().push(output.len());
    Ok(true)
}

/// Search-order information for same-class rows with identical endpoint
/// domains. The dependency order reduces branching overhead without assigning
/// endpoint values to row positions.
struct EdgeClassSearchConstraint {
    active: Vec<bool>,
    ordered: Vec<(usize, usize)>,
}

fn edge_class_search_constraint(
    ctx: &DecodeContext<'_>,
    edge_classes: &[usize],
    choices: &[Vec<[usize; 2]>],
) -> Result<Option<EdgeClassSearchConstraint>, CodecError> {
    if edge_classes.len() != choices.len() {
        return Ok(None);
    }
    let mut normalized = ctx.alloc_filled(
        choices.len(),
        Vec::new(),
        "catia_edge_class_normalized_rows",
    )?;
    for (row, pairs) in normalized.iter_mut().zip(choices) {
        *row = ctx.alloc_filled(
            pairs.len(),
            [0usize; 2],
            "catia_edge_class_normalized_pairs",
        )?;
        for (normalized_pair, pair) in row.iter_mut().zip(pairs) {
            *normalized_pair = *pair;
            normalized_pair.sort_unstable_by(Ord::cmp);
        }
        row.sort_unstable_by(Ord::cmp);
        row.dedup();
    }
    let mut active = ctx.alloc_filled(choices.len(), false, "catia_edge_class_active")?;
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
    Ok(Some(EdgeClassSearchConstraint { active, ordered }))
}

fn changed_quotient_edges(
    left: &MeshQuotient,
    right: &MeshQuotient,
) -> Result<HashSet<usize>, CodecError> {
    let mut left = left.clone();
    let mut right = right.clone();
    let mut changed = HashSet::new();
    for node in 0..left.union.len() {
        let left_root = left.union.find(node)?;
        let right_root = right.union.find(node)?;
        if left_root != right_root
            || left.members(left_root) != right.members(right_root)
            || left.domains[left_root] != right.domains[right_root]
        {
            changed.insert(node / 2);
        }
    }
    Ok(changed)
}

struct MeshSelectionSearch<'a, 'ctx> {
    ctx: &'a DecodeContext<'ctx>,
    assignments: &'a [Vec<MeshFaceBoundaryAssignment>],
    #[cfg(test)]
    possible_face_equations: Vec<Vec<[usize; 2]>>,
    possible_face_choices: Vec<Vec<Vec<[usize; 2]>>>,
    face_work: Vec<Option<usize>>,
    edge_candidates: &'a [Vec<[usize; 2]>],
    edge_rows: &'a [EdgeRow],
    vertex_points: &'a [[f64; 3]],
    candidate_gauge: Option<MeshCandidateGauge<'a>>,
    port_identities: Option<&'a [[u32; 2]]>,
    fixed_face_directions: Vec<Option<MeshFaceDirectionOptions>>,
    fixed_edge_orientations: Vec<Option<bool>>,
    edge_has_fixed_direction: Vec<bool>,
    selected: Vec<MeshFaceSelection>,
    visited_states: HashSet<MeshSelectionStateSignature>,
    outcome: SearchOutcome<(StandardTopologyDraft, Vec<usize>)>,
    face_equation_cache: MeshFaceEquationCache,
}

fn possible_face_equations(faces: &[Vec<MeshFaceBoundaryAssignment>]) -> Vec<Vec<[usize; 2]>> {
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

    let mut faces_equations = Vec::new();
    for assignments in faces {
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
        let mut face_equations = Vec::new();
        for equation in equations {
            face_equations.push(equation);
        }
        face_equations.sort_unstable_by(Ord::cmp);
        faces_equations.push(face_equations);
    }
    faces_equations
}

fn possible_face_choices_with_limit(
    faces: &[Vec<MeshFaceBoundaryAssignment>],
    face_equations: &[Vec<[usize; 2]>],
    limit: usize,
    faces_choices: &mut Vec<Vec<Vec<[usize; 2]>>>,
) -> bool {
    let budget = WorkBudget::new(limit);
    for (assignments, fallback) in faces.iter().zip(face_equations) {
        let mut choices = HashSet::new();
        for assignment in assignments {
            if !budget.charge() {
                return false;
            }
            let unknown = assignment
                .boundaries
                .iter()
                .flatten()
                .filter(|use_| use_.reversed.is_none())
                .count();
            let combinations = u32::try_from(unknown)
                .ok()
                .and_then(|unknown| 1usize.checked_shl(unknown));
            let Some(combinations) = combinations.filter(|combinations| *combinations <= 4_096)
            else {
                choices.clear();
                let copied = fallback.clone();
                choices.insert(copied);
                break;
            };
            'masks: for mask in 0..combinations {
                if !budget.charge() {
                    return false;
                }
                let mut variable = 0usize;
                let mut directions = Vec::new();
                for boundary in &assignment.boundaries {
                    let mut row = Vec::new();
                    for use_ in boundary {
                        let direction = use_.reversed.unwrap_or_else(|| {
                            let shift = unknown - variable - 1;
                            variable += 1;
                            mask & (1usize << shift) != 0
                        });
                        row.push(direction);
                    }
                    directions.push(row);
                }
                let mut equations = Vec::new();
                for (boundary, row) in assignment.boundaries.iter().zip(&directions) {
                    for index in 0..boundary.len() {
                        let next = (index + 1) % boundary.len();
                        let Some(left) = port(boundary[index], row[index], true) else {
                            continue 'masks;
                        };
                        let Some(right) = port(boundary[next], row[next], false) else {
                            continue 'masks;
                        };
                        equations.push(if left <= right {
                            [left, right]
                        } else {
                            [right, left]
                        });
                    }
                }
                equations.sort_unstable_by(Ord::cmp);
                equations.dedup();
                choices.insert(equations);
            }
        }
        let mut face_choices = Vec::new();
        for choice in choices {
            face_choices.push(choice);
        }
        face_choices.sort_unstable_by(Ord::cmp);
        faces_choices.push(face_choices);
    }
    !budget.exhausted()
}

#[cfg(test)]
fn possible_face_choices(
    faces: &[Vec<MeshFaceBoundaryAssignment>],
    face_equations: &[Vec<[usize; 2]>],
) -> Vec<Vec<Vec<[usize; 2]>>> {
    let mut choices = Vec::new();
    assert!(
        possible_face_choices_with_limit(faces, face_equations, usize::MAX, &mut choices),
        "unbounded test face-choice materialization"
    );
    choices
}

fn deduplicate_mesh_quotient_assignments(faces: &mut [Vec<MeshFaceBoundaryAssignment>]) {
    fn canonical_cycle(boundary: &[MeshBoundaryEdgeCandidate]) -> Vec<(usize, Option<bool>)> {
        let mut forward = Vec::new();
        let mut reversed = Vec::new();
        for use_ in boundary {
            forward.push((use_.edge, use_.reversed));
        }
        for use_ in boundary.iter().rev() {
            reversed.push((use_.edge, use_.reversed.map(|value| !value)));
        }
        let mut best = None;
        for values in [&forward, &reversed] {
            for start in 0..values.len() {
                let candidate = values[start..].iter().chain(&values[..start]);
                if best.is_none_or(|(best_values, best_start): (&Vec<_>, usize)| {
                    candidate
                        .clone()
                        .cmp(
                            best_values[best_start..]
                                .iter()
                                .chain(&best_values[..best_start]),
                        )
                        .is_lt()
                }) {
                    best = Some((values, start));
                }
            }
        }
        let mut canonical = Vec::new();
        if let Some((values, start)) = best {
            for &value in values[start..].iter().chain(&values[..start]) {
                canonical.push(value);
            }
        }
        canonical
    }

    for assignments in faces {
        let mut seen = HashSet::new();
        assignments.retain(|assignment| {
            let mut signature = Vec::new();
            for boundary in &assignment.boundaries {
                signature.push(canonical_cycle(boundary));
            }
            signature.sort_unstable_by(Ord::cmp);
            seen.insert(signature)
        });
    }
}

pub(super) fn mesh_assignment_endpoint_cycles_viable_by<'a>(
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

pub(super) fn mesh_assignment_endpoint_cycles_viable_where(
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

#[derive(Debug)]
pub(super) struct MeshEndpointPairSupport {
    pub(super) by_edge: HashMap<usize, HashSet<[usize; 2]>>,
}

pub(super) fn mesh_assignment_endpoint_cycle_support_by<'a>(
    assignment: &MeshFaceBoundaryAssignment,
    budget: Option<&WorkBudget<'_>>,
    candidates: impl Fn(usize) -> Option<MeshEndpointCandidates<'a>>,
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Option<MeshEndpointPairSupport> {
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
        let mut prefixes = Vec::new();
        prefixes.push(identity.clone());
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
pub(super) fn mesh_assignment_endpoint_cycles_viable(
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
) -> bool {
    crate::test_support::with_service_context(|ctx| {
        mesh_assignment_endpoint_cycles_viable_with(assignment, edge_candidates, None, None)
            .expect("service resource budget")
            .unwrap_or(true)
    })
}

pub(super) fn mesh_face_endpoint_configurations(
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
fn endpoint_configuration_boundary_cycle_viable(
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

fn endpoint_configuration_cycles_viable(
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

fn endpoint_configuration_for_assignment(
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
enum MeshDirectionEnumerationError {
    Invalid,
    Overflow,
}

fn endpoint_configuration_boundary_directions(
    ctx: &DecodeContext<'_>,
    boundary: &[MeshBoundaryEdgeCandidate],
    pairs: &HashMap<usize, [usize; 2]>,
) -> Result<Result<Vec<Vec<bool>>, MeshDirectionEnumerationError>, CodecError> {
    if boundary.is_empty() {
        return Ok(Err(MeshDirectionEnumerationError::Invalid));
    }
    let first = &boundary[0];
    let Some(&first_pair) = pairs.get(&first.edge) else {
        return Ok(Err(MeshDirectionEnumerationError::Invalid));
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
        let directions = ctx.alloc_filled(1, direction, "catia_endpoint_initial_direction")?;
        states.push((start, current, directions));
    }
    for use_ in &boundary[1..] {
        let Some(&pair) = pairs.get(&use_.edge) else {
            return Ok(Err(MeshDirectionEnumerationError::Invalid));
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
                    return Ok(Err(MeshDirectionEnumerationError::Overflow));
                }
            }
        }
        states = next;
        if states.is_empty() {
            return Ok(Ok(Vec::new()));
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
    Ok(Ok(solutions))
}

fn endpoint_configuration_directions(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    configuration: &MeshFaceEndpointConfiguration,
) -> Result<Result<MeshFaceDirectionOptions, MeshDirectionEnumerationError>, CodecError> {
    let mut pairs = HashMap::new();
    for &(edge, pair) in configuration {
        pairs.insert(edge, pair);
    }
    if pairs.len() != configuration.len() {
        return Ok(Err(MeshDirectionEnumerationError::Invalid));
    }
    let mut alternatives =
        ctx.alloc_filled(1, Vec::new(), "catia_endpoint_initial_alternatives")?;
    for boundary in &assignment.boundaries {
        let boundary_options =
            match endpoint_configuration_boundary_directions(ctx, boundary, &pairs)? {
                Ok(options) => options,
                Err(error) => return Ok(Err(error)),
            };
        let mut next = Vec::new();
        for prefix in &alternatives {
            for boundary_directions in &boundary_options {
                let mut alternative = prefix.clone();
                alternative.push(boundary_directions.clone());
                next.push(alternative);
                if next.len() > MAX_FACE_ENDPOINT_CONFIGURATION_WORK {
                    return Ok(Err(MeshDirectionEnumerationError::Overflow));
                }
            }
        }
        alternatives = next;
        if alternatives.is_empty() {
            break;
        }
    }
    Ok(Ok(alternatives))
}

#[derive(Clone)]
pub(super) struct MeshEndpointRelationChoice {
    pub(super) id: usize,
    pub(super) selection: MeshEndpointRelationSelection,
}
/// Enumerated endpoint relation or deferred enumeration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum MeshEndpointRelationSelection {
    Enumerated {
        assignments: Vec<usize>,
        edge_pairs: MeshFaceEndpointConfiguration,
    },
    Deferred,
}

impl MeshEndpointRelationSelection {
    /// Explicit edge constraints of this selection.
    pub(super) fn edge_pairs(&self) -> &[(usize, [usize; 2])] {
        match self {
            Self::Enumerated { edge_pairs, .. } => edge_pairs,
            Self::Deferred => &[],
        }
    }

    fn is_unconstrained(&self) -> bool {
        match self {
            Self::Enumerated { edge_pairs, .. } => edge_pairs.is_empty(),
            Self::Deferred => true,
        }
    }

    /// Selection with canonical assignment and endpoint ordering.
    pub(super) fn normalized(&self) -> Self {
        match self {
            Self::Enumerated {
                assignments,
                edge_pairs,
            } => {
                let mut assignments = assignments.clone();
                assignments.sort_unstable_by(Ord::cmp);
                assignments.dedup();
                let mut edge_pairs = edge_pairs.clone();
                for (_, pair) in &mut edge_pairs {
                    pair.sort_unstable_by(Ord::cmp);
                }
                edge_pairs.sort_unstable_by(Ord::cmp);
                Self::Enumerated {
                    assignments,
                    edge_pairs,
                }
            }
            Self::Deferred => Self::Deferred,
        }
    }
}

type MeshEndpointRelationSelections = Vec<Vec<usize>>;
pub(super) type MeshEndpointRelationStateSignature = (
    Vec<Option<[usize; 2]>>,
    Vec<Vec<MeshEndpointRelationSelection>>,
);
type MeshEndpointSolutionPredicate<'a> = dyn Fn(&[Option<[usize; 2]>]) -> bool + 'a;
type MeshFixedDirectionOption = (Vec<Vec<bool>>, MeshQuotient, Vec<Option<bool>>);

pub(super) fn raw_endpoint_relation_state_signature(
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
) -> MeshEndpointRelationStateSignature {
    let mut normalized_assigned = Vec::new();
    for pair in assigned {
        let pair = match *pair {
            Some(mut pair) => {
                pair.sort_unstable_by(Ord::cmp);
                Some(pair)
            }
            None => None,
        };
        normalized_assigned.push(pair);
    }
    let mut normalized_domains = Vec::new();
    for choices in domains {
        let mut row = Vec::new();
        for choice in choices {
            let normalized = choice.selection.normalized();
            row.push(normalized);
        }
        row.sort_unstable_by(Ord::cmp);
        normalized_domains.push(row);
    }
    (normalized_assigned, normalized_domains)
}

fn endpoint_relation_state_signature(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<Option<MeshEndpointRelationStateSignature>, CodecError> {
    if let Some(gauge) = candidate_gauge {
        canonicalize_endpoint_relation_state(ctx, domains, assigned, gauge)
    } else {
        Ok(Some(raw_endpoint_relation_state_signature(
            domains, assigned,
        )))
    }
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
    let has_unconstrained_choice = domains
        .iter()
        .flatten()
        .any(|choice| choice.selection.is_unconstrained());
    let mut candidates = Vec::new();
    let mut possible = Vec::new();
    for base in base_candidates {
        let copy = base.clone();
        candidates.push(copy);
        possible.push(Vec::<[usize; 2]>::new());
    }
    if !has_unconstrained_choice {
        for choice in domains.iter().flatten() {
            for &(edge, pair) in choice.selection.edge_pairs() {
                let row = possible.get_mut(edge)?;
                row.push(pair);
            }
        }
    }
    for (edge, assigned) in assigned.iter().enumerate() {
        if let Some(pair) = assigned {
            candidates[edge].retain(|candidate| same_unordered_pair(*candidate, *pair));
        } else if !has_unconstrained_choice && !possible[edge].is_empty() {
            candidates[edge].retain(|candidate| {
                possible[edge]
                    .iter()
                    .any(|possible| same_unordered_pair(*candidate, *possible))
            });
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
    let mut canonical = Vec::new();

    for row in directions {
        let mut complement = row.clone();
        for direction in &mut complement {
            *direction = !*direction;
        }
        canonical.push(if complement < *row {
            complement
        } else {
            row.clone()
        });
    }
    canonical
}

type EndpointRelationKey = Vec<Option<[usize; 2]>>;
type EndpointRelationKeys<'a> = Vec<(&'a MeshEndpointRelationChoice, EndpointRelationKey)>;

fn canonical_endpoint_relation_key(
    choice: &MeshEndpointRelationChoice,
    edges: &[usize],
) -> EndpointRelationKey {
    let mut key = Vec::new();
    for &edge in edges {
        let pair = choice
            .selection
            .edge_pairs()
            .iter()
            .find_map(|&(candidate, pair)| (candidate == edge).then_some(pair))
            .map(|mut pair| {
                if pair[1] < pair[0] {
                    pair.swap(0, 1);
                }
                pair
            });
        key.push(pair);
    }
    key
}

fn complete_endpoint_relation_keys<'a>(
    choices: &'a [MeshEndpointRelationChoice],
    edges: &[usize],
) -> (bool, EndpointRelationKeys<'a>) {
    let mut keys = Vec::new();
    for choice in choices {
        let key = canonical_endpoint_relation_key(choice, edges);
        keys.push((choice, key));
    }
    let complete = keys.iter().all(|(_, key)| key.iter().all(Option::is_some));
    (complete, keys)
}

fn build_endpoint_relation_constraints(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    budget: &WorkBudget<'_>,
) -> Result<Option<MeshEndpointRelationConstraints>, CodecError> {
    let mut shared_edges = HashMap::<(usize, usize), Vec<usize>>::new();
    let mut edge_faces = HashMap::<usize, HashSet<usize>>::new();
    for (face, choices) in domains.iter().enumerate() {
        for choice in choices {
            for &(edge, _) in choice.selection.edge_pairs() {
                edge_faces.entry(edge).or_default().insert(face);
            }
        }
    }
    for (edge, faces) in edge_faces {
        let mut sorted_faces = Vec::new();
        for face in faces {
            sorted_faces.push(face);
        }
        sorted_faces.sort_unstable_by(Ord::cmp);
        for (left_index, &left) in sorted_faces.iter().enumerate() {
            for &right in &sorted_faces[left_index + 1..] {
                for pair in [(left, right), (right, left)] {
                    shared_edges.entry(pair).or_default().push(edge);
                }
            }
        }
    }

    let mut shared_rows = Vec::new();
    for (pair, mut edges) in shared_edges {
        edges.sort_unstable_by(Ord::cmp);
        shared_rows.push((pair, edges));
    }
    shared_rows.sort_unstable_by_key(|(left, _)| *left);
    let mut arcs = ctx.alloc_filled(domains.len(), Vec::new(), "catia_endpoint_relation_arcs")?;
    let mut incoming = ctx.alloc_filled(
        domains.len(),
        Vec::new(),
        "catia_endpoint_relation_incoming",
    )?;
    let mut choice_counts = Vec::new();
    for domain in domains {
        choice_counts.push(domain.len());
    }
    for ((face, neighbor), edges) in shared_rows {
        // A pair is in `shared_edges` only because both faces are in
        // `edge_faces` for the shared edge, and a face reaches `edge_faces`
        // only through an edge one of its own choices names. Both domains
        // therefore hold at least one choice and the charge is at least two,
        // so the pair owes no empty-step unit.
        // Plain `+`: both operands are lengths of live allocations, so each is
        // at most `isize::MAX` and their sum is inside `usize`.
        let index_work = domains[face].len() + domains[neighbor].len();
        if !budget.charge_by(index_work) {
            return Ok(None);
        }
        let (left_complete, left_choices) = complete_endpoint_relation_keys(&domains[face], &edges);
        let (right_complete, right_choices) =
            complete_endpoint_relation_keys(&domains[neighbor], &edges);
        let supports = if left_complete && right_complete {
            let mut index = HashMap::<EndpointRelationKey, Vec<usize>>::new();
            for (choice, key) in right_choices {
                index.entry(key).or_default().push(choice.id);
            }
            let mut supports = Vec::new();
            for (_, key) in &left_choices {
                let mut mask = ctx.alloc_filled(
                    bitset_words(domains[neighbor].len()),
                    0u64,
                    "catia_endpoint_relation_support_mask",
                )?;
                for &other in index.get(key).into_iter().flatten() {
                    mask[other / 64] |= 1u64 << (other % 64);
                }
                supports.push(mask);
            }
            supports
        } else {
            let Some(comparisons) = domains[face].len().checked_mul(domains[neighbor].len()) else {
                return Ok(None);
            };
            let comparison_work = work_units(comparisons);
            if !budget.charge_by(comparison_work) {
                return Ok(None);
            }
            let mut supports = Vec::new();
            for (_, left_key) in &left_choices {
                let mut mask = ctx.alloc_filled(
                    bitset_words(domains[neighbor].len()),
                    0u64,
                    "catia_endpoint_relation_support_mask",
                )?;
                for (other, right_key) in &right_choices {
                    let compatible = left_key.iter().zip(right_key).all(|(left, right)| {
                        left.as_ref()
                            .zip(right.as_ref())
                            .is_none_or(|(left, right)| same_unordered_pair(*left, *right))
                    });
                    if compatible {
                        mask[other.id / 64] |= 1u64 << (other.id % 64);
                    }
                }
                supports.push(mask);
            }
            supports
        };
        let arc_index = arcs[face].len();
        arcs[face].push(MeshEndpointRelationArc { neighbor, supports });
        incoming[neighbor].push((face, arc_index));
    }
    Ok(Some(MeshEndpointRelationConstraints {
        arcs,
        incoming,
        choice_counts,
    }))
}

fn propagate_endpoint_relation_domains(
    ctx: &DecodeContext<'_>,
    domains: &mut [Vec<MeshEndpointRelationChoice>],
    assigned: &mut [Option<[usize; 2]>],
    constraints: &MeshEndpointRelationConstraints,
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    let mut dirty_faces = Vec::new();
    for (face, choices) in domains.iter().enumerate() {
        if choices.len() != constraints.choice_counts[face] {
            dirty_faces.push(face);
        }
    }
    let mut first_pass = true;
    loop {
        let mut changed = false;
        for (face, choices) in domains.iter_mut().enumerate() {
            if !budget.charge_by(work_units(choices.len())) {
                return Ok(false);
            }
            let before = choices.len();
            choices.retain(|choice| {
                choice.selection.edge_pairs().iter().all(|&(edge, pair)| {
                    assigned[edge].is_none_or(|selected| same_unordered_pair(selected, pair))
                })
            });
            if choices.is_empty() {
                return Ok(false);
            }
            if choices.len() != before {
                dirty_faces.push(face);
                changed = true;
            }
        }

        let mut active = Vec::new();
        for &choice_count in &constraints.choice_counts {
            let mask = ctx.alloc_filled(
                bitset_words(choice_count),
                0u64,
                "catia_endpoint_relation_active_mask",
            )?;
            active.push(mask);
        }
        for (face, choices) in domains.iter().enumerate() {
            for choice in choices {
                let Some(active_word) = active[face].get_mut(choice.id / 64) else {
                    return Ok(false);
                };
                *active_word |= 1u64 << (choice.id % 64);
            }
        }
        let mut queue = VecDeque::new();
        if first_pass && dirty_faces.is_empty() {
            for (face, arcs) in constraints.arcs.iter().enumerate() {
                for arc in 0..arcs.len() {
                    queue.push_back((face, arc));
                }
            }
        } else {
            for face in dirty_faces.drain(..) {
                for &incoming in &constraints.incoming[face] {
                    queue.push_back(incoming);
                }
            }
        }
        first_pass = false;
        while let Some((face, arc_index)) = queue.pop_front() {
            let arc = &constraints.arcs[face][arc_index];
            let neighbor_active = &active[arc.neighbor];
            let before = domains[face].len();
            domains[face].retain(|choice| {
                let Some(supports) = arc.supports.get(choice.id) else {
                    return false;
                };
                if !budget.charge_by(work_units(supports.len())) {
                    return false;
                }
                supports
                    .iter()
                    .zip(neighbor_active)
                    .any(|(supported, active)| supported & active != 0)
            });
            if budget.exhausted() || domains[face].is_empty() {
                return Ok(false);
            }
            if domains[face].len() == before {
                continue;
            }
            active[face].fill(0);
            for choice in &domains[face] {
                active[face][choice.id / 64] |= 1u64 << (choice.id % 64);
            }
            changed = true;
            for &incoming in constraints.incoming[face]
                .iter()
                .filter(|&&(source, _)| source != arc.neighbor)
            {
                queue.push_back(incoming);
            }
        }
        // A pair present with one value in every surviving choice of one face
        // is a forced edge relation, even when the face still has assignment
        // or boundary-direction alternatives. Record it before branching on
        // those independent alternatives.
        for choices in domains.iter() {
            if !budget.charge_by(work_units(choices.len())) {
                return Ok(false);
            }
            let choice_count = choices.len();
            let mut pairs = HashMap::<usize, HashSet<[usize; 2]>>::new();
            let mut counts = HashMap::<usize, usize>::new();
            for choice in choices {
                for &(edge, pair) in choice.selection.edge_pairs() {
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
                    Some(selected) if !same_unordered_pair(selected, pair) => return Ok(false),
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
            for &(edge, pair) in choices[0].selection.edge_pairs() {
                match assigned[edge] {
                    Some(selected) if !same_unordered_pair(selected, pair) => return Ok(false),
                    Some(_) => {}
                    None => {
                        assigned[edge] = Some(pair);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            return Ok(true);
        }
    }
}

// The recursive walk keeps branch-owned domains and shared memo state explicit;
// a context object would hide which values are cloned for each branch.
struct WalkEndpointRelationDomainsInputs<
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
    F,
> where
    F: FnMut(MeshEndpointRelationSelections, Vec<[usize; 2]>) -> Result<bool, CodecError>,
{
    domains: Vec<Vec<MeshEndpointRelationChoice>>,
    face_assignments: &'input0 [Vec<MeshFaceBoundaryAssignment>],
    assigned: Vec<Option<[usize; 2]>>,
    constraints: &'input1 MeshEndpointRelationConstraints,
    point_count: usize,
    budget: &'input3 WorkBudget<'input2>,
    state_memo: &'input4 mut HashSet<MeshEndpointRelationStateSignature>,
    candidate_gauge: Option<MeshCandidateGauge<'input5>>,
    priority_edges: Option<&'input6 [bool]>,
    partial_solution_valid: Option<&'input8 MeshEndpointSolutionPredicate<'input7>>,
    coordinate_domains: Option<&'input9 MeshCoordinateRootDomains>,
    coordinate_budget: Option<&'input11 WorkBudget<'input10>>,
    evaluate: &'input12 mut F,
}

fn walk_endpoint_relation_domains<F>(
    ctx: &DecodeContext<'_>,
    inputs: WalkEndpointRelationDomainsInputs<
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
        F,
    >,
) -> Result<bool, CodecError>
where
    F: FnMut(MeshEndpointRelationSelections, Vec<[usize; 2]>) -> Result<bool, CodecError>,
{
    let WalkEndpointRelationDomainsInputs {
        domains,
        face_assignments,
        assigned,
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
    } = inputs;

    let _depth = ctx.enter_nested("catia_endpoint_relation_walk_depth")?;
    if budget.exhausted() || !budget.charge() {
        return Ok(true);
    }
    let mut domains = domains;
    let mut assigned = assigned;
    let propagated = if domains.iter().all(|choices| choices.len() == 1) {
        true
    } else {
        propagate_endpoint_relation_domains(ctx, &mut domains, &mut assigned, constraints, budget)?
    };
    if !propagated {
        return Ok(false);
    }
    // The partial predicate is monotone by contract: it can reject only
    // assignments that no completion can repair. Apply it as soon as
    // propagation fixes endpoint pairs, before branching over domains.
    if let Some(valid) = partial_solution_valid {
        if !valid(&assigned) {
            return Ok(false);
        }
    }
    // The final coordinate binding is surjective: every coordinate row must
    // occur in the selected endpoint pairs. When every surviving relation
    // choice has an explicit edge configuration, their point union is an
    // upper bound for every completion of this branch.
    if domains
        .iter()
        .flatten()
        .all(|choice| !choice.selection.is_unconstrained())
    {
        let mut possible_points = HashSet::new();
        for point in assigned.iter().flatten().flatten().copied() {
            possible_points.insert(point);
        }
        for point in domains
            .iter()
            .flatten()
            .flat_map(|choice| {
                choice
                    .selection
                    .edge_pairs()
                    .iter()
                    .flat_map(|(_, pair)| pair)
            })
            .copied()
        {
            possible_points.insert(point);
        }
        if possible_points.len() < point_count {
            return Ok(false);
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
                return Ok(false);
            };
            if coordinate_domains
                .refine_candidates(ctx, &candidates, Some(coordinate_budget))?
                .is_none()
                && !coordinate_budget.exhausted()
            {
                return Ok(false);
            }
        }
    }
    if state_memo.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
        const MAX_GAUGE_STATE_ALTERNATIVES: usize = 512;
        let alternative_count = domains.iter().map(Vec::len).sum::<usize>();
        let signature =
            if candidate_gauge.is_some() && alternative_count <= MAX_GAUGE_STATE_ALTERNATIVES {
                endpoint_relation_state_signature(ctx, &domains, &assigned, candidate_gauge)?
            } else {
                Some(raw_endpoint_relation_state_signature(&domains, &assigned))
            };
        if let Some(signature) = signature {
            if !state_memo.insert(signature) {
                return Ok(false);
            }
        }
    }
    let mut priority_counts = Vec::new();
    if let Some(edges) = priority_edges.filter(|_| domains.iter().any(|choices| choices.len() > 1))
    {
        for choices in &domains {
            let mut priority = HashSet::new();
            for edge in choices
                .iter()
                .flat_map(|choice| choice.selection.edge_pairs().iter().map(|(edge, _)| *edge))
            {
                if edges.get(edge).copied().unwrap_or(false) {
                    priority.insert(edge);
                }
            }
            priority_counts.push(priority.len());
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
            let priority_count = priority_counts.get(*face).copied().unwrap_or(0);
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
            for &(edge, pair) in choice.selection.edge_pairs() {
                match edge_pairs[edge] {
                    Some(selected) if !same_unordered_pair(selected, pair) => return Ok(false),
                    Some(_) => {}
                    None => edge_pairs[edge] = Some(pair),
                }
            }
        }
        let mut completed_pairs = Vec::new();
        for pair in edge_pairs {
            let Some(pair) = pair else {
                return Ok(false);
            };
            completed_pairs.push(pair);
        }
        let mut selections = Vec::new();
        for (face, choices) in domains.iter().enumerate() {
            let Some(choice) = choices.first() else {
                return Ok(false);
            };
            let viable = if let MeshEndpointRelationSelection::Enumerated { assignments, .. } =
                &choice.selection
            {
                assignments.clone()
            } else {
                let mut viable = Vec::new();
                for (assignment, assignment_value) in face_assignments[face].iter().enumerate() {
                    let Some(configuration) =
                        endpoint_configuration_for_assignment(assignment_value, &completed_pairs)
                    else {
                        continue;
                    };
                    if endpoint_configuration_cycles_viable(assignment_value, &configuration)
                        != Some(true)
                    {
                        continue;
                    }
                    viable.push(assignment);
                }
                if viable.is_empty() {
                    return Ok(false);
                }
                viable
            };
            selections.push(viable);
        }
        return evaluate(selections, completed_pairs);
    };
    let mut assigned_points = HashSet::new();
    for point in assigned.iter().flatten().flatten().copied() {
        assigned_points.insert(point);
    }
    let mut point_support = HashMap::<usize, usize>::new();
    for choices in &domains {
        for choice in choices {
            let mut points = HashSet::new();
            for point in choice
                .selection
                .edge_pairs()
                .iter()
                .flat_map(|(_, pair)| pair)
                .copied()
            {
                points.insert(point);
            }
            for point in points {
                *point_support.entry(point).or_default() += 1;
            }
        }
    }
    let mut branch_order = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        let mut points = HashSet::new();
        for point in choice
            .selection
            .edge_pairs()
            .iter()
            .flat_map(|(_, pair)| pair)
            .copied()
        {
            points.insert(point);
        }
        let Some(score) = points
            .into_iter()
            .filter(|point| !assigned_points.contains(point))
            .try_fold(0usize, |score, point| {
                point_count
                    .checked_sub(point_support.get(&point).copied().unwrap_or(0))?
                    .checked_add(1)
                    .and_then(|contribution| score.checked_add(contribution))
            })
        else {
            return Ok(true);
        };
        branch_order.push((score, index));
    }
    branch_order.sort_unstable_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| choices[*left].id.cmp(&choices[*right].id))
    });
    for (_, index) in branch_order {
        if budget.exhausted() {
            return Ok(true);
        }
        let mut branch = domains.clone();
        branch[face].clear();
        let choice = &choices[index];
        let selection = choice.selection.clone();
        branch[face].push(MeshEndpointRelationChoice {
            id: choice.id,
            selection,
        });
        if walk_endpoint_relation_domains(
            ctx,
            crate::solve::mesh_quotient::WalkEndpointRelationDomainsInputs {
                domains: branch,
                face_assignments,
                assigned: assigned.clone(),
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
            },
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Collect one face's endpoint relation choices. A missing configuration list
/// is an unknown result from bounded enumeration, not an empty domain.
fn collect_endpoint_relation_face_choices(
    face_assignments: &[MeshFaceBoundaryAssignment],
    face_configurations: &[Option<MeshFaceEndpointConfigurations>],
    covered: &mut [bool],
) -> Option<Vec<MeshEndpointRelationChoice>> {
    let mut choices_by_configuration = HashMap::<MeshFaceEndpointConfiguration, Vec<usize>>::new();
    let mut unknown = false;
    for (assignment, configurations) in face_configurations.iter().enumerate() {
        let Some(configurations) = configurations else {
            unknown = true;
            continue;
        };
        for configuration in configurations {
            for &(edge, _) in configuration {
                let covered = covered.get_mut(edge)?;
                *covered = true;
            }
            let face_assignment = face_assignments.get(assignment)?;
            if endpoint_configuration_cycles_viable(face_assignment, configuration) != Some(true) {
                continue;
            }
            let mut relation_configuration = configuration.clone();
            for (_, pair) in &mut relation_configuration {
                pair.sort_unstable_by(Ord::cmp);
            }
            relation_configuration.sort_unstable_by(Ord::cmp);

            (choices_by_configuration
                .entry(relation_configuration)
                .or_default())
            .push(assignment);
        }
    }
    let mut choices = Vec::new();
    for (edge_pairs, mut assignments) in choices_by_configuration {
        assignments.sort_unstable_by(Ord::cmp);
        assignments.dedup();
        choices.push(MeshEndpointRelationChoice {
            id: 0,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments,
                edge_pairs,
            },
        });
    }
    choices.sort_unstable_by(|left, right| {
        (left.selection.edge_pairs(), &left.selection)
            .cmp(&(right.selection.edge_pairs(), &right.selection))
    });
    if unknown {
        // A stopped enumeration is not evidence that the assignment has no
        // configuration. The wildcard lets the relation walker defer that
        // assignment to the complete endpoint search.
        choices.push(MeshEndpointRelationChoice {
            id: 0,
            selection: MeshEndpointRelationSelection::Deferred,
        });
    }
    for (id, choice) in choices.iter_mut().enumerate() {
        choice.id = id;
    }
    Some(choices)
}

/// Solve the unordered endpoint-configuration relation before selecting
/// intrinsic cycle orientations. A configuration records one candidate pair
/// for every edge in a face. Shared edges must agree on that pair, but their
/// row-port order is selected later by the quotient search.
// These independent inputs describe one bounded relation phase; keeping them
// separate preserves the ownership of parsed evidence and branch state.
#[derive(Clone, Copy)]
struct ResolveEndpointConfigurationRelationStreamingInputs<
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
> {
    assignments: &'input0 [Vec<MeshFaceBoundaryAssignment>],
    endpoint_configurations: &'input1 [Vec<Option<MeshFaceEndpointConfigurations>>],
    edge_candidates: &'input2 [Vec<[usize; 2]>],
    edge_rows: &'input3 [EdgeRow],
    vertex_points: &'input4 [[f64; 3]],
    port_identities: &'input5 [[u32; 2]],
    budget: &'input7 WorkBudget<'input6>,
    partial_solution_valid: Option<&'input9 MeshEndpointSolutionPredicate<'input8>>,
    complete_solution_valid: Option<&'input11 MeshEndpointSolutionPredicate<'input10>>,
    candidate_gauge: Option<MeshCandidateGauge<'input12>>,
    priority_edges: Option<&'input13 [bool]>,
    coordinate_domains: Option<&'input14 MeshCoordinateRootDomains>,
}

fn resolve_endpoint_configuration_relation_streaming(
    ctx: &DecodeContext<'_>,
    inputs: ResolveEndpointConfigurationRelationStreamingInputs<
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
) -> Result<Option<MeshEndpointResolve>, CodecError> {
    let ResolveEndpointConfigurationRelationStreamingInputs {
        assignments,
        endpoint_configurations,
        edge_candidates,
        edge_rows,
        vertex_points,
        port_identities,
        budget,
        partial_solution_valid,
        complete_solution_valid,
        candidate_gauge,
        priority_edges,
        coordinate_domains,
    } = inputs;

    if assignments.len() != endpoint_configurations.len()
        || edge_candidates.iter().any(Vec::is_empty)
    {
        return Ok(None);
    }
    let mut domains = Vec::new();
    let mut covered = ctx.alloc_filled(
        edge_candidates.len(),
        false,
        "catia_endpoint_relation_covered",
    )?;
    for (face_assignments, face_configurations) in assignments.iter().zip(endpoint_configurations) {
        if face_assignments.len() != face_configurations.len() {
            return Ok(None);
        }
        let choices = collect_endpoint_relation_face_choices(
            face_assignments,
            face_configurations,
            &mut covered,
        );
        let Some(choices) = choices else {
            return Ok(None);
        };
        if choices.is_empty() {
            return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Rejected(()))));
        }
        if !budget.charge_by(choices.len()) {
            return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))));
        }
        domains.push(choices);
    }
    if covered.iter().any(|covered| !covered) {
        return Ok(None);
    }
    let Some(constraints) = build_endpoint_relation_constraints(ctx, &domains, budget)? else {
        return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))));
    };
    let mut resolved = None;
    let mut relation_state_memo =
        HashSet::<(MeshEndpointRelationSelections, Vec<[usize; 2]>)>::new();
    let mut relation_walk_state_memo = HashSet::<MeshEndpointRelationStateSignature>::new();
    let coordinate_budget =
        coordinate_domains.map(|_| budget.child_slice(MAX_MESH_CONSTRAINT_OPERATIONS));
    let mut ambiguous = false;
    let mut exhausted = false;
    let mut evaluate = |selections: MeshEndpointRelationSelections,
                        edge_pairs: Vec<[usize; 2]>|
     -> Result<bool, CodecError> {
        let mut point_set = HashSet::new();
        for &point in edge_pairs.iter().flatten() {
            point_set.insert(point);
        }
        if point_set.len() != vertex_points.len() {
            return Ok(false);
        }
        if let Some(valid) = partial_solution_valid {
            let mut candidate_pairs = Vec::new();
            for &pair in &edge_pairs {
                candidate_pairs.push(Some(pair));
            }
            if !valid(&candidate_pairs) {
                return Ok(false);
            }
        }
        if relation_state_memo.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let canonical_pairs = if let Some(gauge) = candidate_gauge {
                canonicalize_complete_endpoint_pairs(&edge_pairs, gauge)
            } else {
                Some(edge_pairs.clone())
            };
            let Some(canonical_pairs) = canonical_pairs else {
                return Ok(false);
            };
            let selection_copy = selections.clone();
            if !relation_state_memo.insert((selection_copy, canonical_pairs)) {
                return Ok(false);
            }
        }
        let mut assignment_domains = Vec::new();
        for (face, options) in selections.iter().enumerate() {
            let mut row = Vec::new();
            for &assignment in options {
                let Some(source) = assignments[face].get(assignment) else {
                    continue;
                };
                let mut boundaries = Vec::new();
                for boundary in &source.boundaries {
                    let copy = boundary.clone();
                    boundaries.push(copy);
                }
                row.push(MeshFaceBoundaryAssignment { boundaries });
            }
            assignment_domains.push(row);
        }
        if assignment_domains.iter().any(Vec::is_empty) {
            return Ok(false);
        }
        let mut candidates = Vec::new();
        for &pair in &edge_pairs {
            let row = vec![pair];
            candidates.push(row);
        }
        let endpoint_resolution_budget = budget.child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        let outcome = if assignment_domains.iter().all(|domain| domain.len() == 1) {
            let mut selected = Vec::new();
            let mut complete = true;
            for mut domain in assignment_domains {
                if let Some(choice) = domain.pop() {
                    selected.push(choice);
                } else {
                    complete = false;
                    break;
                }
            }
            if complete {
                resolve_fixed_mesh_endpoint_pairs(
                    ctx,
                    MeshEndpointGeometry {
                        edge_rows,
                        vertex_points,
                    },
                    &candidates,
                    &selected,
                    port_identities,
                    &endpoint_resolution_budget,
                    candidate_gauge,
                )?
            } else {
                MeshSolve::Failed(MeshCandidateFailure::Rejected(()))
            }
        } else {
            resolve_fixed_mesh_endpoint_assignment_domains(
                ctx,
                MeshEndpointGeometry {
                    edge_rows,
                    vertex_points,
                },
                &candidates,
                &assignment_domains,
                port_identities,
                &endpoint_resolution_budget,
                candidate_gauge,
            )?
        };
        match outcome {
            MeshSolve::Solved((topology, assignment)) => {
                if let Some(valid) = complete_solution_valid {
                    let Some(candidate_pairs) =
                        mesh_candidate_point_pairs(ctx, &topology, &assignment)?
                    else {
                        return Ok(false);
                    };
                    if !valid(&candidate_pairs) {
                        return Ok(false);
                    }
                }
                let candidate = (topology, assignment);
                if let Some(previous) = &resolved {
                    let equivalent = mesh_candidates_equivalent_with_context(
                        ctx,
                        previous,
                        &candidate,
                        candidate_gauge,
                    )?;
                    if !equivalent {
                        ambiguous = true;
                        return Ok(true);
                    }
                } else {
                    resolved = Some(candidate);
                }
            }
            MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => {
                ambiguous = true;
                return Ok(true);
            }
            MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => {
                exhausted = true;
                return Ok(true);
            }
            MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
        }
        if budget.exhausted() {
            exhausted = true;
            return Ok(true);
        }
        Ok(false)
    };
    walk_endpoint_relation_domains(
        ctx,
        crate::solve::mesh_quotient::WalkEndpointRelationDomainsInputs {
            domains,
            face_assignments: assignments,
            assigned: ctx.alloc_filled(
                edge_candidates.len(),
                None,
                "catia_endpoint_relation_assigned",
            )?,
            constraints: &constraints,
            point_count: vertex_points.len(),
            budget,
            state_memo: &mut relation_walk_state_memo,
            candidate_gauge,
            priority_edges,
            partial_solution_valid,
            coordinate_domains,
            coordinate_budget: coordinate_budget.as_ref(),
            evaluate: &mut evaluate,
        },
    )?;
    Ok(if ambiguous {
        Some(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())))
    } else if exhausted || budget.exhausted() {
        Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(())))
    } else if let Some((topology, assignment)) = resolved {
        Some(MeshSolve::Solved((topology, assignment)))
    } else {
        Some(MeshSolve::Failed(MeshCandidateFailure::Rejected(())))
    })
}

fn mesh_candidate_point_pairs(
    ctx: &DecodeContext<'_>,
    topology: &StandardTopologyDraft,
    point_assignment: &[usize],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    let Some(edge_vertices) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    let mut pairs = Vec::new();
    for [start, end] in edge_vertices {
        let (Some(start), Some(end)) = (point_assignment.get(start), point_assignment.get(end))
        else {
            return Ok(None);
        };
        pairs.push(Some([*start, *end]));
    }
    Ok(Some(pairs))
}

fn endpoint_pairs_respect_candidate_domains(
    pairs: &[Option<[usize; 2]>],
    edge_candidates: &[Vec<[usize; 2]>],
) -> bool {
    if pairs.len() != edge_candidates.len() {
        return false;
    }
    pairs.iter().zip(edge_candidates).all(|(pair, candidates)| {
        let Some(pair) = pair else {
            return true;
        };
        let valid = candidates.is_empty()
            || candidates
                .iter()
                .any(|candidate| same_unordered_pair(*candidate, *pair));
        valid
    })
}

fn restore_unique_endpoint_pair_orientations(
    pairs: &[[usize; 2]],
    candidates: &[Vec<[usize; 2]>],
) -> Option<Vec<[usize; 2]>> {
    if pairs.len() != candidates.len() {
        return None;
    }
    let mut oriented = Vec::new();
    for (&pair, candidates) in pairs.iter().zip(candidates) {
        let mut unique = None;
        let mut multiple = false;
        for &candidate in candidates {
            if !same_unordered_pair(candidate, pair) {
                continue;
            }
            if unique.is_some_and(|previous| previous != candidate) {
                multiple = true;
                break;
            }
            unique = Some(candidate);
        }
        let direction = if multiple {
            pair
        } else if let Some(unique) = unique {
            unique
        } else if candidates.is_empty() {
            pair
        } else {
            return None;
        };
        oriented.push(direction);
    }
    Some(oriented)
}

fn materialize_boundary_domains(
    domains: &[MeshFaceBoundaryDomain],
    edge_pairs: &[[usize; 2]],
) -> Result<Option<Vec<Vec<MeshFaceBoundaryAssignment>>>, CodecError> {
    let mut materialized = Vec::new();

    for domain in domains {
        let assignments = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                let mut copies = Vec::new();

                for assignment in assignments {
                    let boundaries = assignment.boundaries.clone();
                    copies.push(MeshFaceBoundaryAssignment { boundaries });
                }
                copies
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                let Some(assignment) = deferred_boundary_assignment(domain, edge_pairs)? else {
                    return Ok(None);
                };
                vec![assignment]
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                let Some(cycles) = incidence_cycles(edges, edge_pairs)? else {
                    return Ok(None);
                };
                let [cycle] = cycles.as_slice() else {
                    return Ok(None);
                };
                let length = cycle.len();
                let mut boundary = Vec::new();

                for (index, &(edge, reversed)) in cycle.iter().enumerate() {
                    boundary.push(MeshBoundaryEdgeCandidate {
                        edge,
                        start: index,
                        end: (index + 1) % length,
                        reversed: Some(reversed),
                    });
                }
                let boundaries = vec![boundary];
                vec![MeshFaceBoundaryAssignment { boundaries }]
            }
        };
        materialized.push(assignments);
    }
    Ok(Some(materialized))
}

// A concrete face assignment is viable only when each selected incident face
// can host the edge in its retained trim-domain representation. This is a
// necessary incidence check; it does not select an alternate face.
fn mesh_domains_have_incident_edge_support(
    edge_faces: &[[usize; 2]],
    domains: &[MeshFaceBoundaryDomain],
) -> bool {
    edge_faces.iter().enumerate().all(|(edge, faces)| {
        faces.iter().all(|face| {
            let Some(domain) = domains.get(*face) else {
                return false;
            };
            match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    assignments.iter().any(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .any(|candidate| candidate.edge == edge)
                    })
                }
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => edges.contains(&edge),
                MeshFaceBoundaryDomain::DeferredValidation(coverage) => {
                    coverage.missing_edges.contains(&edge)
                        || coverage
                            .cycles
                            .iter()
                            .flat_map(|cycle| &cycle.exact_uses)
                            .any(|(candidate, _)| candidate.edge == edge)
                }
            }
        })
    })
}

#[cfg(test)]
mod face_domain_support_tests {
    use super::{
        endpoint_pairs_respect_candidate_domains, materialize_boundary_domains,
        mesh_domains_have_incident_edge_support, restore_unique_endpoint_pair_orientations,
    };
    use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;
    use crate::solve::missing_edge::MeshDeferredFaceBoundary;
    use crate::solve::missing_edge::MeshFaceBoundaryAssignment;
    use crate::solve::missing_edge::MeshFaceBoundaryDomain;
    use cadmpeg_core::CodecError;
    use std::collections::HashSet;

    fn assignment(edge: usize) -> MeshFaceBoundaryAssignment {
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 1,
                reversed: None,
            }]],
        }
    }

    #[test]
    fn every_concrete_incident_face_must_host_the_edge() {
        let edge_faces = [[0, 1]];
        let valid = vec![
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
        ];
        assert!(mesh_domains_have_incident_edge_support(&edge_faces, &valid));

        let wrong_face = vec![
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
            MeshFaceBoundaryDomain::Ordered(vec![assignment(1)]),
        ];
        assert!(!mesh_domains_have_incident_edge_support(
            &edge_faces,
            &wrong_face
        ));

        let unordered = vec![
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
            MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0]),
        ];
        assert!(mesh_domains_have_incident_edge_support(
            &edge_faces,
            &unordered
        ));

        let deferred = vec![
            MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
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
                ]],
            }]),
            MeshFaceBoundaryDomain::DeferredValidation(MeshDeferredFaceBoundary {
                cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
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
            }),
        ];
        assert!(mesh_domains_have_incident_edge_support(
            &[[0, 1], [0, 1]],
            &deferred
        ));
        assert!(!mesh_domains_have_incident_edge_support(
            &[[0, 1], [0, 1], [0, 1]],
            &deferred
        ));
    }

    #[test]
    fn endpoint_pairs_must_remain_inside_candidate_domains() {
        let candidates = vec![vec![[1, 2], [2, 3]], Vec::new()];
        assert!(endpoint_pairs_respect_candidate_domains(
            &[Some([2, 1]), None],
            &candidates,
        ));
        assert!(!endpoint_pairs_respect_candidate_domains(
            &[Some([0, 2]), None],
            &candidates,
        ));
        assert!(!endpoint_pairs_respect_candidate_domains(
            &[Some([1, 2])],
            &candidates,
        ));
    }

    #[test]
    fn unique_candidate_restores_endpoint_pair_orientation() {
        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                restore_unique_endpoint_pair_orientations(
                    &[[0, 1], [2, 3], [4, 5]],
                    &[vec![[1, 0]], vec![[2, 3], [3, 2]], Vec::new()],
                )
            })
            .expect("service resource budget"),
            Some(vec![[1, 0], [2, 3], [4, 5]])
        );
        assert!(crate::test_support::with_service_context(|ctx| {
            restore_unique_endpoint_pair_orientations(&[[0, 1]], &[vec![[2, 3]]])
        })
        .expect("service resource budget")
        .is_none());
    }

    #[test]
    fn complete_endpoint_pairs_materialize_deferred_boundaries() {
        catia_test_context!(ctx);
        let domains = [MeshFaceBoundaryDomain::DeferredValidation(
            MeshDeferredFaceBoundary {
                cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
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
            },
        )];

        let assignments = materialize_boundary_domains(&domains, &[[0, 1], [0, 1]])
            .expect("service resource budget")
            .expect("closed deferred boundary");
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].len(), 1);
        assert_eq!(
            assignments[0][0].boundaries[0]
                .iter()
                .map(|use_| use_.edge)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn complete_endpoint_pairs_materialize_unordered_cycle() {
        catia_test_context!(ctx);
        let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];
        let assignments = materialize_boundary_domains(&domains, &[[0, 1], [1, 2], [2, 0]])
            .expect("service resource budget")
            .expect("closed unordered boundary");

        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].len(), 1);
        assert_eq!(assignments[0][0].boundaries[0].len(), 3);
        assert!(assignments[0][0].boundaries[0]
            .iter()
            .all(|use_| use_.reversed.is_some()));
    }
}

#[derive(Clone, Copy)]
struct MeshEndpointGeometry<'a> {
    edge_rows: &'a [EdgeRow],
    vertex_points: &'a [[f64; 3]],
}

fn resolve_fixed_mesh_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    geometry: MeshEndpointGeometry<'_>,
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[MeshFaceBoundaryAssignment],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<MeshEndpointResolve, CodecError> {
    let MeshEndpointGeometry {
        edge_rows,
        vertex_points,
    } = geometry;
    if selected.is_empty()
        || edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }
    let mut assignment_domains = Vec::new();

    for assignment in selected {
        let domain = vec![assignment.clone()];
        assignment_domains.push(domain);
    }
    if edge_candidates
        .iter()
        .any(|candidates| candidates.len() != 1)
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }
    if let Some(resolved) = resolve_singleton_mesh_endpoint_candidates(
        ctx,
        crate::solve::mesh_quotient::ResolveSingletonMeshEndpointCandidatesInputs {
            edge_rows,
            vertex_points,
            edge_candidates,
            assignments: &assignment_domains,
            port_identities,
            edge_direction_evidence: None,
            budget,
            candidate_gauge,
        },
    )? {
        if !matches!(
            &resolved,
            MeshSolve::Failed(MeshCandidateFailure::Rejected(()))
        ) {
            return Ok(resolved);
        }
    }
    if edge_candidates.iter().any(Vec::is_empty) {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }
    let mut edge_pairs = Vec::new();

    for candidates in edge_candidates {
        if let Some(&pair) = candidates.first() {
            edge_pairs.push(pair);
        }
    }
    let mut fixed_face_directions = Vec::new();

    let mut direction_overflow = false;
    for assignment in selected {
        let Some(configuration) = endpoint_configuration_for_assignment(assignment, &edge_pairs)
        else {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
        };
        let directions = match endpoint_configuration_directions(ctx, assignment, &configuration)? {
            Ok(directions) => directions,
            Err(MeshDirectionEnumerationError::Overflow) => {
                direction_overflow = true;
                break;
            }
            Err(MeshDirectionEnumerationError::Invalid) => {
                return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())))
            }
        };
        if directions.is_empty() {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
        }
        fixed_face_directions.push(Some(directions));
    }
    let use_fixed_direction_search = !direction_overflow;
    if direction_overflow {
        let directions = ctx.alloc_filled(
            assignment_domains.len(),
            None,
            "catia_general_mesh_fixed_face_directions",
        )?;
        fixed_face_directions = directions;
    }
    let mut edge_has_fixed_direction = ctx.alloc_filled(
        edge_candidates.len(),
        false,
        "catia_fixed_mesh_edge_directions",
    )?;
    for use_ in selected
        .iter()
        .flat_map(|assignment| &assignment.boundaries)
        .flatten()
    {
        if use_.reversed.is_some() {
            let Some(fixed) = edge_has_fixed_direction.get_mut(use_.edge) else {
                return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
            };
            *fixed = true;
        }
    }
    let Some(quotient) =
        initial_mesh_quotient(edge_candidates, vertex_points.len(), port_identities)?
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    };
    let mut direct_quotient = quotient.clone();
    let mut direct_orientations = edge_has_fixed_direction
        .iter()
        .map(|fixed| (!fixed).then_some(false))
        .collect::<Vec<_>>();
    let mut direct_directions = Vec::new();

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
        )?
        else {
            direct_possible = false;
            break;
        };
        direct_orientations = next_orientations;
        direct_directions.push(directions);
    }
    if direct_possible {
        let outcome =
            reconstruct_mesh_selection(edge_rows, vertex_points, selected, &direct_directions)?
                .map(|topology| {
                    resolve_mesh_selection_from_quotient(
                        ctx,
                        topology,
                        direct_quotient,
                        vertex_points,
                        edge_candidates,
                        port_identities,
                        budget,
                    )
                })
                .transpose()?
                .flatten();
        if let Some(outcome) = outcome {
            match outcome {
                MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
                resolved => return Ok(resolved),
            }
        }
        if budget.exhausted() {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(())));
        }
    }
    let face_work = ctx.alloc_filled(
        assignment_domains.len(),
        Some(1usize),
        "catia_fixed_face_work",
    )?;
    let fixed_edge_orientations = if use_fixed_direction_search {
        edge_has_fixed_direction
            .iter()
            .map(|fixed| (!fixed).then_some(false))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut search = MeshSelectionSearch {
        ctx,
        assignments: &assignment_domains,
        #[cfg(test)]
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work,
        edge_candidates,
        edge_rows,
        vertex_points,
        candidate_gauge,
        port_identities: Some(port_identities),
        fixed_face_directions,
        fixed_edge_orientations,
        edge_has_fixed_direction: if use_fixed_direction_search {
            edge_has_fixed_direction
        } else {
            Vec::new()
        },
        selected: ctx.alloc_filled(assignment_domains.len(), None, "catia_fixed_mesh_selection")?,
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    if use_fixed_direction_search {
        search.search_fixed_direction_with_budget(&quotient, budget)?;
    } else {
        search.search_with_budget(&quotient, budget, budget)?;
    }
    Ok(search.outcome.into())
}

/// Materialize boundary-assignment alternatives after the relation phase has
/// fixed every edge endpoint pair. Re-entering the general endpoint resolver
/// here rebuilds the same relation and recurses indefinitely when multiple
/// boundary assignments share one endpoint configuration. Enumerate only the
/// remaining assignment choices, then apply the exact fixed-pair materializer.
fn resolve_fixed_mesh_endpoint_assignment_domains(
    ctx: &DecodeContext<'_>,
    geometry: MeshEndpointGeometry<'_>,
    edge_candidates: &[Vec<[usize; 2]>],
    assignment_domains: &[Vec<MeshFaceBoundaryAssignment>],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<MeshEndpointResolve, CodecError> {
    // Keep recursive search state explicit so budget, solution, and ambiguity
    // ownership remain visible at every branch.
    struct FixedEndpointAssignmentSearch<
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
        face: usize,
        assignment_domains: &'input0 [Vec<MeshFaceBoundaryAssignment>],
        selected: &'input1 mut Vec<MeshFaceBoundaryAssignment>,
        edge_rows: &'input2 [EdgeRow],
        vertex_points: &'input3 [[f64; 3]],
        edge_candidates: &'input4 [Vec<[usize; 2]>],
        port_identities: &'input5 [[u32; 2]],
        budget: &'input7 WorkBudget<'input6>,
        candidate_gauge: Option<MeshCandidateGauge<'input8>>,
        outcome: &'input9 mut SearchOutcome<(StandardTopologyDraft, Vec<usize>)>,
    }
    fn visit(
        ctx: &DecodeContext<'_>,
        inputs: FixedEndpointAssignmentSearch<'_, '_, '_, '_, '_, '_, '_, '_, '_, '_>,
    ) -> Result<(), CodecError> {
        let FixedEndpointAssignmentSearch {
            face,
            assignment_domains,
            selected,
            edge_rows,
            vertex_points,
            edge_candidates,
            port_identities,
            budget,
            candidate_gauge,
            outcome,
        } = inputs;

        if outcome.is_closed() || budget.exhausted() {
            return Ok(());
        }
        if !budget.charge() {
            outcome.exhaust();
            return Ok(());
        }
        if face == assignment_domains.len() {
            let resolved = resolve_fixed_mesh_endpoint_pairs(
                ctx,
                MeshEndpointGeometry {
                    edge_rows,
                    vertex_points,
                },
                edge_candidates,
                selected,
                port_identities,
                budget,
                candidate_gauge,
            )?;
            match resolved {
                MeshSolve::Solved((topology, assignment)) => {
                    let candidate = (topology, assignment);
                    let equivalent = if let SearchOutcome::Solved(previous) = &*outcome {
                        mesh_candidates_equivalent_with_context(
                            ctx,
                            previous,
                            &candidate,
                            candidate_gauge,
                        )?
                    } else {
                        false
                    };
                    outcome.record_solved(candidate, |_, _| equivalent);
                }
                MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
                MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => outcome.mark_ambiguous(),
                MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => outcome.exhaust(),
            }
            return Ok(());
        }
        for assignment in &assignment_domains[face] {
            selected.push(assignment.clone());
            visit(
                ctx,
                FixedEndpointAssignmentSearch {
                    face: face + 1,
                    assignment_domains,
                    selected,
                    edge_rows,
                    vertex_points,
                    edge_candidates,
                    port_identities,
                    budget,
                    candidate_gauge,
                    outcome,
                },
            )?;
            selected.pop();
            if outcome.is_closed() {
                return Ok(());
            }
        }
        Ok(())
    }

    let MeshEndpointGeometry {
        edge_rows,
        vertex_points,
    } = geometry;
    if assignment_domains.is_empty()
        || assignment_domains.iter().any(Vec::is_empty)
        || edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
        || edge_candidates
            .iter()
            .any(|candidates| candidates.len() != 1)
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }

    let mut outcome = SearchOutcome::Open;
    visit(
        ctx,
        FixedEndpointAssignmentSearch {
            face: 0,
            assignment_domains,
            selected: &mut Vec::new(),
            edge_rows,
            vertex_points,
            edge_candidates,
            port_identities,
            budget,
            candidate_gauge,
            outcome: &mut outcome,
        },
    )?;
    if budget.exhausted() {
        outcome.exhaust();
    }
    Ok(outcome.into())
}

pub(super) fn prune_mesh_endpoint_pair_support(
    ctx: &DecodeContext<'_>,
    assignments: &mut [Vec<MeshFaceBoundaryAssignment>],
    edge_candidates: &mut [Vec<[usize; 2]>],
) -> Result<bool, CodecError> {
    prune_mesh_endpoint_pair_support_with_limit(
        ctx,
        assignments,
        edge_candidates,
        MAX_MESH_CONSTRAINT_OPERATIONS,
    )
}

pub(super) fn prune_mesh_endpoint_pair_support_with_limit(
    ctx: &DecodeContext<'_>,
    assignments: &mut [Vec<MeshFaceBoundaryAssignment>],
    edge_candidates: &mut [Vec<[usize; 2]>],
    limit: usize,
) -> Result<bool, CodecError> {
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
                return Ok(true);
            }
            if face.is_empty() {
                return Ok(false);
            }
            changed |= face.len() != before;
        }
        let mut incident_faces = ctx.alloc_filled(
            edge_candidates.len(),
            Vec::new(),
            "catia_prune_incident_rows",
        )?;
        for (face, choices) in assignments.iter().enumerate() {
            for use_ in choices
                .iter()
                .flat_map(|assignment| assignment.boundaries.iter().flatten())
            {
                let Some(faces) = incident_faces.get_mut(use_.edge) else {
                    continue;
                };
                if faces.last() != Some(&face) {
                    faces.push(face);
                }
            }
        }
        for edge in 0..edge_candidates.len() {
            if edge_candidates[edge].is_empty() {
                continue;
            }
            let incident_faces = &incident_faces[edge];
            let before = edge_candidates[edge].len();
            let mut decisions = ctx.alloc_filled(before, false, "catia_prune_pair_decisions")?;

            // Read the unchanged candidate table, then apply this row's decisions.
            // A complete table copy per row repeats all unrelated storage.
            for (pair, decision) in edge_candidates[edge].iter().zip(&mut decisions) {
                *decision = incident_faces.iter().all(|face| {
                    assignments[*face].iter().any(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .any(|use_| use_.edge == edge)
                            && mesh_assignment_endpoint_cycles_viable_with(
                                assignment,
                                edge_candidates,
                                Some((edge, *pair)),
                                Some(&budget),
                            )
                            .unwrap_or(true)
                    })
                });
            }
            let mut position = 0;
            edge_candidates[edge].retain(|_| {
                let supported = decisions[position];
                position += 1;
                supported
            });
            if budget.exhausted() {
                // Do not turn incomplete propagation into a contradiction.
                return Ok(true);
            }
            if edge_candidates[edge].is_empty() {
                return Ok(false);
            }
            if edge_candidates[edge].len() != before {
                continue 'fixpoint;
            }
        }
        if !changed {
            return Ok(true);
        }
    }
}

// The arguments are independent serialized evidence, solver state, and budget
// inputs; grouping them would hide their ownership without reducing coupling.
#[derive(Clone, Copy)]
struct ResolveSingletonMeshEndpointCandidatesInputs<
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
    'input6,
    'input7,
    'input8,
> {
    edge_rows: &'input0 [EdgeRow],
    vertex_points: &'input1 [[f64; 3]],
    edge_candidates: &'input2 [Vec<[usize; 2]>],
    assignments: &'input3 [Vec<MeshFaceBoundaryAssignment>],
    port_identities: &'input4 [[u32; 2]],
    edge_direction_evidence: Option<&'input5 [bool]>,
    budget: &'input7 WorkBudget<'input6>,
    candidate_gauge: Option<MeshCandidateGauge<'input8>>,
}

fn resolve_singleton_mesh_endpoint_candidates(
    ctx: &DecodeContext<'_>,
    inputs: ResolveSingletonMeshEndpointCandidatesInputs<'_, '_, '_, '_, '_, '_, '_, '_, '_>,
) -> Result<Option<MeshEndpointResolve>, CodecError> {
    let ResolveSingletonMeshEndpointCandidatesInputs {
        edge_rows,
        vertex_points,
        edge_candidates,
        assignments,
        port_identities,
        edge_direction_evidence,
        budget,
        candidate_gauge,
    } = inputs;

    if edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
        || edge_direction_evidence.is_some_and(|evidence| evidence.len() != edge_rows.len())
        || edge_candidates
            .iter()
            .any(|candidates| candidates.len() != 1)
    {
        return Ok(None);
    }

    let mut selected = Vec::new();
    let mut endpoint_labelled_directions = Vec::new();

    for face in assignments {
        let mut seen = HashSet::new();
        let mut first = None;
        for assignment in face {
            let mut directions = Vec::new();

            let mut valid = true;
            for boundary in &assignment.boundaries {
                let Some(row) = singleton_mesh_boundary_directions(
                    boundary,
                    edge_candidates,
                    edge_direction_evidence,
                ) else {
                    valid = false;
                    break;
                };
                directions.push(row);
            }
            if !valid {
                continue;
            }
            let Some(signature) =
                canonical_singleton_coordinate_cycles(assignment, &directions, edge_candidates)
            else {
                continue;
            };
            if seen.contains(&signature) {
                continue;
            }

            seen.insert(signature);
            if first.is_some() {
                return Ok(None);
            }
            first = Some((assignment.clone(), directions));
        }
        let Some((assignment, directions)) = first else {
            return Ok(None);
        };
        selected.push(assignment);
        endpoint_labelled_directions.push(directions);
    }
    if let Some(topology) = reconstruct_singleton_coordinate_topology(
        ctx,
        edge_rows,
        vertex_points,
        edge_candidates,
        &selected,
        &endpoint_labelled_directions,
    )? {
        let mut point_assignment = Vec::new();

        point_assignment.extend(0..vertex_points.len());
        return Ok(Some(MeshSolve::Solved((topology, point_assignment))));
    }
    // An unresolved coedge direction does not select a point endpoint. It is
    // a row-orientation gauge. Let the exact coordinate binding prove the
    // resulting cycle, then try the endpoint-labelled gauge only when the
    // fixed false direction cannot bind.
    let mut fixed_directions = Vec::new();

    for assignment in &selected {
        let mut face_directions = Vec::new();

        for boundary in &assignment.boundaries {
            if boundary.is_empty() {
                return Ok(None);
            }
            let mut directions = Vec::new();

            directions.extend(boundary.iter().map(|use_| use_.reversed.unwrap_or(false)));
            face_directions.push(directions);
        }
        fixed_directions.push(face_directions);
    }
    if let Some(resolved) = resolve_singleton_mesh_selection(
        ctx,
        crate::solve::mesh_quotient::selection_search::ResolveSingletonMeshSelectionInputs {
            edge_rows,
            vertex_points,
            edge_candidates,
            selected: &selected,
            directions: &fixed_directions,
            port_identities,
            budget,
            candidate_gauge,
        },
    )? {
        match resolved {
            MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
            resolved => return Ok(Some(resolved)),
        }
    }

    resolve_singleton_mesh_selection(
        ctx,
        crate::solve::mesh_quotient::selection_search::ResolveSingletonMeshSelectionInputs {
            edge_rows,
            vertex_points,
            edge_candidates,
            selected: &selected,
            directions: &endpoint_labelled_directions,
            port_identities,
            budget,
            candidate_gauge,
        },
    )
}

// Endpoint materialization receives independent evidence, budgets, predicates,
// and gauge state so each fallback remains separately bounded and auditable.
struct ResolveStandardMeshEndpointCandidatesInputs<
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
> {
    edge_rows: &'input0 [EdgeRow],
    vertex_points: &'input1 [[f64; 3]],
    edge_candidates: &'input2 [Vec<[usize; 2]>],
    assignments: Vec<Vec<MeshFaceBoundaryAssignment>>,
    port_identities: &'input3 [[u32; 2]],
    prepared_quotient: Option<&'input4 MeshQuotient>,
    edge_direction_evidence: Option<&'input5 [bool]>,
    budget: &'input7 WorkBudget<'input6>,
    partial_solution_valid: Option<&'input9 MeshEndpointSolutionPredicate<'input8>>,
    complete_solution_valid: Option<&'input11 MeshEndpointSolutionPredicate<'input10>>,
    candidate_gauge: Option<MeshCandidateGauge<'input12>>,
    priority_edges: Option<&'input13 [bool]>,
}

fn resolve_standard_mesh_endpoint_candidates(
    ctx: &DecodeContext<'_>,
    inputs: ResolveStandardMeshEndpointCandidatesInputs<
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
) -> Result<MeshEndpointResolve, CodecError> {
    const MAX_SELECTION_WORK: usize = 100_000;

    let ResolveStandardMeshEndpointCandidatesInputs {
        edge_rows,
        vertex_points,
        edge_candidates,
        mut assignments,
        port_identities,
        prepared_quotient,
        edge_direction_evidence,
        budget,
        partial_solution_valid,
        complete_solution_valid,
        candidate_gauge,
        priority_edges,
    } = inputs;

    let face_count = assignments.len();
    let mut edge_candidates = edge_candidates.to_vec();
    if !prune_mesh_endpoint_pair_support(ctx, &mut assignments, &mut edge_candidates)? {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }
    let quotient = if let Some(prepared) = prepared_quotient {
        Some(prepared.clone())
    } else {
        initial_mesh_quotient(&edge_candidates, vertex_points.len(), port_identities)?
    };
    let Some(quotient) = quotient else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    };
    for face in &mut assignments {
        let mut retained = Vec::new();
        for assignment in std::mem::take(face) {
            if quotient.assignment_has_option(&assignment, &edge_candidates, Some(budget))? {
                retained.push(assignment);
            }
        }
        *face = retained;
        if budget.exhausted() {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(())));
        }
        if face.is_empty() {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
        }
    }
    if let Some(resolved) = resolve_singleton_mesh_endpoint_candidates(
        ctx,
        crate::solve::mesh_quotient::ResolveSingletonMeshEndpointCandidatesInputs {
            edge_rows,
            vertex_points,
            edge_candidates: &edge_candidates,
            assignments: &assignments,
            port_identities,
            edge_direction_evidence,
            budget,
            candidate_gauge,
        },
    )? {
        return Ok(resolved);
    }
    let coordinate_domains = if let Some(preparation_limit) = quotient
        .clone()
        .coordinate_domain_preparation_limit(vertex_points.len(), &edge_candidates)?
    {
        let preparation_budget = budget.child_slice(preparation_limit);
        let mut coordinate_quotient = quotient.clone();
        coordinate_quotient.prepare_coordinate_root_domains(
            ctx,
            vertex_points.len(),
            &edge_candidates,
            Some(&preparation_budget),
        )?
    } else {
        None
    };
    let mut face_work = Vec::new();

    let mut total_work = 0usize;
    for face in &assignments {
        let Some(next) = total_work.checked_add(face.len()) else {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
        };
        total_work = next;
        face_work.push(Some(face.len()));
    }
    if total_work > MAX_SELECTION_WORK {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(())));
    }
    let face_equations = possible_face_equations(&assignments);
    let mut face_choices = Vec::new();
    if !possible_face_choices_with_limit(
        &assignments,
        &face_equations,
        MAX_MESH_CONSTRAINT_OPERATIONS,
        &mut face_choices,
    ) {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(())));
    }
    let unselected = ctx.alloc_filled(
        edge_candidates.len(),
        None,
        "catia_endpoint_unselected_edges",
    )?;
    let configuration_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut endpoint_configurations = Vec::new();
    for face in &assignments {
        let mut face_configurations = Vec::new();
        for assignment in face {
            let configurations = if configuration_budget.exhausted() {
                None
            } else {
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
            };
            face_configurations.push(configurations);
        }
        endpoint_configurations.push(face_configurations);
    }
    let relation = resolve_endpoint_configuration_relation_streaming(
        ctx,
        crate::solve::mesh_quotient::ResolveEndpointConfigurationRelationStreamingInputs {
            assignments: &assignments,
            endpoint_configurations: &endpoint_configurations,
            edge_candidates: &edge_candidates,
            edge_rows,
            vertex_points,
            port_identities,
            budget,
            partial_solution_valid,
            complete_solution_valid,
            candidate_gauge,
            priority_edges,
            coordinate_domains: coordinate_domains.as_ref(),
        },
    )?;
    if let Some(resolved) = relation {
        return Ok(resolved);
    }
    let mut search = MeshSelectionSearch {
        ctx,
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
        fixed_face_directions: ctx.alloc_filled(
            face_count,
            None,
            "catia_mesh_fixed_face_directions",
        )?,
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: ctx.alloc_filled(face_count, None, "catia_mesh_selected_faces")?,
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    search.search_with_budget(&quotient, budget, budget)?;
    Ok(search.outcome.into())
}

/// Resolve geometric endpoint alternatives through face incidence before
/// applying the exact trim-mesh endpoint quotient. Endpoint graphs must close every
/// face and produce one topology modulo vertex labels, edge direction, and cycle start.
/// `partial_solution_valid` receives partial assignments during search. It must
/// be monotone: once it rejects a selected subset, assigning more edges cannot
/// make that subset valid. `partial_constraint_edges` identifies every edge
/// whose assignment can affect that predicate and must be kept in the same
/// incidence component. `preferred_assignment_edges` identifies additional
/// variables that should be selected before unrelated incidence variables;
/// those variables do not require a single incidence component.
/// `complete_solution_valid` is evaluated only after every endpoint pair has
/// been assigned. Use it for global preferences whose result cannot be known
/// from a partial assignment.
pub(crate) struct ParseStandardMeshCandidateOutcomeInputs<
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
    FP,
    FC,
> where
    FP: Fn(&[Option<[usize; 2]>]) -> bool,
    FC: Fn(&[Option<[usize; 2]>]) -> bool,
{
    pub(crate) bytes: &'input0 [u8],
    pub(crate) edge_faces: &'input1 [[usize; 2]],
    pub(crate) edge_candidates: &'input2 [Vec<[usize; 2]>],
    pub(crate) edge_classes: &'input3 [usize],
    pub(crate) edge_geometry: &'input4 [MeshEdgeGeometry],
    pub(crate) edge_identity_evidence: &'input5 [bool],
    pub(crate) edge_direction_evidence: &'input6 [bool],
    pub(crate) global_handle_ports: bool,
    pub(crate) partial_constraint_edges: &'input7 [bool],
    pub(crate) preferred_assignment_edges: &'input8 [bool],
    pub(crate) priority_edges: Option<&'input9 [bool]>,
    pub(crate) assignment_dependencies: Option<&'input10 [Vec<usize>]>,
    pub(crate) budget: &'input12 WorkBudget<'input11>,
    pub(crate) partial_solution_valid: FP,
    pub(crate) complete_solution_valid: FC,
}

pub(crate) fn parse_standard_mesh_candidate_outcome<FP, FC>(
    ctx: &DecodeContext<'_>,
    inputs: ParseStandardMeshCandidateOutcomeInputs<
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
        FP,
        FC,
    >,
) -> Result<MeshCandidateSolve, CodecError>
where
    FP: Fn(&[Option<[usize; 2]>]) -> bool,
    FC: Fn(&[Option<[usize; 2]>]) -> bool,
{
    let ParseStandardMeshCandidateOutcomeInputs {
        bytes,
        edge_faces,
        edge_candidates,
        edge_classes,
        edge_geometry,
        edge_identity_evidence,
        edge_direction_evidence,
        global_handle_ports,
        partial_constraint_edges,
        preferred_assignment_edges,
        priority_edges,
        assignment_dependencies,
        budget,
        partial_solution_valid,
        complete_solution_valid,
    } = inputs;

    let endpoint_budget = budget.child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
    let Some((face_count, edge_rows, vertex_points, mut mesh_domains, port_identities)) =
        (|| -> Result<Option<_>, CodecError> {
            let Some(face_run) = largest_fbb_run(bytes) else {
                return Ok(None);
            };
            let face_count = face_run.face_count();
            let after_faces = face_run.after_faces();
            let Some((edge_rows, vertex_header)) = parse_edge_tables(bytes, after_faces)? else {
                return Ok(None);
            };
            let Some(vertex_points) = parse_vertex_table(bytes, vertex_header) else {
                return Ok(None);
            };
            let boundary_context = StandardMeshBoundaryContext::parse_ports(
                ctx,
                bytes,
                edge_faces,
                global_handle_ports,
            )?;
            let Some(boundary_context) = boundary_context else {
                return Ok(None);
            };
            let mesh_domains = standard_mesh_boundary_domains_from_context(
                ctx,
                &boundary_context,
                Some(edge_candidates),
                true,
            )?;
            let Some(mesh_domains) = mesh_domains else {
                return Ok(None);
            };
            // Standard-row endpoints are oriented by the complete face quotient.
            let Some(port_identities) =
                crate::solve::missing_edge::solver_ports(ctx, bytes, global_handle_ports)?
            else {
                return Ok(None);
            };
            Ok(Some((
                face_count,
                edge_rows,
                vertex_points,
                mesh_domains,
                port_identities,
            )))
        })()?
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::InputStructure,
        )));
    };
    let coordinate_gauge = build_mesh_coordinate_gauge(
        ctx,
        vertex_points.len(),
        &edge_rows,
        edge_faces,
        edge_geometry,
        edge_candidates,
        edge_identity_evidence,
    )?;
    let candidate_gauge = Some(MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces,
        edge_geometry,
        edge_candidates,
        edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    });
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_candidates.len()
        || edge_rows.len() != edge_classes.len()
        || edge_rows.len() != edge_geometry.len()
        || edge_rows.len() != edge_direction_evidence.len()
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
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::InputCardinality,
        )));
    }
    if mesh_domains.len() != face_count {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::FaceBoundaryCardinality,
        )));
    }
    for domain in &mut mesh_domains {
        if let MeshFaceBoundaryDomain::Ordered(assignments) = domain {
            deduplicate_mesh_quotient_assignments(std::slice::from_mut(assignments));
        }
    }
    if !mesh_domains_have_incident_edge_support(edge_faces, &mesh_domains) {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::QuotientPreparation,
        )));
    }
    if port_identities.len() != edge_rows.len() {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::PortCardinality,
        )));
    }
    let Some(mut mesh_quotient) =
        initial_mesh_quotient(edge_candidates, vertex_points.len(), &port_identities)?
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::QuotientPreparation,
        )));
    };
    let mut propagated_quotient = mesh_quotient.clone();
    match propagate_common_ordered_face_quotients(
        ctx,
        &mesh_domains,
        edge_candidates,
        &mut propagated_quotient,
        budget,
    )? {
        Some(()) => mesh_quotient = propagated_quotient,
        None if budget.exhausted() => {}
        None => {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
                MeshCandidateRejection::QuotientPreparation,
            )))
        }
    }
    if edge_candidates.iter().any(Vec::is_empty)
        && propagate_common_boundary_components(
            ctx,
            &mesh_domains,
            edge_candidates,
            &mut mesh_quotient,
        )?
        .is_none()
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::QuotientPreparation,
        )));
    }
    let completed_edge_candidates = edge_candidates.to_vec();
    if !mesh_quotient.edge_domains_viable(&completed_edge_candidates)? {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::QuotientPreparation,
        )));
    }
    let Some(class_constraint) =
        edge_class_search_constraint(ctx, edge_classes, &completed_edge_candidates)?
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::EdgeClassConstraint,
        )));
    };
    let mut constraint_edges = Vec::new();

    constraint_edges.extend(
        partial_constraint_edges
            .iter()
            .zip(preferred_assignment_edges)
            .zip(&class_constraint.active)
            .map(|((partial, preferred), class)| *partial || *preferred || *class),
    );
    let mut assignment_predecessors = ctx.alloc_filled(
        completed_edge_candidates.len(),
        None,
        "catia_mesh_assignment_predecessors",
    )?;
    for &(left, right) in &class_constraint.ordered {
        assignment_predecessors[right] = Some(
            assignment_predecessors[right].map_or(left, |predecessor: usize| predecessor.max(left)),
        );
    }
    let constrained_partial_solution_valid = |pairs: &[Option<[usize; 2]>]| {
        endpoint_pairs_respect_candidate_domains(pairs, &completed_edge_candidates)
            && partial_solution_valid(pairs)
    };
    let complete_preference_rejected = Cell::new(false);
    let constrained_complete_solution_valid = |pairs: &[Option<[usize; 2]>]| {
        let valid = endpoint_pairs_respect_candidate_domains(pairs, &completed_edge_candidates)
            && complete_solution_valid(pairs);
        if !valid {
            complete_preference_rejected.set(true);
        }
        valid
    };
    let mut incidence_solution = None;
    let mut incidence_ambiguity = None;
    let mut incidence_exhausted = false;
    let mut endpoint_resolution_memo = HashMap::<Vec<[usize; 2]>, MeshEndpointResolve>::new();
    let pair_solutions = visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy(ctx, crate::solve::incidence::VisitIncidenceEndpointPairSolutionsWithCoordinateRootPolicyInputs { edge_rows: &edge_rows, vertex_points: &vertex_points, edge_faces, edge_candidates: &completed_edge_candidates, face_count, mesh_assignments: Some(&mesh_domains), mesh_quotient: Some(&mesh_quotient), coordinate_root_policy: CoordinateRootPolicy::DeferToVisitor, partial_solution_valid: Some(MeshPartialEndpointConstraint {
        active_edges: &constraint_edges,
        coupled_edges: partial_constraint_edges,
        assignment_order: AssignmentOrder::new(
            Some(&assignment_predecessors),
            assignment_dependencies,
        ),
        valid: &constrained_partial_solution_valid,
    }), complete_solution_budget: Some(&endpoint_budget), solution_valid: &|pairs| {
        let mut completed = Vec::new();

        completed.extend(pairs.iter().copied().map(Some));
        Ok(constrained_complete_solution_valid(&completed))
    }, visitor: &mut |pairs| -> Result<ControlFlow<()>, CodecError> {
        let endpoint_resolution = if let Some(cached) = endpoint_resolution_memo.get(pairs) {
            cached.clone()
        } else {
            let Some(oriented_pairs) = restore_unique_endpoint_pair_orientations(pairs, &completed_edge_candidates)
            else {
                return Ok(ControlFlow::Continue(()));
            };
            let mut singleton = Vec::new();

            for &pair in &oriented_pairs {
                singleton.push(vec![pair]);
            }
            let Some(mut mesh_assignments) =
                materialize_boundary_domains(&mesh_domains, &oriented_pairs)?
            else {
                return Ok(ControlFlow::Continue(()));
            };
            deduplicate_mesh_quotient_assignments(&mut mesh_assignments);
            // This child owns the incidence-to-endpoint relation phase. The
            // complete materialization invoked by that relation takes its
            // own MAX_MESH_CONSTRAINT_OPERATIONS child slice.
            let endpoint_resolution_budget =
                endpoint_budget.child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
            let resolution = resolve_standard_mesh_endpoint_candidates(ctx, crate::solve::mesh_quotient::ResolveStandardMeshEndpointCandidatesInputs { edge_rows: &edge_rows, vertex_points: &vertex_points, edge_candidates: &singleton, assignments: mesh_assignments, port_identities: &port_identities, prepared_quotient: None, edge_direction_evidence: Some(edge_direction_evidence), budget: &endpoint_resolution_budget, partial_solution_valid: Some(&constrained_partial_solution_valid), complete_solution_valid: Some(&constrained_complete_solution_valid), candidate_gauge, priority_edges })?;
            // Exhaustion is deterministic for this input and child budget.
            // The parent budget only decreases, so retrying the same key
            // cannot turn an exhausted materialization into a solution.
            if endpoint_resolution_memo.len() < MAX_ENDPOINT_RESOLUTION_MEMO_ENTRIES {
                let endpoint_key = pairs.to_vec();
                let retained = resolution.clone();
                endpoint_resolution_memo.insert(endpoint_key, retained);
            }
            resolution
        };
        let candidate = match endpoint_resolution {
            MeshSolve::Solved((topology, assignment)) => (topology, assignment),
            MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {
                return Ok(ControlFlow::Continue(()))
            }
            MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => {
                incidence_ambiguity = Some(MeshCandidateAmbiguity::EndpointResolution);
                return Ok(ControlFlow::Break(()));
            }
            MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => {
                incidence_exhausted = true;
                return Ok(ControlFlow::Break(()));
            }
        };
        match &incidence_solution {
            Some(stored) => {
                if !mesh_candidates_equivalent_with_context(
                    ctx,
                    stored,
                    &candidate,
                    candidate_gauge,
                )? {
                    incidence_ambiguity =
                        Some(MeshCandidateAmbiguity::DistinctTopologySolutions);
                    return Ok(ControlFlow::Break(()));
                }
                Ok(ControlFlow::Continue(()))
            }
            None => {
                incidence_solution = Some(candidate);
                Ok(ControlFlow::Continue(()))
            }
        }
    } })?;
    if let Some(ambiguity) = incidence_ambiguity {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(
            ambiguity,
        )));
    }
    let exhaustion = |from_endpoint_resolution: bool| {
        if complete_preference_rejected.get() {
            MeshCandidateExhaustion::PreferredSolutionSearch
        } else if from_endpoint_resolution {
            MeshCandidateExhaustion::EndpointResolution
        } else {
            MeshCandidateExhaustion::IncidenceEnumeration
        }
    };
    let incidence_rejection = match pair_solutions {
        IncidenceSolve::Ambiguous => {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(
                MeshCandidateAmbiguity::CoordinateRootClosure,
            )));
        }
        IncidenceSolve::Exhausted => {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(
                exhaustion(incidence_exhausted),
            )));
        }
        _ if incidence_exhausted => {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(
                exhaustion(true),
            )));
        }
        IncidenceSolve::Rejected(rejection) => {
            MeshEndpointIncidenceRejection::NoAssignment(rejection)
        }
        IncidenceSolve::Solved(_) => MeshEndpointIncidenceRejection::BoundaryReconstruction,
    };
    if let Some((topology, assignment)) = incidence_solution {
        // Canonicalization is a representation step; retain the validated raw candidate if unavailable.
        let (topology, assignment) =
            canonicalize_mesh_candidate_for_output(ctx, &topology, &assignment, candidate_gauge)?
                .unwrap_or((topology, assignment));
        return Ok(MeshSolve::Solved((topology, assignment)));
    }
    let fallback = (|| -> Result<Option<MeshEndpointResolve>, CodecError> {
        let mut assignments = Vec::new();

        for domain in mesh_domains {
            match domain {
                MeshFaceBoundaryDomain::Ordered(face) => assignments.push(face),
                MeshFaceBoundaryDomain::UnorderedFullCycle(_)
                | MeshFaceBoundaryDomain::DeferredValidation(_) => return Ok(None),
            }
        }
        let resolution = resolve_standard_mesh_endpoint_candidates(
            ctx,
            crate::solve::mesh_quotient::ResolveStandardMeshEndpointCandidatesInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates,
                assignments,
                port_identities: &port_identities,
                prepared_quotient: Some(&mesh_quotient),
                edge_direction_evidence: Some(edge_direction_evidence),
                budget: &endpoint_budget,
                partial_solution_valid: Some(&constrained_partial_solution_valid),
                complete_solution_valid: Some(&constrained_complete_solution_valid),
                candidate_gauge,
                priority_edges,
            },
        )?;
        Ok(Some(resolution))
    })()?;
    Ok(match fallback {
        Some(MeshSolve::Solved((topology, assignment))) => {
            // Canonicalization is a representation step; retain the validated raw candidate if unavailable.
            let (topology, assignment) = canonicalize_mesh_candidate_for_output(
                ctx,
                &topology,
                &assignment,
                candidate_gauge,
            )?
            .unwrap_or((topology, assignment));
            MeshSolve::Solved((topology, assignment))
        }
        Some(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(()))) => MeshSolve::Failed(
            MeshCandidateFailure::Ambiguous(MeshCandidateAmbiguity::EndpointResolution),
        ),
        Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))) => MeshSolve::Failed(
            MeshCandidateFailure::Exhausted(if complete_preference_rejected.get() {
                MeshCandidateExhaustion::PreferredSolutionSearch
            } else {
                MeshCandidateExhaustion::EndpointResolution
            }),
        ),
        Some(MeshSolve::Failed(MeshCandidateFailure::Rejected(()))) | None => {
            MeshSolve::Failed(MeshCandidateFailure::Rejected(
                MeshCandidateRejection::EndpointIncidence(incidence_rejection),
            ))
        }
    })
}

/// Solve a standard mesh whose repeated edge-face rows still carry alternate
/// second-face domains. Each concrete face assignment is handed to the
/// existing solver, so optional incidences never become required trim uses.
/// A second solved assignment is semantic ambiguity: no topology gauge may
/// erase a different edge-to-face incidence graph.
pub(crate) fn parse_standard_mesh_candidate_outcome_with_face_assignments<F>(
    ctx: &DecodeContext<'_>,
    candidates: MeshFaceAssignmentCandidates<'_>,
    budget: &WorkBudget<'_>,
    mut solve: F,
) -> Result<MeshFaceDomainCandidateSolve, CodecError>
where
    F: FnMut(&[[usize; 2]], &WorkBudget<'_>) -> Result<MeshCandidateSolve, CodecError>,
{
    let mut solution: Option<(Vec<[usize; 2]>, StandardTopologyDraft, Vec<usize>)> = None;
    let mut rejection = None;
    let mut ambiguity = None;
    let mut exhaustion = None;
    let mut evaluate = |assignment: &[[usize; 2]]| -> Result<bool, CodecError> {
        if !budget.charge() {
            exhaustion = Some(MeshCandidateExhaustion::FaceDomainEnumeration);
            return Ok(false);
        }
        let branch_budget = budget.child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
        let outcome = solve(assignment, &branch_budget)?;
        if !budget.charge_by(branch_budget.consumed()) {
            exhaustion = Some(MeshCandidateExhaustion::FaceDomainEnumeration);
            return Ok(false);
        }
        match outcome {
            MeshSolve::Solved((topology, point_assignment)) => {
                if let Some((_, stored_topology, stored_assignment)) = &solution {
                    if stored_topology != &topology || stored_assignment != &point_assignment {
                        ambiguity = Some(MeshCandidateAmbiguity::DistinctTopologySolutions);
                        return Ok(false);
                    }
                    return Ok(true);
                }
                solution = Some((assignment.to_vec(), topology, point_assignment));
                Ok(true)
            }
            MeshSolve::Failed(MeshCandidateFailure::Rejected(reason)) => {
                rejection.get_or_insert(reason);
                Ok(true)
            }
            MeshSolve::Failed(MeshCandidateFailure::Ambiguous(reason)) => {
                ambiguity = Some(reason);
                Ok(false)
            }
            MeshSolve::Failed(MeshCandidateFailure::Exhausted(reason)) => {
                exhaustion = Some(reason);
                Ok(false)
            }
        }
    };
    let visit = match candidates {
        MeshFaceAssignmentCandidates::Domains {
            edge_faces,
            allowed_faces,
            face_count,
        } => visit_duplicate_face_assignments(
            ctx,
            edge_faces,
            allowed_faces,
            face_count,
            MAX_FACE_DOMAIN_ASSIGNMENTS,
            &mut evaluate,
        )?,
        MeshFaceAssignmentCandidates::Concrete {
            assignments,
            face_count,
        } => {
            let edge_count = assignments.first().map_or(0, Vec::len);
            if assignments.len() > MAX_FACE_DOMAIN_ASSIGNMENTS
                || assignments.iter().any(|assignment| {
                    assignment.len() != edge_count
                        || assignment.iter().flatten().any(|face| *face >= face_count)
                })
            {
                None
            } else {
                let mut outcome = DuplicateFaceAssignmentVisit::Complete;
                for assignment in assignments {
                    if !evaluate(assignment)? {
                        outcome = DuplicateFaceAssignmentVisit::Stopped;
                        break;
                    }
                }
                Some(outcome)
            }
        }
    };
    if visit.is_none() {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::InputStructure,
        )));
    }
    if let Some(ambiguity) = ambiguity {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(
            ambiguity,
        )));
    }
    if let Some(exhaustion) = exhaustion {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(
            exhaustion,
        )));
    }
    if matches!(visit, Some(DuplicateFaceAssignmentVisit::Exhausted)) {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(
            MeshCandidateExhaustion::FaceDomainEnumeration,
        )));
    }
    Ok(
        if let Some((faces, topology, point_assignment)) = solution {
            MeshSolve::Solved((faces, topology, point_assignment))
        } else {
            MeshSolve::Failed(MeshCandidateFailure::Rejected(
                rejection.unwrap_or(MeshCandidateRejection::InputStructure),
            ))
        },
    )
}

#[test]
fn relation_coordinate_candidates_keep_only_surviving_pair_values() {
    catia_test_context!(ctx);
    let base_candidates = vec![vec![[0, 1], [0, 2]], vec![[1, 2]]];
    let domains = vec![
        vec![MeshEndpointRelationChoice {
            id: 0,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0],
                edge_pairs: vec![(0, [0, 1]), (1, [1, 2])],
            },
        }],
        vec![MeshEndpointRelationChoice {
            id: 1,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0],
                edge_pairs: vec![(0, [0, 1])],
            },
        }],
    ];
    let assigned = vec![None, None];
    assert_eq!(
        relation_coordinate_candidate_domains(&domains, &assigned, &base_candidates),
        Some(vec![vec![[0, 1]], vec![[1, 2]]]),
    );

    let unknown_domains = vec![vec![MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Deferred,
    }]];
    assert_eq!(
        relation_coordinate_candidate_domains(&unknown_domains, &assigned, &base_candidates),
        Some(base_candidates.clone()),
    );
    assert!(relation_coordinate_candidate_domains(
        &domains,
        &[Some([2, 3]), None],
        &base_candidates
    )
    .is_none());
}

#[test]
fn mesh_candidate_rejection_retains_the_failed_solver_stage() {
    catia_test_context!(ctx);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(matches!(
        parse_standard_mesh_candidate_outcome(
            &ctx,
            ParseStandardMeshCandidateOutcomeInputs {
                bytes: &[],
                edge_faces: &[],
                edge_candidates: &[],
                edge_classes: &[],
                edge_geometry: &[],
                edge_identity_evidence: &[],
                edge_direction_evidence: &[],
                global_handle_ports: false,
                partial_constraint_edges: &[],
                preferred_assignment_edges: &[],
                priority_edges: None,
                assignment_dependencies: None,
                budget: &budget,
                partial_solution_valid: |_| true,
                complete_solution_valid: |_| true,
            }
        )
        .expect("service resource budget"),
        MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::InputStructure
        ))
    ));
}

#[test]
fn face_domain_solver_returns_the_unique_concrete_assignment() {
    catia_test_context!(ctx);
    let edge_faces = [[0, 0]];
    let allowed_faces = [vec![2, 1]];
    let budget = WorkBudget::new(MAX_MESH_TOPOLOGY_OPERATIONS);
    let mut visited = Vec::new();
    let result = parse_standard_mesh_candidate_outcome_with_face_assignments(
        &ctx,
        MeshFaceAssignmentCandidates::Domains {
            edge_faces: &edge_faces,
            allowed_faces: &allowed_faces,
            face_count: 3,
        },
        &budget,
        |faces, _| {
            visited.push(faces.to_vec());
            Ok(if faces[0][1] == 2 {
                MeshSolve::Solved((
                    StandardTopologyDraft {
                        faces: Vec::new(),
                        edge_rows: Vec::new(),
                        vertex_points: Vec::new(),
                        logical_vertex_count: 0,
                    },
                    Vec::new(),
                ))
            } else {
                MeshSolve::Failed(MeshCandidateFailure::Rejected(
                    MeshCandidateRejection::InputStructure,
                ))
            })
        },
    )
    .expect("service resource budget");

    let MeshSolve::Solved((faces, _, _)) = result else {
        panic!("face-domain solver did not retain the unique branch");
    };
    assert_eq!(visited, vec![vec![[0, 0]], vec![[0, 1]], vec![[0, 2]]]);
    assert_eq!(faces, vec![[0, 2]]);
}

#[test]
fn face_domain_solver_evaluates_only_endpoint_closed_assignments() {
    catia_test_context!(ctx);
    let assignments = [vec![[0, 0]], vec![[0, 2]]];
    let budget = WorkBudget::new(MAX_MESH_TOPOLOGY_OPERATIONS);
    let mut visited = Vec::new();
    let result = parse_standard_mesh_candidate_outcome_with_face_assignments(
        &ctx,
        MeshFaceAssignmentCandidates::Concrete {
            assignments: &assignments,
            face_count: 3,
        },
        &budget,
        |faces, _| {
            visited.push(faces.to_vec());
            Ok(if faces[0][1] == 2 {
                MeshSolve::Solved((
                    StandardTopologyDraft {
                        faces: Vec::new(),
                        edge_rows: Vec::new(),
                        vertex_points: Vec::new(),
                        logical_vertex_count: 0,
                    },
                    Vec::new(),
                ))
            } else {
                MeshSolve::Failed(MeshCandidateFailure::Rejected(
                    MeshCandidateRejection::InputStructure,
                ))
            })
        },
    )
    .expect("service resource budget");

    let MeshSolve::Solved((faces, _, _)) = result else {
        panic!("face-domain solver did not retain the closed assignment");
    };
    assert_eq!(visited, assignments);
    assert_eq!(faces, vec![[0, 2]]);
}

#[test]
fn face_domain_solver_reports_distinct_assignments_as_ambiguity() {
    catia_test_context!(ctx);
    let edge_faces = [[0, 0]];
    let allowed_faces = [vec![1, 2]];
    let budget = WorkBudget::new(MAX_MESH_TOPOLOGY_OPERATIONS);
    let result = parse_standard_mesh_candidate_outcome_with_face_assignments(
        &ctx,
        MeshFaceAssignmentCandidates::Domains {
            edge_faces: &edge_faces,
            allowed_faces: &allowed_faces,
            face_count: 3,
        },
        &budget,
        |assignment, _| {
            Ok(MeshSolve::Solved((
                StandardTopologyDraft {
                    faces: Vec::new(),
                    edge_rows: Vec::new(),
                    vertex_points: Vec::new(),
                    logical_vertex_count: 0,
                },
                vec![assignment[0][1]],
            )))
        },
    )
    .expect("service resource budget");

    assert!(matches!(
        result,
        MeshSolve::Failed(MeshCandidateFailure::Ambiguous(
            MeshCandidateAmbiguity::DistinctTopologySolutions
        ))
    ));
}

#[test]
fn endpoint_configuration_relation_solves_cycle_orientation_globally() {
    catia_test_context!(ctx);
    let edge_rows = (0..3)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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

    let Some(MeshSolve::Solved((topology, point_assignment))) =
        resolve_endpoint_configuration_relation_streaming(
            &ctx,
            crate::solve::mesh_quotient::ResolveEndpointConfigurationRelationStreamingInputs {
                assignments: &assignments,
                endpoint_configurations: &endpoint_configurations,
                edge_candidates: &edge_candidates,
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                port_identities: &port_identities,
                budget: &budget,
                partial_solution_valid: None,
                complete_solution_valid: None,
                candidate_gauge: None,
                priority_edges: None,
                coordinate_domains: None,
            },
        )
        .expect("service resource budget")
    else {
        panic!("endpoint configuration relation did not solve the cycle");
    };

    assert_eq!(topology.faces.len(), 1);
    assert_eq!(point_assignment, vec![0, 1, 2]);
}

#[test]
fn fixed_endpoint_pairs_materialize_duplicate_boundary_assignments() {
    catia_test_context!(ctx);
    let edge_rows = (0..3)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let vertex_points = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let edge_candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];
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
    };
    let assignments = vec![vec![assignment.clone(), assignment]];
    let endpoint_configurations = vec![vec![
        Some(vec![vec![(0, [0, 1]), (1, [1, 2]), (2, [0, 2])]]),
        Some(vec![vec![(0, [0, 1]), (1, [1, 2]), (2, [0, 2])]]),
    ]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let result = resolve_endpoint_configuration_relation_streaming(
        &ctx,
        crate::solve::mesh_quotient::ResolveEndpointConfigurationRelationStreamingInputs {
            assignments: &assignments,
            endpoint_configurations: &endpoint_configurations,
            edge_candidates: &edge_candidates,
            edge_rows: &edge_rows,
            vertex_points: &vertex_points,
            port_identities: &[[0, 1], [2, 3], [4, 5]],
            budget: &budget,
            partial_solution_valid: None,
            complete_solution_valid: None,
            candidate_gauge: None,
            priority_edges: None,
            coordinate_domains: None,
        },
    )
    .expect("service resource budget");

    let Some(MeshSolve::Solved((topology, point_assignment))) = result else {
        panic!("endpoint relation did not materialize duplicate assignments");
    };
    assert_eq!(topology.faces.len(), 1);
    assert_eq!(point_assignment, vec![0, 1, 2]);
}

#[test]
fn endpoint_relation_keeps_stopped_face_assignments() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let face_assignments = vec![assignment.clone(), assignment];
    let face_configurations = vec![Some(vec![vec![(0, [0, 0])]]), None];
    let mut covered = vec![false];

    let choices = crate::test_support::with_service_context(|ctx| {
        collect_endpoint_relation_face_choices(
            &face_assignments,
            &face_configurations,
            &mut covered,
        )
    })
    .expect("service resource budget")
    .expect("well-formed endpoint relation choices");

    assert!(covered[0]);
    assert!(choices
        .iter()
        .any(|choice| { matches!(choice.selection, MeshEndpointRelationSelection::Deferred) }));
    assert!(choices
        .iter()
        .any(|choice| matches!(&choice.selection, MeshEndpointRelationSelection::Enumerated { assignments, edge_pairs } if assignments == &[0] && edge_pairs == &[(0, [0, 0])])));
}

#[test]
fn raw_endpoint_relation_state_signature_ignores_local_order() {
    catia_test_context!(ctx);
    let left = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 7,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![2, 0, 2],
                    edge_pairs: vec![(1, [3, 2]), (0, [1, 0])],
                },
            },
            MeshEndpointRelationChoice {
                id: 3,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![4],
                    edge_pairs: vec![(2, [5, 4])],
                },
            },
        ],
        vec![MeshEndpointRelationChoice {
            id: 9,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![3, 1],
                edge_pairs: vec![(0, [1, 0])],
            },
        }],
    ];
    let right = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 30,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![4],
                    edge_pairs: vec![(2, [4, 5])],
                },
            },
            MeshEndpointRelationChoice {
                id: 70,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![0, 2],
                    edge_pairs: vec![(0, [0, 1]), (1, [2, 3])],
                },
            },
        ],
        vec![MeshEndpointRelationChoice {
            id: 90,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![1, 3],
                edge_pairs: vec![(0, [0, 1])],
            },
        }],
    ];
    let left_assigned = vec![Some([3, 2]), None, Some([5, 4])];
    let right_assigned = vec![Some([2, 3]), None, Some([4, 5])];

    assert_eq!(
        raw_endpoint_relation_state_signature(&left, &left_assigned)
            .expect("service resource budget"),
        raw_endpoint_relation_state_signature(&right, &right_assigned)
            .expect("service resource budget"),
    );
}

#[test]
fn endpoint_relation_requires_one_joint_support_for_all_shared_edges() {
    catia_test_context!(ctx);
    let mut domains = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![0],
                    edge_pairs: vec![(0, [0, 1]), (1, [2, 3])],
                },
            },
            MeshEndpointRelationChoice {
                id: 1,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![1],
                    edge_pairs: vec![(0, [4, 5]), (1, [6, 7])],
                },
            },
        ],
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![0],
                    edge_pairs: vec![(0, [0, 1]), (1, [6, 7])],
                },
            },
            MeshEndpointRelationChoice {
                id: 1,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![1],
                    edge_pairs: vec![(0, [4, 5]), (1, [6, 7])],
                },
            },
        ],
    ];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let constraints = build_endpoint_relation_constraints(&ctx, &domains, &budget)
        .expect("service resource budget")
        .expect("shared-edge relation constraints should build");
    let mut assigned = vec![None; 2];

    assert!(propagate_endpoint_relation_domains(
        &ctx,
        &mut domains,
        &mut assigned,
        &constraints,
        &budget,
    )
    .expect("service resource budget"));
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
    catia_test_context!(ctx);
    let mut domains = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![0],
                    edge_pairs: vec![(0, [0, 1])],
                },
            },
            MeshEndpointRelationChoice {
                id: 1,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![1],
                    edge_pairs: vec![(0, [2, 3])],
                },
            },
        ],
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![0],
                    edge_pairs: vec![(0, [4, 5])],
                },
            },
            MeshEndpointRelationChoice {
                id: 1,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![1],
                    edge_pairs: Vec::new(),
                },
            },
        ],
    ];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let constraints = build_endpoint_relation_constraints(&ctx, &domains, &budget)
        .expect("service resource budget")
        .expect("shared-edge relation constraints should build");
    let mut assigned = vec![None];

    assert!(propagate_endpoint_relation_domains(
        &ctx,
        &mut domains,
        &mut assigned,
        &constraints,
        &budget,
    )
    .expect("service resource budget"));
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

    let configurations = crate::test_support::with_service_context(|ctx| {
        mesh_face_endpoint_configurations(
            std::slice::from_ref(&assignment),
            &candidates,
            &[None; 3],
            &budget,
        )
    })
    .expect("service resource budget")
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

    catia_test_context!(ctx);
    let directions = endpoint_configuration_directions(&ctx, &assignment, &configuration)
        .expect("service resource budget")
        .expect("unresolved boundary directions should enumerate");

    assert_eq!(directions.len(), 1);
    assert_eq!(directions[0].len(), 2);
    assert_eq!(directions[0][0].len(), 2);
    assert_eq!(directions[0][1].len(), 2);
}

#[test]
fn endpoint_cycle_adjacency_charges_implicit_candidate_enumeration() {
    catia_test_context!(ctx);
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
        )
        .expect("service resource budget"),
        None
    );
    assert!(budget.exhausted());
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
        crate::test_support::with_service_context(|ctx| quotient.merge(0, 2))
            .expect("service merge")
            .expect("shared left endpoint");
        crate::test_support::with_service_context(|ctx| quotient.merge(1, 3))
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
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture fits input limit");
    let mut quotient = MeshQuotient::new(
        (0..4)
            .map(|node| Arc::new(HashSet::from([usize::from(node % 2 != 0)])))
            .collect(),
    );
    crate::test_support::with_service_context(|ctx| quotient.merge(0, 2))
        .expect("service merge")
        .expect("shared left endpoint");
    crate::test_support::with_service_context(|ctx| quotient.merge(1, 3))
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
fn coordinate_root_preparation_budgets_independent_components_separately() {
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
        crate::test_support::with_service_context(|ctx| quotient.merge(node + 1, node + 2))
            .expect("service merge")
            .expect("disjoint coordinate roots merge");
        crate::test_support::with_service_context(|ctx| quotient.merge(node + 3, node + 4))
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

#[test]
fn singleton_mesh_path_handles_many_independent_face_cycles() {
    const FACE_COUNT: usize = 128;
    catia_test_context!(ctx);
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
            [
                cadmpeg_core::convert::f64_from_index(point)
                    .expect("fixture index is exactly representable"),
                0.0,
                0.0,
            ],
            [
                cadmpeg_core::convert::f64_from_index(point + 1)
                    .expect("fixture index is exactly representable"),
                0.0,
                0.0,
            ],
            [
                cadmpeg_core::convert::f64_from_index(point + 2)
                    .expect("fixture index is exactly representable"),
                0.0,
                0.0,
            ],
            [
                cadmpeg_core::convert::f64_from_index(point + 3)
                    .expect("fixture index is exactly representable"),
                0.0,
                0.0,
            ],
        ]);
        edge_rows.extend((0..4).map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        }));
        edge_candidates.extend([
            vec![[point, point + 1]],
            vec![[point + 1, point + 2]],
            vec![[point + 2, point + 3]],
            vec![[point, point + 3]],
        ]);
        let identity = u32::try_from(edge * 2).expect("fixture value fits u32");
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

    let MeshSolve::Solved((topology, point_assignment)) =
        resolve_singleton_mesh_endpoint_candidates(
            &ctx,
            crate::solve::mesh_quotient::ResolveSingletonMeshEndpointCandidatesInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates: &edge_candidates,
                assignments: &assignments,
                port_identities: &port_identities,
                edge_direction_evidence: None,
                budget: &budget,
                candidate_gauge: None,
            },
        )
        .expect("service resource budget")
        .expect("singleton path applies")
    else {
        panic!("singleton path did not solve");
    };
    assert_eq!(topology.faces.len(), FACE_COUNT);
    assert_eq!(point_assignment.len(), FACE_COUNT * 4);
}

#[test]
fn singleton_mesh_path_filters_endpoint_incompatible_face_assignments() {
    catia_test_context!(ctx);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let edge_rows = (0..3)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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

    let MeshSolve::Solved((topology, point_assignment)) =
        resolve_singleton_mesh_endpoint_candidates(
            &ctx,
            crate::solve::mesh_quotient::ResolveSingletonMeshEndpointCandidatesInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates: &edge_candidates,
                assignments: &assignments,
                port_identities: &port_identities,
                edge_direction_evidence: None,
                budget: &budget,
                candidate_gauge: None,
            },
        )
        .expect("service resource budget")
        .expect("endpoint filtering should leave one face assignment")
    else {
        panic!("endpoint filtering did not solve");
    };
    assert_eq!(topology.faces.len(), 1);
    assert_eq!(point_assignment, vec![0, 1, 2]);
}

#[test]
fn singleton_mesh_path_handles_closed_endpoint_pairs() {
    catia_test_context!(ctx);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let edge_rows = vec![{
        assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
        EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row")
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

    let MeshSolve::Solved((topology, point_assignment)) =
        resolve_singleton_mesh_endpoint_candidates(
            &ctx,
            crate::solve::mesh_quotient::ResolveSingletonMeshEndpointCandidatesInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates: &edge_candidates,
                assignments: &assignments,
                port_identities: &port_identities,
                edge_direction_evidence: None,
                budget: &budget,
                candidate_gauge: None,
            },
        )
        .expect("service resource budget")
        .expect("closed endpoint pair should use a direction gauge")
    else {
        panic!("closed endpoint pair did not solve");
    };
    assert_eq!(topology.logical_vertex_count, 1);
    assert_eq!(point_assignment, vec![0]);
}

#[cfg(test)]
mod bitset_and_root_count_tests {
    use super::{bitset_words, required_component_roots, NonZeroUsize};

    /// A bitset over a domain of no choices states no words. Nothing floors it
    /// at one: the readers index by a choice identifier below the bit count, so
    /// an empty domain is never read.
    #[test]
    fn a_bitset_over_an_empty_domain_states_no_words() {
        assert_eq!(bitset_words(0), 0);
        assert_eq!(bitset_words(1), 1);
        assert_eq!(bitset_words(64), 1);
        assert_eq!(bitset_words(65), 2);
        assert_eq!(bitset_words(128), 2);
    }

    /// A component owns at least one root, so a merge capacity that reaches or
    /// passes its root count leaves that one root. A component of no roots is
    /// a state the argument type does not hold.
    #[test]
    fn a_merge_capacity_at_or_above_the_root_count_leaves_one_root() {
        let three = NonZeroUsize::new(3).expect("three roots");
        assert_eq!(required_component_roots(three, 0), 3);
        assert_eq!(required_component_roots(three, 1), 2);
        assert_eq!(required_component_roots(three, 3), 1);
        assert_eq!(required_component_roots(three, 9), 1);
    }
}

#[cfg(test)]
mod direct_matching_tests {
    use super::selection_search::resolve_mesh_selection_from_quotient;
    use super::{initial_mesh_quotient, MAX_MESH_CONSTRAINT_OPERATIONS};
    use crate::families::standard::topology::EdgeBoundaryLayout;
    use crate::families::standard::topology::EdgeRow;
    use crate::families::standard::topology::StandardTopologyDraft;
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
            .expect("service resource budget")
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
        .expect("service resource budget")
        .is_none());

        let singleton_candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];
        let singleton_quotient = initial_mesh_quotient(&singleton_candidates, 3, &port_identities)
            .expect("service resource budget")
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
            run(&ctx).expect("service resource budget"),
            Some(super::MeshSolve::Solved(_))
        ));
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[test]
fn face_domain_local_work_exhaustion_reports_an_exhausted_search() {
    crate::test_support::with_service_context(|ctx| {
        let budget = cadmpeg_core::decode::WorkBudget::new(0);
        let assignments = [vec![[0, 0]]];
        let outcome = parse_standard_mesh_candidate_outcome_with_face_assignments(
            ctx,
            MeshFaceAssignmentCandidates::Concrete {
                assignments: &assignments,
                face_count: 1,
            },
            &budget,
            |_, _| panic!("an exhausted slice must not run the branch"),
        )
        .expect("local slice exhaustion is not a session refusal");
        assert_eq!(
            outcome,
            MeshSolve::Failed(MeshCandidateFailure::Exhausted(
                MeshCandidateExhaustion::FaceDomainEnumeration
            ))
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[cfg(test)]
#[test]
fn ordered_face_local_ceiling_skips_constraint_propagation() {
    crate::test_support::with_service_context(|ctx| {
        let mut quotient =
            MeshQuotient::new((0..130).map(|_| Arc::new(HashSet::from([0]))).collect());
        let domains = [MeshFaceBoundaryDomain::Ordered(
            (0..65)
                .map(|edge| MeshFaceBoundaryAssignment {
                    boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: Some(false),
                    }]],
                })
                .collect(),
        )];
        let candidates = vec![Vec::new(); 65];
        let budget = cadmpeg_core::decode::WorkBudget::new(1_000_000);
        // 65 edges exceed the 64-unit face slice: the face contributes no
        // equations and the quotient keeps its 130 independent roots.
        propagate_common_ordered_face_quotients(ctx, &domains, &candidates, &mut quotient, &budget)
            .expect("a face over its local ceiling is skipped")
            .expect("skipping a face leaves the quotient viable");
        assert_eq!(quotient.root_count().expect("root count"), 130);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[cfg(test)]
mod preparation_tests {
    use super::{propagate_common_ordered_face_quotients, MeshQuotient};
    use crate::solve::missing_edge::{
        MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
    };
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn directed_face_merges_only_its_corner_roots() {
        crate::test_support::with_service_context(|ctx| {
            let shared = Arc::new(HashSet::from([0, 1]));
            let mut quotient = MeshQuotient::new(vec![shared; 1_000]);
            let domains = [MeshFaceBoundaryDomain::Ordered(vec![
                MeshFaceBoundaryAssignment {
                    boundaries: vec![(0..4)
                        .map(|edge| MeshBoundaryEdgeCandidate {
                            edge,
                            start: edge,
                            end: (edge + 1) % 4,
                            reversed: Some(false),
                        })
                        .collect()],
                },
            ])];
            let candidates = vec![Vec::new(); 500];
            let budget = cadmpeg_core::decode::WorkBudget::new(1_000_000);
            propagate_common_ordered_face_quotients(
                ctx,
                &domains,
                &candidates,
                &mut quotient,
                &budget,
            )
            .expect("directed face work fits")
            .expect("complete corner equations");
            for [left, right] in [[1, 2], [3, 4], [5, 6], [7, 0]] {
                assert_eq!(
                    quotient.root(left).expect("corner root"),
                    quotient.root(right).expect("corner root")
                );
            }
            assert_eq!(quotient.root_count().expect("root count"), 996);
            assert_eq!(quotient.root(8).expect("independent root"), 8);
        });
    }
}

#[cfg(test)]
#[test]
fn boundary_component_exhausted_slice_returns_no_states() {
    crate::test_support::with_service_context(|ctx| {
        let budget = cadmpeg_core::decode::WorkBudget::new(0);
        assert!(!budget.charge());
        let domain = MeshFaceBoundaryDomain::Ordered(Vec::new());
        assert!(
            advance_boundary_component_states(ctx, &domain, &[], &[], 128, &budget)
                .expect("component slice exhaustion is not a session refusal")
                .is_none()
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[cfg(test)]
mod pair_support_tests {
    use super::prune_mesh_endpoint_pair_support;
    use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};

    #[test]
    fn endpoint_pair_pruning_keeps_independent_one_edge_faces() {
        const EDGES: usize = 512;
        let mut assignments = (0..EDGES)
            .map(|edge| {
                vec![MeshFaceBoundaryAssignment {
                    boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: None,
                    }]],
                }]
            })
            .collect::<Vec<_>>();
        let mut candidates = vec![vec![[0, 0]]; EDGES];
        crate::test_support::with_service_context(|ctx| {
            assert!(
                prune_mesh_endpoint_pair_support(ctx, &mut assignments, &mut candidates)
                    .expect("independent one-edge faces require linear candidate storage")
            );
        });
        assert!(assignments.iter().all(|choices| choices.len() == 1));
        assert!(candidates.iter().all(|pairs| pairs == &[[0, 0]]));
    }
}
