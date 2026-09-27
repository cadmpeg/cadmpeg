//! Mesh-quotient constraint solver for standard nested B-rep topology.
//!
//! Closes vertex-coordinate quotients and enumerates face endpoint configurations.

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
    incidence_cycles, orient_face_cycles, reconstruct_mesh_selection, Boundary, CoedgeUse, EdgeRow,
    FaceTopology, StandardTopology,
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
/// orientation selection. The decode session applies its own global work cap.
pub(crate) const MAX_MESH_CONSTRAINT_OPERATIONS: usize = 1_000_000;
/// The relation walk and its endpoint materialization proof are independent
/// bounded phases and each uses the complete mesh-constraint allowance.
pub(crate) const MAX_MESH_TOPOLOGY_OPERATIONS: usize =
    MAX_MESH_CONSTRAINT_OPERATIONS.saturating_mul(2);
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
pub(crate) type MeshCandidateSolve = MeshSolve<(StandardTopology, Vec<usize>)>;

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
type MeshFaceDomainCandidateSolve = MeshSolve<(Vec<[usize; 2]>, StandardTopology, Vec<usize>)>;

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
    MeshSolve<(StandardTopology, Vec<usize>), MeshCandidateFailure<(), (), ()>>;

fn copy_mesh_endpoint_resolution(
    ctx: &DecodeContext<'_>,
    resolution: &MeshEndpointResolve,
) -> Result<MeshEndpointResolve, CodecError> {
    Ok(match resolution {
        MeshSolve::Solved((topology, points)) => MeshSolve::Solved((
            topology.clone_charged(ctx)?,
            crate::resource::copy_retained_slice(ctx, points, "catia_endpoint_memo_points")?,
        )),
        MeshSolve::Failed(failure) => MeshSolve::Failed(*failure),
    })
}

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
    let support_work = edge_ids
        .iter()
        .map(|edge| edge_candidates[*edge].len().saturating_mul(2))
        .sum::<usize>();
    if support_work > 0 && budget.is_some_and(|budget| !budget.charge_by(support_work)) {
        return Ok(false);
    }
    let mut supports = Vec::new();
    for &edge in edge_ids {
        let mut edge_supports = HashMap::<usize, HashSet<usize>>::new();
        for [left, right] in edge_candidates[edge].iter().copied() {
            for (point, neighbor) in [(left, right), (right, left)] {
                crate::resource::admit_map_entry(
                    ctx,
                    &mut edge_supports,
                    &point,
                    "catia_arc_support_points",
                )?;
                crate::resource::insert_set(
                    ctx,
                    edge_supports.entry(point).or_default(),
                    neighbor,
                    "catia_arc_support_neighbors",
                )?;
            }
        }
        crate::resource::push(ctx, &mut supports, edge_supports, "catia_arc_support_edges")?;
    }
    let mut queued = ctx.alloc_filled(edges.len(), [true; 2], "catia_arc_queued")?;
    let mut queue = VecDeque::new();
    for edge in 0..edges.len() {
        for side in 0..2 {
            crate::resource::push_back(ctx, &mut queue, (edge, side), "catia_arc_queue")?;
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
            crate::resource::insert_set(ctx, &mut other_domain, point, "catia_arc_other_domain")?;
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
                crate::resource::push_back(
                    ctx,
                    &mut queue,
                    (neighbor, revised_side),
                    "catia_arc_queue",
                )?;
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
            crate::resource::push_back(ctx, &mut queue, (edge, side), "catia_arc_from_queue")?;
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
            crate::resource::insert_set(
                ctx,
                &mut other_domain,
                point,
                "catia_arc_from_other_domain",
            )?;
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
                crate::resource::push_back(
                    ctx,
                    &mut queue,
                    (neighbor, revised_side),
                    "catia_arc_from_queue",
                )?;
            }
        }
    }
    Ok(true)
}

fn enforce_sparse_endpoint_membership(
    ctx: &DecodeContext<'_>,
    domains: &mut [Vec<usize>],
    edges: &[[usize; 2]],
    edge_ids: &[usize],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&WorkBudget<'_>>,
) -> Result<bool, CodecError> {
    let mut ordered = Vec::new();
    for edge in 0..edges.len() {
        crate::resource::push(ctx, &mut ordered, edge, "catia_sparse_ordered_edges")?;
    }
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
            return Ok(false);
        }
        let mut allowed = HashSet::new();
        for point in candidates.iter().flatten().copied() {
            crate::resource::insert_set(ctx, &mut allowed, point, "catia_sparse_allowed_points")?;
        }
        domains[left].retain(|point| allowed.contains(point));
        if right != left {
            domains[right].retain(|point| allowed.contains(point));
        }
        if domains[left].is_empty() || domains[right].is_empty() {
            return Ok(false);
        }
    }
    Ok(true)
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
        "catia_arc_support_points",
        "catia_arc_support_neighbors",
        "catia_arc_support_edges",
        "catia_arc_queued",
        "catia_arc_queue",
        "catia_arc_other_domain",
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
        "catia_arc_from_other_domain",
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

#[derive(Clone)]
pub(crate) struct MeshQuotient {
    union: UnionFind,
    domains: Vec<Arc<HashSet<usize>>>,
    members: Vec<Vec<usize>>,
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
    pub(super) fn width_upper_bound(&self, ctx: &DecodeContext<'_>) -> Result<usize, CodecError> {
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
            MeshImplicitEdgeCandidateSource::Required { remaining, .. } => Ok(*remaining),
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
                    let Some(&right_point) = right.get(*right_index) else {
                        return None;
                    };
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
    pub(super) fn clone_charged(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            domains: Arc::new(crate::resource::copy_retained_rows(
                ctx,
                &self.domains,
                "catia_coordinate_root_clone_domains",
                "catia_coordinate_root_clone_points",
            )?),
            edges: Arc::clone(&self.edges),
            root_edges: Arc::clone(&self.root_edges),
            edge_candidates: Arc::clone(&self.edge_candidates),
            coverage_matching: crate::resource::copy_retained_slice(
                ctx,
                &self.coverage_matching,
                "catia_coordinate_root_clone_matching",
            )?,
            point_count: self.point_count,
        })
    }

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

    pub(super) fn edge_candidate_points(
        &self,
        ctx: &DecodeContext<'_>,
        edge: usize,
    ) -> Result<Option<Vec<usize>>, CodecError> {
        let Some(candidates) = self.edge_candidates.get(edge) else {
            return Ok(None);
        };
        if !candidates.is_empty() {
            let mut points = Vec::new();
            for point in candidates.iter().flatten().copied() {
                crate::resource::push(
                    ctx,
                    &mut points,
                    point,
                    "catia_coordinate_edge_candidate_points",
                )?;
            }
            points.sort_unstable();
            points.dedup();
            return Ok(Some(points));
        }
        let Some(&[left, right]) = self.edges.get(edge) else {
            return Ok(None);
        };
        let mut points = crate::resource::copy_slice(
            ctx,
            &self.domains[left],
            "catia_coordinate_edge_points_left",
        )?;
        if right != left {
            crate::resource::reserve_vec(
                ctx,
                &mut points,
                self.domains[right].len(),
                "catia_coordinate_edge_points_right",
            )?;
            points.extend_from_slice(&self.domains[right]);
            points.sort_unstable();
            points.dedup();
        }
        Ok(Some(points))
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
                crate::resource::push(
                    ctx,
                    &mut roots_by_point[point],
                    root,
                    "catia_quotient_roots_by_point_entries",
                )?;
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
        let mut affected_edges =
            crate::resource::copy_slice(ctx, initial_edges, "catia_quotient_refine_initial_edges")?;
        let mut coverage_matching = crate::resource::copy_slice(
            ctx,
            &self.coverage_matching,
            "catia_quotient_refine_coverage_matching",
        )?;
        let propagate_globally = propagate_all_different;
        loop {
            let mut domain_lengths = Vec::new();
            for domain in &domains {
                crate::resource::push(
                    ctx,
                    &mut domain_lengths,
                    domain.len(),
                    "catia_quotient_refine_domain_lengths",
                )?;
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
                    crate::resource::push(
                        ctx,
                        &mut roots_by_point[point],
                        root,
                        "catia_quotient_refine_root_entries",
                    )?;
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
                    crate::resource::push(
                        ctx,
                        &mut changed_roots,
                        root,
                        "catia_quotient_refine_changed_roots",
                    )?;
                }
            }
            let affected_points = if propagate_globally {
                let mut all_points = Vec::new();
                for point in 0..self.point_count {
                    crate::resource::push(
                        ctx,
                        &mut all_points,
                        point,
                        "catia_quotient_refine_all_points",
                    )?;
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
                    crate::resource::push_back(
                        ctx,
                        &mut root_queue,
                        root,
                        "catia_quotient_refine_root_queue",
                    )?;
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
                                crate::resource::push_back(
                                    ctx,
                                    &mut root_queue,
                                    neighbor,
                                    "catia_quotient_refine_root_queue",
                                )?;
                            }
                        }
                    }
                }
                let mut points = Vec::new();
                for (point, reached) in reached_points.into_iter().enumerate() {
                    if reached {
                        crate::resource::push(
                            ctx,
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
            for &point in &affected_points {
                let domain = crate::resource::copy_slice(
                    ctx,
                    &roots_by_point[point],
                    "catia_quotient_refine_affected_domain_roots",
                )?;
                crate::resource::push(
                    ctx,
                    &mut affected_domains,
                    domain,
                    "catia_quotient_refine_affected_domains",
                )?;
                crate::resource::push(
                    ctx,
                    &mut affected_matching,
                    coverage_matching[point],
                    "catia_quotient_refine_affected_matching",
                )?;
            }
            let support_count = affected_domains.iter().map(Vec::len).sum::<usize>();
            let propagation_work = support_count.saturating_mul(4);
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
                    crate::resource::push(
                        ctx,
                        &mut affected_roots,
                        root,
                        "catia_quotient_refine_affected_roots",
                    )?;
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
                    crate::resource::push(
                        ctx,
                        &mut affected_edges,
                        edge,
                        "catia_quotient_refine_affected_edges",
                    )?;
                }
            }
            affected_edges.sort_unstable();
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
            return Ok(Some(self.clone_charged(ctx)?));
        }
        if !candidates.is_empty() && !candidates.contains(&pair) {
            return Ok(None);
        }
        if candidates.is_empty() && !self.supports_edge_candidate(edge, pair) {
            return Ok(None);
        }
        let mut edge_candidates = crate::resource::copy_retained_rows(
            ctx,
            self.edge_candidates.as_ref(),
            "catia_coordinate_refine_candidate_rows",
            "catia_coordinate_refine_candidate_pairs",
        )?;
        edge_candidates[edge] = crate::resource::copy_retained_slice(
            ctx,
            &[pair],
            "catia_coordinate_refine_selected_pair",
        )?;
        let Some(RefinedCoordinateDomains {
            domains,
            coverage_matching,
        }) = self.refine_domains(
            ctx,
            crate::resource::copy_retained_rows(
                ctx,
                &self.domains,
                "catia_coordinate_refine_domain_rows",
                "catia_coordinate_refine_domain_points",
            )?,
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
                crate::resource::push(
                    ctx,
                    &mut changed,
                    edge,
                    "catia_coordinate_refine_changed_edges",
                )?;
            }
        }
        if changed.is_empty() {
            return Ok(Some(self.clone_charged(ctx)?));
        }
        let Some(RefinedCoordinateDomains {
            domains,
            coverage_matching,
        }) = self.refine_domains(
            ctx,
            crate::resource::copy_retained_rows(
                ctx,
                &self.domains,
                "catia_coordinate_refine_domain_rows",
                "catia_coordinate_refine_domain_points",
            )?,
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
            edge_candidates: Arc::new(crate::resource::copy_retained_rows(
                ctx,
                edge_candidates,
                "catia_coordinate_refine_candidate_rows",
                "catia_coordinate_refine_candidate_pairs",
            )?),
            coverage_matching,
            point_count: self.point_count,
        }))
    }
}

pub(super) fn initial_mesh_quotient(
    ctx: &DecodeContext<'_>,
    edge_candidates: &[Vec<[usize; 2]>],
    point_count: usize,
    port_identities: &[[u32; 2]],
) -> Result<Option<MeshQuotient>, CodecError> {
    if port_identities.len() != edge_candidates.len() {
        return Ok(None);
    }
    let mut all_points = HashSet::new();
    for point in 0..point_count {
        crate::resource::insert_set(ctx, &mut all_points, point, "catia_initial_quotient_points")?;
    }
    let all_points = Arc::new(all_points);
    let mut domains = Vec::new();
    for candidates in edge_candidates {
        let domain = if candidates.is_empty() {
            Arc::clone(&all_points)
        } else {
            let mut points = HashSet::new();
            for &point in candidates.iter().flatten() {
                crate::resource::insert_set(
                    ctx,
                    &mut points,
                    point,
                    "catia_initial_quotient_candidate_points",
                )?;
            }
            Arc::new(points)
        };
        if domain.is_empty() || domain.iter().any(|point| *point >= point_count) {
            return Ok(None);
        }
        crate::resource::push(
            ctx,
            &mut domains,
            Arc::clone(&domain),
            "catia_initial_quotient_domains",
        )?;
        crate::resource::push(ctx, &mut domains, domain, "catia_initial_quotient_domains")?;
    }
    let mut quotient = MeshQuotient::new_charged(ctx, domains)?;
    let mut node_by_identity = HashMap::new();
    for (edge, ports) in port_identities.iter().enumerate() {
        for (port, identity) in ports.iter().copied().enumerate() {
            let node = edge * 2 + port;
            if let Some(&previous) = node_by_identity.get(&identity) {
                if quotient.merge_charged(ctx, previous, node)?.is_none() {
                    return Ok(None);
                }
            } else {
                crate::resource::insert_map(
                    ctx,
                    &mut node_by_identity,
                    identity,
                    node,
                    "catia_initial_quotient_identities",
                )?;
            }
        }
    }
    Ok(quotient
        .edge_domains_viable(ctx, edge_candidates)?
        .then_some(quotient))
}

#[cfg(test)]
#[test]
fn initial_quotient_points_refuse_before_invalid_candidate_result() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let candidates = [vec![[0, 1]]];
    let identities = [[10, 11]];
    catia_test_context!(service_ctx);
    assert!(
        initial_mesh_quotient(&service_ctx, &candidates, 1, &identities)
            .expect("service resource budget")
            .is_none()
    );
    assert!(
        initial_mesh_quotient(&service_ctx, &candidates, 2, &identities)
            .expect("service resource budget")
            .is_some()
    );

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert!(
        matches!(initial_mesh_quotient(&ctx, &candidates, 1, &identities),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia_initial_quotient_points")
    );
}

#[cfg(test)]
#[test]
fn initial_quotient_union_and_merge_refuse_each_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let candidates = [vec![[0, 1]], vec![[0, 1]]];
    let identities = [[10, 11], [10, 12]];
    catia_test_context!(service_ctx);
    assert!(
        initial_mesh_quotient(&service_ctx, &candidates, 2, &identities)
            .expect("service resource budget")
            .is_some()
    );

    let mut refused = HashSet::new();
    for cap in 0..=48 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match initial_mesh_quotient(&ctx, &candidates, 2, &identities) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                refused.insert(limit.operation);
            }
            Ok(Some(_)) => {}
            Ok(None) => panic!("consistent identities must admit a quotient"),
            Err(error) => panic!("unexpected quotient refusal: {error}"),
        }
    }
    for operation in [
        "catia_quotient_union",
        "catia_quotient_members",
        "catia_quotient_member_nodes",
        "catia_quotient_intersection",
        "catia_quotient_merged_members",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
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
    pub(crate) fn clone_charged(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let union = self
            .union
            .clone_charged(ctx, "catia_quotient_clone_union")?;
        let domains =
            crate::resource::copy_slice(ctx, &self.domains, "catia_quotient_clone_domains")?;
        let mut members = Vec::new();
        crate::resource::reserve_vec(
            ctx,
            &mut members,
            self.members.len(),
            "catia_quotient_clone_member_rows",
        )?;
        for row in &self.members {
            members.push(crate::resource::copy_slice(
                ctx,
                row,
                "catia_quotient_clone_member_nodes",
            )?);
        }
        Ok(Self {
            union,
            domains,
            members,
        })
    }

    pub(crate) fn new_charged(
        ctx: &DecodeContext<'_>,
        domains: Vec<Arc<HashSet<usize>>>,
    ) -> Result<Self, CodecError> {
        let union = UnionFind::charged(ctx, domains.len(), "catia_quotient_union")?;
        let mut members = ctx.alloc_filled(domains.len(), Vec::new(), "catia_quotient_members")?;
        for (node, group) in members.iter_mut().enumerate() {
            crate::resource::push(ctx, group, node, "catia_quotient_member_nodes")?;
        }
        Ok(Self {
            union,
            domains,
            members,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(domains: Vec<Arc<HashSet<usize>>>) -> Self {
        Self {
            union: UnionFind::new(domains.len()),
            members: (0..domains.len()).map(|node| vec![node]).collect(),
            domains,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.domains.len()
    }

    pub(crate) fn find(&mut self, node: usize) -> usize {
        self.union.find(node)
    }

    pub(crate) fn root(&self, node: usize) -> usize {
        self.union.root(node)
    }

    pub(crate) fn domains(&self) -> &[Arc<HashSet<usize>>] {
        &self.domains
    }

    fn members(&self, root: usize) -> &[usize] {
        &self.members[root]
    }

    pub(super) fn coordinate_domain_preparation_limit(
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

    pub(super) fn signature_work(&mut self) -> usize {
        let mut work = 0usize;
        for node in 0..self.union.len() {
            if self.union.find(node) == node {
                work = work
                    .saturating_add(self.members(node).len())
                    .saturating_add(self.domains[node].len());
            }
        }
        work_units(work)
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

    pub(super) fn signature_charged(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<MeshQuotientSignature, CodecError> {
        let mut components = Vec::new();
        for node in 0..self.union.len() {
            if self.union.find(node) != node {
                continue;
            }
            let mut members = crate::resource::copy_slice(
                ctx,
                self.members(node),
                "catia_quotient_signature_members",
            )?;
            members.sort_unstable();
            let mut domain = Vec::new();
            for &point in self.domains[node].iter() {
                crate::resource::push(ctx, &mut domain, point, "catia_quotient_signature_domain")?;
            }
            domain.sort_unstable();
            crate::resource::push(
                ctx,
                &mut components,
                (members, domain),
                "catia_quotient_signature_components",
            )?;
        }
        components.sort_unstable();
        Ok(components)
    }

    pub(super) fn root_count(&mut self) -> usize {
        (0..self.union.len())
            .filter(|node| self.union.find(*node) == *node)
            .count()
    }

    #[cfg(test)]
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

    pub(crate) fn merge_charged(
        &mut self,
        ctx: &DecodeContext<'_>,
        left: usize,
        right: usize,
    ) -> Result<Option<usize>, CodecError> {
        let left = self.union.find(left);
        let right = self.union.find(right);
        if left == right {
            return Ok(Some(left));
        }
        let mut intersection = HashSet::new();
        for &point in self.domains[left].intersection(&self.domains[right]) {
            crate::resource::insert_set(
                ctx,
                &mut intersection,
                point,
                "catia_quotient_intersection",
            )?;
        }
        if intersection.is_empty() {
            return Ok(None);
        }
        let child_members = self.members[right].len();
        crate::resource::reserve_vec(
            ctx,
            &mut self.members[left],
            child_members,
            "catia_quotient_merged_members",
        )?;
        self.union.union(left, right);
        let root = self.union.find(left);
        self.domains[root] = Arc::new(intersection);
        let child_members = std::mem::take(&mut self.members[right]);
        self.members[root].extend(child_members);
        Ok(Some(root))
    }

    pub(crate) fn edge_domains_viable(
        &mut self,
        ctx: &DecodeContext<'_>,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<bool, CodecError> {
        self.propagate_edge_domains(
            ctx,
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
        if self.union.len() != edge_candidates.len().saturating_mul(2) {
            return Ok(None);
        }
        let mut roots = Vec::new();
        for node in 0..self.union.len() {
            if self.union.find(node) == node {
                crate::resource::push(ctx, &mut roots, node, "catia_quotient_roots")?;
            }
        }
        if roots.len() < point_count {
            return Ok(None);
        }
        let mut root_indices = HashMap::new();
        for (index, root) in roots.iter().copied().enumerate() {
            crate::resource::insert_map(
                ctx,
                &mut root_indices,
                root,
                index,
                "catia_quotient_root_indices",
            )?;
        }
        let mut edges = Vec::new();
        for edge in 0..edge_candidates.len() {
            let Some(&left) = root_indices.get(&self.union.find(edge * 2)) else {
                return Ok(None);
            };
            let Some(&right) = root_indices.get(&self.union.find(edge * 2 + 1)) else {
                return Ok(None);
            };
            crate::resource::push(ctx, &mut edges, [left, right], "catia_quotient_edges")?;
        }
        let mut domains = Vec::new();
        for root in roots.iter().copied() {
            let mut domain = Vec::new();
            for point in self.domains[root]
                .iter()
                .copied()
                .filter(|point| *point < point_count)
            {
                crate::resource::push(ctx, &mut domain, point, "catia_quotient_domain_points")?;
            }
            domain.sort_unstable();
            crate::resource::push(ctx, &mut domains, domain, "catia_quotient_domains")?;
        }
        if domains.iter().any(Vec::is_empty) {
            return Ok(None);
        }
        let mut edge_ids = Vec::new();
        for edge in 0..edges.len() {
            crate::resource::push(ctx, &mut edge_ids, edge, "catia_quotient_edge_ids")?;
        }
        let mut root_edges =
            ctx.alloc_filled(roots.len(), Vec::new(), "catia_quotient_root_edges")?;
        for (edge, [left, right]) in edges.iter().copied().enumerate() {
            crate::resource::push(
                ctx,
                &mut root_edges[left],
                edge,
                "catia_quotient_root_edge_entries",
            )?;
            if right != left {
                crate::resource::push(
                    ctx,
                    &mut root_edges[right],
                    edge,
                    "catia_quotient_root_edge_entries",
                )?;
            }
        }
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
        let mut supported_candidates = crate::resource::copy_retained_rows(
            ctx,
            edge_candidates,
            "catia_quotient_supported_candidate_rows",
            "catia_quotient_supported_candidate_pairs",
        )?;
        loop {
            let mut changed = Vec::new();
            for (edge, candidates) in supported_candidates.iter_mut().enumerate() {
                if candidates.is_empty() {
                    continue;
                }
                let [left, right] = edges[edge];
                let before = candidates.len();
                if budget.is_some_and(|budget| !budget.charge_by(before)) {
                    ctx.charge_work(0, "catia quotient domain preparation")?;
                    return Err(ctx.refuse_codec_limit("catia quotient domain preparation", 0, 1));
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
                    crate::resource::push(ctx, &mut changed, edge, "catia_quotient_changed_edges")?;
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
            crate::resource::copy_retained_rows(
                ctx,
                &coordinate_domains.domains,
                "catia_quotient_refine_domain_copy",
                "catia_quotient_refine_domain_points",
            )?,
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
        ctx: &DecodeContext<'_>,
        root: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<bool, CodecError> {
        let (mut edges, _reservation) = crate::resource::temporary_set(
            ctx,
            self.members(root).len(),
            "catia_quotient_component_edges",
        )?;
        for edge in self.members(root).iter().map(|node| node / 2) {
            if !edge_candidates[edge].is_empty() {
                edges.insert(edge);
            }
        }
        self.propagate_edge_domains(ctx, edges, edge_candidates, budget)
    }

    fn affected_edges_for_nodes(
        &mut self,
        ctx: &DecodeContext<'_>,
        nodes: &[usize],
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<HashSet<usize>, CodecError> {
        let mut affected = HashSet::new();
        for &node in nodes {
            let root = self.union.find(node);
            for edge in self.members(root).iter().map(|member| member / 2) {
                if !edge_candidates[edge].is_empty() {
                    crate::resource::insert_set(
                        ctx,
                        &mut affected,
                        edge,
                        "catia_quotient_affected_edges",
                    )?;
                }
            }
        }
        Ok(affected)
    }

    fn propagate_edge_domains(
        &mut self,
        ctx: &DecodeContext<'_>,
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
                if !edge_candidates[edge].is_empty() && queued.insert(edge) {
                    queue.push_back(edge);
                }
            }
        }

        let (mut queue, _queue_reservation) = crate::resource::temporary_queue(
            ctx,
            edge_candidates.len(),
            "catia_quotient_edge_queue",
        )?;
        let (mut queued, _queued_reservation) = crate::resource::temporary_set(
            ctx,
            edge_candidates.len(),
            "catia_quotient_queued_edges",
        )?;
        for edge in edges {
            if queued.insert(edge) {
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
            let start = self.union.find(edge * 2);
            let end = self.union.find(edge * 2 + 1);
            if start == end {
                let (mut supported, _support_reservation) = crate::resource::temporary_set(
                    ctx,
                    candidates.len(),
                    "catia_quotient_self_support",
                )?;
                for pair in candidates {
                    ctx.charge_work(1, "catia quotient edge support")?;
                    if pair[0] == pair[1] && self.domains[start].contains(&pair[0]) {
                        supported.insert(pair[0]);
                    }
                }
                if supported.is_empty() {
                    return Ok(false);
                }
                if supported != *self.domains[start] {
                    ctx.charge_collection_items(
                        supported.len() as u64,
                        "catia_quotient_domain_update",
                    )?;
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
            let Some(support_capacity) = candidates.len().checked_mul(2) else {
                return Err(ctx.refuse_codec_limit(
                    "catia_quotient_pair_support",
                    u64::MAX,
                    u64::MAX,
                ));
            };
            let (mut supported_starts, _start_reservation) = crate::resource::temporary_set(
                ctx,
                support_capacity,
                "catia_quotient_start_support",
            )?;
            let (mut supported_ends, _end_reservation) = crate::resource::temporary_set(
                ctx,
                support_capacity,
                "catia_quotient_end_support",
            )?;
            for &[left, right] in candidates {
                ctx.charge_work(1, "catia quotient edge support")?;
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
                ctx.charge_collection_items(
                    supported_starts.len() as u64,
                    "catia_quotient_domain_update",
                )?;
                self.domains[start] = Arc::new(supported_starts);
                enqueue_component_edges(
                    self.members(start),
                    edge_candidates,
                    &mut queue,
                    &mut queued,
                );
            }
            if supported_ends != *self.domains[end] {
                ctx.charge_collection_items(
                    supported_ends.len() as u64,
                    "catia_quotient_domain_update",
                )?;
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
        ctx: &DecodeContext<'_>,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<bool, CodecError> {
        loop {
            let mut roots_by_point = HashMap::<usize, Vec<usize>>::new();
            for node in 0..self.union.len() {
                let root = self.union.find(node);
                if root != node || self.domains[root].len() != 1 {
                    continue;
                }
                let Some(&point) = self.domains[root].iter().next() else {
                    return Ok(false);
                };
                if let Some(roots) = roots_by_point.get_mut(&point) {
                    crate::resource::push(ctx, roots, root, "catia_singleton_point_roots")?;
                } else {
                    let mut roots = Vec::new();
                    crate::resource::push(ctx, &mut roots, root, "catia_singleton_point_roots")?;
                    crate::resource::insert_map(
                        ctx,
                        &mut roots_by_point,
                        point,
                        roots,
                        "catia_singleton_points",
                    )?;
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
                            crate::resource::insert_set(
                                ctx,
                                &mut affected_edges,
                                edge,
                                "catia_singleton_affected_edges",
                            )?;
                        }
                    }
                    if self.merge_charged(ctx, first, root)?.is_none() {
                        return Ok(false);
                    }
                    changed = true;
                }
            }
            if !changed {
                return Ok(true);
            }
            if !self.propagate_edge_domains(ctx, affected_edges, edge_candidates, None)? {
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
        let result = coordinate_assignment::close_coordinate_roots_with_incidence(
            ctx,
            self,
            point_count,
            edge_candidates,
            incidence,
            budget,
            component_search_budget,
            &ambiguous,
            &exhausted,
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

    fn assignment_has_option(
        &self,
        ctx: &DecodeContext<'_>,
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
            ctx: &DecodeContext<'_>,
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
                if state
                    .quotient
                    .merge_charged(ctx, previous_end, current_start)?
                    .is_none()
                {
                    return Ok(false);
                }
            }
            crate::resource::push(
                ctx,
                &mut state.directions,
                reversed,
                "catia_assignment_directions",
            )?;
            state.at += 1;
            Ok(true)
        }

        let mut states = Vec::new();
        crate::resource::push(
            ctx,
            &mut states,
            State {
                boundary_index: 0,
                at: 0,
                directions: Vec::new(),
                quotient: self.clone_charged(ctx)?,
            },
            "catia_assignment_states",
        )?;
        while let Some(mut state) = states.pop() {
            loop {
                ctx.charge_work(1, "catia assignment option search")?;
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
                    if state
                        .quotient
                        .merge_charged(ctx, last_end, first_start)?
                        .is_none()
                    {
                        break;
                    }
                    if !state.quotient.edge_domains_viable(ctx, edge_candidates)? {
                        break;
                    }
                    state.boundary_index += 1;
                    state.at = 0;
                    state.directions.clear();
                    continue;
                }
                if let Some(reversed) = boundary[state.at].reversed {
                    if !advance(ctx, &mut state, boundary, reversed)? {
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
                        directions: crate::resource::copy_slice(
                            ctx,
                            &state.directions,
                            "catia_assignment_direction_copy",
                        )?,
                        quotient: state.quotient.clone_charged(ctx)?,
                    };
                    if advance(ctx, &mut next, boundary, reversed)?
                        && next.quotient.edge_domains_viable(ctx, edge_candidates)?
                    {
                        crate::resource::push(ctx, &mut states, next, "catia_assignment_states")?;
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
                    let Some(root) = quotient.merge(previous_end, current_start) else {
                        return;
                    };
                    if !quotient
                        .propagate_component_edge_domains(ctx, root, edge_candidates, None)
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
                    let Some(root) = quotient.merge(last_end, first_start) else {
                        return;
                    };
                    if quotient
                        .propagate_component_edge_domains(ctx, root, edge_candidates, None)
                        .expect("service resource budget")
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

    pub(super) fn assignment_options_limited(
        &self,
        ctx: &DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        oriented_edges: &HashSet<usize>,
        limit: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Vec<MeshOrientationOption>, CodecError> {
        fn copy_directions(
            ctx: &DecodeContext<'_>,
            directions: &[Vec<bool>],
        ) -> Result<Vec<Vec<bool>>, CodecError> {
            let mut copy = Vec::new();
            crate::resource::reserve_vec(
                ctx,
                &mut copy,
                directions.len(),
                "catia_orientation_boundary_rows",
            )?;
            for row in directions {
                copy.push(crate::resource::copy_slice(
                    ctx,
                    row,
                    "catia_orientation_boundary_directions",
                )?);
            }
            Ok(copy)
        }

        #[allow(clippy::too_many_arguments)]
        fn walk(
            ctx: &DecodeContext<'_>,
            boundaries: &[Vec<MeshBoundaryEdgeCandidate>],
            boundary_index: usize,
            at: usize,
            boundary_directions: &mut Vec<bool>,
            directions: &mut Vec<Vec<bool>>,
            mut quotient: MeshQuotient,
            edge_candidates: &[Vec<[usize; 2]>],
            output: &mut Vec<(Vec<Vec<bool>>, MeshQuotient)>,
            seen: &mut HashMap<u64, Vec<usize>>,
            oriented: &mut HashSet<usize>,
            gaugeable_edges: &HashSet<usize>,
            limit: usize,
            budget: Option<&WorkBudget<'_>>,
        ) -> Result<(), CodecError> {
            let _depth = ctx.enter_nested("catia orientation search")?;
            ctx.charge_work(1, "catia orientation search")?;
            if output.len() >= limit {
                return Ok(());
            }
            if budget.is_some_and(|budget| !budget.charge()) {
                return Ok(());
            }
            if boundary_index == boundaries.len() {
                if admit_orientation_option(ctx, seen, output, directions, &quotient)? {
                    crate::resource::push(
                        ctx,
                        output,
                        (copy_directions(ctx, directions)?, quotient),
                        "catia_orientation_options",
                    )?;
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
                let Some(root) = quotient.merge_charged(ctx, last_end, first_start)? else {
                    return Ok(());
                };
                if !quotient.propagate_component_edge_domains(ctx, root, edge_candidates, budget)? {
                    return Ok(());
                }
                crate::resource::push(
                    ctx,
                    directions,
                    std::mem::take(boundary_directions),
                    "catia_orientation_direction_rows",
                )?;
                walk(
                    ctx,
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
                )?;
                *boundary_directions = directions.pop().unwrap_or_default();
                return Ok(());
            }
            let edge = boundary[at].edge;
            let first =
                crate::resource::insert_set(ctx, oriented, edge, "catia_orientation_seen_edges")?;
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
                    let Some(root) = quotient.merge_charged(ctx, previous_end, current_start)?
                    else {
                        return Ok(());
                    };
                    if !quotient.propagate_component_edge_domains(
                        ctx,
                        root,
                        edge_candidates,
                        budget,
                    )? {
                        return Ok(());
                    }
                }
                crate::resource::push(
                    ctx,
                    boundary_directions,
                    reversed,
                    "catia_orientation_boundary_directions",
                )?;
                walk(
                    ctx,
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
                )?;
                boundary_directions.pop();
                Ok(())
            };
            match (boundary[at].reversed, first) {
                (Some(reversed), _) => advance(reversed, quotient)?,
                (None, true) if gaugeable_edges.contains(&edge) => advance(false, quotient)?,
                (None, _) => {
                    advance(false, quotient.clone_charged(ctx)?)?;
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
        let mut direction_union = self
            .union
            .clone_charged(ctx, "catia_orientation_direction_union")?;
        let mut gaugeable_edges = HashSet::new();
        for edge in assignment.boundaries.iter().flatten().map(|use_| use_.edge) {
            let Some(right_node) = edge.checked_mul(2).and_then(|node| node.checked_add(1)) else {
                continue;
            };
            if right_node >= self.domains.len() {
                continue;
            }
            let left_node = right_node - 1;
            let left_root = direction_union.find(left_node);
            let right_root = direction_union.find(right_node);
            if left_root == right_root
                || (self.domains[left_root] == self.domains[right_root]
                    && self.members(left_root) == [left_node]
                    && self.members(right_root) == [right_node])
            {
                crate::resource::insert_set(
                    ctx,
                    &mut gaugeable_edges,
                    edge,
                    "catia_orientation_gaugeable_edges",
                )?;
            }
        }
        let mut oriented = HashSet::new();
        crate::resource::reserve_set(
            ctx,
            &mut oriented,
            oriented_edges.len(),
            "catia_orientation_seen_edges",
        )?;
        oriented.extend(oriented_edges.iter().copied());
        let mut variable_count = 0usize;
        let mut orientation_plan = Vec::new();
        crate::resource::reserve_vec(
            ctx,
            &mut orientation_plan,
            assignment.boundaries.len(),
            "catia_orientation_plan_rows",
        )?;
        for boundary in &assignment.boundaries {
            let mut row = Vec::new();
            crate::resource::reserve_vec(
                ctx,
                &mut row,
                boundary.len(),
                "catia_orientation_plan_values",
            )?;
            for use_ in boundary {
                row.push(match use_.reversed {
                    Some(reversed) => (reversed, None),
                    None if crate::resource::insert_set(
                        ctx,
                        &mut oriented,
                        use_.edge,
                        "catia_orientation_seen_edges",
                    )? && gaugeable_edges.contains(&use_.edge) =>
                    {
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
                crate::resource::reserve_vec(
                    ctx,
                    &mut directions,
                    orientation_plan.len(),
                    "catia_orientation_direction_rows",
                )?;
                for boundary in &orientation_plan {
                    let mut row = Vec::new();
                    crate::resource::reserve_vec(
                        ctx,
                        &mut row,
                        boundary.len(),
                        "catia_orientation_boundary_directions",
                    )?;
                    for (fixed, variable) in boundary {
                        row.push(variable.map_or(*fixed, |variable| {
                            let shift = variable_count - variable - 1;
                            mask & (1usize << shift) != 0
                        }));
                    }
                    directions.push(row);
                }
                let mut oriented = HashSet::new();
                crate::resource::reserve_set(
                    ctx,
                    &mut oriented,
                    oriented_edges.len(),
                    "catia_orientation_seen_edges",
                )?;
                oriented.extend(oriented_edges.iter().copied());
                let mut uses_gauge = true;
                for (boundary, row) in assignment.boundaries.iter().zip(&directions) {
                    for (use_, direction) in boundary.iter().zip(row) {
                        let first = crate::resource::insert_set(
                            ctx,
                            &mut oriented,
                            use_.edge,
                            "catia_orientation_seen_edges",
                        )?;
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
                let mut quotient = self.clone_charged(ctx)?;
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
                        let Some(root) = quotient.merge_charged(ctx, left_end, right_start)? else {
                            merged = false;
                            break 'merge;
                        };
                        crate::resource::push(
                            ctx,
                            &mut merged_nodes,
                            root,
                            "catia_orientation_merged_nodes",
                        )?;
                    }
                }
                if !merged {
                    continue;
                }
                let affected_edges =
                    quotient.affected_edges_for_nodes(ctx, &merged_nodes, edge_candidates)?;
                if !quotient.propagate_edge_domains(ctx, affected_edges, edge_candidates, budget)? {
                    continue;
                }
                if admit_orientation_option(ctx, &mut seen, &output, &directions, &quotient)? {
                    crate::resource::push(
                        ctx,
                        &mut output,
                        (directions, quotient),
                        "catia_orientation_options",
                    )?;
                }
            }
            return Ok(output);
        }
        let mut output = Vec::new();
        let mut seen = HashMap::<u64, Vec<usize>>::new();
        let mut oriented = HashSet::new();
        crate::resource::reserve_set(
            ctx,
            &mut oriented,
            oriented_edges.len(),
            "catia_orientation_seen_edges",
        )?;
        oriented.extend(oriented_edges.iter().copied());
        walk(
            ctx,
            &assignment.boundaries,
            0,
            0,
            &mut Vec::new(),
            &mut Vec::new(),
            self.clone_charged(ctx)?,
            edge_candidates,
            &mut output,
            &mut seen,
            &mut oriented,
            &gaugeable_edges,
            limit,
            budget,
        )?;
        Ok(output)
    }

    fn assignment_options_for_directions(
        &self,
        ctx: &DecodeContext<'_>,
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
            let mut quotient = self.clone_charged(ctx)?;
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
                    if quotient
                        .merge_charged(ctx, left_end, right_start)?
                        .is_none()
                    {
                        merged = false;
                        break 'boundaries;
                    }
                }
            }
            if !merged {
                continue;
            }
            if admit_orientation_option(ctx, &mut seen, &output, directions, &quotient)? {
                crate::resource::push(
                    ctx,
                    &mut output,
                    (copy_mesh_boundary_directions(ctx, directions)?, quotient),
                    "catia_fixed_direction_options",
                )?;
            }
        }
        Ok(output)
    }

    fn merge_label_directions_in_place(
        &mut self,
        ctx: &DecodeContext<'_>,
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
        crate::resource::reserve_vec(
            ctx,
            &mut directions,
            assignment.boundaries.len(),
            "catia_label_direction_rows",
        )?;
        for (boundary, labels) in assignment.boundaries.iter().zip(label_directions) {
            let mut row = Vec::new();
            crate::resource::reserve_vec(
                ctx,
                &mut row,
                boundary.len(),
                "catia_label_direction_values",
            )?;
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
                if self.merge_charged(ctx, left_end, right_start)?.is_none() {
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
        ctx: &DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        label_directions: &[Vec<bool>],
        edge_orientations: &[Option<bool>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<MeshOrientationOption>, CodecError> {
        let mut quotient = self.clone_charged(ctx)?;
        let directions = quotient.merge_label_directions_in_place(
            ctx,
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
            ctx: &DecodeContext<'_>,
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
        ) -> Result<(), CodecError> {
            let _depth = ctx.enter_nested("catia_point_assignment_walk")?;
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
                return Ok(());
            }
            if budget.is_some_and(|budget| !budget.charge()) {
                return Ok(());
            }
            let values_for = |root: usize, assigned: &[Option<usize>], used: &HashSet<usize>| -> Result<Vec<usize>, CodecError> {
                let mut values = Vec::new();
                for point in domains[root].iter().copied().filter(|point| !used.contains(point)) {
                    if value_viable(
                            root,
                            point,
                            domains,
                            edge_roots,
                            root_edges,
                            edge_candidates,
                            edge_neighbors,
                            assigned,
                            used,
                        ) {
                        crate::resource::push(ctx, &mut values, point, "catia_point_assignment_values")?;
                    }
                }
                Ok(values)
            };
            let mut propagated = Vec::new();
            let branch = loop {
                let mut values = Vec::new();
                for (root, _) in assigned.iter().enumerate().filter(|(_, point)| point.is_none()) {
                    let candidates = values_for(root, assigned, used)?;
                    crate::resource::push(ctx, &mut values, (root, candidates), "catia_point_assignment_value_rows")?;
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
                    if !crate::resource::insert_set(ctx, used, point, "catia_point_assignment_used")? {
                        dead = true;
                        break;
                    }
                    assigned[root] = Some(point);
                    crate::resource::push(ctx, &mut propagated, (root, point), "catia_point_assignment_propagated")?;
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
                    crate::resource::reserve_vec(ctx, &mut solution, assigned.len(), "catia_point_assignment_solution")?;
                    for value in assigned.iter().flatten() {
                        solution.push(*value);
                    }
                    crate::resource::push(ctx, solutions, solution, "catia_point_assignment_solutions")?;
                }
                rollback(assigned, used, propagated);
                return Ok(());
            };
            for point in values {
                assigned[root] = Some(point);
                crate::resource::insert_set(ctx, used, point, "catia_point_assignment_used")?;
                walk(
                    ctx,
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
            let root = self.union.find(node);
            if root == node {
                crate::resource::push(ctx, &mut roots, root, "catia_point_assignment_roots")?;
            }
        }
        if roots.len() != point_count {
            return Ok(PointAssignmentOutcome::Complete(Vec::new()));
        }
        let mut domains = Vec::new();
        crate::resource::reserve_vec(ctx, &mut domains, roots.len(), "catia_point_assignment_domains")?;
        let mut root_indices = HashMap::new();
        for (index, root) in roots.iter().copied().enumerate() {
            domains.push(Arc::clone(&self.domains[root]));
            crate::resource::insert_map(ctx, &mut root_indices, root, index, "catia_point_assignment_root_indices")?;
        }
        let mut edge_roots = Vec::new();
        crate::resource::reserve_vec(ctx, &mut edge_roots, edge_candidates.len(), "catia_point_assignment_edge_roots")?;
        for edge in 0..edge_candidates.len() {
            let (Some(left), Some(right)) = (
                root_indices.get(&self.union.find(edge * 2)),
                root_indices.get(&self.union.find(edge * 2 + 1)),
            ) else { return Ok(PointAssignmentOutcome::Complete(Vec::new())); };
            edge_roots.push([*left, *right]);
        }
        let mut root_edges =
            ctx.alloc_filled(roots.len(), Vec::new(), "catia point assignment root edges")?;
        for (edge_index, edge) in edge_roots.iter().enumerate() {
            crate::resource::push(
                ctx,
                &mut root_edges[edge[0]],
                edge_index,
                "catia_point_root_edge_entries",
            )?;
            if edge[1] != edge[0] {
                crate::resource::push(
                    ctx,
                    &mut root_edges[edge[1]],
                    edge_index,
                    "catia_point_root_edge_entries",
                )?;
            }
        }
        let mut edge_neighbors = Vec::new();
        crate::resource::reserve_vec(ctx, &mut edge_neighbors, edge_candidates.len(), "catia_point_assignment_neighbor_rows")?;
        for candidates in edge_candidates {
            let mut neighbors = PointNeighbors::new();
            for [left, right] in candidates {
                for (from, to) in [(*left, *right), (*right, *left)] {
                    crate::resource::admit_map_entry(ctx, &mut neighbors, &from, "catia_point_assignment_neighbor_keys")?;
                    crate::resource::insert_set(ctx, neighbors.entry(from).or_default(), to, "catia_point_assignment_neighbor_points")?;
                }
            }
            edge_neighbors.push(neighbors);
        }

        let mut solutions = Vec::new();
        let mut assigned = ctx.alloc_filled(domains.len(), None, "catia point assignment slots")?;
        let mut used = HashSet::new();
        walk(
            ctx,
            &domains,
            &edge_roots,
            &root_edges,
            edge_candidates,
            &edge_neighbors,
            &mut assigned,
            &mut used,
            &mut solutions,
            solution_limit,
            budget,
        )?;
        if budget.is_some_and(WorkBudget::exhausted) {
            Ok(PointAssignmentOutcome::Exhausted)
        } else {
            let mut completed = Vec::new();
            crate::resource::reserve_vec(ctx, &mut completed, solutions.len(), "catia_point_assignment_completed")?;
            for solution in solutions {
                let mut pairs = HashMap::new();
                for (root, point) in roots.iter().copied().zip(solution) {
                    crate::resource::insert_map(ctx, &mut pairs, root, point, "catia_point_assignment_completed_pairs")?;
                }
                completed.push(pairs);
            }
            Ok(PointAssignmentOutcome::Complete(completed))
        }
    }
}

struct DeferredFaceQuotientOptions {
    alternatives: Vec<MeshQuotient>,
    base_nodes: Vec<usize>,
}

fn materialize_deferred_quotient_option(
    ctx: &DecodeContext<'_>,
    base: &MeshQuotient,
    local: &MeshQuotient,
    base_nodes: &[usize],
    affected_edges: impl IntoIterator<Item = usize>,
    edge_candidates: &[Vec<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> Result<Option<MeshQuotient>, CodecError> {
    let mut materialized = base.clone_charged(ctx)?;
    for local_node in 0..base_nodes.len() {
        let local_root = local.union.root(local_node);
        if local_root != local_node
            && materialized
                .merge_charged(ctx, base_nodes[local_root], base_nodes[local_node])?
                .is_none()
        {
            return Ok(None);
        }
    }
    Ok(materialized
        .propagate_edge_domains(ctx, affected_edges, edge_candidates, Some(budget))?
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

    #[allow(clippy::too_many_arguments)]
    fn fill_gap(
        ctx: &DecodeContext<'_>,
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
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("catia deferred gap search")?;
        ctx.charge_work(1, "catia deferred gap search")?;
        if output.len() >= limit || budget.exhausted() {
            return Ok(());
        }
        if at == target {
            let mut quotient = quotient;
            if quotient
                .merge_charged(ctx, previous_end, gaps[gap].right_start)?
                .is_none()
            {
                return Ok(());
            }
            walk_gaps(
                ctx,
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
            )?;
            return Ok(());
        }
        let options = (missing_edges.len() - used.count_ones() as usize).saturating_mul(2);
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
                let mut next = quotient.clone_charged(ctx)?;
                if next.merge_charged(ctx, previous_end, start)?.is_none() {
                    continue;
                }
                let end_root = next.union.find(end);
                if !crate::resource::insert_set(
                    ctx,
                    &mut seen,
                    (rank, end_root, next.signature_charged(ctx)?),
                    "catia_deferred_seen_gap_states",
                )? {
                    continue;
                }
                fill_gap(
                    ctx,
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
                )?;
                if output.len() >= limit || budget.exhausted() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_gaps(
        ctx: &DecodeContext<'_>,
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
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("catia deferred gap walk")?;
        ctx.charge_work(1, "catia deferred gap walk")?;
        if output.len() >= limit || budget.exhausted() {
            return Ok(());
        }
        if gap == gaps.len() {
            if used.count_ones() as usize != missing_edges.len() {
                return Ok(());
            }
            let mut affected_edges = HashSet::new();
            for &edge in missing_edges {
                if !edge_candidates[edge].is_empty() {
                    crate::resource::insert_set(
                        ctx,
                        &mut affected_edges,
                        edge,
                        "catia_deferred_affected_edges",
                    )?;
                }
            }
            if materialize_deferred_quotient_option(
                ctx,
                base_quotient,
                quotient,
                base_nodes,
                affected_edges,
                edge_candidates,
                budget,
            )?
            .is_some()
            {
                crate::resource::push(
                    ctx,
                    output,
                    quotient.clone_charged(ctx)?,
                    "catia_deferred_alternatives",
                )?;
            }
            return Ok(());
        }
        let remaining_edges = missing_edges.len() - used.count_ones() as usize;
        let remaining_gaps = gaps.len() - gap - 1;
        let minimum = 1;
        let maximum = gaps[gap]
            .capacity
            .min(remaining_edges.saturating_sub(remaining_gaps));
        if maximum < minimum {
            return Ok(());
        }
        if maximum > minimum && !budget.charge_by(maximum - minimum + 1) {
            return Ok(());
        }
        for target in minimum..=maximum {
            fill_gap(
                ctx,
                gaps,
                gap,
                0,
                target,
                used,
                gaps[gap].left_end,
                missing_edges,
                missing_nodes,
                edge_candidates,
                quotient.clone_charged(ctx)?,
                base_quotient,
                base_nodes,
                output,
                limit,
                budget,
            )?;
            if output.len() >= limit || budget.exhausted() {
                return Ok(());
            }
        }
        Ok(())
    }

    if domain.missing_edges.len() > u64::BITS as usize {
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
            crate::resource::push(
                ctx,
                &mut gaps,
                Gap {
                    left_end,
                    right_start,
                    capacity,
                },
                "catia_deferred_gaps",
            )?;
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
            crate::resource::push(
                ctx,
                &mut base_nodes,
                quotient.union.root(node),
                "catia_deferred_base_nodes",
            )?;
        }
    }
    for &edge in &domain.missing_edges {
        for node in [edge * 2, edge * 2 + 1] {
            crate::resource::push(
                ctx,
                &mut base_nodes,
                quotient.union.root(node),
                "catia_deferred_base_nodes",
            )?;
        }
    }
    base_nodes.sort_unstable();
    base_nodes.dedup();
    let mut local_by_base = HashMap::new();
    for (local, &base) in base_nodes.iter().enumerate() {
        crate::resource::insert_map(
            ctx,
            &mut local_by_base,
            base,
            local,
            "catia_deferred_local_index",
        )?;
    }
    for gap in &mut gaps {
        gap.left_end = local_by_base[&quotient.union.root(gap.left_end)];
        gap.right_start = local_by_base[&quotient.union.root(gap.right_start)];
    }
    let mut missing_nodes = Vec::new();
    for &edge in &domain.missing_edges {
        crate::resource::push(
            ctx,
            &mut missing_nodes,
            [
                local_by_base[&quotient.union.root(edge * 2)],
                local_by_base[&quotient.union.root(edge * 2 + 1)],
            ],
            "catia_deferred_missing_nodes",
        )?;
    }
    let mut local_domains = Vec::new();
    for &root in &base_nodes {
        crate::resource::push(
            ctx,
            &mut local_domains,
            quotient.domains[root].clone(),
            "catia_deferred_local_domains",
        )?;
    }
    let local_quotient = MeshQuotient::new_charged(ctx, local_domains)?;
    let mut ranked_gaps = Vec::new();
    for gap in gaps {
        let single_edge_options = if gap.capacity == 1 {
            let mut count = 0usize;
            for (rank, _) in domain.missing_edges.iter().enumerate() {
                for reversed in [false, true] {
                    let start = missing_nodes[rank][usize::from(reversed)];
                    let end = missing_nodes[rank][usize::from(!reversed)];
                    let mut trial = local_quotient.clone_charged(ctx)?;
                    if trial.merge_charged(ctx, gap.left_end, start)?.is_some()
                        && trial.merge_charged(ctx, end, gap.right_start)?.is_some()
                    {
                        count += 1;
                    }
                }
            }
            count
        } else {
            usize::MAX
        };
        crate::resource::push(
            ctx,
            &mut ranked_gaps,
            (gap, (gap.capacity, single_edge_options)),
            "catia_deferred_ranked_gaps",
        )?;
    }
    ranked_gaps.sort_unstable_by_key(|(_, key)| *key);
    let mut gaps = Vec::new();
    for (gap, _) in ranked_gaps {
        crate::resource::push(ctx, &mut gaps, gap, "catia_deferred_sorted_gaps")?;
    }
    let mut output = Vec::new();
    walk_gaps(
        ctx,
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
    )?;
    Ok(
        (!budget.exhausted()).then_some(DeferredFaceQuotientOptions {
            alternatives: output,
            base_nodes,
        }),
    )
}

fn propagate_common_deferred_quotients(
    ctx: &DecodeContext<'_>,
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
            crate::resource::push(
                ctx,
                &mut signature,
                alternative.union.find(node),
                "catia_deferred_common_signature",
            )?;
        }
        if let Some(members) = equivalence_classes.get_mut(&signature) {
            crate::resource::push(ctx, members, node, "catia_deferred_common_members")?;
        } else {
            let mut members = Vec::new();
            crate::resource::push(ctx, &mut members, node, "catia_deferred_common_members")?;
            crate::resource::insert_map(
                ctx,
                &mut equivalence_classes,
                signature,
                members,
                "catia_deferred_common_classes",
            )?;
        }
    }
    for nodes in equivalence_classes.into_values() {
        let Some((&representative, rest)) = nodes.split_first() else {
            continue;
        };
        for &node in rest {
            if quotient
                .merge_charged(
                    ctx,
                    options.base_nodes[representative],
                    options.base_nodes[node],
                )?
                .is_none()
            {
                return Ok(None);
            }
        }
    }
    for local in 0..node_count {
        let mut allowed = HashSet::new();
        for alternative in &mut options.alternatives {
            let root = alternative.union.find(local);
            for &point in alternative.domains[root].iter() {
                crate::resource::insert_set(
                    ctx,
                    &mut allowed,
                    point,
                    "catia_deferred_common_allowed",
                )?;
            }
        }
        let root = quotient.union.find(options.base_nodes[local]);
        let mut narrowed = HashSet::new();
        for &point in quotient.domains[root].intersection(&allowed) {
            crate::resource::insert_set(
                ctx,
                &mut narrowed,
                point,
                "catia_deferred_common_narrowed",
            )?;
        }
        if narrowed.is_empty() {
            return Ok(None);
        }
        quotient.domains[root] = Arc::new(narrowed);
    }
    let mut affected_edges = HashSet::new();
    for node in options.base_nodes {
        let root = quotient.union.find(node);
        for edge in quotient.members(root).iter().map(|node| node / 2) {
            if !edge_candidates[edge].is_empty() {
                crate::resource::insert_set(
                    ctx,
                    &mut affected_edges,
                    edge,
                    "catia_deferred_common_edges",
                )?;
            }
        }
    }
    Ok(quotient
        .propagate_edge_domains(ctx, affected_edges, edge_candidates, Some(budget))?
        .then_some(()))
}

fn common_supported_corner_equations(
    ctx: &DecodeContext<'_>,
    quotient: &mut MeshQuotient,
    assignments: &[MeshFaceBoundaryAssignment],
    budget: &WorkBudget<'_>,
) -> Result<Option<HashSet<[usize; 2]>>, CodecError> {
    fn compatible(quotient: &MeshQuotient, left: usize, right: usize) -> bool {
        let left = quotient.union.root(left);
        let right = quotient.union.root(right);
        left == right || !quotient.domains[left].is_disjoint(&quotient.domains[right])
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
                let directions = match (|| -> Result<Vec<Vec<bool>>, CodecError> {
                    let mut directions = Vec::new();
                    for use_ in boundary {
                        let mut options = Vec::new();
                        if let Some(reversed) = use_.reversed {
                            crate::resource::push(
                                ctx,
                                &mut options,
                                reversed,
                                "catia_boundary_direction_options",
                            )?;
                        } else {
                            crate::resource::push(
                                ctx,
                                &mut options,
                                false,
                                "catia_boundary_direction_options",
                            )?;
                            crate::resource::push(
                                ctx,
                                &mut options,
                                true,
                                "catia_boundary_direction_options",
                            )?;
                        }
                        crate::resource::push(
                            ctx,
                            &mut directions,
                            options,
                            "catia_boundary_directions",
                        )?;
                    }
                    Ok(directions)
                })() {
                    Ok(directions) => directions,
                    Err(error) => return Some(Err(error)),
                };
                let mut supported = match (|| -> Result<Vec<Vec<Vec<bool>>>, CodecError> {
                    let mut supported = Vec::new();
                    for index in 0..boundary.len() {
                        let width = directions[(index + 1) % boundary.len()].len();
                        let height = directions[index].len();
                        let mut grid = Vec::new();
                        for _ in 0..height {
                            let row = ctx.alloc_filled(width, false, "catia_boundary_dir_row")?;
                            crate::resource::push(ctx, &mut grid, row, "catia_boundary_dir_grid")?;
                        }
                        crate::resource::push(
                            ctx,
                            &mut supported,
                            grid,
                            "catia_boundary_supported_grids",
                        )?;
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
                            crate::resource::push(
                                ctx,
                                &mut forward,
                                row,
                                "catia_boundary_forward_rows",
                            )?;
                        }
                        Ok(forward)
                    })() {
                        Ok(forward) => forward,
                        Err(error) => return Some(Err(error)),
                    };
                    forward[0][first] = true;
                    for index in 0..boundary.len().saturating_sub(1) {
                        for left in 0..directions[index].len() {
                            if !forward[index][left] {
                                continue;
                            }
                            for right in 0..directions[index + 1].len() {
                                let left_node =
                                    port(boundary[index], directions[index][left], true)?;
                                let right_node =
                                    port(boundary[index + 1], directions[index + 1][right], false)?;
                                if compatible(quotient, left_node, right_node) {
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
                            crate::resource::push(
                                ctx,
                                &mut backward,
                                row,
                                "catia_boundary_backward_rows",
                            )?;
                        }
                        Ok(backward)
                    })() {
                        Ok(backward) => backward,
                        Err(error) => return Some(Err(error)),
                    };
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
                                    let Some(right_node) = port(
                                        boundary[index + 1],
                                        directions[index + 1][right],
                                        false,
                                    ) else {
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
                                    let right_node = port(
                                        boundary[index + 1],
                                        directions[index + 1][right],
                                        false,
                                    )?;
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
                                let equation = if left <= right {
                                    [left, right]
                                } else {
                                    [right, left]
                                };
                                if let Err(error) = crate::resource::insert_set(
                                    ctx,
                                    &mut equations,
                                    equation,
                                    "catia_boundary_corner_equations",
                                ) {
                                    return Some(Err(error));
                                }
                            }
                        }
                    }
                    if equations.len() == 1 {
                        if let Some(equation) = equations.into_iter().next() {
                            if let Err(error) = crate::resource::insert_set(
                                ctx,
                                &mut forced,
                                equation,
                                "catia_boundary_forced_corners",
                            ) {
                                return Some(Err(error));
                            }
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
    ctx: &DecodeContext<'_>,
    mut alternatives: Vec<MeshQuotient>,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
) -> Result<Option<()>, CodecError> {
    let node_count = quotient.union.len();
    let mut equivalence_classes = HashMap::<Vec<usize>, Vec<usize>>::new();
    for node in 0..node_count {
        let mut signature = Vec::new();
        for alternative in &mut alternatives {
            crate::resource::push(
                ctx,
                &mut signature,
                alternative.union.find(node),
                "catia_common_quotient_signature",
            )?;
        }
        if let Some(members) = equivalence_classes.get_mut(&signature) {
            crate::resource::push(ctx, members, node, "catia_common_quotient_members")?;
        } else {
            let mut members = Vec::new();
            crate::resource::push(ctx, &mut members, node, "catia_common_quotient_members")?;
            crate::resource::insert_map(
                ctx,
                &mut equivalence_classes,
                signature,
                members,
                "catia_common_quotient_classes",
            )?;
        }
    }
    for nodes in equivalence_classes.into_values() {
        let Some((&representative, rest)) = nodes.split_first() else {
            continue;
        };
        for &node in rest {
            if quotient.merge_charged(ctx, representative, node)?.is_none() {
                return Ok(None);
            }
        }
    }

    let mut roots = Vec::new();
    for node in 0..node_count {
        if quotient.union.find(node) == node {
            crate::resource::push(ctx, &mut roots, node, "catia_common_quotient_roots")?;
        }
    }
    for root in roots {
        let representative = quotient.members(root)[0];
        let mut allowed = HashSet::new();
        for alternative in &mut alternatives {
            let alternative_root = alternative.union.find(representative);
            for &point in alternative.domains[alternative_root].iter() {
                crate::resource::insert_set(
                    ctx,
                    &mut allowed,
                    point,
                    "catia_common_quotient_allowed",
                )?;
            }
        }
        let mut narrowed = HashSet::new();
        for &point in quotient.domains[root].intersection(&allowed) {
            crate::resource::insert_set(
                ctx,
                &mut narrowed,
                point,
                "catia_common_quotient_narrowed",
            )?;
        }
        if narrowed.is_empty() {
            return Ok(None);
        }
        quotient.domains[root] = Arc::new(narrowed);
    }
    Ok(quotient
        .edge_domains_viable(ctx, edge_candidates)?
        .then_some(()))
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
        let mut face_order = match ctx.alloc_filled(domains.len(), 0usize, "catia_ordered_face_order") {
            Ok(order) => order,
            Err(error) => return Some(Err(error)),
        };
        for (face, slot) in face_order.iter_mut().enumerate() {
            *slot = face;
        }
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
                            let merged = match quotient.merge_charged(ctx, left_node, right_node) {
                                Ok(Some(root)) => root,
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            };
                            if let Err(error) = crate::resource::push(
                                ctx,
                                &mut merged_nodes,
                                merged,
                                "catia_ordered_merged_nodes",
                            ) {
                                return Some(Err(error));
                            }
                        }
                    }
                    let affected_edges = match quotient.affected_edges_for_nodes(
                        ctx,
                        &merged_nodes,
                        edge_candidates,
                    ) {
                        Ok(edges) => edges,
                        Err(error) => return Some(Err(error)),
                    };
                    match quotient.propagate_edge_domains(
                        ctx,
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
                            ctx,
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
                        let merged = match quotient.merge_charged(ctx, left, right) {
                            Ok(Some(root)) => root,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                        if let Err(error) = crate::resource::push(
                            ctx,
                            &mut merged_nodes,
                            merged,
                            "catia_ordered_merged_nodes",
                        ) {
                            return Some(Err(error));
                        }
                    }
                    let affected_edges = match quotient.affected_edges_for_nodes(
                        ctx,
                        &merged_nodes,
                        edge_candidates,
                    ) {
                        Ok(edges) => edges,
                        Err(error) => return Some(Err(error)),
                    };
                    match quotient.propagate_edge_domains(
                        ctx,
                        affected_edges,
                        edge_candidates,
                        Some(budget),
                    ) {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(error) => return Some(Err(error)),
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
                    let options = match quotient.assignment_options_limited(
                        ctx,
                        assignment,
                        edge_candidates,
                        &HashSet::new(),
                        MAX_FACE_OPTIONS + 1,
                        Some(&face_budget),
                    ) {
                        Ok(options) => options,
                        Err(error) => return Some(Err(error)),
                    };
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
                match propagate_common_full_quotients(ctx, alternatives, edge_candidates, quotient)
                {
                    Ok(Some(())) => {}
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                }
            }
            if quotient.monotone_measure() == before {
                return Some(Ok(()));
            }
        }
    })()
    .transpose()
}

fn mesh_boundary_domain_edges(
    ctx: &DecodeContext<'_>,
    domain: &MeshFaceBoundaryDomain,
) -> Result<Vec<usize>, CodecError> {
    let mut edges = Vec::new();
    match domain {
        MeshFaceBoundaryDomain::Ordered(assignments) => {
            for assignment in assignments {
                for use_ in assignment.boundaries.iter().flatten() {
                    crate::resource::push(
                        ctx,
                        &mut edges,
                        use_.edge,
                        "catia_boundary_domain_edges",
                    )?;
                }
            }
        }
        MeshFaceBoundaryDomain::UnorderedFullCycle(ordered_edges) => {
            edges = crate::resource::copy_slice(ctx, ordered_edges, "catia_boundary_domain_edges")?;
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            edges = crate::resource::copy_slice(
                ctx,
                &domain.missing_edges,
                "catia_boundary_domain_edges",
            )?;
            for cycle in &domain.cycles {
                for (use_, _) in &cycle.exact_uses {
                    crate::resource::push(
                        ctx,
                        &mut edges,
                        use_.edge,
                        "catia_boundary_domain_edges",
                    )?;
                }
            }
        }
    }
    edges.sort_unstable();
    edges.dedup();
    Ok(edges)
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
            self.ctx.charge_work(1, "catia unordered boundary search")?;
            if !self.budget.charge() {
                return Ok(false);
            }
            if boundary.len() == self.edges.len() {
                if self.compatible.contains(&(previous_end, first_start)) {
                    let completed = crate::resource::copy_slice(
                        self.ctx,
                        boundary,
                        "catia_unordered_completed_boundary",
                    )?;
                    let mut boundaries = Vec::new();
                    crate::resource::push(
                        self.ctx,
                        &mut boundaries,
                        completed,
                        "catia_unordered_boundary_rows",
                    )?;
                    crate::resource::push(
                        self.ctx,
                        &mut self.assignments,
                        MeshFaceBoundaryAssignment { boundaries },
                        "catia_unordered_assignments",
                    )?;
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
                    crate::resource::push(
                        self.ctx,
                        boundary,
                        MeshBoundaryEdgeCandidate {
                            edge,
                            start: 0,
                            end: 0,
                            reversed: Some(reversed),
                        },
                        "catia_unordered_search_boundary",
                    )?;
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

    if edges.is_empty() || edges.len() > u64::BITS as usize {
        return Ok(None);
    }
    let edge_count = edges.len();
    let mut edges = crate::resource::copy_slice(ctx, edges, "catia_unordered_sorted_edges")?;
    edges.sort_unstable();
    edges.dedup();
    if edges.len() != edge_count {
        return Ok(None);
    }
    let mut quotient = quotient.clone_charged(ctx)?;
    let mut nodes = Vec::new();
    for &edge in &edges {
        crate::resource::push(ctx, &mut nodes, edge * 2, "catia_unordered_nodes")?;
        crate::resource::push(ctx, &mut nodes, edge * 2 + 1, "catia_unordered_nodes")?;
    }
    let mut compatible = HashSet::new();
    for &left in &nodes {
        let left_root = quotient.union.find(left);
        for &right in &nodes {
            let right_root = quotient.union.find(right);
            if left_root == right_root
                || !quotient.domains[left_root].is_disjoint(&quotient.domains[right_root])
            {
                crate::resource::insert_set(
                    ctx,
                    &mut compatible,
                    (left, right),
                    "catia_unordered_compatible",
                )?;
            }
        }
    }
    let first = edges[0];
    let first_start = first * 2;
    let mut boundary = Vec::new();
    crate::resource::push(
        ctx,
        &mut boundary,
        MeshBoundaryEdgeCandidate {
            edge: first,
            start: 0,
            end: 0,
            reversed: Some(false),
        },
        "catia_unordered_search_boundary",
    )?;
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
    let mut next = Vec::new();
    let mut signatures = HashSet::new();
    let domain_edges = mesh_boundary_domain_edges(ctx, domain)?;
    for (state, oriented_edges) in states {
        let remaining = limit.saturating_add(1).saturating_sub(next.len());
        if remaining == 0 {
            return Ok(None);
        }
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
                        crate::resource::push(
                            ctx,
                            &mut candidates,
                            quotient,
                            "catia_component_candidates",
                        )?;
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
                    let mut candidates = Vec::new();
                    crate::resource::push(
                        ctx,
                        &mut candidates,
                        state.clone_charged(ctx)?,
                        "catia_component_candidates",
                    )?;
                    candidates
                } else {
                    let mut affected_edges = HashSet::new();
                    for &edge in domain_edges
                        .iter()
                        .filter(|edge| !edge_candidates[**edge].is_empty())
                    {
                        crate::resource::insert_set(
                            ctx,
                            &mut affected_edges,
                            edge,
                            "catia_component_affected_edges",
                        )?;
                    }
                    let mut candidates = Vec::new();
                    for local in &options.alternatives {
                        if let Some(candidate) = materialize_deferred_quotient_option(
                            ctx,
                            state,
                            local,
                            &options.base_nodes,
                            affected_edges.iter().copied(),
                            edge_candidates,
                            budget,
                        )? {
                            crate::resource::push(
                                ctx,
                                &mut candidates,
                                candidate,
                                "catia_component_candidates",
                            )?;
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
                        crate::resource::push(
                            ctx,
                            &mut candidates,
                            quotient,
                            "catia_component_candidates",
                        )?;
                    }
                }
                candidates
            }
        };
        for mut candidate in candidates {
            let mut next_oriented = HashSet::new();
            for &edge in oriented_edges.iter().chain(domain_edges.iter()) {
                crate::resource::insert_set(
                    ctx,
                    &mut next_oriented,
                    edge,
                    "catia_component_oriented_edges",
                )?;
            }
            if !budget.charge_by(
                candidate
                    .signature_work()
                    .saturating_add(work_units(next_oriented.len())),
            ) {
                return Ok(None);
            }
            let mut oriented_signature = Vec::new();
            for &edge in &next_oriented {
                crate::resource::push(
                    ctx,
                    &mut oriented_signature,
                    edge,
                    "catia_component_oriented_signature",
                )?;
            }
            oriented_signature.sort_unstable();
            if crate::resource::insert_set(
                ctx,
                &mut signatures,
                (candidate.signature_charged(ctx)?, oriented_signature),
                "catia_component_signatures",
            )? {
                crate::resource::push(
                    ctx,
                    &mut next,
                    (candidate, next_oriented),
                    "catia_component_states",
                )?;
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
        let edges = mesh_boundary_domain_edges(ctx, domain)?;
        if edges.iter().any(|edge| edge_candidates[*edge].is_empty()) {
            crate::resource::push(ctx, &mut active_faces, face, "catia_component_active_faces")?;
        }
        crate::resource::push(ctx, &mut domain_edges, edges, "catia_component_domain_rows")?;
    }
    let mut active_index = HashMap::new();
    for (index, &face) in active_faces.iter().enumerate() {
        crate::resource::insert_map(
            ctx,
            &mut active_index,
            face,
            index,
            "catia_component_active_index",
        )?;
    }
    let mut components = UnionFind::charged(ctx, active_faces.len(), "catia_component_union")?;
    let mut edge_owner = HashMap::<usize, usize>::new();
    for &face in &active_faces {
        let index = active_index[&face];
        for &edge in &domain_edges[face] {
            if let Some(previous) = crate::resource::insert_map(
                ctx,
                &mut edge_owner,
                edge,
                index,
                "catia_component_edge_owner",
            )? {
                components.union(previous, index);
            }
        }
    }
    let mut faces_by_component = HashMap::<usize, (usize, Vec<usize>)>::new();
    for face in active_faces {
        let root = components.find(active_index[&face]);
        // `active_faces` is built by an ascending enumeration, so the face that
        // creates a component's entry is that component's smallest face.
        if let Some((_, faces)) = faces_by_component.get_mut(&root) {
            crate::resource::push(ctx, faces, face, "catia_component_face_members")?;
        } else {
            let mut faces = Vec::new();
            crate::resource::push(ctx, &mut faces, face, "catia_component_face_members")?;
            crate::resource::insert_map(
                ctx,
                &mut faces_by_component,
                root,
                (face, faces),
                "catia_component_face_groups",
            )?;
        }
    }
    let mut face_components = Vec::new();
    for group in faces_by_component.into_values() {
        crate::resource::push(ctx, &mut face_components, group, "catia_component_groups")?;
    }
    face_components.sort_by_key(|(smallest_face, _)| *smallest_face);

    for (_, mut faces) in face_components {
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
                    let key = face_key(**face);
                    (key.0, usize::MAX - shared, key)
                })
                .map(|(index, _)| index);
            let Some(next) = next else {
                return Ok(None);
            };
            let face = faces.swap_remove(next);
            for &edge in &domain_edges[face] {
                crate::resource::insert_set(
                    ctx,
                    &mut selected_edges,
                    edge,
                    "catia_component_selected_edges",
                )?;
            }
            crate::resource::push(
                ctx,
                &mut ordered_faces,
                face,
                "catia_component_ordered_faces",
            )?;
        }
        let budget = WorkBudget::new(MAX_COMPONENT_OPERATIONS);
        for _ in 0..MAX_COMPONENT_ROUNDS {
            let before = quotient.monotone_measure();
            let mut cursor = 0usize;
            while cursor < ordered_faces.len() {
                let mut states = Vec::new();
                crate::resource::push(
                    ctx,
                    &mut states,
                    (quotient.clone_charged(ctx)?, HashSet::<usize>::new()),
                    "catia_component_states",
                )?;
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
                    crate::resource::push(
                        ctx,
                        &mut alternatives,
                        state,
                        "catia_component_alternatives",
                    )?;
                }
                if propagate_common_full_quotients(ctx, alternatives, edge_candidates, quotient)?
                    .is_none()
                {
                    return Ok(None);
                }
                cursor += processed;
            }
            if quotient.monotone_measure() == before {
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

fn orientation_fingerprint(quotient: &MeshQuotient, directions: &[Vec<bool>]) -> u64 {
    let mut hasher = DefaultHasher::new();
    quotient.union.len().hash(&mut hasher);
    for node in 0..quotient.union.len() {
        let root = quotient.union.root(node);
        quotient.members(root).iter().min().hash(&mut hasher);
        let domain = &quotient.domains[root];
        domain.len().hash(&mut hasher);
        let mut xor = 0u64;
        let mut sum = 0u64;
        for point in domain.iter() {
            let mut point_hasher = DefaultHasher::new();
            point.hash(&mut point_hasher);
            let value = point_hasher.finish();
            xor ^= value;
            sum = sum.wrapping_add(value);
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
    hasher.finish()
}

fn orientation_options_equivalent(
    left_quotient: &MeshQuotient,
    left_directions: &[Vec<bool>],
    right_quotient: &MeshQuotient,
    right_directions: &[Vec<bool>],
) -> bool {
    if left_quotient.union.len() != right_quotient.union.len()
        || left_directions.len() != right_directions.len()
    {
        return false;
    }
    for node in 0..left_quotient.union.len() {
        let left_root = left_quotient.union.root(node);
        let right_root = right_quotient.union.root(node);
        if left_quotient.members(left_root).iter().min()
            != right_quotient.members(right_root).iter().min()
            || left_quotient.domains[left_root] != right_quotient.domains[right_root]
        {
            return false;
        }
    }
    left_directions
        .iter()
        .zip(right_directions)
        .all(|(left, right)| {
            left.len() == right.len()
                && (0..left.len()).all(|index| {
                    canonical_direction_bit(left, index) == canonical_direction_bit(right, index)
                })
        })
}

fn admit_orientation_option(
    ctx: &DecodeContext<'_>,
    seen: &mut HashMap<u64, Vec<usize>>,
    output: &[MeshOrientationOption],
    directions: &[Vec<bool>],
    quotient: &MeshQuotient,
) -> Result<bool, CodecError> {
    let work = u64::try_from(quotient.union.len())
        .map_err(|_| ctx.refuse_codec_limit("catia_orientation_dedup_work", u64::MAX, u64::MAX))?;
    ctx.charge_work(work, "catia_orientation_dedup_work")?;
    let fingerprint = orientation_fingerprint(quotient, directions);
    if let Some(indices) = seen.get(&fingerprint) {
        for &index in indices {
            ctx.charge_work(1, "catia_orientation_dedup_compare")?;
            let (prior_directions, prior_quotient) = &output[index];
            if orientation_options_equivalent(
                quotient,
                directions,
                prior_quotient,
                prior_directions,
            ) {
                return Ok(false);
            }
        }
    }
    crate::resource::admit_map_entry(
        ctx,
        seen,
        &fingerprint,
        "catia_orientation_fingerprint_keys",
    )?;
    crate::resource::push(
        ctx,
        seen.entry(fingerprint).or_default(),
        output.len(),
        "catia_orientation_fingerprint_indices",
    )?;
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
            normalized_pair.sort_unstable();
        }
        row.sort_unstable();
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
            crate::resource::push(ctx, &mut ordered, (left, right), "catia_edge_class_ordered_pairs")?;
        }
    }
    Ok(Some(EdgeClassSearchConstraint { active, ordered }))
}

#[test]
fn edge_class_ordered_pairs_refuse_before_growth() {
    let choices = vec![vec![[0, 1], [1, 2]], vec![[0, 1], [1, 2]]];
    let mut refused = HashSet::new();
    for cap in 0..32 {
        match crate::test_support::with_collection_limit(cap, |ctx| edge_class_search_constraint(ctx, &[0, 0], &choices)) {
            Err(CodecError::ResourceLimit(limit)) => { refused.insert(limit.operation); }
            Ok(Some(_)) => break,
            Ok(None) => panic!("identical edge classes must have a search order"),
            Err(error) => panic!("unexpected edge class refusal: {error}"),
        }
    }
    assert!(refused.contains("catia_edge_class_ordered_pairs"));
    assert_eq!(crate::test_support::with_service_context(|ctx| edge_class_search_constraint(ctx, &[0, 0], &choices))
        .expect("service resource budget").expect("matching edge classes").ordered, vec![(0, 1)]);
}

fn changed_quotient_edges(
    ctx: &DecodeContext<'_>,
    left: &MeshQuotient,
    right: &MeshQuotient,
) -> Result<HashSet<usize>, CodecError> {
    let mut left = left.clone_charged(ctx)?;
    let mut right = right.clone_charged(ctx)?;
    let mut changed = HashSet::new();
    for node in 0..left.union.len() {
        let left_root = left.union.find(node);
        let right_root = right.union.find(node);
        if left_root != right_root
            || left.members(left_root) != right.members(right_root)
            || left.domains[left_root] != right.domains[right_root]
        {
            crate::resource::insert_set(
                ctx,
                &mut changed,
                node / 2,
                "catia_changed_quotient_edges",
            )?;
        }
    }
    Ok(changed)
}

#[test]
fn changed_quotient_edges_refuse_before_result_set_growth() {
    let points = Arc::new(HashSet::from([0]));
    let left = MeshQuotient::new(vec![Arc::clone(&points), Arc::clone(&points)]);
    let mut right = left.clone();
    assert!(right.merge(0, 1).is_some());
    assert_eq!(
        crate::test_support::with_service_context(|ctx| changed_quotient_edges(ctx, &left, &right))
            .expect("service resource budget"),
        HashSet::from([0])
    );
    let mut refused = HashSet::new();
    for cap in 0..64 {
        match crate::test_support::with_collection_limit(cap, |ctx| {
            changed_quotient_edges(ctx, &left, &right)
        }) {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(_) => break,
            _ => panic!("unexpected changed quotient result"),
        }
    }
    assert!(refused.contains("catia_changed_quotient_edges"));
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
    outcome: SearchOutcome<(StandardTopology, Vec<usize>)>,
    face_equation_cache: MeshFaceEquationCache,
}

fn possible_face_equations(
    ctx: &DecodeContext<'_>,
    faces: &[Vec<MeshFaceBoundaryAssignment>],
) -> Result<Vec<Vec<[usize; 2]>>, CodecError> {
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
                            ctx.charge_work(1, "catia_possible_face_equation_work")?;
                            crate::resource::insert_set(
                                ctx,
                                &mut equations,
                                if left <= right {
                                    [left, right]
                                } else {
                                    [right, left]
                                },
                                "catia_possible_face_equation_keys",
                            )?;
                        }
                    }
                }
            }
        }
        let mut face_equations = Vec::new();
        for equation in equations {
            crate::resource::push(
                ctx,
                &mut face_equations,
                equation,
                "catia_possible_face_equation_values",
            )?;
        }
        face_equations.sort_unstable();
        crate::resource::push(
            ctx,
            &mut faces_equations,
            face_equations,
            "catia_possible_face_equation_faces",
        )?;
    }
    Ok(faces_equations)
}

fn possible_face_choices_with_limit(
    ctx: &DecodeContext<'_>,
    faces: &[Vec<MeshFaceBoundaryAssignment>],
    face_equations: &[Vec<[usize; 2]>],
    limit: usize,
    faces_choices: &mut Vec<Vec<Vec<[usize; 2]>>>,
) -> Result<bool, CodecError> {
    let budget = WorkBudget::new(limit);
    for (assignments, fallback) in faces.iter().zip(face_equations) {
        let mut choices = HashSet::new();
        for assignment in assignments {
            if !budget.charge() {
                return Ok(false);
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
                let copied = crate::resource::copy_slice(
                    ctx,
                    fallback,
                    "catia_possible_face_choice_fallback_equations",
                )?;
                crate::resource::insert_set(
                    ctx,
                    &mut choices,
                    copied,
                    "catia_possible_face_choice_keys",
                )?;
                break;
            };
            'masks: for mask in 0..combinations {
                if !budget.charge() {
                    return Ok(false);
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
                        crate::resource::push(
                            ctx,
                            &mut row,
                            direction,
                            "catia_possible_face_choice_directions",
                        )?;
                    }
                    crate::resource::push(
                        ctx,
                        &mut directions,
                        row,
                        "catia_possible_face_choice_direction_rows",
                    )?;
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
                        crate::resource::push(
                            ctx,
                            &mut equations,
                            if left <= right {
                                [left, right]
                            } else {
                                [right, left]
                            },
                            "catia_possible_face_choice_equations",
                        )?;
                    }
                }
                equations.sort_unstable();
                equations.dedup();
                crate::resource::insert_set(
                    ctx,
                    &mut choices,
                    equations,
                    "catia_possible_face_choice_keys",
                )?;
            }
        }
        let mut face_choices = Vec::new();
        for choice in choices {
            crate::resource::push(
                ctx,
                &mut face_choices,
                choice,
                "catia_possible_face_choice_values",
            )?;
        }
        face_choices.sort_unstable();
        crate::resource::push(
            ctx,
            faces_choices,
            face_choices,
            "catia_possible_face_choice_faces",
        )?;
    }
    Ok(!budget.exhausted())
}

#[cfg(test)]
fn possible_face_choices(
    ctx: &DecodeContext<'_>,
    faces: &[Vec<MeshFaceBoundaryAssignment>],
    face_equations: &[Vec<[usize; 2]>],
) -> Vec<Vec<Vec<[usize; 2]>>> {
    let mut choices = Vec::new();
    assert!(
        possible_face_choices_with_limit(ctx, faces, face_equations, usize::MAX, &mut choices)
            .expect("service resource budget"),
        "unbounded test face-choice materialization"
    );
    choices
}

fn deduplicate_mesh_quotient_assignments(
    ctx: &DecodeContext<'_>,
    faces: &mut [Vec<MeshFaceBoundaryAssignment>],
) -> Result<(), CodecError> {
    fn canonical_cycle(
        ctx: &DecodeContext<'_>,
        boundary: &[MeshBoundaryEdgeCandidate],
    ) -> Result<Vec<(usize, Option<bool>)>, CodecError> {
        let mut forward = Vec::new();
        let mut reversed = Vec::new();
        for use_ in boundary {
            crate::resource::push(
                ctx,
                &mut forward,
                (use_.edge, use_.reversed),
                "catia_mesh_quotient_cycle_forward",
            )?;
        }
        for use_ in boundary.iter().rev() {
            crate::resource::push(
                ctx,
                &mut reversed,
                (use_.edge, use_.reversed.map(|value| !value)),
                "catia_mesh_quotient_cycle_reverse",
            )?;
        }
        let mut best = None;
        for values in [&forward, &reversed] {
            for start in 0..values.len() {
                let work = u64::try_from(values.len()).map_err(|_| {
                    ctx.refuse_codec_limit("catia_mesh_quotient_cycle_compare", u64::MAX, u64::MAX)
                })?;
                ctx.charge_work(work, "catia_mesh_quotient_cycle_compare")?;
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
                crate::resource::push(
                    ctx,
                    &mut canonical,
                    value,
                    "catia_mesh_quotient_canonical_cycle",
                )?;
            }
        }
        Ok(canonical)
    }

    for assignments in faces {
        let mut seen = HashSet::new();
        let mut refusal = None;
        assignments.retain(|assignment| {
            if refusal.is_some() {
                return true;
            }
            let result = (|| -> Result<bool, CodecError> {
                let mut signature = Vec::new();
                for boundary in &assignment.boundaries {
                    let cycle = canonical_cycle(ctx, boundary)?;
                    crate::resource::push(
                        ctx,
                        &mut signature,
                        cycle,
                        "catia_mesh_quotient_signature_boundaries",
                    )?;
                }
                signature.sort_unstable();
                crate::resource::insert_set(
                    ctx,
                    &mut seen,
                    signature,
                    "catia_mesh_quotient_seen_assignments",
                )
            })();
            match result {
                Ok(retain) => retain,
                Err(error) => {
                    refusal = Some(error);
                    true
                }
            }
        });
        if let Some(error) = refusal {
            return Err(error);
        }
    }
    Ok(())
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
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    budget: Option<&WorkBudget<'_>>,
    candidates: impl Fn(usize) -> Option<MeshEndpointCandidates<'a>>,
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Result<Option<MeshEndpointPairSupport>, CodecError> {
    const MAX_LOCAL_ENDPOINT_STATES: usize = 65_536;

    type EndpointRelation = BTreeMap<usize, BTreeSet<usize>>;

    fn insert_relation(
        ctx: &DecodeContext<'_>,
        relation: &mut EndpointRelation,
        start: usize,
        end: usize,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if !relation.contains_key(&start) {
            ctx.charge_collection_items(1, operation)?;
        }
        let ends = relation.entry(start).or_default();
        if !ends.contains(&end) {
            ctx.charge_collection_items(1, operation)?;
        }
        Ok(ends.insert(end))
    }

    fn clone_relation(
        ctx: &DecodeContext<'_>,
        relation: &EndpointRelation,
        operation: &'static str,
    ) -> Result<EndpointRelation, CodecError> {
        let mut copy = EndpointRelation::new();
        for (&start, ends) in relation {
            for &end in ends {
                insert_relation(ctx, &mut copy, start, end, operation)?;
            }
        }
        Ok(copy)
    }

    fn compose_relations(
        ctx: &DecodeContext<'_>,
        left: &EndpointRelation,
        right: &EndpointRelation,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<EndpointRelation>, CodecError> {
        let mut composed = EndpointRelation::new();
        let mut state_count = 0usize;
        for (&start, middles) in left {
            for middle in middles {
                let Some(ends) = right.get(middle) else {
                    continue;
                };
                for &end in ends {
                    if budget.is_some_and(|budget| !budget.charge()) {
                        return Ok(None);
                    }
                    if insert_relation(
                        ctx,
                        &mut composed,
                        start,
                        end,
                        "catia_endpoint_relation_composition",
                    )? {
                        let Some(next_count) = state_count.checked_add(1) else {
                            return Ok(None);
                        };
                        state_count = next_count;
                        if state_count > MAX_LOCAL_ENDPOINT_STATES {
                            return Ok(None);
                        }
                    }
                }
            }
        }
        Ok(Some(composed))
    }

    fn identity_relation(
        ctx: &DecodeContext<'_>,
        points: &BTreeSet<usize>,
    ) -> Result<EndpointRelation, CodecError> {
        let mut identity = EndpointRelation::new();
        for &point in points {
            insert_relation(
                ctx,
                &mut identity,
                point,
                point,
                "catia_endpoint_identity_relation",
            )?;
        }
        Ok(identity)
    }

    (|| -> Option<Result<MeshEndpointPairSupport, CodecError>> {
        let charge = || budget.is_none_or(WorkBudget::charge);
        let mut assignment_support = HashMap::<usize, HashSet<[usize; 2]>>::new();
        for boundary in &assignment.boundaries {
            if boundary.is_empty() {
                return Some(Ok(MeshEndpointPairSupport {
                    by_edge: HashMap::new(),
                }));
            }
            let mut points = BTreeSet::new();
            let mut layers = Vec::<(usize, Vec<[usize; 2]>, EndpointRelation)>::new();
            for use_ in boundary {
                let values = match candidates(use_.edge)? {
                    MeshEndpointCandidates::Explicit(values) => match crate::resource::copy_slice(
                        ctx,
                        values,
                        "catia_endpoint_layer_values",
                    ) {
                        Ok(values) => values,
                        Err(error) => return Some(Err(error)),
                    },
                    MeshEndpointCandidates::Implicit(values) => {
                        let mut collected = Vec::new();
                        for value in values.take(MAX_LOCAL_ENDPOINT_STATES + 1) {
                            if let Err(error) = crate::resource::push(
                                ctx,
                                &mut collected,
                                value,
                                "catia_endpoint_layer_values",
                            ) {
                                return Some(Err(error));
                            }
                        }
                        collected
                    }
                    MeshEndpointCandidates::Selected(value) => {
                        let mut selected = Vec::new();
                        if let Err(error) = crate::resource::push(
                            ctx,
                            &mut selected,
                            value,
                            "catia_endpoint_layer_values",
                        ) {
                            return Some(Err(error));
                        }
                        selected
                    }
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
                    if let Err(error) = crate::resource::push(
                        ctx,
                        &mut retained,
                        pair,
                        "catia_endpoint_layer_retained_pairs",
                    ) {
                        return Some(Err(error));
                    }
                    for point in pair {
                        if !points.contains(&point) {
                            if let Err(error) =
                                ctx.charge_collection_items(1, "catia_endpoint_layer_points")
                            {
                                return Some(Err(error));
                            }
                            points.insert(point);
                        }
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
                        if let Err(error) = insert_relation(
                            ctx,
                            &mut relation,
                            start,
                            end,
                            "catia_endpoint_layer_relation",
                        ) {
                            return Some(Err(error));
                        }
                    }
                }
                retained.sort_unstable();
                retained.dedup();
                if retained.is_empty() {
                    return Some(Ok(MeshEndpointPairSupport {
                        by_edge: HashMap::new(),
                    }));
                }
                if let Err(error) = crate::resource::push(
                    ctx,
                    &mut layers,
                    (use_.edge, retained, relation),
                    "catia_endpoint_layers",
                ) {
                    return Some(Err(error));
                }
            }
            if points.len() > MAX_LOCAL_ENDPOINT_STATES {
                return None;
            }
            let identity = match identity_relation(ctx, &points) {
                Ok(identity) => identity,
                Err(error) => return Some(Err(error)),
            };
            let Some(layer_count) = layers.len().checked_add(1) else {
                return Some(Err(ctx.refuse_codec_limit(
                    "catia_endpoint_relation_layers",
                    u64::MAX,
                    u64::MAX,
                )));
            };
            let mut prefixes = Vec::new();
            let first = match clone_relation(ctx, &identity, "catia_endpoint_prefix_identity") {
                Ok(identity) => identity,
                Err(error) => return Some(Err(error)),
            };
            if let Err(error) =
                crate::resource::push(ctx, &mut prefixes, first, "catia_endpoint_prefixes")
            {
                return Some(Err(error));
            }
            for (index, (_, _, relation)) in layers.iter().enumerate() {
                let composed = match compose_relations(ctx, &prefixes[index], relation, budget) {
                    Ok(Some(composed)) => composed,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                if let Err(error) =
                    crate::resource::push(ctx, &mut prefixes, composed, "catia_endpoint_prefixes")
                {
                    return Some(Err(error));
                }
            }
            let mut suffixes = match ctx.alloc_filled(
                layer_count,
                EndpointRelation::new(),
                "catia_endpoint_suffixes",
            ) {
                Ok(suffixes) => suffixes,
                Err(error) => return Some(Err(error)),
            };
            suffixes[layers.len()] = identity;
            for layer in (0..layers.len()).rev() {
                suffixes[layer] =
                    match compose_relations(ctx, &layers[layer].2, &suffixes[layer + 1], budget) {
                        Ok(Some(composed)) => composed,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
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
                        if let Err(error) = crate::resource::insert_set(
                            ctx,
                            &mut layer_support,
                            pair,
                            "catia_endpoint_layer_support",
                        ) {
                            return Some(Err(error));
                        }
                    }
                }
                if let Err(error) = crate::resource::admit_map_entry(
                    ctx,
                    &mut boundary_support,
                    &edge,
                    "catia_endpoint_boundary_support",
                ) {
                    return Some(Err(error));
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
                return Some(Ok(MeshEndpointPairSupport {
                    by_edge: HashMap::new(),
                }));
            }
            for (edge, supported) in boundary_support {
                if let Err(error) = crate::resource::admit_map_entry(
                    ctx,
                    &mut assignment_support,
                    &edge,
                    "catia_endpoint_assignment_support",
                ) {
                    return Some(Err(error));
                }
                assignment_support
                    .entry(edge)
                    .and_modify(|retained| retained.retain(|pair| supported.contains(pair)))
                    .or_insert(supported);
            }
            if assignment_support.values().any(HashSet::is_empty) {
                return Some(Ok(MeshEndpointPairSupport {
                    by_edge: HashMap::new(),
                }));
            }
        }
        Some(Ok(MeshEndpointPairSupport {
            by_edge: assignment_support,
        }))
    })()
    .transpose()
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
    mesh_assignment_endpoint_cycles_viable_with(assignment, edge_candidates, None, None)
        .unwrap_or(true)
}

pub(super) fn mesh_face_endpoint_configurations(
    ctx: &DecodeContext<'_>,
    assignments: &[MeshFaceBoundaryAssignment],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[Option<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> Result<Option<MeshFaceEndpointConfigurations>, CodecError> {
    fn insert_pair(
        ctx: &DecodeContext<'_>,
        configuration: &mut MeshFaceEndpointConfiguration,
        edge: usize,
        mut pair: [usize; 2],
    ) -> Result<bool, CodecError> {
        pair.sort_unstable();
        match configuration.iter().find(|(stored, _)| *stored == edge) {
            Some((_, stored)) => Ok(*stored == pair),
            None => {
                crate::resource::push(
                    ctx,
                    configuration,
                    (edge, pair),
                    "catia_face_configuration_pairs",
                )?;
                Ok(true)
            }
        }
    }

    fn boundary_configurations(
        ctx: &DecodeContext<'_>,
        boundary: &[MeshBoundaryEdgeCandidate],
        edge_candidates: &[Vec<[usize; 2]>],
        selected: &[Option<[usize; 2]>],
        work: &mut usize,
        budget: &WorkBudget<'_>,
    ) -> Result<Option<MeshFaceEndpointConfigurations>, CodecError> {
        let charge = |work: &mut usize| {
            *work = work.checked_add(1)?;
            (*work <= MAX_FACE_ENDPOINT_CONFIGURATION_WORK && budget.charge()).then_some(())
        };
        if boundary.is_empty()
            || boundary
                .iter()
                .any(|use_| edge_candidates.get(use_.edge).is_none_or(Vec::is_empty))
        {
            return Ok(None);
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
                if insert_pair(ctx, &mut configuration, boundary[0].edge, pair)? {
                    if charge(work).is_none() {
                        return Ok(None);
                    }
                    crate::resource::push(
                        ctx,
                        &mut states,
                        (start, current, configuration),
                        "catia_face_configuration_initial_states",
                    )?;
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
                        if charge(work).is_none() {
                            return Ok(None);
                        }
                        let mut configuration = crate::resource::copy_slice(
                            ctx,
                            &configuration,
                            "catia_face_configuration_state_pairs",
                        )?;
                        if insert_pair(ctx, &mut configuration, use_.edge, pair)? {
                            crate::resource::push(
                                ctx,
                                &mut next,
                                (start, endpoint, configuration),
                                "catia_face_configuration_next_states",
                            )?;
                        }
                    }
                }
            }
            states = next;
            if states.is_empty() {
                return Ok(Some(Vec::new()));
            }
        }
        let mut seen = HashSet::new();
        let mut completed = Vec::new();
        for (_, _, mut configuration) in states
            .into_iter()
            .filter(|(start, current, _)| start == current)
        {
            configuration.sort_unstable();
            if seen.contains(&configuration) {
                continue;
            }
            let key = crate::resource::copy_slice(
                ctx,
                &configuration,
                "catia_face_configuration_seen_pairs",
            )?;
            crate::resource::insert_set(ctx, &mut seen, key, "catia_face_configuration_seen_keys")?;
            crate::resource::push(
                ctx,
                &mut completed,
                configuration,
                "catia_face_configuration_boundary_results",
            )?;
        }
        Ok(Some(completed))
    }

    if selected.len() != edge_candidates.len() {
        return Ok(None);
    }
    let mut work = 0usize;
    let mut configurations = HashSet::new();
    for assignment in assignments {
        let mut combined = Vec::new();
        crate::resource::push(
            ctx,
            &mut combined,
            Vec::new(),
            "catia_face_configuration_combined_rows",
        )?;
        for boundary in &assignment.boundaries {
            let Some(boundary) = boundary_configurations(
                ctx,
                boundary,
                edge_candidates,
                selected,
                &mut work,
                budget,
            )?
            else {
                return Ok(None);
            };
            let mut next = Vec::new();
            for stored in combined {
                for candidate in &boundary {
                    let Some(next_work) = work.checked_add(1) else {
                        return Ok(None);
                    };
                    work = next_work;
                    if work > MAX_FACE_ENDPOINT_CONFIGURATION_WORK || !budget.charge() {
                        return Ok(None);
                    }
                    let mut merged = crate::resource::copy_slice(
                        ctx,
                        &stored,
                        "catia_face_configuration_combined_pairs",
                    )?;
                    let mut compatible = true;
                    for &(edge, pair) in candidate {
                        if !insert_pair(ctx, &mut merged, edge, pair)? {
                            compatible = false;
                            break;
                        }
                    }
                    if compatible {
                        merged.sort_unstable();
                        crate::resource::push(
                            ctx,
                            &mut next,
                            merged,
                            "catia_face_configuration_next_combined",
                        )?;
                    }
                }
            }
            combined = next;
        }
        for configuration in combined {
            crate::resource::insert_set(
                ctx,
                &mut configurations,
                configuration,
                "catia_face_configuration_result_keys",
            )?;
        }
    }
    let mut results = Vec::new();
    for configuration in configurations {
        crate::resource::push(
            ctx,
            &mut results,
            configuration,
            "catia_face_configuration_result_rows",
        )?;
    }
    results.sort_unstable();
    Ok(Some(results))
}

/// Return whether an unordered endpoint configuration can close every
/// boundary cycle of one face assignment. Configuration pairs are normalized
/// by `mesh_face_endpoint_configurations`; the checks below still compare
/// them as unordered pairs so callers cannot depend on that representation.
fn endpoint_configuration_boundary_cycle_viable(
    ctx: &DecodeContext<'_>,
    boundary: &[MeshBoundaryEdgeCandidate],
    pairs: &HashMap<usize, [usize; 2]>,
) -> Result<Option<bool>, CodecError> {
    if boundary.is_empty() {
        return Ok(None);
    }
    let Some(first) = boundary.first() else {
        return Ok(None);
    };
    let Some(&first_pair) = pairs.get(&first.edge) else {
        return Ok(None);
    };
    let mut states = Vec::new();
    crate::resource::push(
        ctx,
        &mut states,
        (first_pair[0], first_pair[1]),
        "catia_endpoint_cycle_initial_states",
    )?;
    if first_pair[0] != first_pair[1] {
        crate::resource::push(
            ctx,
            &mut states,
            (first_pair[1], first_pair[0]),
            "catia_endpoint_cycle_initial_states",
        )?;
    }
    for use_ in &boundary[1..] {
        let Some(&pair) = pairs.get(&use_.edge) else {
            return Ok(None);
        };
        let mut next = Vec::new();
        for (start, current) in states {
            for direction in [false, true] {
                if pair[0] == pair[1] && direction {
                    continue;
                }
                let edge_start = if direction { pair[1] } else { pair[0] };
                let edge_end = if direction { pair[0] } else { pair[1] };
                if edge_start == current {
                    crate::resource::push(
                        ctx,
                        &mut next,
                        (start, edge_end),
                        "catia_endpoint_cycle_next_states",
                    )?;
                }
            }
        }
        states = next;
        if states.is_empty() {
            return Ok(Some(false));
        }
    }
    Ok(Some(
        states.into_iter().any(|(start, current)| start == current),
    ))
}

fn endpoint_configuration_cycles_viable(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    configuration: &MeshFaceEndpointConfiguration,
) -> Result<Option<bool>, CodecError> {
    let mut pairs = HashMap::new();
    for &(edge, pair) in configuration {
        crate::resource::insert_map(ctx, &mut pairs, edge, pair, "catia_endpoint_cycle_pair_map")?;
    }
    if pairs.len() != configuration.len() {
        return Ok(None);
    }
    if assignment.boundaries.is_empty() {
        return Ok(Some(false));
    }
    for boundary in &assignment.boundaries {
        if endpoint_configuration_boundary_cycle_viable(ctx, boundary, &pairs)? != Some(true) {
            return Ok(Some(false));
        }
    }
    Ok(Some(true))
}

fn endpoint_configuration_for_assignment(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    edge_pairs: &[[usize; 2]],
) -> Result<Option<MeshFaceEndpointConfiguration>, CodecError> {
    let mut pairs = HashMap::<usize, [usize; 2]>::new();
    for use_ in assignment.boundaries.iter().flatten() {
        let Some(&pair) = edge_pairs.get(use_.edge) else {
            return Ok(None);
        };
        let mut pair = pair;
        pair.sort_unstable();
        match pairs.get(&use_.edge) {
            Some(previous) if *previous != pair => return Ok(None),
            Some(_) => {}
            None => {
                crate::resource::insert_map(
                    ctx,
                    &mut pairs,
                    use_.edge,
                    pair,
                    "catia_endpoint_assignment_pair_map",
                )?;
            }
        }
    }
    let mut configuration = Vec::new();
    for pair in pairs {
        crate::resource::push(
            ctx,
            &mut configuration,
            pair,
            "catia_endpoint_assignment_configuration",
        )?;
    }
    configuration.sort_unstable();
    Ok(Some(configuration))
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
            crate::resource::push(ctx, &mut states, (start, current, directions), "catia_endpoint_initial_states")?;
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
                let mut directions = crate::resource::copy_slice(ctx, &directions, "catia_endpoint_direction_prefix")?;
                crate::resource::push(ctx, &mut directions, direction, "catia_endpoint_direction_step")?;
                crate::resource::push(ctx, &mut next, (start, edge_end, directions), "catia_endpoint_direction_states")?;
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
            crate::resource::push(ctx, &mut solutions, directions, "catia_endpoint_boundary_solutions")?;
        }
    }
    solutions.sort_unstable();
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
        crate::resource::insert_map(ctx, &mut pairs, edge, pair, "catia_endpoint_configuration_pairs")?;
    }
    if pairs.len() != configuration.len() {
        return Ok(Err(MeshDirectionEnumerationError::Invalid));
    }
    let mut alternatives = ctx.alloc_filled(1, Vec::new(), "catia_endpoint_initial_alternatives")?;
    for boundary in &assignment.boundaries {
        let boundary_options = match endpoint_configuration_boundary_directions(ctx, boundary, &pairs)? {
            Ok(options) => options,
            Err(error) => return Ok(Err(error)),
        };
        let mut next = Vec::new();
        for prefix in &alternatives {
            for boundary_directions in &boundary_options {
                let mut alternative = crate::resource::copy_retained_rows(ctx, prefix, "catia_endpoint_alternative_prefix_rows", "catia_endpoint_alternative_prefix_directions")?;
                crate::resource::push(ctx, &mut alternative, crate::resource::copy_slice(ctx, boundary_directions, "catia_endpoint_boundary_direction_copy")?, "catia_endpoint_alternative_boundary")?;
                crate::resource::push(ctx, &mut next, alternative, "catia_endpoint_alternatives")?;
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
    pub(super) fn normalized(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        match self {
            Self::Enumerated {
                assignments,
                edge_pairs,
            } => {
                let mut assignments = crate::resource::copy_retained_slice(
                    ctx,
                    assignments,
                    "catia_relation_normalized_assignments",
                )?;
                assignments.sort_unstable();
                assignments.dedup();
                let mut edge_pairs = crate::resource::copy_retained_slice(
                    ctx,
                    edge_pairs,
                    "catia_relation_normalized_pairs",
                )?;
                for (_, pair) in &mut edge_pairs {
                    pair.sort_unstable();
                }
                edge_pairs.sort_unstable();
                Ok(Self::Enumerated {
                    assignments,
                    edge_pairs,
                })
            }
            Self::Deferred => Ok(Self::Deferred),
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
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
) -> Result<MeshEndpointRelationStateSignature, CodecError> {
    let mut normalized_assigned = Vec::new();
    for pair in assigned {
        let pair = pair.map(|mut pair| {
            pair.sort_unstable();
            pair
        });
        crate::resource::push(
            ctx,
            &mut normalized_assigned,
            pair,
            "catia_relation_signature_assigned",
        )?;
    }
    let mut normalized_domains = Vec::new();
    for choices in domains {
        let mut row = Vec::new();
        for choice in choices {
            let normalized = choice.selection.normalized(ctx)?;
            crate::resource::push(
                ctx,
                &mut row,
                normalized,
                "catia_relation_signature_choices",
            )?;
        }
        row.sort_unstable();
        crate::resource::push(
            ctx,
            &mut normalized_domains,
            row,
            "catia_relation_signature_domains",
        )?;
    }
    Ok((normalized_assigned, normalized_domains))
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
            ctx, domains, assigned,
        )?))
    }
}

/// Return the edge-pair superset admitted by the surviving relation domains.
/// A relation branch can only select pairs from this set, so coordinate
/// infeasibility of the superset is a sound branch rejection.
fn relation_coordinate_candidate_domains(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
    base_candidates: &[Vec<[usize; 2]>],
) -> Result<Option<Vec<Vec<[usize; 2]>>>, CodecError> {
    if assigned.len() != base_candidates.len() {
        return Ok(None);
    }
    let has_unconstrained_choice = domains
        .iter()
        .flatten()
        .any(|choice| choice.selection.is_unconstrained());
    let mut candidates = Vec::new();
    let mut possible = Vec::new();
    for base in base_candidates {
        let copy =
            crate::resource::copy_slice(ctx, base, "catia_relation_coordinate_candidate_pairs")?;
        crate::resource::push(
            ctx,
            &mut candidates,
            copy,
            "catia_relation_coordinate_candidate_rows",
        )?;
        crate::resource::push(
            ctx,
            &mut possible,
            Vec::<[usize; 2]>::new(),
            "catia_relation_coordinate_possible_rows",
        )?;
    }
    if !has_unconstrained_choice {
        for choice in domains.iter().flatten() {
            for &(edge, pair) in choice.selection.edge_pairs() {
                let Some(row) = possible.get_mut(edge) else {
                    return Ok(None);
                };
                crate::resource::push(ctx, row, pair, "catia_relation_coordinate_possible_pairs")?;
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
            return Ok(None);
        }
    }
    Ok(Some(candidates))
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

fn copy_mesh_boundary_directions(
    ctx: &DecodeContext<'_>,
    directions: &[Vec<bool>],
) -> Result<Vec<Vec<bool>>, CodecError> {
    let mut copy = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut copy,
        directions.len(),
        "catia_direction_copy_rows",
    )?;
    for row in directions {
        copy.push(crate::resource::copy_slice(
            ctx,
            row,
            "catia_direction_copy_values",
        )?);
    }
    Ok(copy)
}

fn canonical_mesh_boundary_directions(
    ctx: &DecodeContext<'_>,
    directions: &[Vec<bool>],
) -> Result<Vec<Vec<bool>>, CodecError> {
    let mut canonical = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut canonical,
        directions.len(),
        "catia_canonical_mesh_direction_rows",
    )?;
    for row in directions {
        let mut complement =
            crate::resource::copy_slice(ctx, row, "catia_canonical_mesh_direction_values")?;
        for direction in &mut complement {
            *direction = !*direction;
        }
        canonical.push(if complement < *row {
            complement
        } else {
            crate::resource::copy_slice(ctx, row, "catia_canonical_mesh_direction_values")?
        });
    }
    Ok(canonical)
}

type EndpointRelationKey = Vec<Option<[usize; 2]>>;
type EndpointRelationKeys<'a> = Vec<(&'a MeshEndpointRelationChoice, EndpointRelationKey)>;

fn canonical_endpoint_relation_key(
    ctx: &DecodeContext<'_>,
    choice: &MeshEndpointRelationChoice,
    edges: &[usize],
) -> Result<EndpointRelationKey, CodecError> {
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
        crate::resource::push(ctx, &mut key, pair, "catia_endpoint_relation_key_values")?;
    }
    Ok(key)
}

fn complete_endpoint_relation_keys<'a>(
    ctx: &DecodeContext<'_>,
    choices: &'a [MeshEndpointRelationChoice],
    edges: &[usize],
) -> Result<(bool, EndpointRelationKeys<'a>), CodecError> {
    let mut keys = Vec::new();
    for choice in choices {
        let key = canonical_endpoint_relation_key(ctx, choice, edges)?;
        crate::resource::push(
            ctx,
            &mut keys,
            (choice, key),
            "catia_endpoint_relation_keys",
        )?;
    }
    let complete = keys.iter().all(|(_, key)| key.iter().all(Option::is_some));
    Ok((complete, keys))
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
                crate::resource::admit_map_entry(
                    ctx,
                    &mut edge_faces,
                    &edge,
                    "catia_endpoint_relation_edge_face_keys",
                )?;
                crate::resource::insert_set(
                    ctx,
                    edge_faces.entry(edge).or_default(),
                    face,
                    "catia_endpoint_relation_edge_faces",
                )?;
            }
        }
    }
    for (edge, faces) in edge_faces {
        let mut sorted_faces = Vec::new();
        for face in faces {
            crate::resource::push(
                ctx,
                &mut sorted_faces,
                face,
                "catia_endpoint_relation_sorted_faces",
            )?;
        }
        sorted_faces.sort_unstable();
        for (left_index, &left) in sorted_faces.iter().enumerate() {
            for &right in &sorted_faces[left_index + 1..] {
                for pair in [(left, right), (right, left)] {
                    crate::resource::admit_map_entry(
                        ctx,
                        &mut shared_edges,
                        &pair,
                        "catia_endpoint_relation_shared_keys",
                    )?;
                    crate::resource::push(
                        ctx,
                        shared_edges.entry(pair).or_default(),
                        edge,
                        "catia_endpoint_relation_shared_edges",
                    )?;
                }
            }
        }
    }

    let mut shared_rows = Vec::new();
    for (pair, mut edges) in shared_edges {
        edges.sort_unstable();
        crate::resource::push(
            ctx,
            &mut shared_rows,
            (pair, edges),
            "catia_endpoint_relation_shared_rows",
        )?;
    }
    shared_rows.sort_unstable_by_key(|(pair, _)| *pair);
    let mut arcs = ctx.alloc_filled(domains.len(), Vec::new(), "catia_endpoint_relation_arcs")?;
    let mut incoming = ctx.alloc_filled(
        domains.len(),
        Vec::new(),
        "catia_endpoint_relation_incoming",
    )?;
    let mut choice_counts = Vec::new();
    for domain in domains {
        crate::resource::push(
            ctx,
            &mut choice_counts,
            domain.len(),
            "catia_endpoint_relation_choice_counts",
        )?;
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
        let (left_complete, left_choices) =
            complete_endpoint_relation_keys(ctx, &domains[face], &edges)?;
        let (right_complete, right_choices) =
            complete_endpoint_relation_keys(ctx, &domains[neighbor], &edges)?;
        let supports = if left_complete && right_complete {
            let mut index = HashMap::<EndpointRelationKey, Vec<usize>>::new();
            for (choice, key) in right_choices {
                crate::resource::admit_map_entry(
                    ctx,
                    &mut index,
                    &key,
                    "catia_endpoint_relation_index_keys",
                )?;
                crate::resource::push(
                    ctx,
                    index.entry(key).or_default(),
                    choice.id,
                    "catia_endpoint_relation_index_values",
                )?;
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
                crate::resource::push(
                    ctx,
                    &mut supports,
                    mask,
                    "catia_endpoint_relation_support_rows",
                )?;
            }
            supports
        } else {
            let comparison_work =
                work_units(domains[face].len().saturating_mul(domains[neighbor].len()));
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
                crate::resource::push(
                    ctx,
                    &mut supports,
                    mask,
                    "catia_endpoint_relation_support_rows",
                )?;
            }
            supports
        };
        let arc_index = arcs[face].len();
        crate::resource::push(
            ctx,
            &mut arcs[face],
            MeshEndpointRelationArc { neighbor, supports },
            "catia_endpoint_relation_arc_entries",
        )?;
        crate::resource::push(
            ctx,
            &mut incoming[neighbor],
            (face, arc_index),
            "catia_endpoint_relation_incoming_entries",
        )?;
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
            crate::resource::push(
                ctx,
                &mut dirty_faces,
                face,
                "catia_endpoint_relation_dirty_faces",
            )?;
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
                crate::resource::push(
                    ctx,
                    &mut dirty_faces,
                    face,
                    "catia_endpoint_relation_dirty_faces",
                )?;
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
            crate::resource::push(
                ctx,
                &mut active,
                mask,
                "catia_endpoint_relation_active_rows",
            )?;
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
                    crate::resource::push_back(
                        ctx,
                        &mut queue,
                        (face, arc),
                        "catia_endpoint_relation_queue",
                    )?;
                }
            }
        } else {
            for face in dirty_faces.drain(..) {
                for &incoming in &constraints.incoming[face] {
                    crate::resource::push_back(
                        ctx,
                        &mut queue,
                        incoming,
                        "catia_endpoint_relation_queue",
                    )?;
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
                crate::resource::push_back(
                    ctx,
                    &mut queue,
                    incoming,
                    "catia_endpoint_relation_queue",
                )?;
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
                    crate::resource::admit_map_entry(
                        ctx,
                        &mut pairs,
                        &edge,
                        "catia_endpoint_relation_pair_keys",
                    )?;
                    crate::resource::insert_set(
                        ctx,
                        pairs.entry(edge).or_default(),
                        pair,
                        "catia_endpoint_relation_pair_values",
                    )?;
                    crate::resource::admit_map_entry(
                        ctx,
                        &mut counts,
                        &edge,
                        "catia_endpoint_relation_count_keys",
                    )?;
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

fn copy_endpoint_relation_selection(
    ctx: &DecodeContext<'_>,
    selection: &MeshEndpointRelationSelection,
) -> Result<MeshEndpointRelationSelection, CodecError> {
    match selection {
        MeshEndpointRelationSelection::Deferred => Ok(MeshEndpointRelationSelection::Deferred),
        MeshEndpointRelationSelection::Enumerated {
            assignments,
            edge_pairs,
        } => Ok(MeshEndpointRelationSelection::Enumerated {
            assignments: crate::resource::copy_slice(
                ctx,
                assignments,
                "catia_endpoint_relation_copy_assignments",
            )?,
            edge_pairs: crate::resource::copy_slice(
                ctx,
                edge_pairs,
                "catia_endpoint_relation_copy_edge_pairs",
            )?,
        }),
    }
}

fn copy_endpoint_relation_domains(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
) -> Result<Vec<Vec<MeshEndpointRelationChoice>>, CodecError> {
    let mut copy = Vec::new();
    for choices in domains {
        let mut row = Vec::new();
        for choice in choices {
            let selection = copy_endpoint_relation_selection(ctx, &choice.selection)?;
            crate::resource::push(
                ctx,
                &mut row,
                MeshEndpointRelationChoice {
                    id: choice.id,
                    selection,
                },
                "catia_endpoint_relation_copy_choices",
            )?;
        }
        crate::resource::push(ctx, &mut copy, row, "catia_endpoint_relation_copy_faces")?;
    }
    Ok(copy)
}

// The recursive walk keeps branch-owned domains and shared memo state explicit;
// a context object would hide which values are cloned for each branch.
#[allow(clippy::too_many_arguments)]
fn walk_endpoint_relation_domains<F>(
    ctx: &DecodeContext<'_>,
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
) -> Result<bool, CodecError>
where
    F: FnMut(MeshEndpointRelationSelections, Vec<[usize; 2]>) -> Result<bool, CodecError>,
{
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
            crate::resource::insert_set(
                ctx,
                &mut possible_points,
                point,
                "catia_endpoint_relation_possible_points",
            )?;
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
            crate::resource::insert_set(
                ctx,
                &mut possible_points,
                point,
                "catia_endpoint_relation_possible_points",
            )?;
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
                ctx,
                &domains,
                &assigned,
                coordinate_domains.edge_candidates(),
            )?
            else {
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
                Some(raw_endpoint_relation_state_signature(
                    ctx, &domains, &assigned,
                )?)
            };
        if let Some(signature) = signature {
            if !crate::resource::insert_set(
                ctx,
                state_memo,
                signature,
                "catia_endpoint_relation_state_memo",
            )? {
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
                    crate::resource::insert_set(
                        ctx,
                        &mut priority,
                        edge,
                        "catia_endpoint_relation_priority_edges",
                    )?;
                }
            }
            crate::resource::push(
                ctx,
                &mut priority_counts,
                priority.len(),
                "catia_endpoint_relation_priority_counts",
            )?;
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
            crate::resource::push(
                ctx,
                &mut completed_pairs,
                pair,
                "catia_endpoint_relation_completed_pairs",
            )?;
        }
        let mut selections = Vec::new();
        for (face, choices) in domains.iter().enumerate() {
            let Some(choice) = choices.first() else {
                return Ok(false);
            };
            let viable = if let MeshEndpointRelationSelection::Enumerated { assignments, .. } =
                &choice.selection
            {
                crate::resource::copy_slice(
                    ctx,
                    assignments,
                    "catia_endpoint_relation_selected_assignments",
                )?
            } else {
                let mut viable = Vec::new();
                for (assignment, assignment_value) in face_assignments[face].iter().enumerate() {
                    let Some(configuration) = endpoint_configuration_for_assignment(
                        ctx,
                        assignment_value,
                        &completed_pairs,
                    )?
                    else {
                        continue;
                    };
                    if endpoint_configuration_cycles_viable(ctx, assignment_value, &configuration)?
                        != Some(true)
                    {
                        continue;
                    }
                    crate::resource::push(
                        ctx,
                        &mut viable,
                        assignment,
                        "catia_endpoint_relation_deferred_assignments",
                    )?;
                }
                if viable.is_empty() {
                    return Ok(false);
                }
                viable
            };
            crate::resource::push(
                ctx,
                &mut selections,
                viable,
                "catia_endpoint_relation_selection_faces",
            )?;
        }
        return evaluate(selections, completed_pairs);
    };
    let mut assigned_points = HashSet::new();
    for point in assigned.iter().flatten().flatten().copied() {
        crate::resource::insert_set(
            ctx,
            &mut assigned_points,
            point,
            "catia_endpoint_relation_assigned_points",
        )?;
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
                crate::resource::insert_set(
                    ctx,
                    &mut points,
                    point,
                    "catia_endpoint_relation_support_choice_points",
                )?;
            }
            for point in points {
                crate::resource::admit_map_entry(
                    ctx,
                    &mut point_support,
                    &point,
                    "catia_endpoint_relation_support_point_keys",
                )?;
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
            crate::resource::insert_set(
                ctx,
                &mut points,
                point,
                "catia_endpoint_relation_score_points",
            )?;
        }
        let score = points
            .into_iter()
            .filter(|point| !assigned_points.contains(point))
            .map(|point| {
                point_count
                    .saturating_sub(point_support.get(&point).copied().unwrap_or(0))
                    .saturating_add(1)
            })
            .sum::<usize>();
        crate::resource::push(
            ctx,
            &mut branch_order,
            (score, index),
            "catia_endpoint_relation_branch_order",
        )?;
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
        let mut branch = copy_endpoint_relation_domains(ctx, &domains)?;
        branch[face].clear();
        let choice = &choices[index];
        let selection = copy_endpoint_relation_selection(ctx, &choice.selection)?;
        crate::resource::push(
            ctx,
            &mut branch[face],
            MeshEndpointRelationChoice {
                id: choice.id,
                selection,
            },
            "catia_endpoint_relation_branch_choice",
        )?;
        if walk_endpoint_relation_domains(
            ctx,
            branch,
            face_assignments,
            crate::resource::copy_slice(ctx, &assigned, "catia_endpoint_relation_branch_assigned")?,
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
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Collect one face's endpoint relation choices. A missing configuration list
/// is an unknown result from bounded enumeration, not an empty domain.
fn collect_endpoint_relation_face_choices(
    ctx: &DecodeContext<'_>,
    face_assignments: &[MeshFaceBoundaryAssignment],
    face_configurations: &[Option<MeshFaceEndpointConfigurations>],
    covered: &mut [bool],
) -> Result<Option<Vec<MeshEndpointRelationChoice>>, CodecError> {
    let mut choices_by_configuration = HashMap::<MeshFaceEndpointConfiguration, Vec<usize>>::new();
    let mut unknown = false;
    for (assignment, configurations) in face_configurations.iter().enumerate() {
        let Some(configurations) = configurations else {
            unknown = true;
            continue;
        };
        for configuration in configurations {
            for &(edge, _) in configuration {
                let Some(covered) = covered.get_mut(edge) else {
                    return Ok(None);
                };
                *covered = true;
            }
            let Some(face_assignment) = face_assignments.get(assignment) else {
                return Ok(None);
            };
            if endpoint_configuration_cycles_viable(ctx, face_assignment, configuration)?
                != Some(true)
            {
                continue;
            }
            let mut relation_configuration = crate::resource::copy_slice(
                ctx,
                configuration,
                "catia_endpoint_relation_config_pairs",
            )?;
            for (_, pair) in &mut relation_configuration {
                pair.sort_unstable();
            }
            relation_configuration.sort_unstable();
            crate::resource::admit_map_entry(
                ctx,
                &mut choices_by_configuration,
                &relation_configuration,
                "catia_endpoint_relation_configuration_keys",
            )?;
            crate::resource::push(
                ctx,
                choices_by_configuration
                    .entry(relation_configuration)
                    .or_default(),
                assignment,
                "catia_endpoint_relation_configuration_assignments",
            )?;
        }
    }
    let mut choices = Vec::new();
    for (edge_pairs, mut assignments) in choices_by_configuration {
        assignments.sort_unstable();
        assignments.dedup();
        crate::resource::push(
            ctx,
            &mut choices,
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments,
                    edge_pairs,
                },
            },
            "catia_endpoint_relation_face_choices",
        )?;
    }
    choices.sort_unstable_by(|left, right| {
        (left.selection.edge_pairs(), &left.selection)
            .cmp(&(right.selection.edge_pairs(), &right.selection))
    });
    if unknown {
        // A stopped enumeration is not evidence that the assignment has no
        // configuration. The wildcard lets the relation walker defer that
        // assignment to the complete endpoint search.
        crate::resource::push(
            ctx,
            &mut choices,
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Deferred,
            },
            "catia_endpoint_relation_face_choices",
        )?;
    }
    for (id, choice) in choices.iter_mut().enumerate() {
        choice.id = id;
    }
    Ok(Some(choices))
}

/// Solve the unordered endpoint-configuration relation before selecting
/// intrinsic cycle orientations. A configuration records one candidate pair
/// for every edge in a face. Shared edges must agree on that pair, but their
/// row-port order is selected later by the quotient search.
// These independent inputs describe one bounded relation phase; keeping them
// separate preserves the ownership of parsed evidence and branch state.
#[allow(clippy::too_many_arguments)]
fn resolve_endpoint_configuration_relation_streaming(
    ctx: &DecodeContext<'_>,
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
) -> Result<Option<MeshEndpointResolve>, CodecError> {
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
            ctx,
            face_assignments,
            face_configurations,
            &mut covered,
        )?;
        let Some(choices) = choices else {
            return Ok(None);
        };
        if choices.is_empty() {
            return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Rejected(()))));
        }
        if !budget.charge_by(choices.len()) {
            return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))));
        }
        crate::resource::push(
            ctx,
            &mut domains,
            choices,
            "catia_endpoint_relation_face_domains",
        )?;
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
        coordinate_domains.map(|_| budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS));
    let mut ambiguous = false;
    let mut exhausted = false;
    let mut evaluate = |selections: MeshEndpointRelationSelections,
                        edge_pairs: Vec<[usize; 2]>|
     -> Result<bool, CodecError> {
        let mut point_set = HashSet::new();
        for &point in edge_pairs.iter().flatten() {
            crate::resource::insert_set(ctx, &mut point_set, point, "catia_relation_point_set")?;
        }
        if point_set.len() != vertex_points.len() {
            return Ok(false);
        }
        if let Some(valid) = partial_solution_valid {
            let mut candidate_pairs = Vec::new();
            for &pair in &edge_pairs {
                crate::resource::push(
                    ctx,
                    &mut candidate_pairs,
                    Some(pair),
                    "catia_relation_partial_pairs",
                )?;
            }
            if !valid(&candidate_pairs) {
                return Ok(false);
            }
        }
        if relation_state_memo.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let canonical_pairs = if let Some(gauge) = candidate_gauge {
                canonicalize_complete_endpoint_pairs(ctx, &edge_pairs, gauge)?
            } else {
                Some(crate::resource::copy_retained_slice(
                    ctx,
                    &edge_pairs,
                    "catia_relation_canonical_pairs",
                )?)
            };
            let Some(canonical_pairs) = canonical_pairs else {
                return Ok(false);
            };
            let selection_copy = crate::resource::copy_retained_rows(
                ctx,
                &selections,
                "catia_relation_memo_selection_rows",
                "catia_relation_memo_selection_values",
            )?;
            if !crate::resource::insert_set(
                ctx,
                &mut relation_state_memo,
                (selection_copy, canonical_pairs),
                "catia_relation_state_memo",
            )? {
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
                    let copy = crate::resource::copy_retained_slice(
                        ctx,
                        boundary,
                        "catia_relation_assignment_boundary_values",
                    )?;
                    crate::resource::push(
                        ctx,
                        &mut boundaries,
                        copy,
                        "catia_relation_assignment_boundaries",
                    )?;
                }
                crate::resource::push(
                    ctx,
                    &mut row,
                    MeshFaceBoundaryAssignment { boundaries },
                    "catia_relation_assignment_choices",
                )?;
            }
            crate::resource::push(
                ctx,
                &mut assignment_domains,
                row,
                "catia_relation_assignment_domains",
            )?;
        }
        if assignment_domains.iter().any(Vec::is_empty) {
            return Ok(false);
        }
        let mut candidates = Vec::new();
        for &pair in &edge_pairs {
            let mut row = Vec::new();
            crate::resource::push(ctx, &mut row, pair, "catia_relation_candidate_pair")?;
            crate::resource::push(ctx, &mut candidates, row, "catia_relation_candidate_rows")?;
        }
        let endpoint_resolution_budget = budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        let outcome = if assignment_domains.iter().all(|domain| domain.len() == 1) {
            let mut selected = Vec::new();
            let mut complete = true;
            for mut domain in assignment_domains {
                if let Some(choice) = domain.pop() {
                    crate::resource::push(
                        ctx,
                        &mut selected,
                        choice,
                        "catia_relation_selected_assignments",
                    )?;
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
        domains,
        assignments,
        ctx.alloc_filled(
            edge_candidates.len(),
            None,
            "catia_endpoint_relation_assigned",
        )?,
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
    topology: &StandardTopology,
    point_assignment: &[usize],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    let Some(edge_vertices) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    let mut pairs = Vec::new();
    for [start, end] in edge_vertices {
        let (Some(start), Some(end)) =
            (point_assignment.get(start), point_assignment.get(end))
        else {
            return Ok(None);
        };
        crate::resource::push(ctx, &mut pairs, Some([*start, *end]), "catia_mesh_candidate_point_pairs")?;
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
    ctx: &DecodeContext<'_>,
    pairs: &[[usize; 2]],
    candidates: &[Vec<[usize; 2]>],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    if pairs.len() != candidates.len() {
        return Ok(None);
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
            return Ok(None);
        };
        crate::resource::push(ctx, &mut oriented, direction, "catia_oriented_endpoint_pairs")?;
    }
    Ok(Some(oriented))
}

fn materialize_boundary_domains(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    edge_pairs: &[[usize; 2]],
) -> Result<Option<Vec<Vec<MeshFaceBoundaryAssignment>>>, CodecError> {
    let mut materialized = Vec::new();
    crate::resource::reserve_vec(ctx, &mut materialized, domains.len(), "catia materialized boundary domains")?;
    for domain in domains {
        let assignments = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                let mut copies = Vec::new();
                crate::resource::reserve_vec(ctx, &mut copies, assignments.len(), "catia materialized ordered assignments")?;
                for assignment in assignments {
                    let boundaries = crate::resource::copy_retained_rows(
                        ctx,
                        &assignment.boundaries,
                        "catia materialized ordered boundary rows",
                        "catia materialized ordered boundary members",
                    )?;
                    copies.push(MeshFaceBoundaryAssignment { boundaries });
                }
                copies
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                let Some(assignment) = deferred_boundary_assignment(ctx, domain, edge_pairs)?
                else {
                    return Ok(None);
                };
                let mut copies = Vec::new();
                crate::resource::push(ctx, &mut copies, assignment, "catia materialized deferred boundary")?;
                copies
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                let Some(cycles) = incidence_cycles(ctx, edges, edge_pairs)? else {
                    return Ok(None);
                };
                let [cycle] = cycles.as_slice() else {
                    return Ok(None);
                };
                let length = cycle.len();
                let mut boundary = Vec::new();
                crate::resource::reserve_vec(ctx, &mut boundary, length, "catia materialized unordered boundary")?;
                for (index, &(edge, reversed)) in cycle.iter().enumerate() {
                    boundary.push(MeshBoundaryEdgeCandidate {
                        edge,
                        start: index,
                        end: (index + 1) % length,
                        reversed: Some(reversed),
                    });
                }
                let mut boundaries = Vec::new();
                crate::resource::push(ctx, &mut boundaries, boundary, "catia materialized unordered boundary rows")?;
                let mut copies = Vec::new();
                crate::resource::push(ctx, &mut copies, MeshFaceBoundaryAssignment { boundaries }, "catia materialized unordered assignments")?;
                copies
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
    use cadmpeg_core::CodecError;
    use super::{
        endpoint_pairs_respect_candidate_domains, materialize_boundary_domains,
        mesh_domains_have_incident_edge_support, restore_unique_endpoint_pair_orientations,
    };
    use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;
    use crate::solve::missing_edge::MeshDeferredFaceBoundary;
    use crate::solve::missing_edge::MeshFaceBoundaryAssignment;
    use crate::solve::missing_edge::MeshFaceBoundaryDomain;
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
            crate::test_support::with_service_context(|ctx| restore_unique_endpoint_pair_orientations(
                ctx,
                &[[0, 1], [2, 3], [4, 5]],
                &[vec![[1, 0]], vec![[2, 3], [3, 2]], Vec::new()],
            )).expect("service resource budget"),
            Some(vec![[1, 0], [2, 3], [4, 5]])
        );
        assert!(crate::test_support::with_service_context(|ctx| restore_unique_endpoint_pair_orientations(ctx, &[[0, 1]], &[vec![[2, 3]]])).expect("service resource budget").is_none());
    }

    #[test]
    fn endpoint_pair_orientation_result_refuses_before_growth() {
        let result = crate::test_support::with_collection_limit(0, |ctx| {
            restore_unique_endpoint_pair_orientations(ctx, &[[0, 1]], &[vec![[1, 0]]])
        });
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_oriented_endpoint_pairs"
        ));
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

        let assignments = materialize_boundary_domains(&ctx, &domains, &[[0, 1], [0, 1]])
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
    fn complete_endpoint_pairs_refuse_materialization_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

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
        let pairs = [[0, 1], [0, 1]];
        catia_test_context!(service_ctx);
        assert!(materialize_boundary_domains(&service_ctx, &domains, &pairs)
            .expect("service resource budget")
            .is_some());

        let mut refused = HashSet::new();
        for limit in 0..128 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            match materialize_boundary_domains(&ctx, &domains, &pairs) {
                Err(CodecError::ResourceLimit(error)) => {
                    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                    refused.insert(error.operation.to_owned());
                }
                Ok(Some(_)) => break,
                Ok(None) => panic!("closed boundary must materialize"),
                Err(error) => panic!("unexpected refusal: {error}"),
            }
        }
        assert!(refused.contains("catia materialized boundary domains"));
        assert!(refused.contains("catia materialized deferred boundary"));
    }

    #[test]
    fn ordered_boundary_copy_refuses_before_nested_rows() {
        let domains = [MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: Some(false),
            }]],
        }])];
        let mut refused = HashSet::new();
        for cap in 0..8 {
            match crate::test_support::with_collection_limit(cap, |ctx| materialize_boundary_domains(ctx, &domains, &[])) {
                Err(CodecError::ResourceLimit(limit)) => { refused.insert(limit.operation); }
                Ok(Some(_)) => break,
                other => panic!("unexpected ordered boundary result: {other:?}"),
            }
        }
        for operation in [
            "catia materialized boundary domains",
            "catia materialized ordered assignments",
            "catia materialized ordered boundary rows",
            "catia materialized ordered boundary members",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
        assert!(crate::test_support::with_service_context(|ctx| materialize_boundary_domains(ctx, &domains, &[]))
            .expect("service resource budget").is_some());
    }

    #[test]
    fn unordered_boundary_copy_refuses_before_nested_rows() {
        let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];
        let pairs = [[0, 1], [1, 2], [2, 0]];
        let mut refused = HashSet::new();
        for cap in 0..32 {
            match crate::test_support::with_collection_limit(cap, |ctx| materialize_boundary_domains(ctx, &domains, &pairs)) {
                Err(CodecError::ResourceLimit(limit)) => { refused.insert(limit.operation); }
                Ok(Some(_)) => break,
                other => panic!("unexpected unordered boundary result: {other:?}"),
            }
        }
        for operation in [
            "catia materialized unordered boundary",
            "catia materialized unordered boundary rows",
            "catia materialized unordered assignments",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
        assert!(crate::test_support::with_service_context(|ctx| materialize_boundary_domains(ctx, &domains, &pairs))
            .expect("service resource budget").is_some());
    }

    #[test]
    fn complete_endpoint_pairs_materialize_unordered_cycle() {
        catia_test_context!(ctx);
        let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];
        let assignments = materialize_boundary_domains(&ctx, &domains, &[[0, 1], [1, 2], [2, 0]])
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

fn copy_mesh_assignment(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
) -> Result<MeshFaceBoundaryAssignment, CodecError> {
    Ok(MeshFaceBoundaryAssignment {
        boundaries: crate::resource::copy_retained_rows(
            ctx,
            &assignment.boundaries,
            "catia_fixed_assignment_boundary_rows",
            "catia_fixed_assignment_boundary_uses",
        )?,
    })
}

fn copy_mesh_edge_rows(
    ctx: &DecodeContext<'_>,
    rows: &[EdgeRow],
) -> Result<Vec<EdgeRow>, CodecError> {
    let mut copied = Vec::new();
    crate::resource::reserve_vec(ctx, &mut copied, rows.len(), "catia_mesh_edge_copy_rows")?;
    for row in rows {
        copied.push(row.clone_charged(ctx)?);
    }
    Ok(copied)
}

fn fixed_initial_orientations(
    ctx: &DecodeContext<'_>,
    fixed: &[bool],
    operation: &'static str,
) -> Result<Vec<Option<bool>>, CodecError> {
    let mut orientations = Vec::new();
    crate::resource::reserve_vec(ctx, &mut orientations, fixed.len(), operation)?;
    orientations.extend(fixed.iter().map(|fixed| (!fixed).then_some(false)));
    Ok(orientations)
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
    crate::resource::reserve_vec(ctx, &mut assignment_domains, selected.len(), "catia_fixed_assignment_domain_rows")?;
    for assignment in selected {
        assignment_domains.push(ctx.alloc_filled(1, copy_mesh_assignment(ctx, assignment)?, "catia_fixed_assignment_domain_entries")?);
    }
    if edge_candidates
        .iter()
        .any(|candidates| candidates.len() != 1)
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }
    if let Some(resolved) = resolve_singleton_mesh_endpoint_candidates(
        ctx,
        edge_rows,
        vertex_points,
        edge_candidates,
        &assignment_domains,
        port_identities,
        None,
        budget,
        candidate_gauge,
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
    crate::resource::reserve_vec(ctx, &mut edge_pairs, edge_candidates.len(), "catia_fixed_endpoint_pairs")?;
    for candidates in edge_candidates {
        if let Some(&pair) = candidates.first() {
            edge_pairs.push(pair);
        }
    }
    let mut fixed_face_directions = Vec::new();
    crate::resource::reserve_vec(ctx, &mut fixed_face_directions, selected.len(), "catia_fixed_face_direction_rows")?;
    let mut direction_overflow = false;
    for assignment in selected {
        let Some(configuration) =
            endpoint_configuration_for_assignment(ctx, assignment, &edge_pairs)?
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
        initial_mesh_quotient(ctx, edge_candidates, vertex_points.len(), port_identities)?
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    };
    let mut direct_quotient = quotient.clone_charged(ctx)?;
    let mut direct_orientations = fixed_initial_orientations(ctx, &edge_has_fixed_direction, "catia_fixed_direct_orientations")?;
    let mut direct_directions = Vec::new();
    crate::resource::reserve_vec(ctx, &mut direct_directions, selected.len(), "catia_fixed_direct_direction_rows")?;
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
        let mut next_orientations = crate::resource::copy_slice(ctx, &direct_orientations, "catia_fixed_next_orientations")?;
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
            ctx,
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
        let outcome = reconstruct_mesh_selection(
            ctx,
            edge_rows,
            vertex_points,
            selected,
            &direct_directions,
        )?
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
    let face_work = ctx.alloc_filled(assignment_domains.len(), Some(1usize), "catia_fixed_face_work")?;
    let fixed_edge_orientations = if use_fixed_direction_search {
        fixed_initial_orientations(ctx, &edge_has_fixed_direction, "catia_fixed_search_orientations")?
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
#[allow(clippy::items_after_statements)]
fn resolve_fixed_mesh_endpoint_assignment_domains(
    ctx: &DecodeContext<'_>,
    geometry: MeshEndpointGeometry<'_>,
    edge_candidates: &[Vec<[usize; 2]>],
    assignment_domains: &[Vec<MeshFaceBoundaryAssignment>],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<MeshEndpointResolve, CodecError> {
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
    // Keep recursive search state explicit so budget, solution, and ambiguity
    // ownership remain visible at every branch.
    #[allow(clippy::too_many_arguments)]
    fn visit(
        ctx: &DecodeContext<'_>,
        face: usize,
        assignment_domains: &[Vec<MeshFaceBoundaryAssignment>],
        selected: &mut Vec<MeshFaceBoundaryAssignment>,
        edge_rows: &[EdgeRow],
        vertex_points: &[[f64; 3]],
        edge_candidates: &[Vec<[usize; 2]>],
        port_identities: &[[u32; 2]],
        budget: &WorkBudget<'_>,
        candidate_gauge: Option<MeshCandidateGauge<'_>>,
        outcome: &mut SearchOutcome<(StandardTopology, Vec<usize>)>,
    ) -> Result<(), CodecError> {
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
                face + 1,
                assignment_domains,
                selected,
                edge_rows,
                vertex_points,
                edge_candidates,
                port_identities,
                budget,
                candidate_gauge,
                outcome,
            )?;
            selected.pop();
            if outcome.is_closed() {
                return Ok(());
            }
        }
        Ok(())
    }

    let mut outcome = SearchOutcome::Open;
    visit(
        ctx,
        0,
        assignment_domains,
        &mut Vec::new(),
        edge_rows,
        vertex_points,
        edge_candidates,
        port_identities,
        budget,
        candidate_gauge,
        &mut outcome,
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
        for edge in 0..edge_candidates.len() {
            if edge_candidates[edge].is_empty() {
                continue;
            }
            let mut incident_faces = Vec::new();
            for (face, choices) in assignments.iter().enumerate() {
                if choices.iter().any(|assignment| {
                    assignment.boundaries.iter().flatten().any(|use_| use_.edge == edge)
                }) {
                    crate::resource::push(ctx, &mut incident_faces, face, "catia_prune_incident_faces")?;
                }
            }
            let before = edge_candidates[edge].len();
            let snapshot = crate::resource::copy_retained_rows(ctx, edge_candidates, "catia_prune_snapshot_rows", "catia_prune_snapshot_pairs")?;
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

impl MeshSelectionSearch<'_, '_> {
    fn should_stop(&self) -> bool {
        self.outcome.is_closed()
    }

    fn has_exact_singleton_endpoint_domains(&self) -> bool {
        self.port_identities.is_some()
            && self
                .edge_candidates
                .iter()
                .all(|candidates| candidates.len() == 1)
    }

    fn selected_edges(&self) -> Result<HashSet<usize>, CodecError> {
        let mut edges = HashSet::new();
        for (face, selected) in self.selected.iter().enumerate() {
            let Some((index, _)) = selected else {
                continue;
            };
            if let Some(assignment) = self.assignments[face].get(*index) {
                for use_ in assignment.boundaries.iter().flatten() {
                    crate::resource::insert_set(self.ctx, &mut edges, use_.edge, "catia_selection_selected_edges")?;
                }
            }
        }
        Ok(edges)
    }

    #[cfg(test)]
    fn remaining_equation_merge_capacity(
        &self,
        quotient: &mut MeshQuotient,
    ) -> Result<Option<usize>, CodecError> {
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
        let mut possible_root_counts = HashMap::<usize, NonZeroUsize>::new();
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
            // The node itself is the component's first root, so the count is
            // never zero.
            let roots = match possible_root_counts.get(&component) {
                Some(roots) => {
                    let Some(next) = roots.checked_add(1) else {
                        return Ok(None);
                    };
                    next
                }
                None => NonZeroUsize::MIN,
            };
            possible_root_counts.insert(component, roots);
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
                required_component_roots(
                    *roots,
                    component_merge_capacity
                        .get(component)
                        .copied()
                        .unwrap_or(0),
                )
            })
            .sum::<usize>();
        if required_root_count > point_count {
            return Ok(None);
        }
        let required_count = |component: &usize| {
            required_component_roots(
                possible_root_counts[component],
                component_merge_capacity
                    .get(component)
                    .copied()
                    .unwrap_or(0),
            )
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
            return Ok(None);
        }
        domains.sort_unstable_by_key(HashSet::len);
        let domains = domains
            .into_iter()
            .map(|domain| domain.into_iter().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        if !domains_have_distinct_matching(
            self.ctx,
            domains.iter().map(Vec::as_slice),
            point_count,
        )? {
            return Ok(None);
        }
        let mut singleton_component = HashMap::new();
        for node in 0..node_count {
            if quotient.union.find(node) != node || quotient.domains[node].len() != 1 {
                continue;
            }
            let Some(point) = quotient.domains[node].iter().next() else {
                return Ok(None);
            };
            let point = *point;
            let component = possible.find(node);
            if singleton_component
                .insert(point, component)
                .is_some_and(|previous| previous != component)
            {
                return Ok(None);
            }
        }
        Ok(Some(before.saturating_sub(after).min(independent_capacity)))
    }

    fn face_projection_signature(
        &self,
        face: usize,
        quotient: &mut MeshQuotient,
    ) -> Result<MeshQuotientSignature, CodecError> {
        let mut root_set = HashSet::new();
        for use_ in self.assignments[face].iter().flat_map(|assignment| &assignment.boundaries).flatten() {
            for node in [use_.edge * 2, use_.edge * 2 + 1] {
                crate::resource::insert_set(self.ctx, &mut root_set, quotient.union.find(node), "catia_face_projection_roots")?;
            }
        }
        let mut roots = Vec::new();
        crate::resource::reserve_vec(self.ctx, &mut roots, root_set.len(), "catia_face_projection_root_order")?;
        roots.extend(root_set);
        roots.sort_unstable();
        let mut signature = Vec::new();
        crate::resource::reserve_vec(self.ctx, &mut signature, roots.len(), "catia_face_projection_signature_rows")?;
        for root in roots {
            let mut domain = Vec::new();
            crate::resource::reserve_vec(self.ctx, &mut domain, quotient.domains[root].len(), "catia_face_projection_domain_points")?;
            domain.extend(quotient.domains[root].iter().copied());
            domain.sort_unstable();
            signature.push((crate::resource::copy_slice(self.ctx, quotient.members(root), "catia_face_projection_member_nodes")?, domain));
        }
        signature.sort_unstable();
        Ok(signature)
    }

    #[cfg(test)]
    fn propagate_forced_face_equations(
        &self,
        quotient: &mut MeshQuotient,
    ) -> Result<bool, CodecError> {
        let budget = WorkBudget::new(usize::MAX);
        self.propagate_forced_face_equations_from(quotient, None, &budget)
    }

    fn propagate_forced_face_equations_from(
        &self,
        quotient: &mut MeshQuotient,
        changed_edges: Option<&HashSet<usize>>,
        budget: &WorkBudget<'_>,
    ) -> Result<bool, CodecError> {
        let mut queue = VecDeque::new();
        let mut queued = HashSet::new();
        for (face, selected) in self.selected.iter().enumerate() {
            if selected.is_none() && changed_edges.is_none_or(|changed_edges| {
                self.assignments[face].iter().flat_map(|assignment| &assignment.boundaries).flatten().any(|use_| changed_edges.contains(&use_.edge))
            }) {
                crate::resource::push_back(self.ctx, &mut queue, face, "catia_forced_equation_queue")?;
                crate::resource::insert_set(self.ctx, &mut queued, face, "catia_forced_equation_queued")?;
            }
        }
        while let Some(face) = queue.pop_front() {
            if !budget.charge() {
                return Ok(true);
            }
            queued.remove(&face);
            if self.selected[face].is_some() {
                continue;
            }
            let before = quotient.clone_charged(self.ctx)?;
            let mut changed = false;
            let deterministic = self.assignments[face].len() == 1
                && self.assignments[face][0]
                    .boundaries
                    .iter()
                    .flatten()
                    .all(|use_| use_.reversed.is_some());
            let equations = if deterministic {
                let [choice] = self.possible_face_choices[face].as_slice() else {
                    return Ok(false);
                };
                crate::resource::copy_slice(self.ctx, choice, "catia_forced_deterministic_equations")?
            } else {
                let cache_key = (face, self.face_projection_signature(face, quotient)?);
                let cached = {
                    let cache = self.face_equation_cache.borrow();
                    cache.get(&cache_key).map(|equations| crate::resource::copy_slice(self.ctx, equations, "catia_forced_cached_equations")).transpose()?
                };
                if let Some(cached) = cached {
                    cached
                } else {
                    let Some(common) = common_supported_corner_equations(
                        self.ctx,
                        quotient,
                        &self.assignments[face],
                        budget,
                    )?
                    else {
                        return Ok(budget.exhausted());
                    };
                    let mut equations = Vec::new();
                    crate::resource::reserve_vec(self.ctx, &mut equations, common.len(), "catia_forced_common_equations")?;
                    equations.extend(common);
                    let cached_equations = crate::resource::copy_retained_slice(self.ctx, &equations, "catia_forced_cached_equation_copy")?;
                    let mut cache = self.face_equation_cache.borrow_mut();
                    if cache.len() >= MAX_FACE_EQUATION_CACHE_ENTRIES {
                        cache.clear();
                    }
                    let Some(key_row_bytes) = cache_key.1.len().checked_mul(std::mem::size_of::<(Vec<usize>, Vec<usize>)>()) else {
                        return Err(self.ctx.refuse_codec_limit("catia_forced_equation_cache_key", u64::MAX, u64::MAX));
                    };
                    let Some(key_bytes) = cache_key.1.iter().try_fold(
                        key_row_bytes,
                        |total, (members, domain)| {
                            total
                                .checked_add(members.len().checked_mul(std::mem::size_of::<usize>())?)?
                                .checked_add(domain.len().checked_mul(std::mem::size_of::<usize>())?)
                        },
                    ).and_then(|bytes| u64::try_from(bytes).ok()) else {
                        return Err(self.ctx.refuse_codec_limit("catia_forced_equation_cache_key", u64::MAX, u64::MAX));
                    };
                    self.ctx.charge_retained(key_bytes, "catia_forced_equation_cache_key")?;
                    crate::resource::insert_map(self.ctx, &mut cache, cache_key, cached_equations, "catia_forced_equation_cache")?;
                    equations
                }
            };
            for [left, right] in equations {
                if quotient.union.find(left) == quotient.union.find(right) {
                    continue;
                }
                let Some(root) = quotient.merge_charged(self.ctx, left, right)? else {
                    return Ok(false);
                };
                if !quotient.propagate_component_edge_domains(
                    self.ctx,
                    root,
                    self.edge_candidates,
                    None,
                )? {
                    return Ok(false);
                }
                changed = true;
            }
            if !changed {
                continue;
            }
            let changed_edges = changed_quotient_edges(self.ctx, &before, quotient)?;
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
                    crate::resource::insert_set(self.ctx, &mut queued, dependent, "catia_forced_equation_queued")?;
                    crate::resource::push_back(self.ctx, &mut queue, dependent, "catia_forced_equation_queue")?;
                }
            }
        }
        Ok(true)
    }

    fn selection_orientable(&self, selection: &[MeshFaceSelection]) -> Result<bool, CodecError> {
        let mut constraints = Vec::<Vec<(usize, bool)>>::new();
        let mut edge_uses = HashMap::<usize, Vec<(usize, bool)>>::new();
        for (face, selected) in selection.iter().enumerate() {
            let Some((assignment_index, directions)) = selected else {
                continue;
            };
            let Some(assignment) = self.assignments[face].get(*assignment_index) else {
                return Ok(false);
            };
            if assignment.boundaries.len() != directions.len() {
                return Ok(false);
            }
            for (boundary, directions) in assignment.boundaries.iter().zip(directions) {
                if boundary.len() != directions.len() {
                    return Ok(false);
                }
                let node = constraints.len();
                crate::resource::push(self.ctx, &mut constraints, Vec::new(), "catia_selection_constraint_nodes")?;
                for (use_, &direction) in boundary.iter().zip(directions) {
                    let reversed = use_.reversed.unwrap_or(direction);
                    if use_.reversed.is_some() && reversed != direction {
                        return Ok(false);
                    }
                    crate::resource::admit_map_entry(self.ctx, &mut edge_uses, &use_.edge, "catia_selection_edge_keys")?;
                    let uses = edge_uses.entry(use_.edge).or_default();
                    if uses.len() == 2 {
                        return Ok(false);
                    }
                    crate::resource::push(self.ctx, uses, (node, reversed), "catia_selection_edge_uses")?;
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
                    return Ok(false);
                }
            } else {
                crate::resource::push(self.ctx, &mut constraints[*left_node], (*right_node, parity), "catia_selection_adjacent_constraints")?;
                crate::resource::push(self.ctx, &mut constraints[*right_node], (*left_node, parity), "catia_selection_adjacent_constraints")?;
            }
        }
        let mut flips = self
            .ctx
            .alloc_filled(constraints.len(), None, "catia_selection_flips")?;
        for root in 0..constraints.len() {
            if flips[root].is_some() {
                continue;
            }
            flips[root] = Some(false);
            let mut stack = Vec::new();
            crate::resource::push(self.ctx, &mut stack, root, "catia_selection_orientation_stack")?;
            while let Some(node) = stack.pop() {
                self.ctx
                    .charge_work(1, "catia_selection_orientation_work")?;
                let Some(flip) = flips[node] else {
                    return Ok(false);
                };
                for &(neighbor, parity) in &constraints[node] {
                    let required = flip ^ parity;
                    match flips[neighbor] {
                        Some(existing) if existing != required => return Ok(false),
                        Some(_) => {}
                        None => {
                            flips[neighbor] = Some(required);
                            crate::resource::push(self.ctx, &mut stack, neighbor, "catia_selection_orientation_stack")?;
                        }
                    }
                }
            }
        }
        Ok(true)
    }

    fn selected_orientable(&self) -> Result<bool, CodecError> {
        self.selection_orientable(&self.selected)
    }

    fn fixed_remaining_faces_are_orientable(&self) -> Result<bool, CodecError> {
        let mut completion = Vec::new();
        crate::resource::reserve_vec(self.ctx, &mut completion, self.selected.len(), "catia_selection_completion")?;
        for selected in &self.selected {
            completion.push(match selected {
                Some((index, directions)) => Some((*index, copy_mesh_boundary_directions(self.ctx, directions)?)),
                None => None,
            });
        }
        for (face, selected) in completion.iter_mut().enumerate() {
            if selected.is_some() {
                continue;
            }
            let [assignment] = self.assignments[face].as_slice() else {
                continue;
            };
            self.ctx.charge_collection_items(
                u64::try_from(assignment.boundaries.len()).map_err(|_| {
                    self.ctx.refuse_codec_limit(
                        "catia_selection_completion_boundaries",
                        u64::MAX,
                        u64::MAX,
                    )
                })?,
                "catia_selection_completion_boundaries",
            )?;
            for boundary in &assignment.boundaries {
                self.ctx.charge_collection_items(
                    u64::try_from(boundary.len()).map_err(|_| {
                        self.ctx.refuse_codec_limit(
                            "catia_selection_completion_directions",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?,
                    "catia_selection_completion_directions",
                )?;
            }
            let mut directions = Vec::new();
            crate::resource::reserve_admitted_vec(&mut directions, assignment.boundaries.len(), "catia_selection_completion_boundaries")?;
            let mut complete = true;
            for boundary in &assignment.boundaries {
                if boundary.iter().any(|use_| use_.reversed.is_none()) {
                    complete = false;
                    break;
                }
                let mut row = Vec::new();
                crate::resource::reserve_admitted_vec(&mut row, boundary.len(), "catia_selection_completion_directions")?;
                for use_ in boundary {
                    if let Some(reversed) = use_.reversed {
                        row.push(reversed);
                    }
                }
                directions.push(row);
            }
            if !complete {
                continue;
            }
            *selected = Some((0, directions));
        }
        self.selection_orientable(&completion)
    }

    fn prepare_selected_branch(
        &self,
        quotient: &MeshQuotient,
        changed_edges: &HashSet<usize>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<Option<MeshQuotient>, CodecError> {
        let mut measured = quotient.clone_charged(self.ctx)?;
        if !self.has_exact_singleton_endpoint_domains()
            && !self.propagate_forced_face_equations_from(
                &mut measured,
                Some(changed_edges),
                propagation_budget,
            )?
        {
            return Ok(None);
        }
        if !measured.merge_singleton_coordinate_roots(self.ctx, self.edge_candidates)? {
            return Ok(None);
        }
        let root_count = measured.root_count();
        if root_count < self.vertex_points.len() {
            return Ok(None);
        }
        if root_count == self.vertex_points.len()
            && !self.has_exact_singleton_endpoint_domains()
            && !measured.point_assignment_exists(
                self.ctx,
                self.vertex_points.len(),
                self.edge_candidates,
                Some(propagation_budget),
            )?
        {
            return Ok(None);
        }
        let orientable = if self.has_exact_singleton_endpoint_domains() {
            true
        } else {
            self.fixed_remaining_faces_are_orientable()?
        };
        Ok(orientable.then_some(measured))
    }

    #[cfg(test)]
    pub(crate) fn search(&mut self, quotient: &MeshQuotient) -> Result<(), CodecError> {
        self.search_with_limit(quotient, MAX_MESH_CONSTRAINT_OPERATIONS)
    }

    fn search_with_budget(
        &mut self,
        quotient: &MeshQuotient,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<(), CodecError> {
        self.search_from_state(quotient, false, budget, propagation_budget)
    }

    fn fixed_direction_options(
        &self,
        measured: &MeshQuotient,
        face: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Vec<MeshFixedDirectionOption>, CodecError> {
        let Some(direction_options) = self
            .fixed_face_directions
            .get(face)
            .and_then(Option::as_ref)
        else {
            return Ok(Vec::new());
        };
        let [assignment] = self.assignments[face].as_slice() else {
            return Ok(Vec::new());
        };
        let mut seen = HashSet::<(Vec<Vec<bool>>, Vec<Option<bool>>)>::new();
        let mut output = Vec::new();
        for label_directions in direction_options {
            let mut next_orientations = crate::resource::copy_slice(
                self.ctx,
                &self.fixed_edge_orientations,
                "catia_fixed_next_orientations",
            )?;
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
                self.ctx,
                assignment,
                label_directions,
                &next_orientations,
                budget,
            )?;
            let Some((directions, quotient)) = option else {
                continue;
            };
            let signature = (
                canonical_mesh_boundary_directions(self.ctx, &directions)?,
                crate::resource::copy_slice(
                    self.ctx,
                    &next_orientations,
                    "catia_fixed_orientation_signature",
                )?,
            );
            if crate::resource::insert_set(
                self.ctx,
                &mut seen,
                signature,
                "catia_fixed_direction_signatures",
            )? {
                crate::resource::push(
                    self.ctx,
                    &mut output,
                    (directions, quotient, next_orientations),
                    "catia_fixed_direction_options",
                )?;
            }
        }
        Ok(output)
    }

    fn search_fixed_direction_with_budget(
        &mut self,
        quotient: &MeshQuotient,
        budget: &WorkBudget<'_>,
    ) -> Result<(), CodecError> {
        if self.should_stop() {
            return Ok(());
        }
        if !budget.charge() {
            self.outcome.exhaust();
            return Ok(());
        }
        let mut measured = quotient.clone_charged(self.ctx)?;
        if !measured.merge_singleton_coordinate_roots(self.ctx, self.edge_candidates)? {
            return Ok(());
        }
        if measured.root_count() < self.vertex_points.len() {
            return Ok(());
        }
        if self.visited_states.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let signature = self.selection_state_signature(&measured, true)?;
            if !crate::resource::insert_set(
                self.ctx,
                &mut self.visited_states,
                signature,
                "catia_selection_state_memo",
            )? {
                return Ok(());
            }
        }
        let selected_edges = self.selected_edges()?;
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
            return Ok(());
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
                        return Ok(());
                    }
                    self.fixed_edge_orientations[edge] = Some(orientation);
                    self.search_fixed_direction_with_budget(&measured, budget)?;
                }
                self.fixed_edge_orientations[edge] = None;
                return Ok(());
            }
            let mut selected_assignments = Vec::new();
            let mut directions = Vec::new();
            crate::resource::reserve_vec(self.ctx, &mut selected_assignments, self.selected.len(), "catia_fixed_selected_assignments")?;
            crate::resource::reserve_vec(self.ctx, &mut directions, self.selected.len(), "catia_fixed_selected_directions")?;
            for (face, selected) in self.selected.iter().enumerate() {
                let Some((assignment, selected_directions)) = selected else {
                    return Ok(());
                };
                if *assignment != 0 {
                    return Ok(());
                }
                let Some(source) = self.assignments[face].get(*assignment) else {
                    return Ok(());
                };
                selected_assignments.push(copy_mesh_assignment(self.ctx, source)?);
                directions.push(copy_mesh_boundary_directions(self.ctx, selected_directions)?);
            }
            let Some(port_identities) = self.port_identities else {
                return Ok(());
            };
            let Some(outcome) = resolve_singleton_mesh_selection(
                self.ctx,
                self.edge_rows,
                self.vertex_points,
                self.edge_candidates,
                &selected_assignments,
                &directions,
                port_identities,
                budget,
                self.candidate_gauge,
            )?
            else {
                return Ok(());
            };
            match outcome {
                MeshSolve::Solved((topology, assignment)) => {
                    let candidate = (topology, assignment);
                    let gauge = self.candidate_gauge;
                    let equivalent = if let SearchOutcome::Solved(previous) = &self.outcome {
                        previous == &candidate
                            || mesh_candidates_equivalent_with_context(
                                self.ctx, previous, &candidate, gauge,
                            )?
                    } else {
                        false
                    };
                    self.outcome.record_solved(candidate, |_, _| equivalent);
                }
                MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => {
                    self.outcome.mark_ambiguous();
                }
                MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => self.outcome.exhaust(),
                MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
            }
            return Ok(());
        };
        let options = self.fixed_direction_options(&measured, face, Some(budget))?;
        if budget.exhausted() {
            self.outcome.exhaust();
            return Ok(());
        }
        for (directions, next_quotient, next_orientations) in options {
            if self.should_stop() {
                return Ok(());
            }
            let previous_orientations = std::mem::replace(&mut self.fixed_edge_orientations, next_orientations);
            self.selected[face] = Some((0, directions));
            self.search_fixed_direction_with_budget(&next_quotient, budget)?;
            self.selected[face] = None;
            self.fixed_edge_orientations = previous_orientations;
        }
        Ok(())
    }

    #[cfg(test)]
    fn search_with_limit(
        &mut self,
        quotient: &MeshQuotient,
        limit: usize,
    ) -> Result<(), CodecError> {
        let budget = WorkBudget::new(limit);
        let propagation_budget = WorkBudget::new(limit);
        self.search_from_state(quotient, false, &budget, &propagation_budget)
    }

    fn selection_state_signature(
        &self,
        quotient: &MeshQuotient,
        prepared: bool,
    ) -> Result<MeshSelectionStateSignature, CodecError> {
        let mut quotient = quotient.clone_charged(self.ctx)?;
        let mut selected = Vec::new();
        for choice in &self.selected {
            let copy = match choice {
                Some((assignment, directions)) => Some((
                    *assignment,
                    copy_mesh_boundary_directions(self.ctx, directions)?,
                )),
                None => None,
            };
            crate::resource::push(
                self.ctx,
                &mut selected,
                copy,
                "catia_selection_signature_faces",
            )?;
        }
        Ok((
            prepared,
            selected,
            quotient.signature_charged(self.ctx)?,
            crate::resource::copy_slice(
                self.ctx,
                &self.fixed_edge_orientations,
                "catia_selection_signature_edge_orientations",
            )?,
        ))
    }

    fn search_from_state(
        &mut self,
        quotient: &MeshQuotient,
        prepared: bool,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<(), CodecError> {
        if self.should_stop() {
            return Ok(());
        }
        if !budget.charge() {
            self.outcome.exhaust();
            return Ok(());
        }
        if self.visited_states.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let signature = self.selection_state_signature(quotient, prepared)?;
            if !crate::resource::insert_set(
                self.ctx,
                &mut self.visited_states,
                signature,
                "catia_selection_state_memo",
            )? {
                return Ok(());
            }
        }
        self.search_state(quotient, prepared, budget, propagation_budget)
    }

    fn search_state(
        &mut self,
        quotient: &MeshQuotient,
        prepared: bool,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<(), CodecError> {
        let mut measured = quotient.clone_charged(self.ctx)?;
        if !prepared {
            if !self.has_exact_singleton_endpoint_domains()
                && !self.propagate_forced_face_equations_from(
                    &mut measured,
                    None,
                    propagation_budget,
                )?
            {
                return Ok(());
            }
            if !measured.merge_singleton_coordinate_roots(self.ctx, self.edge_candidates)? {
                return Ok(());
            }
            let root_count = measured.root_count();
            if root_count < self.vertex_points.len() {
                return Ok(());
            }
            if !self.has_exact_singleton_endpoint_domains()
                && root_count == self.vertex_points.len()
                && !measured.point_assignment_exists(
                    self.ctx,
                    self.vertex_points.len(),
                    self.edge_candidates,
                    Some(propagation_budget),
                )?
            {
                if propagation_budget.exhausted() {
                    self.outcome.exhaust();
                }
                return Ok(());
            }
            if !self.has_exact_singleton_endpoint_domains()
                && !self.fixed_remaining_faces_are_orientable()?
            {
                return Ok(());
            }
        }
        let selected_edges = self.selected_edges()?;
        let mut adjacent_faces = HashSet::new();
        if !selected_edges.is_empty() {
            for (face, selected) in self.selected.iter().enumerate() {
                if selected.is_none() && self.assignments[face]
                    .iter()
                    .flat_map(|assignment| &assignment.boundaries)
                    .flatten()
                    .any(|use_| selected_edges.contains(&use_.edge)) {
                    crate::resource::insert_set(self.ctx, &mut adjacent_faces, face, "catia_selection_adjacent_faces")?;
                }
            }
        }
        let adjacent_faces = (!adjacent_faces.is_empty()).then_some(adjacent_faces);
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
                let direction_work =
                    direction_work_estimate(assignments.iter().map(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .filter(|use_| use_.reversed.is_none())
                            .count()
                    }));
                let Some(direction_work) = direction_work else {
                    // The face states more direction choices than the work
                    // counter can hold, so no search over it can finish.
                    budget.exhaust();
                    return None;
                };
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
            self.outcome.exhaust();
            return Ok(());
        }
        let Some((_, supported, _, _, _, face)) = next else {
            let mut selected_assignments = Vec::new();
            let mut directions = Vec::new();
            crate::resource::reserve_vec(self.ctx, &mut selected_assignments, self.selected.len(), "catia_search_selected_assignments")?;
            crate::resource::reserve_vec(self.ctx, &mut directions, self.selected.len(), "catia_search_selected_directions")?;
            for (face, selected) in self.selected.iter().enumerate() {
                let Some((index, selected_directions)) = selected else {
                    return Ok(());
                };
                let Some(assignment) = self.assignments[face].get(*index) else {
                    return Ok(());
                };
                selected_assignments.push(copy_mesh_assignment(self.ctx, assignment)?);
                directions.push(copy_mesh_boundary_directions(self.ctx, selected_directions)?);
            }
            if self
                .edge_candidates
                .iter()
                .all(|candidates| candidates.len() == 1)
            {
                if let Some(port_identities) = self.port_identities {
                    let outcome = resolve_singleton_mesh_selection(
                        self.ctx,
                        self.edge_rows,
                        self.vertex_points,
                        self.edge_candidates,
                        &selected_assignments,
                        &directions,
                        port_identities,
                        budget,
                        self.candidate_gauge,
                    )?;
                    if let Some(outcome) = outcome {
                        match outcome {
                            MeshSolve::Solved((topology, assignment)) => {
                                let candidate = (topology, assignment);
                                let gauge = self.candidate_gauge;
                                let equivalent =
                                    if let SearchOutcome::Solved(previous) = &self.outcome {
                                        previous == &candidate
                                            || mesh_candidates_equivalent_with_context(
                                                self.ctx, previous, &candidate, gauge,
                                            )?
                                    } else {
                                        false
                                    };
                                self.outcome.record_solved(candidate, |_, _| equivalent);
                            }
                            MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => {
                                self.outcome.mark_ambiguous();
                            }
                            MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => {
                                self.outcome.exhaust();
                            }
                            MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
                        }
                        if self.should_stop() {
                            return Ok(());
                        }
                    }
                }
            }
            let mut quotient = measured.clone_charged(self.ctx)?;
            let Some(root_points) = quotient.close_coordinate_roots(
                self.ctx,
                self.vertex_points.len(),
                self.edge_candidates,
                Some(budget),
            )?
            else {
                if budget.exhausted() {
                    self.outcome.exhaust();
                }
                return Ok(());
            };
            let candidate = 'candidate: {
                let Some(mut topology) = reconstruct_mesh_selection(
                    self.ctx,
                    self.edge_rows,
                    self.vertex_points,
                    &selected_assignments,
                    &directions,
                )?
                else {
                    break 'candidate None;
                };
                let mut use_counts = self.ctx.alloc_filled(
                    topology.edge_rows.len(),
                    0usize,
                    "catia_search_edge_uses",
                )?;
                for coedge in topology
                    .faces
                    .iter()
                    .flat_map(|face| &face.boundaries)
                    .flat_map(|boundary| &boundary.coedges)
                {
                    use_counts[coedge.edge_row] += 1;
                }
                if use_counts.iter().any(|count| *count > 2) {
                    break 'candidate None;
                }
                if use_counts.iter().all(|count| *count == 2)
                    && orient_face_cycles(self.ctx, &mut topology.faces)?.is_none()
                {
                    break 'candidate None;
                }
                let Some(edge_vertices) = topology.edge_vertices(self.ctx)? else {
                    break 'candidate None;
                };
                let mut point_assignment = self.ctx.alloc_filled(
                    topology.logical_vertex_count,
                    None,
                    "catia_search_point_assignment",
                )?;
                for (edge, vertices) in edge_vertices.into_iter().enumerate() {
                    for (port, vertex) in vertices.into_iter().enumerate() {
                        let root = quotient.union.find(edge * 2 + port);
                        let Some(&point) = root_points.get(&root) else {
                            break 'candidate None;
                        };
                        match point_assignment[vertex] {
                            Some(stored) if stored != point => break 'candidate None,
                            Some(_) => {}
                            None => point_assignment[vertex] = Some(point),
                        }
                    }
                    let [Some(start), Some(end)] = vertices.map(|vertex| point_assignment[vertex]) else {
                        break 'candidate None;
                    };
                    let points = [start, end];
                    let closed_ports =
                        quotient.union.find(edge * 2) == quotient.union.find(edge * 2 + 1);
                    if !mesh_edge_points_compatible(
                        closed_ports,
                        &self.edge_candidates[edge],
                        points,
                    ) {
                        break 'candidate None;
                    }
                }
                let mut completed_points = Vec::new();
                crate::resource::reserve_vec(self.ctx, &mut completed_points, point_assignment.len(), "catia_search_completed_points")?;
                for point in point_assignment {
                    let Some(point) = point else {
                        break 'candidate None;
                    };
                    completed_points.push(point);
                }
                Some((topology, completed_points))
            };
            if let Some(candidate) = candidate {
                let gauge = self.candidate_gauge;
                let equivalent = if let SearchOutcome::Solved(previous) = &self.outcome {
                    previous == &candidate
                        || mesh_candidates_equivalent_with_context(
                            self.ctx, previous, &candidate, gauge,
                        )?
                } else {
                    false
                };
                self.outcome.record_solved(candidate, |_, _| equivalent);
            }
            return Ok(());
        };
        if supported == 0 {
            return Ok(());
        }
        let remaining_work = budget.remaining();
        if remaining_work == 0 {
            self.outcome.exhaust();
            return Ok(());
        }
        let mut options = Vec::new();
        for assignment_index in 0..self.assignments[face].len() {
            if !budget.charge() {
                self.outcome.exhaust();
                return Ok(());
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
                    self.ctx,
                    assignment,
                    direction_options,
                    remaining,
                    Some(budget),
                )
            } else {
                measured.assignment_options_limited(
                    self.ctx,
                    assignment,
                    self.edge_candidates,
                    &selected_edges,
                    remaining,
                    Some(budget),
                )
            }?;
            if budget.exhausted() {
                self.outcome.exhaust();
                return Ok(());
            }
            crate::resource::reserve_vec(self.ctx, &mut options, assignment_options.len(), "catia_search_assignment_options")?;
            options.extend(assignment_options.into_iter().map(|(directions, next_quotient)| {
                (assignment_index, directions, next_quotient)
            }));
        }
        options.retain_mut(|(_, _, quotient)| quotient.root_count() >= self.vertex_points.len());
        if options.is_empty() {
            return Ok(());
        }
        if let [(assignment_index, directions, next_quotient)] = options.as_slice() {
            let changed_edges = changed_quotient_edges(self.ctx, &measured, next_quotient)?;
            self.selected[face] = Some((*assignment_index, copy_mesh_boundary_directions(self.ctx, directions)?));
            if self.selected_orientable()? {
                if let Some(next_quotient) =
                    self.prepare_selected_branch(next_quotient, &changed_edges, propagation_budget)?
                {
                    // The branch preflight has already run. Continue the
                    // forced suffix without another memo entry or preflight.
                    self.search_state(&next_quotient, true, budget, propagation_budget)?;
                } else if budget.exhausted() || propagation_budget.exhausted() {
                    self.outcome.exhaust();
                }
            }
            self.selected[face] = None;
            return Ok(());
        }
        options.sort_unstable_by(|(left_assignment, left_directions, left_quotient), (right_assignment, right_directions, right_quotient)| {
            let measure = |quotient: &MeshQuotient| {
                (0..quotient.union.len())
                    .filter(|&node| quotient.union.root(node) == node)
                    .fold((0usize, 0u128), |(count, freedom), node| {
                        (count + 1, freedom + quotient.domains[node].len() as u128)
                    })
            };
            measure(left_quotient).cmp(&measure(right_quotient))
                .then_with(|| left_assignment.cmp(right_assignment))
                .then_with(|| left_directions.cmp(right_directions))
        });
        for (assignment_index, directions, next_quotient) in options {
            let changed_edges = changed_quotient_edges(self.ctx, &measured, &next_quotient)?;
            self.selected[face] = Some((assignment_index, directions));
            if self.selected_orientable()? {
                if let Some(next_quotient) = self.prepare_selected_branch(
                    &next_quotient,
                    &changed_edges,
                    propagation_budget,
                )? {
                    // `prepare_selected_branch` has already applied the recursive
                    // entry preflight to this quotient.
                    self.search_from_state(&next_quotient, true, budget, propagation_budget)?;
                } else if budget.exhausted() || propagation_budget.exhausted() {
                    self.outcome.exhaust();
                }
            }
            self.selected[face] = None;
            if self.should_stop() {
                return Ok(());
            }
        }
        Ok(())
    }
}

/// The direction choices a face's endpoint assignments state, summed.
///
/// Each assignment states `2^unknown` choices for its `unknown` boundary uses
/// with no stated reversal. `None` states a figure the work counter cannot
/// hold: more choices than any budget can enumerate.
fn direction_work_estimate(mut unknown_uses: impl Iterator<Item = usize>) -> Option<usize> {
    unknown_uses.try_fold(0usize, |total, unknown| {
        let choices = u32::try_from(unknown)
            .ok()
            .and_then(|unknown| 1usize.checked_shl(unknown))?;
        total.checked_add(choices)
    })
}

fn mesh_assignment_can_merge(
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

pub(super) fn mesh_edge_points_compatible(
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
pub(super) fn parse_standard_mesh_endpoint_candidates(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Result<Option<(StandardTopology, Vec<usize>)>, CodecError> {
    let Some(face_run) = largest_fbb_run(bytes) else {
        return Ok(None);
    };
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((edge_rows, vertex_header)) = parse_edge_tables(ctx, bytes, after_faces)? else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    if edge_rows.len() != edge_faces.len() || edge_rows.len() != edge_candidates.len() {
        return Ok(None);
    }
    let Some(mut assignments) =
        standard_mesh_boundary_assignments(ctx, bytes, edge_faces, Some(edge_candidates))?
    else {
        return Ok(None);
    };
    if assignments.len() != face_count {
        return Ok(None);
    }
    deduplicate_mesh_quotient_assignments(ctx, &mut assignments)?;
    // Standard-row occurrence direction is a face-quotient choice. Complete
    // FBB tables retain their scoped handle equalities in these local ports.
    let Some(port_identities) = crate::solve::missing_edge::edge_port_identities(ctx, bytes)?
    else {
        return Ok(None);
    };
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    resolve_standard_mesh_endpoint_candidates(
        ctx,
        &edge_rows,
        &vertex_points,
        edge_candidates,
        assignments,
        &port_identities,
        None,
        None,
        &budget,
        None,
        None,
        None,
        None,
    )
    .map(MeshSolve::into_option)
}

fn singleton_mesh_boundary_directions(
    ctx: &DecodeContext<'_>,
    boundary: &[MeshBoundaryEdgeCandidate],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_direction_evidence: Option<&[bool]>,
) -> Result<Option<Vec<bool>>, CodecError> {
    if boundary.is_empty() {
        return Ok(None);
    }
    let first = boundary[0];
    let Some(first_pair) = edge_candidates.get(first.edge).and_then(|candidates| candidates.first()).copied() else {
        return Ok(None);
    };
    let first_required = first.reversed.filter(|_| {
        edge_direction_evidence
            .and_then(|evidence| evidence.get(first.edge))
            .copied()
            .unwrap_or(false)
    });
    let mut solutions = Vec::new();
    for first_direction in [false, true] {
        if first_required.is_some_and(|required| required != first_direction)
            || (first_required.is_none() && first_pair[0] == first_pair[1] && first_direction)
        {
            continue;
        }
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
        let mut directions = Vec::new();
        crate::resource::push(ctx, &mut directions, first_direction, "catia_singleton_initial_direction")?;
        let mut valid = true;
        for use_ in &boundary[1..] {
            let Some(pair) = edge_candidates.get(use_.edge).and_then(|candidates| candidates.first()).copied() else {
                return Ok(None);
            };
            let required = use_.reversed.filter(|_| {
                edge_direction_evidence
                    .and_then(|evidence| evidence.get(use_.edge))
                    .copied()
                    .unwrap_or(false)
            });
            let false_direction = pair[0] == current && required.is_none_or(|direction| !direction);
            let true_direction = pair[0] != pair[1]
                && pair[1] == current
                && required.is_none_or(|direction| direction);
            let direction = match (false_direction, true_direction) {
                (true, false) => false,
                (false, true) => true,
                _ => {
                valid = false;
                break;
                }
            };
            current = if direction { pair[0] } else { pair[1] };
            crate::resource::push(ctx, &mut directions, direction, "catia_singleton_direction_step")?;
        }
        if valid && current == first_start {
            crate::resource::push(ctx, &mut solutions, directions, "catia_singleton_direction_solutions")?;
        }
    }
    solutions.sort_unstable();
    solutions.dedup();
    if solutions.len() == 2
        && boundary.iter().all(|use_| {
            use_.reversed.is_none()
                || !edge_direction_evidence
                    .and_then(|evidence| evidence.get(use_.edge))
                    .copied()
                    .unwrap_or(false)
        })
    {
        solutions.truncate(1);
    }
    Ok((solutions.len() == 1).then(|| solutions.remove(0)))
}

fn canonical_singleton_coordinate_cycles(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    directions: &[Vec<bool>],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    fn canonical_cycle(ctx: &DecodeContext<'_>, points: &[usize]) -> Result<Vec<usize>, CodecError> {
        if points.is_empty() {
            return Ok(Vec::new());
        }
        let value = |reversed: bool, start: usize, offset: usize| {
            let until_wrap = points.len() - start;
            let index = if offset >= until_wrap { offset - until_wrap } else { start + offset };
            if reversed { points[points.len() - 1 - index] } else { points[index] }
        };
        let mut best = (false, 0usize);
        for reversed in [false, true] {
            for start in 0..points.len() {
                let candidate = (reversed, start);
                if (0..points.len()).map(|offset| value(reversed, start, offset))
                    .cmp((0..points.len()).map(|offset| value(best.0, best.1, offset))).is_lt() {
                    best = candidate;
                }
            }
        }
        let mut canonical = Vec::new();
        crate::resource::reserve_vec(ctx, &mut canonical, points.len(), "catia_singleton_canonical_cycle")?;
        canonical.extend((0..points.len()).map(|offset| value(best.0, best.1, offset)));
        Ok(canonical)
    }

    let mut cycles = Vec::new();
    crate::resource::reserve_vec(ctx, &mut cycles, assignment.boundaries.len().min(directions.len()), "catia_singleton_cycle_rows")?;
    for (boundary, directions) in assignment.boundaries.iter().zip(directions) {
        if boundary.len() != directions.len() {
            return Ok(None);
        }
        let mut points = Vec::new();
        crate::resource::reserve_vec(ctx, &mut points, boundary.len(), "catia_singleton_cycle_points")?;
        for (use_, &reversed) in boundary.iter().zip(directions) {
            let Some(pair) = edge_candidates.get(use_.edge).and_then(|candidates| candidates.first()).copied() else {
                return Ok(None);
            };
            points.push(if reversed { pair[1] } else { pair[0] });
        }
        cycles.push(canonical_cycle(ctx, &points)?);
    }
    cycles.sort_unstable();
    Ok(Some(cycles))
}

fn reconstruct_singleton_coordinate_topology(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[MeshFaceBoundaryAssignment],
    directions: &[Vec<Vec<bool>>],
) -> Result<Option<StandardTopology>, CodecError> {
    if selected.len() != directions.len() {
        return Ok(None);
    }
    let mut faces = Vec::new();
    crate::resource::reserve_vec(ctx, &mut faces, selected.len(), "catia_singleton_topology_faces")?;
    for (assignment, face_directions) in selected.iter().zip(directions) {
        let mut boundaries = Vec::new();
        crate::resource::reserve_vec(ctx, &mut boundaries, assignment.boundaries.len().min(face_directions.len()), "catia_singleton_topology_boundaries")?;
        for (boundary, directions) in assignment.boundaries.iter().zip(face_directions) {
            if boundary.len() != directions.len() || boundary.is_empty() {
                return Ok(None);
            }
            let mut coedges = Vec::new();
            crate::resource::reserve_vec(ctx, &mut coedges, boundary.len(), "catia_singleton_topology_coedges")?;
            for (use_, &reversed) in boundary.iter().zip(directions) {
                let Some(pair) = edge_candidates.get(use_.edge).and_then(|candidates| candidates.first()).copied() else {
                    return Ok(None);
                };
                let [start_vertex, end_vertex] = if reversed { [pair[1], pair[0]] } else { pair };
                coedges.push(CoedgeUse { edge_row: use_.edge, reversed, start_vertex, end_vertex });
            }
            let Some(boundary) = Boundary::new(coedges) else {
                return Ok(None);
            };
            boundaries.push(boundary);
        }
        faces.push(FaceTopology { boundaries });
    }
    let topology = StandardTopology {
        faces,
        edge_rows: copy_mesh_edge_rows(ctx, edge_rows)?,
        vertex_points: crate::resource::copy_retained_slice(ctx, vertex_points, "catia_singleton_topology_points")?,
        logical_vertex_count: vertex_points.len(),
    };
    let Some(_) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    Ok(Some(topology))
}

fn resolve_mesh_selection_from_quotient(
    ctx: &DecodeContext<'_>,
    topology: StandardTopology,
    mut quotient: MeshQuotient,
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
) -> Result<Option<MeshEndpointResolve>, CodecError> {
    let Some(port_count) = edge_candidates.len().checked_mul(2) else {
        return Ok(None);
    };
    if quotient.union.len() != port_count
        || port_identities.len() != edge_candidates.len()
        || quotient.root_count() != vertex_points.len()
    {
        return Ok(None);
    }
    let Some(edge_vertices) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    if edge_vertices.len() != edge_candidates.len() {
        return Ok(None);
    }
    // The direct path is a fast path only for a unique coordinate matching.
    // Non-unique matchings defer to the full search, which applies the mesh gauge.
    let Some(root_points) =
        quotient.point_assignment(ctx, vertex_points.len(), edge_candidates, Some(budget))?
    else {
        return Ok(None);
    };
    let mut point_assignment = ctx.alloc_filled(
        topology.logical_vertex_count,
        None,
        "catia_merged_mesh_point_assignment",
    )?;
    let mut points_by_identity = HashMap::<u32, usize>::new();
    for (edge, [start, end]) in edge_vertices.iter().copied().enumerate() {
        let mut points = [0; 2];
        for (port, vertex) in [start, end].into_iter().enumerate() {
            let root = quotient.union.find(edge * 2 + port);
            let Some(&point) = root_points.get(&root) else {
                return Ok(None);
            };
            match point_assignment[vertex] {
                Some(stored) if stored != point => return Ok(None),
                Some(_) => {}
                None => point_assignment[vertex] = Some(point),
            }
            points[port] = point;
        }
        let closed_ports = quotient.union.find(edge * 2) == quotient.union.find(edge * 2 + 1);
        if !mesh_edge_points_compatible(closed_ports, &edge_candidates[edge], points) {
            return Ok(None);
        }
        for (identity, point) in port_identities[edge].into_iter().zip(points) {
            match crate::resource::insert_map(ctx, &mut points_by_identity, identity, point, "catia_merged_mesh_identity_points")? {
                Some(previous) if previous != point => return Ok(None),
                _ => {}
            }
        }
    }
    let mut completed = Vec::new();
    crate::resource::reserve_vec(ctx, &mut completed, point_assignment.len(), "catia_merged_mesh_completed_points")?;
    for point in point_assignment {
        let Some(point) = point else {
            return Ok(None);
        };
        completed.push(point);
    }
    Ok(Some(MeshSolve::Solved((topology, completed))))
}

fn reduced_distinct_matching(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<usize>],
    point_count: usize,
    budget: &WorkBudget<'_>,
    excluded: Option<(usize, usize)>,
) -> Result<Option<Vec<usize>>, CodecError> {
    let mut assignment = ctx.alloc_filled(domains.len(), None, "catia_reduced_matching")?;
    let mut used = ctx.alloc_filled(point_count, false, "catia_reduced_matching_used")?;
    let mut remaining = Vec::new();
    for (root, domain) in domains.iter().enumerate() {
        if domain.len() == 1 {
            let point = domain[0];
            if excluded.is_some_and(|(excluded_root, excluded_point)| {
                excluded_root == root && excluded_point == point
            }) || used[point]
            {
                return Ok(None);
            }
            used[point] = true;
            assignment[root] = Some(point);
            continue;
        }
        let mut values = Vec::new();
        crate::resource::reserve_vec(ctx, &mut values, domain.len(), "catia_reduced_matching_domain_values")?;
        values.extend(domain.iter().copied().filter(|point| {
            !used[*point]
                && excluded.is_none_or(|(excluded_root, excluded_point)| {
                    excluded_root != root || excluded_point != *point
                })
        }));
        if values.is_empty() {
            return Ok(None);
        }
        crate::resource::push(ctx, &mut remaining, (root, values), "catia_reduced_matching_remaining_rows")?;
    }
    let mut remaining_domains = Vec::new();
    crate::resource::reserve_vec(ctx, &mut remaining_domains, remaining.len(), "catia_reduced_matching_domain_refs")?;
    remaining_domains.extend(remaining.iter().map(|(_, domain)| domain.as_slice()));
    let Some(matching) = distinct_domain_matching_with_budget(
        ctx,
        remaining_domains,
        point_count,
        Some(budget),
        None,
    )?
    else {
        return Ok(None);
    };
    for ((root, _), point) in remaining.into_iter().zip(matching) {
        assignment[root] = Some(point);
    }
    let mut completed = Vec::new();
    crate::resource::reserve_vec(ctx, &mut completed, assignment.len(), "catia_reduced_matching_completed")?;
    for point in assignment {
        let Some(point) = point else {
            return Ok(None);
        };
        completed.push(point);
    }
    Ok(Some(completed))
}

// The selection owns the complete quotient inputs and the optional gauge. The
// explicit signature keeps the two bounded materialization paths symmetric.
#[allow(clippy::too_many_arguments)]
fn resolve_singleton_mesh_selection(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[MeshFaceBoundaryAssignment],
    directions: &[Vec<Vec<bool>>],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<Option<MeshEndpointResolve>, CodecError> {
    if selected.len() != directions.len()
        || edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
    {
        return Ok(None);
    }
    let Some(topology) =
        reconstruct_mesh_selection(ctx, edge_rows, vertex_points, selected, directions)?
    else {
        return Ok(None);
    };
    let Some(edge_vertices) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    let Some(mut quotient) =
        initial_mesh_quotient(ctx, edge_candidates, vertex_points.len(), port_identities)?
    else {
        return Ok(None);
    };
    let mut port_by_vertex = HashMap::<usize, usize>::new();
    for (edge, [start, end]) in edge_vertices.iter().copied().enumerate() {
        for (port, vertex) in [(0, start), (1, end)] {
            let node = edge * 2 + port;
            if let Some(previous) = crate::resource::insert_map(
                ctx,
                &mut port_by_vertex,
                vertex,
                node,
                "catia_mesh_port_by_vertex",
            )? {
                if quotient.merge_charged(ctx, previous, node)?.is_none() {
                    return Ok(None);
                }
            }
        }
    }
    for (edge, candidates) in edge_candidates.iter().enumerate() {
        let &[[left_point, right_point]] = candidates.as_slice() else {
            return Ok(None);
        };
        let left_root = quotient.union.find(edge * 2);
        let right_root = quotient.union.find(edge * 2 + 1);
        if left_root == right_root && left_point != right_point {
            return Ok(None);
        }
        for root in [left_root, right_root] {
            let retained = quotient.domains[root].len()
                .checked_mul(std::mem::size_of::<usize>())
                .and_then(|bytes| bytes.checked_add(std::mem::size_of::<HashSet<usize>>()))
                .and_then(|bytes| u64::try_from(bytes).ok())
                .ok_or_else(|| ctx.refuse_codec_limit("catia_singleton_root_domain_copy", u64::MAX, u64::MAX))?;
            ctx.charge_retained(retained, "catia_singleton_root_domain_copy")?;
            let mut domain = HashSet::new();
            crate::resource::reserve_set(ctx, &mut domain, quotient.domains[root].len(), "catia_singleton_root_domain_copy")?;
            domain.extend(quotient.domains[root].iter().copied().filter(|point| *point == left_point || *point == right_point));
            if domain.is_empty() {
                return Ok(None);
            }
            quotient.domains[root] = Arc::new(domain);
        }
    }
    let mut roots = Vec::new();
    for node in 0..quotient.union.len() {
        if quotient.union.find(node) == node {
            crate::resource::push(ctx, &mut roots, node, "catia_singleton_root_rows")?;
        }
    }
    if roots.len() != vertex_points.len() {
        return Ok(None);
    }
    let mut root_indices = HashMap::new();
    for (index, &root) in roots.iter().enumerate() {
        crate::resource::insert_map(ctx, &mut root_indices, root, index, "catia_singleton_root_indices")?;
    }
    let mut domain_values = Vec::new();
    crate::resource::reserve_vec(ctx, &mut domain_values, roots.len(), "catia_singleton_domain_rows")?;
    for &root in &roots {
        let domain = &quotient.domains[root];
        if domain.is_empty() {
            return Ok(None);
        }
        let mut values = Vec::new();
        crate::resource::reserve_vec(ctx, &mut values, domain.len(), "catia_singleton_domain_values")?;
        values.extend(domain.iter().copied());
        values.sort_unstable();
        domain_values.push(values);
    }
    let first_assignment =
        reduced_distinct_matching(ctx, &domain_values, vertex_points.len(), budget, None)?;
    let Some(first_assignment) = first_assignment else {
        return Ok(budget
            .exhausted()
            .then_some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))));
    };
    let mut edge_use_counts =
        ctx.alloc_filled(edge_rows.len(), 0usize, "catia_mesh_edge_use_counts")?;
    for use_ in selected
        .iter()
        .flat_map(|assignment| &assignment.boundaries)
        .flatten()
    {
        let Some(count) = edge_use_counts.get_mut(use_.edge) else {
            return Ok(None);
        };
        *count += 1;
    }
    if edge_use_counts.iter().any(|count| *count > 2) {
        return Ok(None);
    }
    let mut materialize =
        |assignment: &[usize]| -> Result<Option<(StandardTopology, Vec<usize>)>, CodecError> {
            if assignment.len() != roots.len() {
                return Ok(None);
            }
            let mut point_assignment = ctx.alloc_filled(
                topology.logical_vertex_count,
                None,
                "catia_selection_singleton_point_assignment",
            )?;
            let mut points_by_identity = HashMap::<u32, usize>::new();
            for (edge, [start, end]) in edge_vertices.iter().copied().enumerate() {
                let mut points = [0; 2];
                for (port, vertex) in [start, end].into_iter().enumerate() {
                    let root = quotient.union.find(edge * 2 + port);
                    let Some(&root) = root_indices.get(&root) else {
                        return Ok(None);
                    };
                    let Some(&point) = assignment.get(root) else {
                        return Ok(None);
                    };
                    match point_assignment[vertex] {
                        Some(stored) if stored != point => return Ok(None),
                        Some(_) => {}
                        None => point_assignment[vertex] = Some(point),
                    }
                    points[port] = point;
                }
                let closed_ports =
                    quotient.union.find(edge * 2) == quotient.union.find(edge * 2 + 1);
                if !mesh_edge_points_compatible(closed_ports, &edge_candidates[edge], points) {
                    return Ok(None);
                }
                for (identity, point) in port_identities[edge].into_iter().zip(points) {
                    match crate::resource::insert_map(ctx, &mut points_by_identity, identity, point, "catia_singleton_identity_points")? {
                        Some(previous) if previous != point => return Ok(None),
                        _ => {}
                    }
                }
            }
            let mut points = Vec::new();
            crate::resource::reserve_vec(ctx, &mut points, point_assignment.len(), "catia_singleton_completed_points")?;
            for point in point_assignment {
                let Some(point) = point else {
                    return Ok(None);
                };
                points.push(point);
            }
            Ok(Some((topology.clone_charged(ctx)?, points)))
        };
    let Some(first) = materialize(&first_assignment)? else {
        return Ok(None);
    };
    let mut ambiguous_roots = Vec::new();
    for (root, domain) in domain_values.iter().enumerate() {
        if domain.len() > 1 {
            crate::resource::push(ctx, &mut ambiguous_roots, root, "catia_singleton_ambiguous_roots")?;
        }
    }
    for root in ambiguous_roots {
        let Some(alternate) = reduced_distinct_matching(
            ctx,
            &domain_values,
            vertex_points.len(),
            budget,
            Some((root, first_assignment[root])),
        )?
        else {
            if budget.exhausted() {
                return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))));
            }
            continue;
        };
        let Some(alternate) = materialize(&alternate)? else {
            continue;
        };
        if !mesh_candidates_equivalent_with_context(ctx, &first, &alternate, candidate_gauge)? {
            return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(()))));
        }
    }
    Ok(Some(MeshSolve::Solved((first.0, first.1))))
}

// The arguments are independent serialized evidence, solver state, and budget
// inputs; grouping them would hide their ownership without reducing coupling.
#[allow(clippy::too_many_arguments)]
fn resolve_singleton_mesh_endpoint_candidates(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    assignments: &[Vec<MeshFaceBoundaryAssignment>],
    port_identities: &[[u32; 2]],
    edge_direction_evidence: Option<&[bool]>,
    budget: &WorkBudget<'_>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<Option<MeshEndpointResolve>, CodecError> {
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
    crate::resource::reserve_vec(ctx, &mut selected, assignments.len(), "catia_singleton_selected_assignments")?;
    crate::resource::reserve_vec(ctx, &mut endpoint_labelled_directions, assignments.len(), "catia_singleton_selected_directions")?;
    for face in assignments {
        let mut seen = HashSet::new();
        let mut first = None;
        for assignment in face {
            let mut directions = Vec::new();
            crate::resource::reserve_vec(ctx, &mut directions, assignment.boundaries.len(), "catia_singleton_direction_rows")?;
            let mut valid = true;
            for boundary in &assignment.boundaries {
                let Some(row) = singleton_mesh_boundary_directions(ctx, boundary, edge_candidates, edge_direction_evidence)? else {
                    valid = false;
                    break;
                };
                directions.push(row);
            }
            if !valid {
                continue;
            }
            let Some(signature) = canonical_singleton_coordinate_cycles(ctx, assignment, &directions, edge_candidates)? else {
                continue;
            };
            if seen.contains(&signature) {
                continue;
            }
            let Some(row_bytes) = signature.len().checked_mul(std::mem::size_of::<Vec<usize>>()) else {
                return Err(ctx.refuse_codec_limit("catia_singleton_signature_retained", u64::MAX, u64::MAX));
            };
            let Some(bytes) = signature.iter().try_fold(row_bytes, |total, row| {
                total.checked_add(row.len().checked_mul(std::mem::size_of::<usize>())?)
            }).and_then(|bytes| u64::try_from(bytes).ok()) else {
                return Err(ctx.refuse_codec_limit("catia_singleton_signature_retained", u64::MAX, u64::MAX));
            };
            ctx.charge_retained(bytes, "catia_singleton_signature_retained")?;
            crate::resource::insert_set(ctx, &mut seen, signature, "catia_singleton_signatures")?;
            if first.is_some() {
                return Ok(None);
            }
            first = Some((copy_mesh_assignment(ctx, assignment)?, directions));
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
        crate::resource::reserve_vec(ctx, &mut point_assignment, vertex_points.len(), "catia_singleton_identity_points")?;
        point_assignment.extend(0..vertex_points.len());
        return Ok(Some(MeshSolve::Solved((topology, point_assignment))));
    }
    // An unresolved coedge direction does not select a point endpoint. It is
    // a row-orientation gauge. Let the exact coordinate binding prove the
    // resulting cycle, then try the endpoint-labelled gauge only when the
    // fixed false direction cannot bind.
    let mut fixed_directions = Vec::new();
    crate::resource::reserve_vec(ctx, &mut fixed_directions, selected.len(), "catia_singleton_fixed_face_rows")?;
    for assignment in &selected {
        let mut face_directions = Vec::new();
        crate::resource::reserve_vec(ctx, &mut face_directions, assignment.boundaries.len(), "catia_singleton_fixed_boundary_rows")?;
        for boundary in &assignment.boundaries {
            if boundary.is_empty() {
                return Ok(None);
            }
            let mut directions = Vec::new();
            crate::resource::reserve_vec(ctx, &mut directions, boundary.len(), "catia_singleton_fixed_directions")?;
            directions.extend(boundary.iter().map(|use_| use_.reversed.unwrap_or(false)));
            face_directions.push(directions);
        }
        fixed_directions.push(face_directions);
    }
    if let Some(resolved) = resolve_singleton_mesh_selection(
        ctx,
        edge_rows,
        vertex_points,
        edge_candidates,
        &selected,
        &fixed_directions,
        port_identities,
        budget,
        candidate_gauge,
    )? {
        match resolved {
            MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
            resolved => return Ok(Some(resolved)),
        }
    }

    resolve_singleton_mesh_selection(
        ctx,
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
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    mut assignments: Vec<Vec<MeshFaceBoundaryAssignment>>,
    port_identities: &[[u32; 2]],
    prepared_quotient: Option<&MeshQuotient>,
    edge_direction_evidence: Option<&[bool]>,
    budget: &WorkBudget<'_>,
    partial_solution_valid: Option<&MeshEndpointSolutionPredicate<'_>>,
    complete_solution_valid: Option<&MeshEndpointSolutionPredicate<'_>>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
    priority_edges: Option<&[bool]>,
) -> Result<MeshEndpointResolve, CodecError> {
    const MAX_SELECTION_WORK: usize = 100_000;
    let face_count = assignments.len();
    let mut edge_candidates = crate::resource::copy_retained_rows(ctx, edge_candidates, "catia_standard_endpoint_choice_rows", "catia_standard_endpoint_choice_pairs")?;
    if !prune_mesh_endpoint_pair_support(ctx, &mut assignments, &mut edge_candidates)? {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }
    let quotient = if let Some(prepared) = prepared_quotient {
        Some(prepared.clone_charged(ctx)?)
    } else {
        initial_mesh_quotient(ctx, &edge_candidates, vertex_points.len(), port_identities)?
    };
    let Some(quotient) = quotient else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    };
    for face in &mut assignments {
        let mut retained = Vec::new();
        for assignment in std::mem::take(face) {
            if quotient.assignment_has_option(ctx, &assignment, &edge_candidates, Some(budget))? {
                crate::resource::push(
                    ctx,
                    &mut retained,
                    assignment,
                    "catia_assignment_retained_face_options",
                )?;
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
        edge_rows,
        vertex_points,
        &edge_candidates,
        &assignments,
        port_identities,
        edge_direction_evidence,
        budget,
        candidate_gauge,
    )? {
        return Ok(resolved);
    }
    let coordinate_domains = if let Some(preparation_limit) = quotient
        .clone_charged(ctx)?
        .coordinate_domain_preparation_limit(vertex_points.len(), &edge_candidates)
    {
        let preparation_budget = budget.session_child_slice(preparation_limit);
        let mut coordinate_quotient = quotient.clone_charged(ctx)?;
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
    crate::resource::reserve_vec(ctx, &mut face_work, assignments.len(), "catia_selection_face_work")?;
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
    let face_equations = possible_face_equations(ctx, &assignments)?;
    let mut face_choices = Vec::new();
    if !possible_face_choices_with_limit(
        ctx,
        &assignments,
        &face_equations,
        MAX_MESH_CONSTRAINT_OPERATIONS,
        &mut face_choices,
    )? {
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
                    ctx,
                    std::slice::from_ref(assignment),
                    &edge_candidates,
                    &unselected,
                    &local_budget,
                )?;
                if !configuration_budget.charge_by(local_budget.consumed())
                    || local_budget.exhausted()
                {
                    None
                } else {
                    configurations
                }
            };
            crate::resource::push(
                ctx,
                &mut face_configurations,
                configurations,
                "catia_face_configuration_assignment_rows",
            )?;
        }
        crate::resource::push(
            ctx,
            &mut endpoint_configurations,
            face_configurations,
            "catia_face_configuration_face_rows",
        )?;
    }
    let relation = resolve_endpoint_configuration_relation_streaming(
        ctx,
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn parse_standard_mesh_candidate_outcome<FP, FC>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_classes: &[usize],
    edge_geometry: &[MeshEdgeGeometry],
    edge_identity_evidence: &[bool],
    edge_direction_evidence: &[bool],
    global_handle_ports: bool,
    partial_constraint_edges: &[bool],
    preferred_assignment_edges: &[bool],
    priority_edges: Option<&[bool]>,
    assignment_dependencies: Option<&[Vec<usize>]>,
    budget: &WorkBudget<'_>,
    partial_solution_valid: FP,
    complete_solution_valid: FC,
) -> Result<MeshCandidateSolve, CodecError>
where
    FP: Fn(&[Option<[usize; 2]>]) -> bool,
    FC: Fn(&[Option<[usize; 2]>]) -> bool,
{
    let endpoint_budget = budget.session_child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
    let Some((face_count, edge_rows, vertex_points, mut mesh_domains, port_identities)) =
        (|| -> Result<Option<_>, CodecError> {
            let Some(face_run) = largest_fbb_run(bytes) else {
                return Ok(None);
            };
            let face_count = face_run.face_count();
            let after_faces = face_run.after_faces();
            let Some((edge_rows, vertex_header)) = parse_edge_tables(ctx, bytes, after_faces)?
            else {
                return Ok(None);
            };
            let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
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
            deduplicate_mesh_quotient_assignments(ctx, std::slice::from_mut(assignments))?;
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
        initial_mesh_quotient(ctx, edge_candidates, vertex_points.len(), &port_identities)?
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::QuotientPreparation,
        )));
    };
    let mut propagated_quotient = mesh_quotient.clone_charged(ctx)?;
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
    let completed_edge_candidates = crate::resource::copy_retained_rows(ctx, edge_candidates, "catia_completed_edge_candidate_rows", "catia_completed_edge_candidate_pairs")?;
    if !mesh_quotient.edge_domains_viable(ctx, &completed_edge_candidates)? {
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
    crate::resource::reserve_vec(ctx, &mut constraint_edges, partial_constraint_edges.len(), "catia_mesh_constraint_edges")?;
    constraint_edges.extend(partial_constraint_edges.iter().zip(preferred_assignment_edges).zip(&class_constraint.active).map(|((partial, preferred), class)| *partial || *preferred || *class));
    let mut assignment_predecessors = ctx.alloc_filled(completed_edge_candidates.len(), None, "catia_mesh_assignment_predecessors")?;
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
    let pair_solutions = visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy(
        ctx,
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
            assignment_order: AssignmentOrder::new(
                Some(&assignment_predecessors),
                assignment_dependencies,
            ),
            valid: &constrained_partial_solution_valid,
        }),
        Some(&endpoint_budget),
        &|pairs| {
            let mut completed = Vec::new();
            crate::resource::reserve_vec(ctx, &mut completed, pairs.len(), "catia_mesh_completed_predicate_pairs")?;
            completed.extend(pairs.iter().copied().map(Some));
            Ok(constrained_complete_solution_valid(&completed))
        },
        &mut |pairs| -> Result<ControlFlow<()>, CodecError> {
            let endpoint_resolution = if let Some(cached) = endpoint_resolution_memo.get(pairs) {
                copy_mesh_endpoint_resolution(ctx, cached)?
            } else {
                let Some(oriented_pairs) =
                    restore_unique_endpoint_pair_orientations(ctx, pairs, &completed_edge_candidates)?
                else {
                    return Ok(ControlFlow::Continue(()));
                };
                let mut singleton = Vec::new();
                crate::resource::reserve_vec(ctx, &mut singleton, oriented_pairs.len(), "catia_mesh_singleton_pair_rows")?;
                for &pair in &oriented_pairs {
                    singleton.push(ctx.alloc_filled(1, pair, "catia_mesh_singleton_pair")?);
                }
                let Some(mut mesh_assignments) =
                    materialize_boundary_domains(ctx, &mesh_domains, &oriented_pairs)?
                else {
                    return Ok(ControlFlow::Continue(()));
                };
                deduplicate_mesh_quotient_assignments(ctx, &mut mesh_assignments)?;
                // This child owns the incidence-to-endpoint relation phase. The
                // complete materialization invoked by that relation takes its
                // own MAX_MESH_CONSTRAINT_OPERATIONS child slice.
                let endpoint_resolution_budget =
                    endpoint_budget.session_child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
                let resolution = resolve_standard_mesh_endpoint_candidates(
                    ctx,
                    &edge_rows,
                    &vertex_points,
                    &singleton,
                    mesh_assignments,
                    &port_identities,
                    None,
                    Some(edge_direction_evidence),
                    &endpoint_resolution_budget,
                    Some(&constrained_partial_solution_valid),
                    Some(&constrained_complete_solution_valid),
                    candidate_gauge,
                    priority_edges,
                )?;
                // Exhaustion is deterministic for this input and child budget.
                // The parent budget only decreases, so retrying the same key
                // cannot turn an exhausted materialization into a solution.
                if endpoint_resolution_memo.len() < MAX_ENDPOINT_RESOLUTION_MEMO_ENTRIES {
                    let endpoint_key = crate::resource::copy_retained_slice(ctx, pairs, "catia_endpoint_memo_key")?;
                    let retained = copy_mesh_endpoint_resolution(ctx, &resolution)?;
                    crate::resource::insert_map(ctx, &mut endpoint_resolution_memo, endpoint_key, retained, "catia_endpoint_memo_entries")?;
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
        },
    )?;
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
        crate::resource::reserve_vec(ctx, &mut assignments, mesh_domains.len(), "catia_mesh_fallback_assignment_rows")?;
        for domain in mesh_domains {
            match domain {
                MeshFaceBoundaryDomain::Ordered(face) => assignments.push(face),
                MeshFaceBoundaryDomain::UnorderedFullCycle(_)
                | MeshFaceBoundaryDomain::DeferredValidation(_) => return Ok(None),
            }
        }
        let resolution = resolve_standard_mesh_endpoint_candidates(
            ctx,
            &edge_rows,
            &vertex_points,
            edge_candidates,
            assignments,
            &port_identities,
            Some(&mesh_quotient),
            Some(edge_direction_evidence),
            &endpoint_budget,
            Some(&constrained_partial_solution_valid),
            Some(&constrained_complete_solution_valid),
            candidate_gauge,
            priority_edges,
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
    let mut solution: Option<(Vec<[usize; 2]>, StandardTopology, Vec<usize>)> = None;
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
                solution = Some((crate::resource::copy_retained_slice(ctx, assignment, "catia_face_domain_solution_assignment")?, topology, point_assignment));
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
        relation_coordinate_candidate_domains(&ctx, &domains, &assigned, &base_candidates)
            .expect("service resource budget"),
        Some(vec![vec![[0, 1]], vec![[1, 2]]]),
    );

    let unknown_domains = vec![vec![MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Deferred,
    }]];
    assert_eq!(
        relation_coordinate_candidate_domains(&ctx, &unknown_domains, &assigned, &base_candidates)
            .expect("service resource budget"),
        Some(base_candidates.clone()),
    );
    assert!(relation_coordinate_candidate_domains(
        &ctx,
        &domains,
        &[Some([2, 3]), None],
        &base_candidates,
    )
    .expect("service resource budget")
    .is_none());

    let mut refused = HashSet::new();
    for cap in 0..32 {
        let result = crate::test_support::with_collection_limit(cap, |ctx| {
            relation_coordinate_candidate_domains(ctx, &domains, &assigned, &base_candidates)
        });
        match result {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            _ => panic!("unexpected relation coordinate candidates"),
        }
    }
    for operation in [
        "catia_relation_coordinate_candidate_pairs",
        "catia_relation_coordinate_candidate_rows",
        "catia_relation_coordinate_possible_rows",
        "catia_relation_coordinate_possible_pairs",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn relation_coordinate_candidates_refuse_before_invalid_edge_result() {
    let domains = [vec![MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Enumerated {
            assignments: vec![0],
            edge_pairs: vec![(1, [0, 1])],
        },
    }]];
    let run = |ctx: &DecodeContext<'_>| {
        relation_coordinate_candidate_domains(ctx, &domains, &[None], &[vec![[0, 1]]])
    };
    assert!(crate::test_support::with_service_context(run)
        .expect("service resource budget")
        .is_none());
    assert!(matches!(
        crate::test_support::with_collection_limit(0, run),
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_relation_coordinate_candidate_pairs"
    ));
}

#[test]
fn endpoint_configuration_helpers_refuse_before_absent_results() {
    let use_edge = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let invalid_assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![use_edge(0), use_edge(1)]],
    };
    let assignment_result = |ctx: &DecodeContext<'_>| {
        endpoint_configuration_for_assignment(ctx, &invalid_assignment, &[[0, 1]])
    };
    assert!(crate::test_support::with_service_context(assignment_result)
        .expect("service resource budget")
        .is_none());
    assert!(matches!(
        crate::test_support::with_collection_limit(0, assignment_result),
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_endpoint_assignment_pair_map"
    ));

    let valid_assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![use_edge(0)]],
    };
    let duplicate_configuration = vec![(0, [0, 1]), (0, [0, 1])];
    let cycle_result = |ctx: &DecodeContext<'_>| {
        endpoint_configuration_cycles_viable(ctx, &valid_assignment, &duplicate_configuration)
    };
    assert!(crate::test_support::with_service_context(cycle_result)
        .expect("service resource budget")
        .is_none());
    assert!(matches!(
        crate::test_support::with_collection_limit(0, cycle_result),
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_endpoint_cycle_pair_map"
    ));

    let pairs = HashMap::from([(0, [0, 1]), (1, [1, 0])]);
    let invalid_boundary = vec![use_edge(0), use_edge(1), use_edge(2)];
    let boundary_result = |ctx: &DecodeContext<'_>| {
        endpoint_configuration_boundary_cycle_viable(ctx, &invalid_boundary, &pairs)
    };
    assert!(crate::test_support::with_service_context(boundary_result)
        .expect("service resource budget")
        .is_none());
    assert!(matches!(
        crate::test_support::with_collection_limit(2, boundary_result),
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_endpoint_cycle_next_states"
    ));
}

#[test]
fn endpoint_configuration_helpers_charge_completed_collections() {
    let use_edge = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![use_edge(0), use_edge(1)]],
    };
    let run = |ctx: &DecodeContext<'_>| {
        let configuration =
            endpoint_configuration_for_assignment(ctx, &assignment, &[[0, 1], [1, 0]])?
                .expect("valid assignment");
        endpoint_configuration_cycles_viable(ctx, &assignment, &configuration)
    };
    assert_eq!(
        crate::test_support::with_service_context(run).expect("service resource budget"),
        Some(true)
    );
    let mut refused = HashSet::new();
    for cap in 0..32 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(Some(true)) => break,
            _ => panic!("unexpected endpoint configuration result"),
        }
    }
    for operation in [
        "catia_endpoint_assignment_pair_map",
        "catia_endpoint_assignment_configuration",
        "catia_endpoint_cycle_pair_map",
        "catia_endpoint_cycle_initial_states",
        "catia_endpoint_cycle_next_states",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn mesh_candidate_rejection_retains_the_failed_solver_stage() {
    catia_test_context!(ctx);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(matches!(
        parse_standard_mesh_candidate_outcome(
            &ctx,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            false,
            &[],
            &[],
            None,
            None,
            &budget,
            |_| true,
            |_| true,
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
                    StandardTopology {
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
fn face_domain_solution_copy_refuses_before_retaining_assignment() {
    let assignments = [vec![[0, 0]]];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(MAX_MESH_TOPOLOGY_OPERATIONS);
        parse_standard_mesh_candidate_outcome_with_face_assignments(
            ctx,
            MeshFaceAssignmentCandidates::Concrete {
                assignments: &assignments,
                face_count: 1,
            },
            &budget,
            |_, _| {
                Ok(MeshSolve::Solved((
                    StandardTopology {
                        faces: Vec::new(),
                        edge_rows: Vec::new(),
                        vertex_points: Vec::new(),
                        logical_vertex_count: 0,
                    },
                    Vec::new(),
                )))
            },
        )
    };
    catia_test_context!(service_ctx);
    assert!(matches!(run(&service_ctx), Ok(MeshSolve::Solved(_))));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (limited_ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture input");
    assert!(matches!(
        run(&limited_ctx),
        Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_face_domain_solution_assignment"
    ));
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
                    StandardTopology {
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
                StandardTopology {
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

    let Some(MeshSolve::Solved((topology, point_assignment))) =
        resolve_endpoint_configuration_relation_streaming(
            &ctx,
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
        .expect("service resource budget")
    else {
        panic!("endpoint configuration relation did not solve the cycle");
    };

    assert_eq!(topology.faces.len(), 1);
    assert_eq!(point_assignment, vec![0, 1, 2]);
}

#[test]
fn endpoint_configuration_relation_charges_covered_and_assigned_edges() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let edge_rows = (0..3)
        .map(|_| EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let vertex_points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
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
    let configurations = vec![vec![Some(vec![vec![
        (0, [0, 1]),
        (1, [1, 2]),
        (2, [0, 2]),
    ]])]];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        resolve_endpoint_configuration_relation_streaming(
            ctx,
            &assignments,
            &configurations,
            &edge_candidates,
            &edge_rows,
            &vertex_points,
            &[[0, 1], [2, 3], [4, 5]],
            &budget,
            None,
            None,
            None,
            None,
            None,
        )
    };
    catia_test_context!(service_ctx);
    assert!(matches!(
        run(&service_ctx).expect("service resource budget"),
        Some(MeshSolve::Solved(_))
    ));

    let mut refused = HashSet::new();
    let mut limit = 0;
    let mut completed = false;
    for _ in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation);
                limit = error.used + error.additional;
            }
            Ok(Some(MeshSolve::Solved(_))) => {
                completed = true;
                break;
            }
            Ok(outcome) => panic!("cycle relation must solve, got {outcome:?}"),
            Err(error) => panic!("unexpected relation refusal: {error}"),
        }
    }
    assert!(completed, "adaptive caps must admit the cycle relation");
    assert!(refused.contains("catia_endpoint_relation_covered"));
    assert!(refused.contains("catia_endpoint_relation_assigned"));
    for operation in [
        "catia_relation_point_set",
        "catia_relation_memo_selection_rows",
        "catia_relation_memo_selection_values",
        "catia_relation_canonical_pairs",
        "catia_relation_state_memo",
        "catia_relation_assignment_boundary_values",
        "catia_relation_assignment_boundaries",
        "catia_relation_assignment_choices",
        "catia_relation_assignment_domains",
        "catia_relation_candidate_pair",
        "catia_relation_candidate_rows",
        "catia_relation_selected_assignments",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn singleton_mesh_selection_charges_matching_and_materialization_arrays() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let edge_rows = (0..3)
        .map(|_| EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let vertex_points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];
    let selected = [MeshFaceBoundaryAssignment {
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
    }];
    let directions = [vec![vec![false, false, false]]];
    let identities = [[0, 1], [1, 2], [2, 0]];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        resolve_singleton_mesh_selection(
            ctx,
            &edge_rows,
            &vertex_points,
            &candidates,
            &selected,
            &directions,
            &identities,
            &budget,
            None,
        )
    };
    catia_test_context!(service_ctx);
    assert!(matches!(
        run(&service_ctx).expect("service resource budget"),
        Some(MeshSolve::Solved(_))
    ));

    let mut refused = HashSet::new();
    let mut limit = 0;
    let mut completed = false;
    for _ in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation);
                limit = error.used + error.additional;
            }
            Ok(Some(MeshSolve::Solved(_))) => {
                completed = true;
                break;
            }
            Ok(outcome) => panic!("singleton cycle must solve, got {outcome:?}"),
            Err(error) => panic!("unexpected singleton refusal: {error}"),
        }
    }
    assert!(completed, "adaptive caps must admit the singleton cycle");
    for operation in [
        "catia_reduced_matching",
        "catia_reduced_matching_used",
        "catia_reduced_matching_completed",
        "catia_singleton_root_domain_copy",
        "catia_singleton_root_rows",
        "catia_singleton_root_indices",
        "catia_singleton_domain_rows",
        "catia_singleton_domain_values",
        "catia_singleton_identity_points",
        "catia_singleton_completed_points",
        "catia_mesh_edge_use_counts",
        "catia_selection_singleton_point_assignment",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn general_mesh_search_charges_unselected_and_face_state_arrays() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let edge_rows = (0..2)
        .map(|_| EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let vertex_points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let candidates = [vec![[0, 0], [1, 1]], vec![[0, 0], [1, 1]]];
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    }]];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        resolve_standard_mesh_endpoint_candidates(
            ctx,
            &edge_rows,
            &vertex_points,
            &candidates,
            assignments.clone(),
            &[[0, 0], [1, 1]],
            None,
            None,
            &budget,
            None,
            None,
            None,
            None,
        )
    };
    catia_test_context!(service_ctx);
    run(&service_ctx).expect("service resource budget");

    let mut refused = HashSet::new();
    let mut limit = 0;
    let mut completed = false;
    for _ in 0..512 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation);
                limit = error.used + error.additional;
            }
            Ok(_) => {
                completed = true;
                break;
            }
            Err(error) => panic!("unexpected general search refusal: {error}"),
        }
    }
    assert!(
        completed,
        "adaptive caps must admit the general search fixture: limit {limit}, refusals {refused:?}"
    );
    for operation in [
        "catia_endpoint_unselected_edges",
        "catia_mesh_fixed_face_directions",
        "catia_mesh_selected_faces",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn fixed_mesh_search_charges_edge_direction_and_selection_arrays() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let edge_rows = (0..2)
        .map(|_| EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let vertex_points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let candidates = [vec![[0, 0]], vec![[1, 1]]];
    let selected = [MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    }];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        resolve_fixed_mesh_endpoint_pairs(
            ctx,
            MeshEndpointGeometry {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
            },
            &candidates,
            &selected,
            &[[0, 0], [1, 1]],
            &budget,
            None,
        )
    };
    catia_test_context!(service_ctx);
    run(&service_ctx).expect("service resource budget");

    let mut refused = HashSet::new();
    let mut limit = 0;
    let mut completed = false;
    for _ in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation);
                limit = error.used + error.additional;
            }
            Ok(_) => {
                completed = true;
                break;
            }
            Err(error) => panic!("unexpected fixed search refusal: {error}"),
        }
    }
    assert!(
        completed,
        "adaptive caps must admit the fixed search fixture"
    );
    assert!(refused.contains("catia_fixed_mesh_edge_directions"));
    assert!(refused.contains("catia_fixed_mesh_selection"));
    for operation in [
        "catia_fixed_assignment_domain_rows",
        "catia_fixed_assignment_domain_entries",
        "catia_fixed_assignment_boundary_rows",
        "catia_fixed_assignment_boundary_uses",
        "catia_fixed_endpoint_pairs",
        "catia_fixed_face_direction_rows",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn fixed_mesh_direction_overflow_charges_general_face_state() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    const BOUNDARY_COUNT: usize = 13;
    let edge_count = BOUNDARY_COUNT * 2 + 1;
    let edge_rows = (0..edge_count)
        .map(|_| EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let candidates = vec![vec![[0, 1]]; edge_count];
    let identities = (0..edge_count)
        .map(|edge| [(edge * 2) as u32, (edge * 2 + 1) as u32])
        .collect::<Vec<_>>();
    let selected = [MeshFaceBoundaryAssignment {
        boundaries: (0..BOUNDARY_COUNT)
            .map(|boundary| {
                vec![
                    MeshBoundaryEdgeCandidate {
                        edge: boundary * 2,
                        start: 0,
                        end: 1,
                        reversed: Some(false),
                    },
                    MeshBoundaryEdgeCandidate {
                        edge: boundary * 2 + 1,
                        start: 1,
                        end: 0,
                        reversed: None,
                    },
                ]
            })
            .collect(),
    }];
    let vertex_points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(1);
        resolve_fixed_mesh_endpoint_pairs(
            ctx,
            MeshEndpointGeometry {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
            },
            &candidates,
            &selected,
            &identities,
            &budget,
            None,
        )
    };
    catia_test_context!(service_ctx);
    run(&service_ctx).expect("service resource budget");

    let mut refused = HashSet::new();
    let mut limit = 0;
    let mut completed = false;
    let mut last_operation = "";
    for _ in 0..4_096 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                last_operation = error.operation;
                refused.insert(error.operation);
                limit = error.used + error.additional;
            }
            Ok(_) => {
                completed = true;
                break;
            }
            Err(error) => panic!("unexpected overflow-search refusal: {error}"),
        }
    }
    if !completed {
        assert!(last_operation.starts_with("catia_endpoint_"));
        let mut low = limit;
        let mut high = 1_000_000u64;
        while low < high {
            let mid = low + (high - low) / 2;
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = mid;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            match run(&ctx) {
                Err(CodecError::ResourceLimit(error))
                    if error.operation.starts_with("catia_endpoint_") => low = mid + 1,
                Err(CodecError::ResourceLimit(_)) | Ok(_) => high = mid,
                Err(error) => panic!("unexpected overflow-search refusal: {error}"),
            }
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = low;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                refused.insert(error.operation);
                assert_eq!(error.operation, "catia_general_mesh_fixed_face_directions");
            }
            _ => panic!("expected the first post-enumeration charge"),
        }
        completed = true;
    }
    assert!(completed, "adaptive caps must admit the overflow fixture");
    assert!(refused.contains("catia_general_mesh_fixed_face_directions"));
}

#[test]
fn fixed_endpoint_pairs_materialize_duplicate_boundary_assignments() {
    catia_test_context!(ctx);
    let edge_rows = (0..3)
        .map(|_| EdgeRow {
            kind: 1,
            handles: Vec::new(),
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
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
        &assignments,
        &endpoint_configurations,
        &edge_candidates,
        &edge_rows,
        &vertex_points,
        &[[0, 1], [2, 3], [4, 5]],
        &budget,
        None,
        None,
        None,
        None,
        None,
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
            ctx,
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

    let mut refused = HashSet::new();
    for cap in 0..32 {
        let mut cap_covered = [false];
        let result = crate::test_support::with_collection_limit(cap, |ctx| {
            collect_endpoint_relation_face_choices(
                ctx,
                &face_assignments,
                &face_configurations,
                &mut cap_covered,
            )
        });
        match result {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            _ => panic!("unexpected face-choice result"),
        }
    }
    for operation in [
        "catia_endpoint_relation_config_pairs",
        "catia_endpoint_relation_configuration_keys",
        "catia_endpoint_relation_configuration_assignments",
        "catia_endpoint_relation_face_choices",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn endpoint_relation_face_choices_refuse_before_invalid_edge_result() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let assignments = [assignment.clone(), assignment];
    let configurations = [Some(vec![vec![(0, [0, 0])]]), Some(vec![vec![(1, [0, 0])]])];
    let run = |ctx: &DecodeContext<'_>| {
        let mut covered = [false];
        collect_endpoint_relation_face_choices(ctx, &assignments, &configurations, &mut covered)
    };
    assert!(crate::test_support::with_service_context(run)
        .expect("service resource budget")
        .is_none());
    assert!(matches!(
        crate::test_support::with_collection_limit(0, run),
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_endpoint_cycle_pair_map"
    ));
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
        raw_endpoint_relation_state_signature(&ctx, &left, &left_assigned)
            .expect("service resource budget"),
        raw_endpoint_relation_state_signature(&ctx, &right, &right_assigned)
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
fn endpoint_relation_support_masks_propagate_collection_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    let make_domains = |optional_edge: bool| {
        let choice = |id, edge_pairs| MeshEndpointRelationChoice {
            id,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0],
                edge_pairs,
            },
        };
        let mut right = vec![choice(0, vec![(0, [0, 1]), (1, [2, 3])])];
        if optional_edge {
            right.push(choice(1, vec![(0, [0, 1])]));
        }
        vec![vec![choice(0, vec![(0, [0, 1]), (1, [2, 3])])], right]
    };
    for optional_edge in [false, true] {
        let domains = make_domains(optional_edge);
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        catia_test_context!(service_ctx);
        assert!(
            build_endpoint_relation_constraints(&service_ctx, &domains, &budget)
                .expect("service resource budget")
                .is_some()
        );

        let mut refused = HashSet::new();
        for cap in 0..512 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
            match build_endpoint_relation_constraints(&ctx, &domains, &budget) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    refused.insert(limit.operation);
                }
                Ok(Some(_)) => break,
                _ => panic!("unexpected relation outcome"),
            }
        }
        for operation in [
            "catia_endpoint_relation_edge_face_keys",
            "catia_endpoint_relation_edge_faces",
            "catia_endpoint_relation_sorted_faces",
            "catia_endpoint_relation_shared_keys",
            "catia_endpoint_relation_shared_edges",
            "catia_endpoint_relation_shared_rows",
            "catia_endpoint_relation_arcs",
            "catia_endpoint_relation_incoming",
            "catia_endpoint_relation_choice_counts",
            "catia_endpoint_relation_key_values",
            "catia_endpoint_relation_keys",
            "catia_endpoint_relation_support_mask",
            "catia_endpoint_relation_support_rows",
            "catia_endpoint_relation_arc_entries",
            "catia_endpoint_relation_incoming_entries",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
        if !optional_edge {
            assert!(refused.contains("catia_endpoint_relation_index_keys"));
            assert!(refused.contains("catia_endpoint_relation_index_values"));
        }
    }
}

#[test]
fn endpoint_relation_walk_charges_branch_copies_and_nested_sets() {
    let choices = vec![
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
                edge_pairs: vec![(0, [0, 1])],
            },
        },
    ];
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    };
    let assignments = [vec![assignment.clone(), assignment]];
    let constraints = MeshEndpointRelationConstraints {
        arcs: vec![vec![]],
        incoming: vec![vec![]],
        choice_counts: vec![2],
    };
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        let mut memo = HashSet::new();
        walk_endpoint_relation_domains(
            ctx,
            vec![choices.clone()],
            &assignments,
            vec![None],
            &constraints,
            2,
            &budget,
            &mut memo,
            None,
            Some(&[true]),
            None,
            None,
            None,
            &mut |_, _| Ok(false),
        )
    };
    assert!(!crate::test_support::with_service_context(run).expect("service resource budget"));
    let mut refused = HashSet::new();
    for cap in 0..256 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(false) => break,
            _ => panic!("unexpected endpoint relation walk result"),
        }
    }
    for operation in [
        "catia_endpoint_relation_possible_points",
        "catia_endpoint_relation_state_memo",
        "catia_endpoint_relation_priority_edges",
        "catia_endpoint_relation_priority_counts",
        "catia_endpoint_relation_assigned_points",
        "catia_endpoint_relation_support_choice_points",
        "catia_endpoint_relation_support_point_keys",
        "catia_endpoint_relation_score_points",
        "catia_endpoint_relation_branch_order",
        "catia_endpoint_relation_copy_assignments",
        "catia_endpoint_relation_copy_edge_pairs",
        "catia_endpoint_relation_copy_choices",
        "catia_endpoint_relation_copy_faces",
        "catia_endpoint_relation_branch_choice",
        "catia_endpoint_relation_branch_assigned",
        "catia_endpoint_relation_completed_pairs",
        "catia_endpoint_relation_selected_assignments",
        "catia_endpoint_relation_selection_faces",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn endpoint_relation_active_masks_propagate_collection_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    let choice = MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Enumerated {
            assignments: vec![0],
            edge_pairs: vec![(0, [0, 1])],
        },
    };
    let domains = vec![vec![choice.clone()], vec![choice]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    catia_test_context!(service_ctx);
    let constraints = build_endpoint_relation_constraints(&service_ctx, &domains, &budget)
        .expect("service resource budget")
        .expect("shared edge creates relation constraints");
    let mut service_domains = domains.clone();
    assert!(propagate_endpoint_relation_domains(
        &service_ctx,
        &mut service_domains,
        &mut [None],
        &constraints,
        &budget,
    )
    .expect("service resource budget"));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut limited_domains = domains.clone();
    let error = propagate_endpoint_relation_domains(
        &ctx,
        &mut limited_domains,
        &mut [None],
        &constraints,
        &budget,
    )
    .expect_err("active mask exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_endpoint_relation_active_mask"));

    let mut refused = HashSet::new();
    for cap in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        let mut candidate_domains = domains.clone();
        match propagate_endpoint_relation_domains(
            &ctx,
            &mut candidate_domains,
            &mut [None],
            &constraints,
            &budget,
        ) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                refused.insert(limit.operation);
            }
            Ok(true) => break,
            _ => panic!("unexpected relation propagation outcome"),
        }
    }
    for operation in [
        "catia_endpoint_relation_active_mask",
        "catia_endpoint_relation_active_rows",
        "catia_endpoint_relation_queue",
        "catia_endpoint_relation_pair_keys",
        "catia_endpoint_relation_pair_values",
        "catia_endpoint_relation_count_keys",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn endpoint_relation_dirty_faces_refuse_before_empty_domain() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let choice = MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Enumerated {
            assignments: vec![0],
            edge_pairs: vec![(0, [0, 1])],
        },
    };
    let domains = vec![vec![choice.clone()], vec![choice]];
    catia_test_context!(service_ctx);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let constraints = build_endpoint_relation_constraints(&service_ctx, &domains, &budget)
        .expect("service resource budget")
        .expect("shared edge creates relation constraints");
    let mut empty = domains.clone();
    empty[0].clear();
    assert!(!propagate_endpoint_relation_domains(
        &service_ctx,
        &mut empty,
        &mut [None],
        &constraints,
        &budget,
    )
    .expect("service resource budget"));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture fits input limit");
    let mut empty = domains;
    empty[0].clear();
    assert!(matches!(
        propagate_endpoint_relation_domains(
            &ctx,
            &mut empty,
            &mut [None],
            &constraints,
            &budget,
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia_endpoint_relation_dirty_faces"
    ));
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
            ctx,
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

    let mut refused = HashSet::new();
    for cap in 0..256 {
        let limited_budget = WorkBudget::new(4);
        let result = crate::test_support::with_collection_limit(cap, |ctx| {
            mesh_face_endpoint_configurations(
                ctx,
                std::slice::from_ref(&assignment),
                &candidates,
                &[None; 3],
                &limited_budget,
            )
        });
        match result {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            _ => panic!("unexpected face configuration result"),
        }
    }
    for operation in [
        "catia_face_configuration_pairs",
        "catia_face_configuration_initial_states",
        "catia_face_configuration_state_pairs",
        "catia_face_configuration_next_states",
        "catia_face_configuration_seen_pairs",
        "catia_face_configuration_seen_keys",
        "catia_face_configuration_boundary_results",
        "catia_face_configuration_combined_rows",
        "catia_face_configuration_next_combined",
        "catia_face_configuration_result_keys",
        "catia_face_configuration_result_rows",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn face_endpoint_configurations_charge_combined_pair_copy() {
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
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(32);
        mesh_face_endpoint_configurations(
            ctx,
            std::slice::from_ref(&assignment),
            &candidates,
            &[None; 2],
            &budget,
        )
    };
    assert_eq!(
        crate::test_support::with_service_context(run).expect("service resource budget"),
        Some(vec![vec![(0, [0, 0]), (1, [0, 0])]])
    );
    let mut refused = HashSet::new();
    for cap in 0..128 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            _ => panic!("unexpected face configuration result"),
        }
    }
    assert!(refused.contains("catia_face_configuration_combined_pairs"));
}

#[test]
fn face_endpoint_configurations_refuse_before_bounded_absence() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let candidates = [vec![[0, 0], [1, 1]]];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(1);
        mesh_face_endpoint_configurations(
            ctx,
            std::slice::from_ref(&assignment),
            &candidates,
            &[None],
            &budget,
        )
    };
    assert!(crate::test_support::with_service_context(run)
        .expect("service resource budget")
        .is_none());
    assert!(matches!(
        crate::test_support::with_collection_limit(0, run),
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_face_configuration_combined_rows"
    ));
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
fn endpoint_configuration_directions_refuse_before_state_and_prefix_growth() {
    use std::collections::BTreeSet;

    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate { edge: 0, start: 0, end: 1, reversed: None },
            MeshBoundaryEdgeCandidate { edge: 1, start: 0, end: 1, reversed: None },
        ]],
    };
    let configuration = vec![(0, [0, 1]), (1, [0, 1])];
    let run = |ctx: &DecodeContext<'_>| endpoint_configuration_directions(ctx, &assignment, &configuration);
    crate::test_support::with_service_context(|ctx| {
        assert_eq!(run(ctx).expect("service budget").expect("directions").len(), 1);
    });
    let mut refusals = BTreeSet::new();
    let mut completed = false;
    for cap in 0..=64 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => {
                refusals.insert(limit.operation);
            }
            Ok(Ok(directions)) => {
                assert_eq!(directions.len(), 1);
                completed = true;
                break;
            }
            _ => panic!("unexpected endpoint direction result"),
        }
    }
    assert!(completed, "fixture must fit the final cap");
    for operation in [
        "catia_endpoint_configuration_pairs",
        "catia_endpoint_initial_alternatives",
        "catia_endpoint_initial_direction",
        "catia_endpoint_initial_states",
        "catia_endpoint_direction_prefix",
        "catia_endpoint_direction_step",
        "catia_endpoint_direction_states",
        "catia_endpoint_boundary_solutions",
        "catia_endpoint_boundary_direction_copy",
        "catia_endpoint_alternative_boundary",
        "catia_endpoint_alternatives",
    ] {
        assert!(refusals.contains(operation), "no refusal at {operation}");
    }
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
            |_, _| true,
        ),
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
        quotient.merge(0, 2).expect("shared left endpoint");
        quotient.merge(1, 3).expect("shared right endpoint");
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
        quotient.merge(0, 2).expect("shared left endpoint");
        quotient.merge(1, 3).expect("shared right endpoint");
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
        "catia_coordinate_closure_component_set",
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
    let mut quotient = MeshQuotient::new(
        (0..4)
            .map(|node| Arc::new(HashSet::from([usize::from(node % 2 != 0)])))
            .collect(),
    );
    quotient.merge(0, 2).expect("shared left endpoint");
    quotient.merge(1, 3).expect("shared right endpoint");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture fits input limit");
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
        quotient.merge(0, 2).expect("shared left endpoint");
        quotient.merge(1, 3).expect("shared right endpoint");
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
                && limit.operation == "catia_coordinate_closure_roots"
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
        }
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
    assert!(refused.contains("catia_quotient_refine_domain_copy"));
    assert!(refused.contains("catia_quotient_refine_domain_points"));
    assert!(refused.contains("catia_quotient_roots_by_point"));
    assert!(refused.contains("catia_quotient_refine_roots"));
    assert!(refused.contains("catia_quotient_refine_all_points"));
}

#[test]
fn local_coordinate_refinement_charges_inner_root_entries() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

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
        }
    }
    assert!(refused.contains("catia_quotient_refine_root_entries"));
}

#[test]
fn local_coordinate_refinement_charges_reached_root_and_point_arrays() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let domains = MeshCoordinateRootDomains {
        domains: Arc::new(vec![vec![0, 1], vec![1, 2], vec![0, 2]]),
        edges: Arc::new(vec![[0, 1], [1, 2]]),
        root_edges: Arc::new(vec![vec![0], vec![0, 1], vec![1]]),
        edge_candidates: Arc::new(vec![vec![[0, 1], [1, 2]], vec![[1, 2], [0, 2]]]),
        coverage_matching: vec![0, 1, 2],
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
        }
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
        .expect("service resource budget")
        .expect("Hall refinement is feasible");
    assert_eq!(service.domains[2], vec![2]);
    let mut refused = HashSet::new();
    for cap in 0..256 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(Some(_)) => break,
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
fn coordinate_root_copies_charge_retained_and_nested_collections() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

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
        .expect("service resource budget")
        .is_some());
    let mut clone_refusals = HashSet::new();
    for cap in 0..64 {
        match crate::test_support::with_collection_limit(cap, unchanged) {
            Err(CodecError::ResourceLimit(limit)) => {
                clone_refusals.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            _ => panic!("unexpected unchanged coordinate domains"),
        }
    }
    for operation in [
        "catia_coordinate_root_clone_domains",
        "catia_coordinate_root_clone_points",
        "catia_coordinate_root_clone_matching",
    ] {
        assert!(
            clone_refusals.contains(operation),
            "no refusal at {operation}"
        );
    }

    let selected =
        |ctx: &DecodeContext<'_>| domains.refine_edge_candidate_arc(ctx, 0, [0, 1], None);
    let selected_domains = crate::test_support::with_service_context(selected)
        .expect("service resource budget")
        .expect("selected coordinate domains");
    let mut selected_refusals = HashSet::new();
    for cap in 0..256 {
        match crate::test_support::with_collection_limit(cap, selected) {
            Err(CodecError::ResourceLimit(limit)) => {
                selected_refusals.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            _ => panic!("unexpected selected coordinate domains"),
        }
    }
    for operation in [
        "catia_coordinate_refine_candidate_rows",
        "catia_coordinate_refine_candidate_pairs",
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

    let changed = |ctx: &DecodeContext<'_>| domains.refine_candidates(ctx, &[vec![[0, 1]]], None);
    assert!(crate::test_support::with_service_context(changed)
        .expect("service resource budget")
        .is_some());
    let mut changed_refusals = HashSet::new();
    for cap in 0..256 {
        match crate::test_support::with_collection_limit(cap, changed) {
            Err(CodecError::ResourceLimit(limit)) => {
                changed_refusals.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            _ => panic!("unexpected changed coordinate domains"),
        }
    }
    assert!(changed_refusals.contains("catia_coordinate_refine_changed_edges"));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root fits the input limit");
    assert!(matches!(
        unchanged(&ctx),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "catia_coordinate_root_clone_domains"
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

    let MeshSolve::Solved((topology, point_assignment)) =
        resolve_singleton_mesh_endpoint_candidates(
            &ctx,
            &edge_rows,
            &vertex_points,
            &edge_candidates,
            &assignments,
            &port_identities,
            None,
            &budget,
            None,
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

    let MeshSolve::Solved((topology, point_assignment)) =
        resolve_singleton_mesh_endpoint_candidates(
            &ctx,
            &edge_rows,
            &vertex_points,
            &edge_candidates,
            &assignments,
            &port_identities,
            None,
            &budget,
            None,
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

    let MeshSolve::Solved((topology, point_assignment)) =
        resolve_singleton_mesh_endpoint_candidates(
            &ctx,
            &edge_rows,
            &vertex_points,
            &edge_candidates,
            &assignments,
            &port_identities,
            None,
            &budget,
            None,
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
    use super::{
        initial_mesh_quotient, resolve_mesh_selection_from_quotient, MAX_MESH_CONSTRAINT_OPERATIONS,
    };
    use crate::families::standard::topology::EdgeBoundaryLayout;
    use crate::families::standard::topology::EdgeRow;
    use crate::families::standard::topology::StandardTopology;
    use cadmpeg_core::decode::WorkBudget;

    #[test]
    fn direct_mesh_quotient_defers_non_unique_matching() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        use std::collections::HashSet;

        catia_test_context!(ctx);
        let edge_rows = (0..3)
            .map(|edge| EdgeRow {
                kind: 1,
                handles: vec![edge as u32],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            })
            .collect::<Vec<_>>();
        let topology = StandardTopology {
            faces: vec![crate::families::standard::topology::FaceTopology {
                boundaries: vec![crate::families::standard::topology::Boundary::new(vec![
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
                .expect("nonempty topology boundary")],
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
        let quotient = initial_mesh_quotient(&ctx, &edge_candidates, 3, &port_identities)
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
        let singleton_quotient =
            initial_mesh_quotient(&ctx, &singleton_candidates, 3, &port_identities)
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
        let mut refused = HashSet::new();
        for limit in 0..=256 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (limited_ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            match run(&limited_ctx) {
                Err(CodecError::ResourceLimit(error)) => {
                    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                    refused.insert(error.operation);
                }
                Ok(Some(super::MeshSolve::Solved(_))) => break,
                Ok(_) => panic!("singleton triangle quotient must resolve directly"),
                Err(error) => panic!("unexpected direct quotient refusal: {error}"),
            }
        }
        for operation in [
            "catia_merged_mesh_point_assignment",
            "catia_merged_mesh_identity_points",
            "catia_merged_mesh_completed_points",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[test]
fn singleton_direction_growth_refuses_before_first_item() {
    let boundary = [MeshBoundaryEdgeCandidate {
        edge: 0,
        start: 0,
        end: 1,
        reversed: None,
    }];
    let candidates = vec![vec![[0, 0]]];
    catia_test_context!(service_ctx);
    assert_eq!(
        singleton_mesh_boundary_directions(&service_ctx, &boundary, &candidates, None)
            .expect("service budget"),
        Some(vec![false])
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (limited_ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture input");
    assert!(matches!(
        singleton_mesh_boundary_directions(&limited_ctx, &boundary, &candidates, None),
        Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_singleton_initial_direction"
    ));
}

#[cfg(test)]
#[test]
fn singleton_cycle_signature_refuses_before_storage() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    };
    let directions = vec![vec![false]];
    let candidates = vec![vec![[0, 0]]];
    catia_test_context!(service_ctx);
    assert_eq!(
        canonical_singleton_coordinate_cycles(&service_ctx, &assignment, &directions, &candidates)
            .expect("service budget"),
        Some(vec![vec![0]])
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (limited_ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture input");
    assert!(matches!(
        canonical_singleton_coordinate_cycles(&limited_ctx, &assignment, &directions, &candidates),
        Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_singleton_cycle_rows"
    ));
}
