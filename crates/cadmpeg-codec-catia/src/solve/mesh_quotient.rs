//! Mesh-quotient constraint solver for standard nested B-rep topology.
//!
//! Closes vertex-coordinate quotients and enumerates face endpoint configurations.

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

#[cfg(test)]
use std::num::NonZeroUsize;

use cadmpeg_core::decode::{work_units, DecodeContext, ScopedReservation, WorkBudget};
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
    mesh_candidates_equivalent_with_context, mesh_candidates_identical_with_context,
    MeshCandidateGauge, MeshEdgeGeometry,
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
use crate::solve::matching::distinct_domain_matching_with_budget;
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
use std::rc::Rc;
#[cfg(test)]
use std::sync::Arc;

pub(in crate::solve) mod coordinate_assignment;
use coordinate_assignment::{point_support_run, MeshCoordinateRootDomains, MeshEndpointCandidates};
#[cfg(test)]
use coordinate_assignment::{MeshImplicitEdgeCandidateSource, MeshImplicitEdgeCandidates};
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
/// orientation selection. The decode session applies its own global work cap.
pub(crate) const MAX_MESH_CONSTRAINT_OPERATIONS: usize = 1_000_000;
/// The relation walk and its endpoint materialization proof are independent
/// bounded phases and each uses the complete mesh-constraint allowance.
pub(crate) const MAX_MESH_TOPOLOGY_OPERATIONS: usize = MAX_MESH_CONSTRAINT_OPERATIONS * 2;
pub(super) type MeshQuotientGaugeState<'storage> = (MeshQuotient<'storage>, BTreeSet<usize>);

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

impl<T> MeshSolve<T> {
    /// Preserve session refusal and refuse a search whose local slice ended.
    pub(crate) fn require_work(
        self,
        ctx: &DecodeContext<'_>,
        budget: &WorkBudget<'_>,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(0, "catia_mesh_topology_work")?;
        if budget.exhausted() || matches!(self, Self::Failed(MeshCandidateFailure::Exhausted(_))) {
            let limit = u64_from_index(budget.consumed());
            let requested = limit.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("catia_mesh_topology_work", u64::MAX - 1, u64::MAX)
            })?;
            return Err(ctx.refuse_codec_limit("catia_mesh_topology_work", limit, requested));
        }
        Ok(self)
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

fn copy_mesh_endpoint_resolution(
    ctx: &DecodeContext<'_>,
    resolution: &MeshEndpointResolve,
) -> Result<MeshEndpointResolve, CodecError> {
    Ok(match resolution {
        MeshSolve::Solved((topology, points)) => MeshSolve::Solved((
            topology.clone_charged(ctx)?,
            ctx.copy_slice(points, "catia_endpoint_memo_points")?,
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

/// Owns solver data and its temporary allocation reservation.
pub(crate) struct ScopedValue<'storage, T> {
    value: T,
    storage: Option<ScopedReservation<'storage>>,
}

impl<T> std::ops::Deref for ScopedValue<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> std::ops::DerefMut for ScopedValue<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: Default> Default for ScopedValue<'_, T> {
    fn default() -> Self {
        Self {
            value: T::default(),
            storage: None,
        }
    }
}

#[cfg(test)]
impl<T: Clone> Clone for ScopedValue<'_, T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            storage: None,
        }
    }
}

#[cfg(test)]
impl<T: PartialEq> PartialEq for ScopedValue<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

#[cfg(test)]
impl<T: std::fmt::Debug> std::fmt::Debug for ScopedValue<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}

/// The ascending unique points a root can take, shared with a live reservation.
pub(crate) type PointDomain<'storage> = Rc<ScopedValue<'storage, Vec<usize>>>;

/// Builds a point domain from points in any order.
pub(crate) fn point_domain<'storage>(
    ctx: &'storage DecodeContext<'_>,
    points: impl IntoIterator<Item = usize>,
    operation: &'static str,
) -> Result<PointDomain<'storage>, CodecError> {
    let (value, storage) = ctx.with_scoped_storage(operation, || {
        let mut domain = ctx.collect_vec(points, operation)?;
        ctx.sort_unstable_by(&mut domain, |value| value, Ord::cmp, operation)?;
        ctx.dedup_vec(&mut domain, operation)?;
        Ok::<_, CodecError>(domain)
    })?;
    Ok(Rc::new(ScopedValue {
        value,
        storage: Some(storage),
    }))
}

/// A test domain outside any decode session.
#[cfg(test)]
pub(crate) fn unscoped_point_domain(
    points: impl IntoIterator<Item = usize>,
) -> PointDomain<'static> {
    let mut points = points.into_iter().collect::<Vec<_>>();
    points.sort_unstable();
    points.dedup();
    Rc::new(ScopedValue {
        value: points,
        storage: None,
    })
}

/// Tests membership in an ascending point list.
pub(crate) fn domain_contains(
    ctx: &DecodeContext<'_>,
    domain: &[usize],
    point: usize,
    operation: &'static str,
) -> Result<bool, CodecError> {
    Ok(ctx.binary_search(domain, &point, operation)?.is_ok())
}

/// The ascending points two ascending lists share. The shorter list is
/// walked and each of its points is searched in the longer one.
fn domain_intersection(
    ctx: &DecodeContext<'_>,
    left: &[usize],
    right: &[usize],
    operation: &'static str,
) -> Result<Vec<usize>, CodecError> {
    let (walked, searched) = if left.len() <= right.len() {
        (left, right)
    } else {
        (right, left)
    };
    let mut shared = Vec::new();
    for &point in ctx.admit_iter(walked, operation)? {
        if domain_contains(ctx, searched, point, operation)? {
            ctx.push_vec(&mut shared, point, operation)?;
        }
    }
    Ok(shared)
}

/// Tests whether two ascending lists share no point, searching each point
/// of the shorter list in the longer one until one is shared.
pub(super) fn domains_disjoint(
    ctx: &DecodeContext<'_>,
    left: &[usize],
    right: &[usize],
    operation: &'static str,
) -> Result<bool, CodecError> {
    let (walked, searched) = if left.len() <= right.len() {
        (left, right)
    } else {
        (right, left)
    };
    Ok(!ctx.any_by(
        walked,
        |point| domain_contains(ctx, searched, *point, operation),
        operation,
    )?)
}

/// Edges whose direction an orientation search has fixed: the caller's
/// edges, which are only looked up, and the edges this search fixed itself.
struct OrientedEdges<'inherited, 'storage> {
    inherited: &'inherited BTreeSet<usize>,
    fixed: BTreeSet<usize>,
    storage: ScopedReservation<'storage>,
}

impl<'inherited, 'storage> OrientedEdges<'inherited, 'storage> {
    fn new(
        ctx: &'storage DecodeContext<'_>,
        inherited: &'inherited BTreeSet<usize>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            inherited,
            fixed: BTreeSet::new(),
            storage: ctx.reserve_scoped(0, "catia_orientation_seen_edges")?,
        })
    }

    /// Fixes `edge` and returns whether its direction was still free.
    fn fix(&mut self, ctx: &DecodeContext<'_>, edge: usize) -> Result<bool, CodecError> {
        if ctx.contains_btree_set(self.inherited, &edge, "catia_orientation_seen_edges")? {
            return Ok(false);
        }
        let fixed = &mut self.fixed;
        self.storage
            .with_storage(|| ctx.insert_btree_set(fixed, edge, "catia_orientation_seen_edges"))
    }

    /// Frees an edge this search fixed.
    fn release(&mut self, ctx: &DecodeContext<'_>, edge: usize) -> Result<(), CodecError> {
        ctx.remove_btree_set(&mut self.fixed, &edge, "catia_orientation_seen_edges")?;
        Ok(())
    }
}

/// Endpoint-node classes of a search state and the points each class can take.
///
/// A quotient is temporary search state. Its domain slots and member rows are
/// held under its own reservation, released when the state is dropped.
pub(crate) struct MeshQuotient<'storage> {
    union: UnionFind<'storage>,
    domains: Vec<PointDomain<'storage>>,
    empty_domain: PointDomain<'storage>,
    members: Vec<ScopedValue<'storage, Vec<usize>>>,
    /// The session whose temporary reservations hold this state's storage.
    /// Only test states have none.
    session: Option<&'storage DecodeContext<'storage>>,
    _storage: Option<ScopedReservation<'storage>>,
}

#[cfg(test)]
impl Clone for MeshQuotient<'_> {
    fn clone(&self) -> Self {
        Self {
            union: self.union.clone(),
            domains: self.domains.clone(),
            empty_domain: Rc::clone(&self.empty_domain),
            members: self.members.clone(),
            session: None,
            _storage: None,
        }
    }
}

pub(super) fn initial_mesh_quotient<'storage>(
    ctx: &'storage DecodeContext<'_>,
    edge_candidates: &[Vec<[usize; 2]>],
    point_count: usize,
    port_identities: &[[u32; 2]],
) -> Result<Option<MeshQuotient<'storage>>, CodecError> {
    if port_identities.len() != edge_candidates.len() {
        return Ok(None);
    }
    let mut all_points = None;
    let mut edge_storage = ctx.reserve_scoped(0, "catia_initial_quotient_domains")?;
    let mut edge_domains = Vec::new();
    for candidates in ctx.admit_iter(edge_candidates, "catia_initial_quotient_edges")? {
        let domain = if candidates.is_empty() {
            if all_points.is_none() {
                let (value, storage) = ctx
                    .with_scoped_storage("catia_initial_quotient_points", || {
                        ctx.collect_vec(0..point_count, "catia_initial_quotient_points")
                    })?;
                all_points = Some(Rc::new(ScopedValue {
                    value,
                    storage: Some(storage),
                }));
            }
            Rc::clone(
                all_points
                    .as_ref()
                    .ok_or_else(|| CodecError::malformed("implicit domain owns all points"))?,
            )
        } else {
            point_domain(
                ctx,
                candidates.iter().flatten().copied(),
                "catia_initial_quotient_candidate_points",
            )?
        };
        if domain.last().is_none_or(|point| *point >= point_count) {
            return Ok(None);
        }
        edge_storage.with_storage(|| {
            ctx.push_vec(&mut edge_domains, domain, "catia_initial_quotient_domains")
        })?;
    }
    let mut quotient = MeshQuotient::from_edge_domains(ctx, &edge_domains)?;
    let mut identity_storage = ctx.reserve_scoped(0, "catia_initial_quotient_identity_nodes")?;
    let valid = identity_storage.with_storage(|| -> Result<bool, CodecError> {
        let mut node_by_identity = HashMap::new();
        for (edge, ports) in ctx
            .admit_iter(port_identities, "catia_initial_quotient_ports")?
            .enumerate()
        {
            for (port, identity) in ports.iter().copied().enumerate() {
                let node = edge * 2 + port;
                if let Some(&previous) = ctx.get_hash_map(
                    &node_by_identity,
                    &identity,
                    "catia_initial_quotient_identity_lookup",
                )? {
                    if quotient.merge_charged(ctx, previous, node)?.is_none() {
                        return Ok(false);
                    }
                } else {
                    ctx.insert_hash_map(
                        &mut node_by_identity,
                        identity,
                        node,
                        "catia_initial_quotient_identities",
                    )?;
                }
            }
        }
        Ok(true)
    })?;
    if !valid {
        return Ok(None);
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
                && limit.operation == "catia_initial_quotient_candidate_points")
    );
}

#[cfg(test)]
#[test]
fn explicit_initial_quotient_does_not_materialize_unused_points() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let quotient = initial_mesh_quotient(&ctx, &[vec![[0, 1]]], 1_000_000, &[[0, 1]])
        .expect("explicit domains fit without the unused million-point domain")
        .expect("valid explicit pair");
    assert_eq!(quotient.union.len(), 2);
    drop(quotient);
    let _released = ctx
        .reserve_scoped(4096, "released initial quotient storage")
        .expect("all temporary bytes released");
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
        };
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
    quotient: &mut MeshQuotient<'_>,
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
                crate::test_support::with_service_context(|ctx| quotient.union.find(ctx, edge * 2))
                    .expect("service forest traversal");
            let right = crate::test_support::with_service_context(|ctx| {
                quotient.union.find(ctx, edge * 2 + 1)
            })
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

impl<'storage> MeshQuotient<'storage> {
    pub(crate) fn clone_charged(
        &self,
        ctx: &'storage DecodeContext<'_>,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "catia_quotient_clone_domains")?;
        let (union, domains, members) = storage.with_storage(|| {
            let union = self
                .union
                .clone_charged(ctx, "catia_quotient_clone_union")?;
            let domains = ctx.try_collect_retained_with(
                &self.domains,
                "catia_quotient_clone_domains",
                |domain| Ok::<_, CodecError>(Rc::clone(domain)),
            )?;
            let members = ctx.collect_indexed_vec(
                self.members.len(),
                "catia_quotient_clone_member_rows",
                |root| {
                    let (value, storage) = ctx
                        .with_scoped_storage("catia_quotient_clone_member_nodes", || {
                            ctx.copy_slice(&self.members[root], "catia_quotient_clone_member_nodes")
                        })?;
                    Ok(ScopedValue {
                        value,
                        storage: Some(storage),
                    })
                },
            )?;
            Ok::<_, CodecError>((union, domains, members))
        })?;
        Ok(Self {
            union,
            domains,
            empty_domain: Rc::clone(&self.empty_domain),
            members,
            session: Some(ctx),
            _storage: Some(storage),
        })
    }

    /// Builds one singleton class per node; `domain_at` supplies each node's
    /// points.
    pub(crate) fn new_charged(
        ctx: &'storage DecodeContext<'_>,
        node_count: usize,
        mut domain_at: impl FnMut(usize) -> Result<PointDomain<'storage>, CodecError>,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "catia_quotient_members")?;
        let (union, domains, members) = storage.with_storage(|| {
            let union = UnionFind::charged(ctx, node_count, "catia_quotient_union")?;
            let domains =
                ctx.collect_indexed_vec(node_count, "catia_quotient_domains", &mut domain_at)?;
            let members =
                ctx.collect_indexed_vec(node_count, "catia_quotient_members", |node| {
                    let (value, storage) =
                        ctx.with_scoped_storage("catia_quotient_member_nodes", || {
                            let mut row = ctx.collection_vec(1, "catia_quotient_member_nodes")?;
                            row.push(node);
                            Ok::<_, CodecError>(row)
                        })?;
                    Ok(ScopedValue {
                        value,
                        storage: Some(storage),
                    })
                })?;
            Ok::<_, CodecError>((union, domains, members))
        })?;
        Ok(Self {
            union,
            domains,
            empty_domain: Rc::new(ScopedValue::default()),
            members,
            session: Some(ctx),
            _storage: Some(storage),
        })
    }

    /// Builds the two endpoint nodes of each edge, both with the edge's domain.
    pub(crate) fn from_edge_domains(
        ctx: &'storage DecodeContext<'_>,
        edge_domains: &[PointDomain<'storage>],
    ) -> Result<Self, CodecError> {
        let node_count = edge_domains
            .len()
            .checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit("catia_quotient_union", u64::MAX, u64::MAX))?;
        Self::new_charged(ctx, node_count, |node| {
            Ok(Rc::clone(&edge_domains[node / 2]))
        })
    }

    /// Converts test domains; inputs that share an allocation keep sharing
    /// their domain.
    #[cfg(test)]
    fn unscoped_domains(domains: &[Arc<HashSet<usize>>]) -> Vec<PointDomain<'static>> {
        let mut converted: Vec<PointDomain<'static>> = Vec::new();
        for (index, domain) in domains.iter().enumerate() {
            let shared = domains[..index]
                .iter()
                .position(|earlier| Arc::ptr_eq(earlier, domain));
            converted.push(match shared {
                Some(earlier) => Rc::clone(&converted[earlier]),
                None => unscoped_point_domain(domain.iter().copied()),
            });
        }
        converted
    }

    /// A test quotient outside any decode session.
    #[cfg(test)]
    pub(crate) fn new(domains: Vec<Arc<HashSet<usize>>>) -> Self {
        Self {
            union: UnionFind::new(domains.len()),
            members: (0..domains.len())
                .map(|node| ScopedValue {
                    value: vec![node],
                    storage: None,
                })
                .collect(),
            domains: Self::unscoped_domains(&domains),
            empty_domain: Rc::new(ScopedValue::default()),
            session: None,
            _storage: None,
        }
    }

    /// Holds the ascending points `build` returns as a domain of this state.
    fn new_domain(
        &self,
        operation: &'static str,
        build: impl FnOnce() -> Result<Vec<usize>, CodecError>,
    ) -> Result<PointDomain<'storage>, CodecError> {
        match self.session {
            Some(session) => {
                let (value, storage) = session.with_scoped_storage(operation, build)?;
                Ok(Rc::new(ScopedValue {
                    value,
                    storage: Some(storage),
                }))
            }
            None => Ok(Rc::new(ScopedValue {
                value: build()?,
                storage: None,
            })),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.domains.len()
    }

    pub(crate) fn find(
        &mut self,
        ctx: &DecodeContext<'_>,
        node: usize,
    ) -> Result<usize, CodecError> {
        self.union.find(ctx, node)
    }

    pub(crate) fn root(&self, ctx: &DecodeContext<'_>, node: usize) -> Result<usize, CodecError> {
        self.union.root(ctx, node)
    }

    pub(crate) fn domains(&self) -> &[PointDomain<'storage>] {
        &self.domains
    }

    fn members(&self, root: usize) -> &[usize] {
        &self.members[root]
    }

    pub(super) fn signature_work(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<usize>, CodecError> {
        let mut work = Some(0usize);
        for node in ctx.admit_iter(0..self.union.len(), "catia_quotient_signature_work")? {
            if self.union.find(ctx, node)? == node {
                work = work.and_then(|work| {
                    work.checked_add(self.members(node).len())?
                        .checked_add(self.domains[node].len())
                });
            }
        }
        Ok(work.map(work_units))
    }

    fn monotone_measure(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<(usize, usize)>, CodecError> {
        let mut root_count = 0usize;
        let mut domain_cardinality = Some(0usize);
        for node in ctx.admit_iter(0..self.union.len(), "catia_quotient_monotone_measure")? {
            if self.union.find(ctx, node)? == node {
                root_count += 1;
                domain_cardinality = domain_cardinality
                    .and_then(|total| total.checked_add(self.domains[node].len()));
            }
        }
        Ok(domain_cardinality.map(|cardinality| (root_count, cardinality)))
    }

    pub(super) fn signature_charged(
        &self,
        ctx: &'storage DecodeContext<'_>,
    ) -> Result<MeshQuotientSignature, CodecError> {
        let mut components = Vec::new();
        for node in ctx.admit_iter(0..self.union.len(), "catia_quotient_signature_roots")? {
            if self.union.root(ctx, node)? != node {
                continue;
            }
            let mut members =
                ctx.copy_slice(self.members(node), "catia_quotient_signature_members")?;
            ctx.sort_unstable_by(
                &mut members,
                |value| value,
                Ord::cmp,
                "catia_quotient_signature_members_sort",
            )?;
            let domain = ctx.copy_slice(&self.domains[node], "catia_quotient_signature_domain")?;
            ctx.push_vec(
                &mut components,
                (members, domain),
                "catia_quotient_signature_components",
            )?;
        }
        ctx.sort_unstable_by(
            &mut components,
            |value| value,
            Ord::cmp,
            "catia_quotient_signature_components_sort",
        )?;
        Ok(components)
    }

    pub(super) fn root_count(&self, ctx: &DecodeContext<'_>) -> Result<usize, CodecError> {
        let mut count = 0;
        for node in ctx.admit_iter(0..self.union.len(), "catia_quotient_root_count")? {
            if self.union.root(ctx, node)? == node {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Joins the classes of two nodes when their domains share a point; the
    /// joined class keeps the shared points.
    pub(crate) fn merge_charged(
        &mut self,
        ctx: &DecodeContext<'_>,
        left: usize,
        right: usize,
    ) -> Result<Option<usize>, CodecError> {
        let left = self.union.find(ctx, left)?;
        let right = self.union.find(ctx, right)?;
        if left == right {
            return Ok(Some(left));
        }
        let intersection = self.new_domain("catia_quotient_intersection", || {
            domain_intersection(
                ctx,
                &self.domains[left],
                &self.domains[right],
                "catia_quotient_intersection",
            )
        })?;
        if intersection.is_empty() {
            return Ok(None);
        }
        self.union.union(ctx, left, right)?;
        let root = self.union.find(ctx, left)?;
        let child = if root == left { right } else { left };
        let mut child_members = std::mem::take(&mut self.members[child]);
        let members = &mut self.members[root];
        match &mut members.storage {
            Some(storage) => storage.with_storage(|| {
                ctx.append_vec(
                    &mut members.value,
                    &mut child_members.value,
                    "catia_quotient_merged_members",
                )
            }),
            None => ctx.append_vec(
                &mut members.value,
                &mut child_members.value,
                "catia_quotient_merged_members",
            ),
        }?;
        self.domains[root] = intersection;
        self.domains[child] = Rc::clone(&self.empty_domain);
        Ok(Some(root))
    }

    pub(crate) fn edge_domains_viable(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<bool, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "catia_quotient_constrained_edges")?;
        let mut edges = Vec::new();
        storage.with_storage(|| {
            for (edge, candidates) in ctx
                .admit_iter(edge_candidates, "catia_quotient_constrained_edges")?
                .enumerate()
            {
                if !candidates.is_empty() {
                    ctx.push_vec(&mut edges, edge, "catia_quotient_constrained_edges")?;
                }
            }
            Ok::<_, CodecError>(())
        })?;
        self.propagate_edge_domains(ctx, &edges, edge_candidates, None)
    }

    /// Appends the edges with candidate pairs among the members of `root`.
    fn push_component_edges(
        &self,
        ctx: &DecodeContext<'_>,
        root: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        edges: &mut Vec<usize>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        for &node in ctx.admit_iter(self.members(root), operation)? {
            let edge = node / 2;
            if !edge_candidates[edge].is_empty() {
                ctx.push_vec(edges, edge, operation)?;
            }
        }
        Ok(())
    }

    fn propagate_component_edge_domains(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        root: usize,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<bool, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "catia_quotient_component_edges")?;
        let mut edges = Vec::new();
        storage.with_storage(|| {
            self.push_component_edges(
                ctx,
                root,
                edge_candidates,
                &mut edges,
                "catia_quotient_component_edges",
            )
        })?;
        self.propagate_edge_domains(ctx, &edges, edge_candidates, budget)
    }

    fn affected_edges_for_nodes(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        nodes: &[usize],
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Result<Vec<usize>, CodecError> {
        let mut affected = Vec::new();
        for &node in ctx.admit_iter(nodes, "catia_quotient_affected_edges")? {
            let root = self.union.find(ctx, node)?;
            self.push_component_edges(
                ctx,
                root,
                edge_candidates,
                &mut affected,
                "catia_quotient_affected_edges",
            )?;
        }
        Ok(affected)
    }

    /// Narrows root domains until every queued edge with candidate pairs has
    /// a supporting pair between its endpoint domains. A narrowed root queues
    /// its edges again. Returns false when a domain empties or the budget
    /// ends the search.
    fn propagate_edge_domains(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        edges: &[usize],
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<bool, CodecError> {
        const SUPPORT: &str = "catia quotient edge support";

        /// Queues each edge with candidate pairs among `members` that is not
        /// already queued. An edge is queued at most once at a time, so the
        /// queue never outgrows its capacity of one slot per edge.
        fn enqueue_edges(
            ctx: &DecodeContext<'_>,
            members: &[usize],
            edge_candidates: &[Vec<[usize; 2]>],
            queue: &mut VecDeque<usize>,
            queued: &mut [bool],
        ) -> Result<(), CodecError> {
            for &node in ctx.admit_iter(members, "catia_quotient_edge_queue")? {
                let edge = node / 2;
                if !edge_candidates[edge].is_empty() && !queued[edge] {
                    queued[edge] = true;
                    queue.push_back(edge);
                }
            }
            Ok(())
        }

        let (mut queue, _queue_reservation) =
            ctx.temporary_queue(edge_candidates.len(), "catia_quotient_edge_queue")?;
        let mut queued_storage = ctx.reserve_scoped(0, "catia_quotient_queued_edges")?;
        let mut queued = queued_storage.with_storage(|| {
            ctx.alloc_filled(edge_candidates.len(), false, "catia_quotient_queued_edges")
        })?;
        for &edge in ctx.admit_iter(edges, "catia_quotient_edge_queue")? {
            if !queued[edge] {
                queued[edge] = true;
                queue.push_back(edge);
            }
        }
        while let Some(edge) = ctx.next_charged(
            &mut std::iter::from_fn(|| queue.pop_front()),
            "catia_mesh_quotient_iteration",
        )? {
            queued[edge] = false;
            let candidates = &edge_candidates[edge];
            if budget.is_some_and(|budget| !budget.charge_by(work_units(candidates.len()))) {
                return Ok(false);
            }
            if candidates.is_empty() {
                continue;
            }
            let start = self.union.find(ctx, edge * 2)?;
            let end = self.union.find(ctx, edge * 2 + 1)?;
            // Supported points form a subset of their domain, so a domain
            // changes exactly when its supported points are fewer.
            if start == end {
                let domain = Rc::clone(&self.domains[start]);
                let supported = self.new_domain("catia_quotient_self_support", || {
                    let mut supported = Vec::new();
                    for pair in ctx.admit_iter(candidates, SUPPORT)? {
                        if pair[0] == pair[1] && domain_contains(ctx, &domain, pair[0], SUPPORT)? {
                            ctx.push_vec(&mut supported, pair[0], "catia_quotient_self_support")?;
                        }
                    }
                    ctx.sort_unstable_by(&mut supported, |point| point, Ord::cmp, SUPPORT)?;
                    ctx.dedup_vec(&mut supported, SUPPORT)?;
                    Ok(supported)
                })?;
                if supported.is_empty() {
                    return Ok(false);
                }
                if supported.len() != domain.len() {
                    self.domains[start] = supported;
                    enqueue_edges(
                        ctx,
                        self.members(start),
                        edge_candidates,
                        &mut queue,
                        &mut queued,
                    )?;
                }
                continue;
            }

            let starts = Rc::clone(&self.domains[start]);
            let ends = Rc::clone(&self.domains[end]);
            let mut support_storage = ctx.reserve_scoped(0, "catia_quotient_pair_support")?;
            let supported = support_storage.with_storage(|| {
                let mut supported = Vec::new();
                for &[left, right] in ctx.admit_iter(candidates, SUPPORT)? {
                    for (from, to) in [(left, right), (right, left)] {
                        if domain_contains(ctx, &starts, from, SUPPORT)?
                            && domain_contains(ctx, &ends, to, SUPPORT)?
                        {
                            ctx.push_vec(
                                &mut supported,
                                [from, to],
                                "catia_quotient_pair_support",
                            )?;
                        }
                    }
                }
                Ok::<_, CodecError>(supported)
            })?;
            if supported.is_empty() {
                return Ok(false);
            }
            for (root, previous, side, operation) in [
                (start, &starts, 0, "catia_quotient_start_support"),
                (end, &ends, 1, "catia_quotient_end_support"),
            ] {
                let narrowed = self.new_domain(operation, || {
                    let mut points =
                        ctx.collect_vec(supported.iter().map(|pair| pair[side]), operation)?;
                    ctx.sort_unstable_by(&mut points, |point| point, Ord::cmp, operation)?;
                    ctx.dedup_vec(&mut points, operation)?;
                    Ok(points)
                })?;
                if narrowed.len() != previous.len() {
                    self.domains[root] = narrowed;
                    enqueue_edges(
                        ctx,
                        self.members(root),
                        edge_candidates,
                        &mut queue,
                        &mut queued,
                    )?;
                }
            }
        }
        Ok(true)
    }

    fn assignment_has_option(
        &self,
        ctx: &'storage DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<bool, CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "catia_assignment_has_option_scratch")?;
        scratch.with_storage(|| {
            struct State<'storage> {
                boundary_index: usize,
                at: usize,
                directions: Vec<bool>,
                quotient: MeshQuotient<'storage>,
            }

            fn advance<'storage>(
                ctx: &'storage DecodeContext<'_>,
                state: &mut State<'storage>,
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
                ctx.push_vec(
                    &mut state.directions,
                    reversed,
                    "catia_assignment_directions",
                )?;
                state.at += 1;
                Ok(true)
            }

            let mut states = Vec::new();
            ctx.push_vec(
                &mut states,
                State {
                    boundary_index: 0,
                    at: 0,
                    directions: Vec::new(),
                    quotient: self.clone_charged(ctx)?,
                },
                "catia_assignment_states",
            )?;
            while let Some(mut state) = ctx.next_charged(
                &mut std::iter::from_fn(|| states.pop()),
                "catia_mesh_quotient_iteration",
            )? {
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
                            directions: ctx
                                .copy_slice(&state.directions, "catia_assignment_direction_copy")?,
                            quotient: state.quotient.clone_charged(ctx)?,
                        };
                        if advance(ctx, &mut next, boundary, reversed)?
                            && next.quotient.edge_domains_viable(ctx, edge_candidates)?
                        {
                            ctx.push_vec(&mut states, next, "catia_assignment_states")?;
                        }
                    }
                    break;
                }
            }
            Ok(false)
        })
    }

    #[cfg(test)]
    fn assignment_options(
        &self,
        ctx: &'storage DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> Vec<(Vec<Vec<bool>>, Self)> {
        const MAX_ORIENTED_OPTIONS: usize = 4_096;

        fn boundary_options<'storage>(
            ctx: &'storage DecodeContext<'_>,
            quotient: MeshQuotient<'storage>,
            boundary: &[MeshBoundaryEdgeCandidate],
            edge_candidates: &[Vec<[usize; 2]>],
        ) -> Vec<(Vec<bool>, MeshQuotient<'storage>)> {
            fn advance<'storage>(
                resources: (&'storage DecodeContext<'_>, &[Vec<[usize; 2]>]),
                boundary: &[MeshBoundaryEdgeCandidate],
                at: usize,
                reversed: bool,
                directions: &mut Vec<bool>,
                mut quotient: MeshQuotient<'storage>,
                output: &mut Vec<(Vec<bool>, MeshQuotient<'storage>)>,
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
                        .merge_charged(ctx, previous_end, current_start)
                        .expect("service merge")
                    else {
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

            fn walk<'storage>(
                ctx: &'storage DecodeContext<'_>,
                boundary: &[MeshBoundaryEdgeCandidate],
                at: usize,
                directions: &mut Vec<bool>,
                mut quotient: MeshQuotient<'storage>,
                edge_candidates: &[Vec<[usize; 2]>],
                output: &mut Vec<(Vec<bool>, MeshQuotient<'storage>)>,
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
                        .merge_charged(ctx, last_end, first_start)
                        .expect("service merge")
                    else {
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
        ctx: &'storage DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        oriented_edges: &BTreeSet<usize>,
        limit: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Vec<MeshOrientationOption<'storage>>, CodecError> {
        struct BoundaryOrientationSearch<
            'storage,
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
        > {
            boundaries: &'input0 [Vec<MeshBoundaryEdgeCandidate>],
            boundary_index: usize,
            at: usize,
            boundary_directions: &'input1 mut Vec<bool>,
            directions: &'input2 mut Vec<Vec<bool>>,
            quotient: MeshQuotient<'storage>,
            edge_candidates: &'input3 [Vec<[usize; 2]>],
            output: &'input4 mut Vec<(Vec<Vec<bool>>, MeshQuotient<'storage>)>,
            seen: &'input5 mut HashMap<u64, Vec<usize>>,
            seen_storage: &'input5 mut ScopedReservation<'storage>,
            direction_storage: &'input5 mut ScopedReservation<'storage>,
            oriented: &'input6 mut OrientedEdges<'input10, 'storage>,
            gaugeable_edges: &'input7 HashSet<usize>,
            limit: usize,
            budget: Option<&'input9 WorkBudget<'input8>>,
        }

        fn walk<'storage>(
            ctx: &'storage DecodeContext<'_>,
            inputs: BoundaryOrientationSearch<'storage, '_, '_, '_, '_, '_, '_, '_, '_, '_, '_, '_>,
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
                seen_storage,
                direction_storage,
                oriented,
                gaugeable_edges,
                limit,
                budget,
            } = inputs;

            let _depth = ctx.enter_nested("catia orientation search")?;
            ctx.charge_work(1, "catia orientation search")?;
            if output.len() >= limit {
                return Ok(());
            }
            if budget.is_some_and(|budget| !budget.charge()) {
                return Ok(());
            }
            if boundary_index == boundaries.len() {
                if admit_orientation_option(ctx, seen, seen_storage, output, directions, &quotient)?
                {
                    ctx.push_vec(
                        output,
                        (copy_mesh_boundary_directions(ctx, directions)?, quotient),
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
                direction_storage.with_storage(|| {
                    ctx.push_vec(
                        directions,
                        std::mem::take(boundary_directions),
                        "catia_orientation_direction_rows",
                    )?;
                    Ok::<_, CodecError>(())
                })?;
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
                        seen_storage,
                        direction_storage,
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
            let first = oriented.fix(ctx, edge)?;
            let mut advance = |reversed: bool,
                               mut quotient: MeshQuotient<'storage>|
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
                direction_storage.with_storage(|| {
                    ctx.push_vec(
                        boundary_directions,
                        reversed,
                        "catia_orientation_boundary_directions",
                    )?;
                    Ok::<_, CodecError>(())
                })?;
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
                        seen_storage,
                        direction_storage,
                        oriented,
                        gaugeable_edges,
                        limit,
                        budget,
                    },
                )?;
                boundary_directions.pop();
                Ok(())
            };
            let gauge_fixed = first
                && ctx.contains_hash_set(gaugeable_edges, &edge, "catia_orientation_gauge")?;
            match boundary[at].reversed {
                Some(reversed) => advance(reversed, quotient)?,
                None if gauge_fixed => advance(false, quotient)?,
                None => {
                    advance(false, quotient.clone_charged(ctx)?)?;
                    advance(true, quotient)?;
                }
            }
            if first {
                oriented.release(ctx, edge)?;
            }
            Ok(())
        }

        if limit == 0 {
            return Ok(Vec::new());
        }
        if ctx.any_by(
            &assignment.boundaries,
            |boundary| Ok(boundary.is_empty()),
            "catia_orientation_empty_boundary",
        )? {
            return Ok(Vec::new());
        }
        // Fix a new edge's direction only while its two endpoint labels remain
        // exchangeable. Distinct domains or prior quotient merges make the
        // direction observable and require both orientations.
        let (gaugeable_edges, _gauge_storage) =
            ctx.with_scoped_storage("catia_orientation_gauge_storage", || {
                let mut direction_union = self
                    .union
                    .clone_charged(ctx, "catia_orientation_direction_union")?;
                let mut gaugeable_edges = HashSet::new();
                for boundary in ctx.admit_iter(&assignment.boundaries, "catia_orientation_gauge")? {
                    for use_ in ctx.admit_iter(boundary, "catia_orientation_gauge")? {
                        let edge = use_.edge;
                        let Some(right_node) =
                            edge.checked_mul(2).and_then(|node| node.checked_add(1))
                        else {
                            continue;
                        };
                        if right_node >= self.domains.len() {
                            continue;
                        }
                        let left_node = right_node - 1;
                        let left_root = direction_union.find(ctx, left_node)?;
                        let right_root = direction_union.find(ctx, right_node)?;
                        let exchangeable = || -> Result<bool, CodecError> {
                            let left = &self.domains[left_root];
                            let right = &self.domains[right_root];
                            Ok(self.members(left_root) == [left_node]
                                && self.members(right_root) == [right_node]
                                && left.len() == right.len()
                                && (Rc::ptr_eq(left, right)
                                    || ctx.equal(
                                        &left[..],
                                        &right[..],
                                        "catia_orientation_gauge",
                                    )?))
                        };
                        if left_root == right_root || exchangeable()? {
                            ctx.insert_hash_set(
                                &mut gaugeable_edges,
                                edge,
                                "catia_orientation_gaugeable_edges",
                            )?;
                        }
                    }
                }
                Ok::<_, CodecError>(gaugeable_edges)
            })?;
        let mut oriented = OrientedEdges::new(ctx, oriented_edges)?;
        if self
            .orientation_variable_count(ctx, assignment, &mut oriented, &gaugeable_edges)?
            .is_some_and(|variables| variables <= 8)
        {
            return self.enumerate_orientation_masks(
                ctx,
                assignment,
                edge_candidates,
                oriented_edges,
                &gaugeable_edges,
                limit,
                budget,
            );
        }
        let mut output = Vec::new();
        let mut seen_storage = ctx.reserve_scoped(0, "catia_orientation_fingerprint_keys")?;
        let mut direction_storage =
            ctx.reserve_scoped(0, "catia_orientation_scratch_directions")?;
        let mut seen = HashMap::<u64, Vec<usize>>::new();
        let mut oriented = OrientedEdges::new(ctx, oriented_edges)?;
        walk(
            ctx,
            BoundaryOrientationSearch {
                boundaries: &assignment.boundaries,
                boundary_index: 0,
                at: 0,
                boundary_directions: &mut Vec::new(),
                directions: &mut Vec::new(),
                quotient: self.clone_charged(ctx)?,
                edge_candidates,
                output: &mut output,
                seen: &mut seen,
                seen_storage: &mut seen_storage,
                direction_storage: &mut direction_storage,
                oriented: &mut oriented,
                gaugeable_edges: &gaugeable_edges,
                limit,
                budget,
            },
        )?;
        Ok(output)
    }

    /// Counts the free directions of an assignment: each unreversed use whose
    /// edge is already oriented, or is not gauge-fixed at its first use.
    /// Stops counting once the count passes eight.
    fn orientation_variable_count(
        &self,
        ctx: &DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        oriented: &mut OrientedEdges<'_, '_>,
        gaugeable_edges: &HashSet<usize>,
    ) -> Result<Option<usize>, CodecError> {
        let mut variables = 0usize;
        let mut boundaries = assignment.boundaries.iter();
        while let Some(boundary) = ctx.next_charged(&mut boundaries, "catia_orientation_plan")? {
            let mut uses = boundary.iter();
            while let Some(use_) = ctx.next_charged(&mut uses, "catia_orientation_plan")? {
                if use_.reversed.is_some() {
                    continue;
                }
                if !(oriented.fix(ctx, use_.edge)?
                    && ctx.contains_hash_set(
                        gaugeable_edges,
                        &use_.edge,
                        "catia_orientation_gauge",
                    )?)
                {
                    variables += 1;
                    if variables > 8 {
                        return Ok(None);
                    }
                }
            }
        }
        Ok(Some(variables))
    }

    /// Enumerates every assignment of at most eight free directions, in mask
    /// order. Gauge-fixed and reversed uses keep their fixed direction.
    #[allow(clippy::too_many_arguments)]
    fn enumerate_orientation_masks(
        &self,
        ctx: &'storage DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        edge_candidates: &[Vec<[usize; 2]>],
        oriented_edges: &BTreeSet<usize>,
        gaugeable_edges: &HashSet<usize>,
        limit: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Vec<MeshOrientationOption<'storage>>, CodecError> {
        let ((orientation_plan, variable_count), _plan_storage) =
            ctx.with_scoped_storage("catia_orientation_plan_storage", || {
                let mut oriented = OrientedEdges::new(ctx, oriented_edges)?;
                let mut variable_count = 0usize;
                let mut orientation_plan =
                    ctx.collection_vec(assignment.boundaries.len(), "catia_orientation_plan_rows")?;
                for boundary in ctx.admit_iter(&assignment.boundaries, "catia_orientation_plan")? {
                    let mut row =
                        ctx.collection_vec(boundary.len(), "catia_orientation_plan_values")?;
                    for use_ in ctx.admit_iter(boundary, "catia_orientation_plan")? {
                        row.push(match use_.reversed {
                            Some(reversed) => (reversed, None),
                            None if oriented.fix(ctx, use_.edge)?
                                && ctx.contains_hash_set(
                                    gaugeable_edges,
                                    &use_.edge,
                                    "catia_orientation_gauge",
                                )? =>
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
                Ok::<_, CodecError>((orientation_plan, variable_count))
            })?;
        let mut output = Vec::new();
        let mut seen_storage = ctx.reserve_scoped(0, "catia_orientation_fingerprint_keys")?;
        let mut seen = HashMap::new();
        let combinations = 1usize << variable_count;
        let orientation_work = work_units(ctx.fold(
            &assignment.boundaries,
            0usize,
            |total: usize, boundary| {
                total
                    .checked_add(boundary.len())
                    .ok_or_else(|| CodecError::malformed("CATIA orientation work exceeds usize"))
            },
            "catia_orientation_work",
        )?);
        for mask in 0..combinations {
            if output.len() >= limit {
                break;
            }
            if budget.is_some_and(|budget| !budget.charge_by(orientation_work)) {
                break;
            }
            let (directions, direction_storage) =
                ctx.with_scoped_storage("catia_orientation_candidate_directions", || {
                    let mut directions = ctx.collection_vec(
                        orientation_plan.len(),
                        "catia_orientation_direction_rows",
                    )?;
                    for plan in ctx.admit_iter(&orientation_plan, "catia_orientation_directions")? {
                        let mut row = ctx
                            .collection_vec(plan.len(), "catia_orientation_boundary_directions")?;
                        for &(fixed, variable) in
                            ctx.admit_iter(plan, "catia_orientation_directions")?
                        {
                            row.push(variable.map_or(fixed, |variable| {
                                let shift = variable_count - variable - 1;
                                mask & (1usize << shift) != 0
                            }));
                        }
                        directions.push(row);
                    }
                    Ok::<_, CodecError>(directions)
                })?;
            let mut quotient = self.clone_charged(ctx)?;
            let mut merge_storage = ctx.reserve_scoped(0, "catia_orientation_merge_storage")?;
            if !merge_storage.with_storage(|| {
                let mut merged_nodes = Vec::new();
                let mut merged = true;
                'merge: for (boundary, row) in ctx
                    .admit_iter(&assignment.boundaries, "catia_orientation_merges")?
                    .zip(&directions)
                {
                    for index in ctx.admit_iter(0..boundary.len(), "catia_orientation_merges")? {
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
                        ctx.push_vec(&mut merged_nodes, root, "catia_orientation_merged_nodes")?;
                    }
                }
                if !merged {
                    return Ok(false);
                }
                let affected_edges =
                    quotient.affected_edges_for_nodes(ctx, &merged_nodes, edge_candidates)?;
                if !quotient.propagate_edge_domains(
                    ctx,
                    &affected_edges,
                    edge_candidates,
                    budget,
                )? {
                    return Ok(false);
                }
                Ok::<_, CodecError>(true)
            })? {
                continue;
            }
            if admit_orientation_option(
                ctx,
                &mut seen,
                &mut seen_storage,
                &output,
                &directions,
                &quotient,
            )? {
                direction_storage.commit()?;
                ctx.push_vec(
                    &mut output,
                    (directions, quotient),
                    "catia_orientation_options",
                )?;
            }
        }
        Ok(output)
    }

    fn assignment_options_for_directions(
        &self,
        ctx: &'storage DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        direction_options: &MeshFaceDirectionOptions,
        limit: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Vec<MeshOrientationOption<'storage>>, CodecError> {
        if limit == 0
            || assignment.boundaries.len() != direction_options.first().map_or(0, Vec::len)
        {
            return Ok(Vec::new());
        }
        let work = work_units(ctx.fold(
            &assignment.boundaries,
            0usize,
            |total: usize, boundary| {
                total
                    .checked_add(boundary.len())
                    .ok_or_else(|| CodecError::malformed("CATIA orientation work exceeds usize"))
            },
            "catia_fixed_direction_options",
        )?);
        let mut output = Vec::new();
        let mut seen_storage = ctx.reserve_scoped(0, "catia_orientation_fingerprint_keys")?;
        let mut seen = HashMap::<u64, Vec<usize>>::new();
        let options = &direction_options[..direction_options.len().min(limit)];
        let mut options = options.iter();
        while let Some(directions) =
            ctx.next_charged(&mut options, "catia_fixed_direction_options")?
        {
            if directions.len() != assignment.boundaries.len()
                || ctx.any_by(
                    directions.iter().zip(&assignment.boundaries),
                    |(directions, boundary)| Ok(directions.len() != boundary.len()),
                    "catia_fixed_direction_options",
                )?
            {
                continue;
            }
            if budget.is_some_and(|budget| !budget.charge_by(work)) {
                break;
            }
            let mut quotient = self.clone_charged(ctx)?;
            let mut merged = true;
            'boundaries: for (boundary, boundary_directions) in ctx
                .admit_iter(&assignment.boundaries, "catia_fixed_direction_options")?
                .zip(directions)
            {
                if boundary.is_empty()
                    || ctx.any_by(
                        boundary.iter().zip(boundary_directions),
                        |(use_, direction)| {
                            Ok(use_.reversed.is_some_and(|required| required != *direction))
                        },
                        "catia_fixed_direction_options",
                    )?
                {
                    merged = false;
                    break;
                }
                for index in ctx.admit_iter(0..boundary.len(), "catia_fixed_direction_options")? {
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
            if admit_orientation_option(
                ctx,
                &mut seen,
                &mut seen_storage,
                &output,
                directions,
                &quotient,
            )? {
                ctx.push_vec(
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
        ctx: &'storage DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        label_directions: &[Vec<bool>],
        edge_orientations: &[Option<bool>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<Vec<Vec<bool>>>, CodecError> {
        if assignment.boundaries.len() != label_directions.len()
            || ctx.any_by(
                label_directions.iter().zip(&assignment.boundaries),
                |(directions, boundary)| Ok(directions.len() != boundary.len()),
                "catia_label_direction_rows",
            )?
        {
            return Ok(None);
        }
        let work = work_units(ctx.fold(
            &assignment.boundaries,
            0usize,
            |total: usize, boundary| {
                total
                    .checked_add(boundary.len())
                    .ok_or_else(|| CodecError::malformed("CATIA orientation work exceeds usize"))
            },
            "catia_label_direction_rows",
        )?);
        if budget.is_some_and(|budget| !budget.charge_by(work)) {
            return Ok(None);
        }
        let mut directions = Vec::new();
        ctx.reserve_vec(
            &mut directions,
            assignment.boundaries.len(),
            "catia_label_direction_rows",
        )?;
        for (boundary, labels) in ctx
            .admit_iter(&assignment.boundaries, "catia_label_direction_rows")?
            .zip(label_directions)
        {
            let mut row = Vec::new();
            ctx.reserve_vec(&mut row, boundary.len(), "catia_label_direction_values")?;
            for (use_, &label_direction) in ctx
                .admit_iter(boundary, "catia_label_direction_values")?
                .zip(labels)
            {
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
        'boundaries: for (boundary, boundary_directions) in ctx
            .admit_iter(&assignment.boundaries, "catia_label_direction_merges")?
            .zip(&directions)
        {
            if boundary.is_empty() {
                merged = false;
                break;
            }
            for index in ctx.admit_iter(0..boundary.len(), "catia_label_direction_merges")? {
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
        ctx: &'storage DecodeContext<'_>,
        assignment: &MeshFaceBoundaryAssignment,
        label_directions: &[Vec<bool>],
        edge_orientations: &[Option<bool>],
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<MeshOrientationOption<'storage>>, CodecError> {
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
}

#[cfg(test)]
#[test]
fn quotient_clone_refuses_retained_domains_and_member_nodes() {
    let quotient = MeshQuotient::new(vec![Arc::new(HashSet::from([0usize]))]);
    catia_test_context!(service_ctx);
    assert_eq!(
        quotient
            .clone_charged(&service_ctx)
            .expect("service budget")
            .len(),
        1
    );
    // Clones are temporary search states, so their storage is materialized,
    // not retained.
    crate::test_support::with_retained_limit(0, |ctx| {
        assert_eq!(quotient.clone_charged(ctx).expect("scoped clone").len(), 1);
    });
    let mut refused = std::collections::BTreeSet::new();
    for cap in 0..=256 {
        match crate::test_support::with_materialized_limit(cap, |ctx| {
            quotient.clone_charged(ctx).map(|value| value.len())
        }) {
            Err(CodecError::ResourceLimit(error)) => {
                refused.insert(error.operation);
            }
            Ok(_) => break,
            Err(error) => panic!("unexpected clone refusal: {error}"),
        }
    }
    for operation in [
        "catia_quotient_clone_domains",
        "catia_quotient_clone_member_nodes",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

struct DeferredFaceQuotientOptions<'storage> {
    alternatives: Vec<MeshQuotient<'storage>>,
    base_nodes: Vec<usize>,
}

fn materialize_deferred_quotient_option<'storage>(
    ctx: &'storage DecodeContext<'_>,
    base: &MeshQuotient<'storage>,
    local: &MeshQuotient<'storage>,
    base_nodes: &[usize],
    affected_edges: &[usize],
    edge_candidates: &[Vec<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> Result<Option<MeshQuotient<'storage>>, CodecError> {
    let mut materialized = base.clone_charged(ctx)?;
    for local_node in ctx.admit_iter(0..base_nodes.len(), "catia_deferred_materialize")? {
        let local_root = local.union.root(ctx, local_node)?;
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

fn deferred_face_quotient_options_limited<'storage>(
    ctx: &'storage DecodeContext<'_>,
    domain: &MeshDeferredFaceBoundary,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &MeshQuotient<'storage>,
    limit: usize,
    budget: &WorkBudget<'_>,
) -> Result<Option<DeferredFaceQuotientOptions<'storage>>, CodecError> {
    #[derive(Clone, Copy)]
    struct Gap {
        left_end: usize,
        right_start: usize,
        capacity: usize,
    }

    struct DeferredGapFill<
        'storage,
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
        quotient: MeshQuotient<'storage>,
        base_quotient: &'input4 MeshQuotient<'storage>,
        base_nodes: &'input5 [usize],
        output: &'input6 mut Vec<MeshQuotient<'storage>>,
        limit: usize,
        budget: &'input8 WorkBudget<'input7>,
    }

    fn fill_gap<'storage>(
        ctx: &'storage DecodeContext<'_>,
        inputs: DeferredGapFill<'storage, '_, '_, '_, '_, '_, '_, '_, '_, '_>,
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
        let mut seen_storage = ctx.reserve_scoped(0, "catia_deferred_seen_gap_states")?;
        let mut seen = HashSet::new();
        let mut ranks = 0..missing_edges.len();
        while let Some(rank) = ctx.next_charged(&mut ranks, "catia deferred gap search")? {
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
                let end_root = next.union.find(ctx, end)?;
                let (signature, signature_storage) = ctx
                    .with_scoped_storage("catia_deferred_seen_gap_states", || {
                        next.signature_charged(ctx)
                    })?;
                if !seen_storage.with_storage(|| {
                    let fresh = ctx.insert_hash_set(
                        &mut seen,
                        (rank, end_root, signature),
                        "catia_deferred_seen_gap_states",
                    )?;
                    if fresh {
                        signature_storage.commit()?;
                    }
                    Ok::<_, CodecError>(fresh)
                })? {
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
        'storage,
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
        quotient: &'input4 MeshQuotient<'storage>,
        base_quotient: &'input5 MeshQuotient<'storage>,
        base_nodes: &'input6 [usize],
        output: &'input7 mut Vec<MeshQuotient<'storage>>,
        limit: usize,
        budget: &'input9 WorkBudget<'input8>,
    }

    fn walk_gaps<'storage>(
        ctx: &'storage DecodeContext<'_>,
        inputs: DeferredGapWalk<'storage, '_, '_, '_, '_, '_, '_, '_, '_, '_, '_>,
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
        ctx.charge_work(1, "catia deferred gap walk")?;
        if output.len() >= limit || budget.exhausted() {
            return Ok(());
        }
        if gap == gaps.len() {
            if index_from_u32(used.count_ones()) != missing_edges.len() {
                return Ok(());
            }
            let (affected_edges, _affected_storage) =
                ctx.with_scoped_storage("catia_deferred_affected_edges", || {
                    let mut affected_edges = Vec::new();
                    for &edge in ctx.admit_iter(missing_edges, "catia_deferred_affected_edges")? {
                        if !edge_candidates[edge].is_empty() {
                            ctx.push_vec(
                                &mut affected_edges,
                                edge,
                                "catia_deferred_affected_edges",
                            )?;
                        }
                    }
                    Ok::<_, CodecError>(affected_edges)
                })?;
            if materialize_deferred_quotient_option(
                ctx,
                base_quotient,
                quotient,
                base_nodes,
                &affected_edges,
                edge_candidates,
                budget,
            )?
            .is_some()
            {
                ctx.push_vec(
                    output,
                    quotient.clone_charged(ctx)?,
                    "catia_deferred_alternatives",
                )?;
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
        let mut targets = minimum..=maximum;
        while let Some(target) = ctx.next_charged(&mut targets, "catia deferred gap walk")? {
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
                    quotient: quotient.clone_charged(ctx)?,
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
    let (gaps, _gap_storage) = ctx.with_scoped_storage("catia_deferred_gaps", || {
        let mut gaps = Vec::new();
        for cycle in ctx.admit_iter(&domain.cycles, "catia_deferred_gaps")? {
            if cycle.exact_uses.is_empty() {
                return Ok(None);
            }
            for index in ctx.admit_iter(0..cycle.exact_uses.len(), "catia_deferred_gaps")? {
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
                ctx.push_vec(
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
        Ok::<_, CodecError>(Some(gaps))
    })?;
    let Some(mut gaps) = gaps else {
        return Ok(None);
    };
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
    for gap in ctx.admit_iter(&gaps, "catia_deferred_base_nodes")? {
        for node in [gap.left_end, gap.right_start] {
            ctx.push_vec(
                &mut base_nodes,
                quotient.union.root(ctx, node)?,
                "catia_deferred_base_nodes",
            )?;
        }
    }
    for &edge in ctx.admit_iter(&domain.missing_edges, "catia_deferred_base_nodes")? {
        for node in [edge * 2, edge * 2 + 1] {
            ctx.push_vec(
                &mut base_nodes,
                quotient.union.root(ctx, node)?,
                "catia_deferred_base_nodes",
            )?;
        }
    }
    ctx.sort_unstable_by(
        &mut base_nodes,
        |value| value,
        Ord::cmp,
        "catia_deferred_base_nodes_sort",
    )?;
    ctx.dedup_vec(&mut base_nodes, "catia_deferred_base_nodes_dedup")?;
    // Base roots are ascending, so a root's local node is its position.
    let local_node = |node: usize| -> Result<usize, CodecError> {
        let root = quotient.union.root(ctx, node)?;
        ctx.binary_search(&base_nodes, &root, "catia_deferred_local_index")?
            .map_err(|_| CodecError::malformed("deferred gap root is not a base node"))
    };
    for gap in ctx.admit_iter(&mut gaps, "catia_deferred_local_index")? {
        gap.left_end = local_node(gap.left_end)?;
        gap.right_start = local_node(gap.right_start)?;
    }
    let ((missing_nodes, local_quotient, gaps), _preparation_storage) =
        ctx.with_scoped_storage("catia_deferred_preparation", || {
            let mut missing_nodes = Vec::new();
            for &edge in ctx.admit_iter(&domain.missing_edges, "catia_deferred_missing_nodes")? {
                ctx.push_vec(
                    &mut missing_nodes,
                    [local_node(edge * 2)?, local_node(edge * 2 + 1)?],
                    "catia_deferred_missing_nodes",
                )?;
            }
            let local_quotient = MeshQuotient::new_charged(ctx, base_nodes.len(), |local| {
                Ok(Rc::clone(&quotient.domains[base_nodes[local]]))
            })?;
            let mut ranked_gaps = Vec::new();
            for gap in ctx.admit_iter(gaps, "catia_deferred_ranked_gaps")? {
                let single_edge_options = if gap.capacity == 1 {
                    let mut count = 0usize;
                    for rank in
                        ctx.admit_iter(0..domain.missing_edges.len(), "catia_deferred_ranked_gaps")?
                    {
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
                ctx.push_vec(
                    &mut ranked_gaps,
                    (gap, (gap.capacity, single_edge_options)),
                    "catia_deferred_ranked_gaps",
                )?;
            }
            ctx.sort_unstable_by(
                &mut ranked_gaps,
                |value| &value.1,
                Ord::cmp,
                "catia_deferred_ranked_gaps_sort",
            )?;
            let mut gaps = Vec::new();
            for (gap, _) in ctx.admit_iter(ranked_gaps, "catia_deferred_sorted_gaps")? {
                ctx.push_vec(&mut gaps, gap, "catia_deferred_sorted_gaps")?;
            }
            Ok::<_, CodecError>((missing_nodes, local_quotient, gaps))
        })?;
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

/// Merges, in `quotient`, the nodes that every alternative joins. Alternative
/// node `node` is quotient node `base_node(node)`. Each group's least node
/// is its representative. Returns false when a merge empties a domain.
fn merge_common_classes(
    ctx: &DecodeContext<'_>,
    alternatives: &mut [MeshQuotient<'_>],
    node_count: usize,
    base_node: impl Fn(usize) -> usize,
    quotient: &mut MeshQuotient<'_>,
    signature_operation: &'static str,
    classes_operation: &'static str,
) -> Result<bool, CodecError> {
    let mut storage = ctx.reserve_scoped(0, classes_operation)?;
    let mut signatures = Vec::new();
    storage.with_storage(|| {
        for node in ctx.admit_iter(0..node_count, classes_operation)? {
            let mut signature = Vec::new();
            for alternative in ctx.admit_iter(&mut *alternatives, signature_operation)? {
                ctx.push_vec(
                    &mut signature,
                    alternative.union.find(ctx, node)?,
                    signature_operation,
                )?;
            }
            ctx.push_vec(&mut signatures, (signature, node), classes_operation)?;
        }
        Ok::<_, CodecError>(())
    })?;
    ctx.sort_unstable_by(&mut signatures, |entry| entry, Ord::cmp, classes_operation)?;
    let mut representative = None::<(usize, usize)>;
    for (index, (signature, node)) in ctx.admit_iter(&signatures, classes_operation)?.enumerate() {
        let joined = index
            .checked_sub(1)
            .is_some_and(|previous| signatures[previous].0.len() == signature.len())
            && ctx.equal(&signatures[index - 1].0, signature, classes_operation)?;
        match representative {
            Some((_, first)) if joined => {
                if quotient
                    .merge_charged(ctx, base_node(first), base_node(*node))?
                    .is_none()
                {
                    return Ok(false);
                }
            }
            _ => representative = Some((index, *node)),
        }
    }
    Ok(true)
}

/// Narrows the domain of `root` to the points some alternative allows at
/// `node`. Returns false when no point remains.
fn narrow_to_alternative_points(
    ctx: &DecodeContext<'_>,
    alternatives: &mut [MeshQuotient<'_>],
    node: usize,
    quotient: &mut MeshQuotient<'_>,
    root: usize,
    [allowed_operation, operation]: [&'static str; 2],
) -> Result<bool, CodecError> {
    let mut storage = ctx.reserve_scoped(0, allowed_operation)?;
    let allowed = storage.with_storage(|| {
        let mut allowed = Vec::new();
        for alternative in ctx.admit_iter(&mut *alternatives, allowed_operation)? {
            let alternative_root = alternative.union.find(ctx, node)?;
            ctx.extend_from_slice(
                &mut allowed,
                &alternative.domains[alternative_root],
                allowed_operation,
            )?;
        }
        ctx.sort_unstable_by(&mut allowed, |point| point, Ord::cmp, allowed_operation)?;
        ctx.dedup_vec(&mut allowed, allowed_operation)?;
        Ok::<_, CodecError>(allowed)
    })?;
    let domain = Rc::clone(&quotient.domains[root]);
    let narrowed = quotient.new_domain(operation, || {
        domain_intersection(ctx, &domain, &allowed, operation)
    })?;
    if narrowed.is_empty() {
        return Ok(false);
    }
    quotient.domains[root] = narrowed;
    Ok(true)
}

fn propagate_common_deferred_quotients<'storage>(
    ctx: &'storage DecodeContext<'_>,
    mut options: DeferredFaceQuotientOptions<'storage>,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient<'storage>,
    budget: &WorkBudget<'_>,
) -> Result<Option<()>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_propagate_common_deferred_quotients_scratch")?;
    scratch.with_storage(|| {
        let node_count = options.base_nodes.len();
        let base_nodes = &options.base_nodes;
        if !merge_common_classes(
            ctx,
            &mut options.alternatives,
            node_count,
            |local| base_nodes[local],
            quotient,
            "catia_deferred_common_signature",
            "catia_deferred_common_classes",
        )? {
            return Ok(None);
        }
        for local in ctx.admit_iter(0..node_count, "catia_deferred_common_allowed")? {
            let root = quotient.union.find(ctx, base_nodes[local])?;
            if !narrow_to_alternative_points(
                ctx,
                &mut options.alternatives,
                local,
                quotient,
                root,
                [
                    "catia_deferred_common_allowed",
                    "catia_deferred_common_narrowed",
                ],
            )? {
                return Ok(None);
            }
        }
        let mut affected_edges = Vec::new();
        for &node in ctx.admit_iter(base_nodes, "catia_deferred_common_edges")? {
            let root = quotient.union.find(ctx, node)?;
            quotient.push_component_edges(
                ctx,
                root,
                edge_candidates,
                &mut affected_edges,
                "catia_deferred_common_edges",
            )?;
        }
        Ok(quotient
            .propagate_edge_domains(ctx, &affected_edges, edge_candidates, Some(budget))?
            .then_some(()))
    })
}

/// The corner equations every assignment of a face forces: node pairs that
/// every supported direction choice of a boundary corner joins. Returned
/// ascending; `None` when no assignment is supported or the budget ends.
fn common_supported_corner_equations<'storage>(
    ctx: &'storage DecodeContext<'_>,
    quotient: &mut MeshQuotient<'storage>,
    assignments: &[MeshFaceBoundaryAssignment],
    budget: &WorkBudget<'_>,
) -> Result<Option<ScopedValue<'storage, Vec<[usize; 2]>>>, CodecError> {
    const OPERATION: &str = "catia_boundary_corner_search";
    fn compatible(
        ctx: &DecodeContext<'_>,
        quotient: &MeshQuotient<'_>,
        left: Option<usize>,
        right: Option<usize>,
    ) -> Result<Option<bool>, CodecError> {
        let (Some(left), Some(right)) = (left, right) else {
            return Ok(None);
        };
        let left = quotient.union.root(ctx, left)?;
        let right = quotient.union.root(ctx, right)?;
        Ok(Some(
            left == right
                || !domains_disjoint(
                    ctx,
                    &quotient.domains[left],
                    &quotient.domains[right],
                    OPERATION,
                )?,
        ))
    }
    let mut common = None::<ScopedValue<'storage, Vec<[usize; 2]>>>;
    'assignments: for assignment in ctx.admit_iter(assignments, OPERATION)? {
        if !budget.charge() {
            return Ok(None);
        }
        let mut forced_storage = ctx.reserve_scoped(0, "catia_boundary_forced_corners")?;
        let mut forced = Vec::new();
        for boundary in ctx.admit_iter(&assignment.boundaries, OPERATION)? {
            if boundary.is_empty() {
                return Ok(None);
            }
            // Each use offers its fixed direction or both, so every inner
            // loop below visits at most two directions.
            let (directions, _direction_storage) =
                ctx.with_scoped_storage("catia_boundary_directions", || {
                    ctx.collect_vec(
                        boundary.iter().map(|use_| match use_.reversed {
                            Some(value) => [Some(value), None],
                            None => [Some(false), Some(true)],
                        }),
                        "catia_boundary_directions",
                    )
                })?;
            let last = boundary.len() - 1;
            let (mut supported, _supported_storage) =
                ctx.with_scoped_storage("catia_boundary_supported_grids", || {
                    ctx.alloc_filled(
                        boundary.len(),
                        [[false; 2]; 2],
                        "catia_boundary_supported_grids",
                    )
                })?;
            let corner = |index: usize, left: usize, next: usize, right: usize| {
                let (Some(left_direction), Some(right_direction)) =
                    (directions[index][left], directions[next][right])
                else {
                    return Ok(Some(false));
                };
                compatible(
                    ctx,
                    quotient,
                    port(boundary[index], left_direction, true),
                    port(boundary[next], right_direction, false),
                )
            };
            for first in 0..directions[0].len() {
                if directions[0][first].is_none() {
                    continue;
                }
                let (mut forward, _forward_storage) = ctx
                    .with_scoped_storage("catia_boundary_forward", || {
                        ctx.alloc_filled(boundary.len(), [false; 2], "catia_boundary_forward")
                    })?;
                forward[0][first] = true;
                for index in ctx.admit_iter(0..last, OPERATION)? {
                    for left in 0..directions[index].len() {
                        if !forward[index][left] {
                            continue;
                        }
                        for right in 0..directions[index + 1].len() {
                            let Some(joined) = corner(index, left, index + 1, right)? else {
                                return Ok(None);
                            };
                            if joined {
                                forward[index + 1][right] = true;
                            }
                        }
                    }
                }
                let (mut backward, _backward_storage) = ctx
                    .with_scoped_storage("catia_boundary_backward", || {
                        ctx.alloc_filled(boundary.len(), [false; 2], "catia_boundary_backward")
                    })?;
                for state in 0..directions[last].len() {
                    let Some(joined) = corner(last, state, 0, first)? else {
                        return Ok(None);
                    };
                    backward[last][state] = forward[last][state] && joined;
                }
                for index in ctx.admit_iter(0..last, OPERATION)?.rev() {
                    for left in 0..directions[index].len() {
                        if !forward[index][left] {
                            continue;
                        }
                        for right in 0..directions[index + 1].len() {
                            if !backward[index + 1][right] {
                                continue;
                            }
                            let Some(joined) = corner(index, left, index + 1, right)? else {
                                return Ok(None);
                            };
                            if joined {
                                backward[index][left] = true;
                                break;
                            }
                        }
                    }
                }
                if !backward[0][first] {
                    continue;
                }
                for index in ctx.admit_iter(0..last, OPERATION)? {
                    for left in 0..directions[index].len() {
                        if !forward[index][left] {
                            continue;
                        }
                        for right in 0..directions[index + 1].len() {
                            if !backward[index + 1][right] {
                                continue;
                            }
                            let Some(joined) = corner(index, left, index + 1, right)? else {
                                return Ok(None);
                            };
                            if joined {
                                supported[index][left][right] = true;
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
            if ctx.any_by(
                &supported,
                |transitions| Ok(transitions.iter().flatten().all(|value| !value)),
                OPERATION,
            )? {
                continue 'assignments;
            }
            for index in ctx.admit_iter(0..boundary.len(), OPERATION)? {
                let next = (index + 1) % boundary.len();
                // At most four supported transitions, so at most four equations.
                let mut equations = [None; 4];
                let mut equation_count = 0;
                for left in 0..directions[index].len() {
                    for right in 0..directions[next].len() {
                        if !supported[index][left][right] {
                            continue;
                        }
                        let (Some(left_port), Some(right_port)) = (
                            directions[index][left]
                                .and_then(|direction| port(boundary[index], direction, true)),
                            directions[next][right]
                                .and_then(|direction| port(boundary[next], direction, false)),
                        ) else {
                            return Ok(None);
                        };
                        let left = quotient.union.find(ctx, left_port)?;
                        let right = quotient.union.find(ctx, right_port)?;
                        let equation = if left <= right {
                            [left, right]
                        } else {
                            [right, left]
                        };
                        if !equations[..equation_count].contains(&Some(equation)) {
                            equations[equation_count] = Some(equation);
                            equation_count += 1;
                        }
                    }
                }
                if equation_count == 1 {
                    if let Some(equation) = equations[0] {
                        forced_storage.with_storage(|| {
                            ctx.push_vec(&mut forced, equation, "catia_boundary_forced_corners")
                        })?;
                    }
                }
            }
        }
        ctx.sort_unstable_by(&mut forced, |equation| equation, Ord::cmp, OPERATION)?;
        ctx.dedup_vec(&mut forced, OPERATION)?;
        match &mut common {
            Some(common) => ctx.retain_vec(
                &mut common.value,
                |equation| Ok(ctx.binary_search(&forced, equation, OPERATION)?.is_ok()),
                OPERATION,
            )?,
            None => {
                common = Some(ScopedValue {
                    value: forced,
                    storage: Some(forced_storage),
                })
            }
        }
    }
    Ok(common)
}

fn propagate_common_full_quotients<'storage>(
    ctx: &'storage DecodeContext<'_>,
    mut alternatives: Vec<MeshQuotient<'storage>>,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient<'storage>,
) -> Result<Option<()>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_propagate_common_full_quotients_scratch")?;
    scratch.with_storage(|| {
        let node_count = quotient.union.len();
        if !merge_common_classes(
            ctx,
            &mut alternatives,
            node_count,
            |node| node,
            quotient,
            "catia_common_quotient_signature",
            "catia_common_quotient_classes",
        )? {
            return Ok(None);
        }
        let mut roots = Vec::new();
        for node in ctx.admit_iter(0..node_count, "catia_common_quotient_roots")? {
            if quotient.union.find(ctx, node)? == node {
                ctx.push_vec(&mut roots, node, "catia_common_quotient_roots")?;
            }
        }
        for root in ctx.admit_iter(roots, "catia_common_quotient_allowed")? {
            let representative = quotient.members(root)[0];
            if !narrow_to_alternative_points(
                ctx,
                &mut alternatives,
                representative,
                quotient,
                root,
                [
                    "catia_common_quotient_allowed",
                    "catia_common_quotient_narrowed",
                ],
            )? {
                return Ok(None);
            }
        }
        Ok(quotient
            .edge_domains_viable(ctx, edge_candidates)?
            .then_some(()))
    })
}

pub(super) fn propagate_common_ordered_face_quotients<'storage>(
    ctx: &'storage DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient<'storage>,
    budget: &WorkBudget<'_>,
) -> Result<Option<()>, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "catia_propagate_common_ordered_face_quotients_scratch")?;
    scratch.with_storage(|| {
        (|| -> Option<Result<(), CodecError>> {
            const MAX_FACE_OPTIONS: usize = 4_096;
            const MAX_ORDERED_FACE_CONSTRAINT_OPERATIONS: usize = 64;
            const MAX_DEFERRED_FACE_CONSTRAINT_OPERATIONS: usize = 512;
            let mut face_order = match ctx.collect_vec(0..domains.len(), "catia_ordered_face_order")
            {
                Ok(order) => order,
                Err(error) => return Some(Err(error)),
            };
            let order_key = |face: &usize| match &domains[*face] {
                MeshFaceBoundaryDomain::DeferredValidation(_) => (0, 0),
                MeshFaceBoundaryDomain::Ordered(assignments) => (1, assignments.len()),
                MeshFaceBoundaryDomain::UnorderedFullCycle(_) => (2, 0),
            };
            if let Err(error) = ctx.sort_unstable_by_key(
                &mut face_order,
                |value| order_key(value),
                Ord::cmp,
                "catia_ordered_face_order_sort",
            ) {
                return Some(Err(error));
            }
            loop {
                if let Err(error) = ctx.charge_work(1, "catia_mesh_quotient_iteration") {
                    return Some(Err(error));
                }
                let before = match quotient.monotone_measure(ctx) {
                    Ok(measure) => measure?,
                    Err(error) => return Some(Err(error)),
                };
                let faces = match ctx.admit_iter(&face_order, "catia_ordered_face_order") {
                    Ok(faces) => faces,
                    Err(error) => return Some(Err(error.into())),
                };
                for &face in faces {
                    let mut face_storage = match ctx.reserve_scoped(0, "catia_ordered_face_storage")
                    {
                        Ok(storage) => storage,
                        Err(error) => return Some(Err(error)),
                    };
                    let result = face_storage.with_storage(|| {
                        (|| -> Option<Result<(), CodecError>> {
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
                            let face_budget = ctx.work_budget(u64_from_index(face_limit));
                            let refuse_face = || match ctx.resource_refusal() {
                                Some(limit) => CodecError::ResourceLimit(limit),
                                None => ctx.refuse_codec_limit(
                                    "catia_ordered_face_constraint_work",
                                    u64_from_index(face_limit),
                                    u64_from_index(face_limit + 1),
                                ),
                            };
                            if let MeshFaceBoundaryDomain::DeferredValidation(domain) = domain {
                                let mut merged_nodes = Vec::new();
                                let cycles = match ctx
                                    .admit_iter(&domain.cycles, "catia_ordered_deferred_cycles")
                                {
                                    Ok(cycles) => cycles,
                                    Err(error) => return Some(Err(error.into())),
                                };
                                for cycle in cycles {
                                    let uses = match ctx.admit_iter(
                                        0..cycle.exact_uses.len(),
                                        "catia_ordered_deferred_cycles",
                                    ) {
                                        Ok(uses) => uses,
                                        Err(error) => return Some(Err(error.into())),
                                    };
                                    for index in uses {
                                        let (left, left_span) = cycle.exact_uses[index];
                                        let right = cycle.exact_uses
                                            [(index + 1) % cycle.exact_uses.len()]
                                        .0;
                                        let left_end = (left.start + left_span) % cycle.length;
                                        let capacity =
                                            (right.start + cycle.length - left_end) % cycle.length;
                                        if capacity != 0 {
                                            continue;
                                        }
                                        if left.reversed.is_none() || right.reversed.is_none() {
                                            continue;
                                        }
                                        let (left_reversed, right_reversed) =
                                            (left.reversed?, right.reversed?);
                                        let left_node = left
                                            .edge
                                            .checked_mul(2)?
                                            .checked_add(usize::from(!left_reversed))?;
                                        let right_node = right
                                            .edge
                                            .checked_mul(2)?
                                            .checked_add(usize::from(right_reversed))?;
                                        let merged = match quotient
                                            .merge_charged(ctx, left_node, right_node)
                                        {
                                            Ok(Some(root)) => root,
                                            Ok(None) => return None,
                                            Err(error) => return Some(Err(error)),
                                        };
                                        if let Err(error) = ctx.push_vec(
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
                                    &affected_edges,
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
                                    Ok(None) => {
                                        if face_budget.exhausted() {
                                            return Some(Err(refuse_face()));
                                        }
                                        return Some(Ok(()));
                                    }
                                    Err(error) => return Some(Err(error)),
                                };
                                if face_budget.exhausted() {
                                    return Some(Err(refuse_face()));
                                }
                                if options.alternatives.len() > MAX_FACE_OPTIONS {
                                    return Some(Err(ctx.refuse_codec_limit(
                                        "catia_ordered_face_options",
                                        u64_from_index(MAX_FACE_OPTIONS),
                                        u64_from_index(options.alternatives.len()),
                                    )));
                                }
                                if !options.alternatives.is_empty() {
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
                                return Some(Ok(()));
                            }
                            let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                                return Some(Ok(()));
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
                                let equations = match ctx
                                    .admit_iter(equations.as_slice(), "catia_ordered_merged_nodes")
                                {
                                    Ok(equations) => equations,
                                    Err(error) => return Some(Err(error.into())),
                                };
                                for &[left, right] in equations {
                                    let merged = match quotient.merge_charged(ctx, left, right) {
                                        Ok(Some(root)) => root,
                                        Ok(None) => return None,
                                        Err(error) => return Some(Err(error)),
                                    };
                                    if let Err(error) = ctx.push_vec(
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
                                    &affected_edges,
                                    edge_candidates,
                                    Some(budget),
                                ) {
                                    Ok(true) => {}
                                    Ok(false) => return None,
                                    Err(error) => return Some(Err(error)),
                                }
                            }
                            if face_budget.exhausted() {
                                return Some(Err(refuse_face()));
                            }
                            let mut alternatives = Vec::new();
                            let assignments = match ctx
                                .admit_iter(assignments, "catia_ordered_face_assignments")
                            {
                                Ok(assignments) => assignments,
                                Err(error) => return Some(Err(error.into())),
                            };
                            for assignment in assignments {
                                let Some(work) = (match quotient.signature_work(ctx) {
                                    Ok(work) => work,
                                    Err(error) => return Some(Err(error)),
                                }) else {
                                    return Some(Err(refuse_face()));
                                };
                                if !face_budget.charge_by(work) {
                                    return Some(Err(refuse_face()));
                                }
                                let options = match quotient.assignment_options_limited(
                                    ctx,
                                    assignment,
                                    edge_candidates,
                                    &BTreeSet::new(),
                                    MAX_FACE_OPTIONS + 1,
                                    Some(&face_budget),
                                ) {
                                    Ok(options) => options,
                                    Err(error) => return Some(Err(error)),
                                };
                                if face_budget.exhausted() {
                                    return Some(Err(refuse_face()));
                                }
                                if options.len() > MAX_FACE_OPTIONS {
                                    return Some(Err(refuse_face()));
                                }
                                let options = match ctx
                                    .admit_iter(options, "catia_ordered_face_alternatives")
                                {
                                    Ok(options) => options,
                                    Err(error) => return Some(Err(error.into())),
                                };
                                for (_, quotient) in options {
                                    if let Err(error) = ctx.push_vec(
                                        &mut alternatives,
                                        quotient,
                                        "catia_ordered_face_alternatives",
                                    ) {
                                        return Some(Err(error));
                                    }
                                }
                                if alternatives.len() > MAX_FACE_OPTIONS {
                                    return Some(Err(refuse_face()));
                                }
                            }
                            if alternatives.is_empty() {
                                return Some(Ok(()));
                            }
                            match propagate_common_full_quotients(
                                ctx,
                                alternatives,
                                edge_candidates,
                                quotient,
                            ) {
                                Ok(Some(())) => {}
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            }
                            Some(Ok(()))
                        })()
                        .transpose()
                    });
                    match result {
                        Ok(Some(())) => {}
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    }
                }
                if match quotient.monotone_measure(ctx) {
                    Ok(measure) => measure?,
                    Err(error) => return Some(Err(error)),
                } == before
                {
                    return Some(Ok(()));
                }
            }
        })()
        .transpose()
    })
}

fn mesh_boundary_domain_edges(
    ctx: &DecodeContext<'_>,
    domain: &MeshFaceBoundaryDomain,
) -> Result<Vec<usize>, CodecError> {
    let mut edges = Vec::new();
    match domain {
        MeshFaceBoundaryDomain::Ordered(assignments) => {
            for assignment in ctx.admit_iter(assignments, "catia_boundary_domain_edges")? {
                for boundary in
                    ctx.admit_iter(&assignment.boundaries, "catia_boundary_domain_edges")?
                {
                    for use_ in ctx.admit_iter(boundary, "catia_boundary_domain_edges")? {
                        ctx.push_vec(&mut edges, use_.edge, "catia_boundary_domain_edges")?;
                    }
                }
            }
        }
        MeshFaceBoundaryDomain::UnorderedFullCycle(ordered_edges) => {
            edges = ctx.copy_slice(ordered_edges, "catia_boundary_domain_edges")?;
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            edges = ctx.copy_slice(&domain.missing_edges, "catia_boundary_domain_edges")?;
            for cycle in ctx.admit_iter(&domain.cycles, "catia_boundary_domain_edges")? {
                for (use_, _) in ctx.admit_iter(&cycle.exact_uses, "catia_boundary_domain_edges")? {
                    ctx.push_vec(&mut edges, use_.edge, "catia_boundary_domain_edges")?;
                }
            }
        }
    }
    ctx.sort_unstable_by(
        &mut edges,
        |value| value,
        Ord::cmp,
        "catia_boundary_domain_edges_sort",
    )?;
    ctx.dedup_vec(&mut edges, "catia_boundary_domain_edges_dedup")?;
    Ok(edges)
}

pub(super) fn bounded_unordered_cycle_assignments<'storage>(
    ctx: &'storage DecodeContext<'_>,
    edges: &[usize],
    quotient: &MeshQuotient<'storage>,
    limit: usize,
    budget: &WorkBudget<'_>,
) -> Result<Option<Vec<MeshFaceBoundaryAssignment>>, CodecError> {
    /// Endpoint positions are `rank * 2 + side`, at most 128 of them, and
    /// `compatible` is their row-major adjacency.
    struct Search<'a, 'b, 'c> {
        edges: &'a [usize],
        compatible: &'a [bool],
        ctx: &'a DecodeContext<'b>,
        limit: usize,
        budget: &'a WorkBudget<'c>,
        assignments: Vec<MeshFaceBoundaryAssignment>,
        boundary_storage: ScopedReservation<'a>,
    }

    impl Search<'_, '_, '_> {
        fn compatible(&self, left: usize, right: usize) -> bool {
            self.compatible[left * self.edges.len() * 2 + right]
        }

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
                if self.compatible(previous_end, first_start) {
                    let completed = self
                        .ctx
                        .copy_slice(boundary, "catia_unordered_completed_boundary")?;
                    let mut boundaries = Vec::new();
                    self.ctx.push_vec(
                        &mut boundaries,
                        completed,
                        "catia_unordered_boundary_rows",
                    )?;
                    self.ctx.push_vec(
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
                    let start = rank * 2 + usize::from(reversed);
                    if !self.compatible(previous_end, start) {
                        continue;
                    }
                    self.boundary_storage.with_storage(|| {
                        self.ctx.push_vec(
                            boundary,
                            MeshBoundaryEdgeCandidate {
                                edge,
                                start: 0,
                                end: 0,
                                reversed: Some(reversed),
                            },
                            "catia_unordered_search_boundary",
                        )
                    })?;
                    if !self.walk(
                        first_start,
                        rank * 2 + usize::from(!reversed),
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
    let (prepared, _preparation_storage) =
        ctx.with_scoped_storage("catia_unordered_preparation", || {
            let edge_count = edges.len();
            let mut edges = ctx.copy_slice(edges, "catia_unordered_sorted_edges")?;
            ctx.sort_unstable_by(
                &mut edges,
                |value| value,
                Ord::cmp,
                "catia_unordered_sorted_edges_sort",
            )?;
            edges.dedup();
            if edges.len() != edge_count {
                return Ok(None);
            }
            let mut quotient = quotient.clone_charged(ctx)?;
            let mut nodes = Vec::new();
            for &edge in &edges {
                ctx.push_vec(&mut nodes, edge * 2, "catia_unordered_nodes")?;
                ctx.push_vec(&mut nodes, edge * 2 + 1, "catia_unordered_nodes")?;
            }
            // At most 128 endpoints, so the adjacency has at most 16384 cells.
            let mut compatible = ctx.alloc_filled(
                nodes.len() * nodes.len(),
                false,
                "catia_unordered_compatible",
            )?;
            for (left_position, &left) in nodes.iter().enumerate() {
                let left_root = quotient.union.find(ctx, left)?;
                for (right_position, &right) in nodes.iter().enumerate() {
                    let right_root = quotient.union.find(ctx, right)?;
                    compatible[left_position * nodes.len() + right_position] = left_root
                        == right_root
                        || !domains_disjoint(
                            ctx,
                            &quotient.domains[left_root],
                            &quotient.domains[right_root],
                            "catia_unordered_compatible",
                        )?;
                }
            }
            Ok::<_, CodecError>(Some((edges, compatible)))
        })?;
    let Some((edges, compatible)) = prepared else {
        return Ok(None);
    };
    let first = edges[0];
    let first_start = 0;
    let mut boundary_storage = ctx.reserve_scoped(0, "catia_unordered_search_boundary")?;
    let mut boundary = Vec::new();
    boundary_storage.with_storage(|| {
        ctx.push_vec(
            &mut boundary,
            MeshBoundaryEdgeCandidate {
                edge: first,
                start: 0,
                end: 0,
                reversed: Some(false),
            },
            "catia_unordered_search_boundary",
        )?;
        Ok::<_, CodecError>(())
    })?;
    let mut search = Search {
        edges: &edges,
        compatible: &compatible,
        ctx,
        limit,
        budget,
        assignments: Vec::new(),
        boundary_storage,
    };
    Ok(search
        .walk(first_start, 1, 1, &mut boundary)?
        .then_some(search.assignments))
}

fn advance_boundary_component_states<'storage>(
    ctx: &'storage DecodeContext<'_>,
    domain: &MeshFaceBoundaryDomain,
    states: &[MeshQuotientGaugeState<'storage>],
    edge_candidates: &[Vec<[usize; 2]>],
    limit: usize,
    budget: &WorkBudget<'_>,
) -> Result<Option<ScopedValue<'storage, Vec<MeshQuotientGaugeState<'storage>>>>, CodecError> {
    let (outcome, storage) = ctx.with_scoped_storage(
        "catia_component_state_storage",
        || -> Result<Option<Vec<MeshQuotientGaugeState<'storage>>>, CodecError> {
            let mut next = Vec::new();
            let mut signature_storage = ctx.reserve_scoped(0, "catia_component_signatures")?;
            let mut signatures = HashSet::new();
            let (domain_edges, _edge_storage) = ctx
                .with_scoped_storage("catia_component_domain_edges", || {
                    mesh_boundary_domain_edges(ctx, domain)
                })?;
            for (state, oriented_edges) in ctx.admit_iter(states, "catia_component_states")? {
                let Some(remaining) = limit
                    .checked_add(1)
                    .and_then(|end| end.checked_sub(next.len()))
                else {
                    return Err(ctx.refuse_codec_limit(
                        "catia_boundary_component_states",
                        u64_from_index(limit),
                        u64::MAX,
                    ));
                };
                if remaining == 0 {
                    return Err(ctx.refuse_codec_limit(
                        "catia_boundary_component_states",
                        u64_from_index(limit),
                        u64_from_index(next.len()),
                    ));
                }
                let (candidates, _candidate_storage) =
                    ctx.with_scoped_storage("catia_component_candidates", || {
                        let candidates = match domain {
                            MeshFaceBoundaryDomain::Ordered(assignments) => {
                                let mut candidates = Vec::new();
                                for assignment in
                                    ctx.admit_iter(assignments, "catia_component_candidates")?
                                {
                                    let options = state.assignment_options_limited(
                                        ctx,
                                        assignment,
                                        edge_candidates,
                                        oriented_edges,
                                        remaining,
                                        Some(budget),
                                    )?;
                                    for (_, quotient) in
                                        ctx.admit_iter(options, "catia_component_candidates")?
                                    {
                                        ctx.push_vec(
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
                                if options.alternatives.is_empty()
                                    && domain.missing_edges.is_empty()
                                {
                                    let mut candidates = Vec::new();
                                    ctx.push_vec(
                                        &mut candidates,
                                        state.clone_charged(ctx)?,
                                        "catia_component_candidates",
                                    )?;
                                    candidates
                                } else {
                                    let mut affected_edges = Vec::new();
                                    for &edge in ctx.admit_iter(
                                        &domain_edges,
                                        "catia_component_affected_edges",
                                    )? {
                                        if !edge_candidates[edge].is_empty() {
                                            ctx.push_vec(
                                                &mut affected_edges,
                                                edge,
                                                "catia_component_affected_edges",
                                            )?;
                                        }
                                    }
                                    let mut candidates = Vec::new();
                                    for local in ctx.admit_iter(
                                        &options.alternatives,
                                        "catia_component_candidates",
                                    )? {
                                        if let Some(candidate) =
                                            materialize_deferred_quotient_option(
                                                ctx,
                                                state,
                                                local,
                                                &options.base_nodes,
                                                &affected_edges,
                                                edge_candidates,
                                                budget,
                                            )?
                                        {
                                            ctx.push_vec(
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
                                let Some(assignments) = bounded_unordered_cycle_assignments(
                                    ctx, edges, state, remaining, budget,
                                )?
                                else {
                                    return Ok(None);
                                };
                                let mut candidates = Vec::new();
                                for assignment in
                                    ctx.admit_iter(&assignments, "catia_component_candidates")?
                                {
                                    let options = state.assignment_options_limited(
                                        ctx,
                                        assignment,
                                        edge_candidates,
                                        oriented_edges,
                                        remaining,
                                        Some(budget),
                                    )?;
                                    for (_, quotient) in
                                        ctx.admit_iter(options, "catia_component_candidates")?
                                    {
                                        ctx.push_vec(
                                            &mut candidates,
                                            quotient,
                                            "catia_component_candidates",
                                        )?;
                                    }
                                }
                                candidates
                            }
                        };
                        Ok::<_, CodecError>(Some(candidates))
                    })?;
                let Some(candidates) = candidates else {
                    return Ok(None);
                };
                for mut candidate in ctx.admit_iter(candidates, "catia_component_candidates")? {
                    let mut next_oriented = ctx.collect_btree_set(
                        oriented_edges.iter().copied(),
                        "catia_component_oriented_edges",
                    )?;
                    for &edge in ctx.admit_iter(&domain_edges, "catia_component_oriented_edges")? {
                        ctx.insert_btree_set(
                            &mut next_oriented,
                            edge,
                            "catia_component_oriented_edges",
                        )?;
                    }
                    let Some(work) = candidate
                        .signature_work(ctx)?
                        .and_then(|work| work.checked_add(work_units(next_oriented.len())))
                    else {
                        return Ok(None);
                    };
                    if !budget.charge_by(work) {
                        return Ok(None);
                    }
                    if signature_storage.with_storage(|| {
                        let oriented_signature = ctx.collect_vec(
                            next_oriented.iter().copied(),
                            "catia_component_oriented_signature",
                        )?;
                        ctx.insert_hash_set(
                            &mut signatures,
                            (candidate.signature_charged(ctx)?, oriented_signature),
                            "catia_component_signatures",
                        )
                    })? {
                        ctx.push_vec(
                            &mut next,
                            (candidate, next_oriented),
                            "catia_component_states",
                        )?;
                    }
                    if next.len() > limit {
                        return Err(ctx.refuse_codec_limit(
                            "catia_boundary_component_states",
                            u64_from_index(limit),
                            u64_from_index(next.len()),
                        ));
                    }
                }
                if budget.exhausted() {
                    return Ok(None);
                }
            }
            Ok((!next.is_empty()).then_some(next))
        },
    )?;
    ctx.charge_work(0, "catia_boundary_component_work")?;
    if budget.exhausted() {
        let limit = u64_from_index(budget.consumed());
        let requested = limit.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_boundary_component_work", u64::MAX - 1, u64::MAX)
        })?;
        return Err(ctx.refuse_codec_limit("catia_boundary_component_work", limit, requested));
    }
    Ok(outcome.map(|value| ScopedValue {
        value,
        storage: Some(storage),
    }))
}

pub(super) fn propagate_common_boundary_components<'storage>(
    ctx: &'storage DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient<'storage>,
) -> Result<Option<()>, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "catia_propagate_common_boundary_components_scratch")?;
    scratch.with_storage(|| {
        const MAX_COMPONENT_STATES: usize = 128;
        const MAX_COMPONENT_OPERATIONS: usize = 8_192;
        const MAX_COMPONENT_ROUNDS: usize = 8;

        let mut domain_edges = Vec::new();
        let mut active_faces = Vec::new();
        for (face, domain) in ctx
            .admit_iter(domains, "catia_component_domain_rows")?
            .enumerate()
        {
            let edges = mesh_boundary_domain_edges(ctx, domain)?;
            if ctx.any_by(
                &edges,
                |edge| Ok(edge_candidates[*edge].is_empty()),
                "catia_component_active_faces",
            )? {
                ctx.push_vec(&mut active_faces, face, "catia_component_active_faces")?;
            }
            ctx.push_vec(&mut domain_edges, edges, "catia_component_domain_rows")?;
        }
        let mut components = UnionFind::charged(ctx, active_faces.len(), "catia_component_union")?;
        let mut edge_owner = HashMap::<usize, usize>::new();
        for (index, &face) in ctx
            .admit_iter(&active_faces, "catia_component_edge_owner")?
            .enumerate()
        {
            for &edge in ctx.admit_iter(&domain_edges[face], "catia_component_edge_owner")? {
                if let Some(previous) =
                    ctx.insert_hash_map(&mut edge_owner, edge, index, "catia_component_edge_owner")?
                {
                    components.union(ctx, previous, index)?;
                }
            }
        }
        // Ascending active faces open each component's group at its smallest
        // face, so the groups are already in smallest-face order.
        let mut group_by_root =
            ctx.alloc_filled(active_faces.len(), None, "catia_component_face_groups")?;
        let mut face_components = Vec::new();
        for (index, &face) in ctx
            .admit_iter(&active_faces, "catia_component_face_members")?
            .enumerate()
        {
            let root = components.find(ctx, index)?;
            match group_by_root[root] {
                Some(group) => {
                    let faces: &mut Vec<usize> = &mut face_components[group];
                    ctx.push_vec(faces, face, "catia_component_face_members")?;
                }
                None => {
                    group_by_root[root] = Some(face_components.len());
                    let mut faces = Vec::new();
                    ctx.push_vec(&mut faces, face, "catia_component_face_members")?;
                    ctx.push_vec(&mut face_components, faces, "catia_component_groups")?;
                }
            }
        }

        let face_keys = ctx.try_collect_vec(
            domains.iter().enumerate().map(|(face, domain)| {
                Ok::<_, CodecError>(match domain {
                    MeshFaceBoundaryDomain::Ordered(assignments) => {
                        let direction_work = ctx
                            .fold(
                                assignments,
                                Some(0usize),
                                |total, assignment| {
                                    let assignment_work = ctx.fold(
                                        &assignment.boundaries,
                                        Some(0usize),
                                        |total, boundary| {
                                            let unresolved = ctx.fold(
                                                boundary,
                                                0usize,
                                                |count, use_| {
                                                    Ok(count + usize::from(use_.reversed.is_none()))
                                                },
                                                "catia_component_face_key_scan",
                                            )?;
                                            Ok(total
                                                .and_then(|total| total.checked_add(unresolved)))
                                        },
                                        "catia_component_face_key_scan",
                                    )?;
                                    Ok(total
                                        .zip(assignment_work)
                                        .and_then(|(total, work)| total.checked_add(work)))
                                },
                                "catia_component_face_key_scan",
                            )?
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(
                                    "catia_component_face_key_scan",
                                    u64::MAX - 1,
                                    u64::MAX,
                                )
                            })?;
                        (0, assignments.len(), direction_work, face)
                    }
                    MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                        (1, domain.missing_edges.len(), 0, face)
                    }
                    MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => (2, edges.len(), 0, face),
                })
            }),
            "catia_component_face_keys",
        )?;
        for mut faces in ctx.admit_iter(face_components, "catia_component_groups")? {
            let (ordered_faces, _face_storage) =
                ctx.with_scoped_storage("catia_component_face_storage", || {
                    let mut ordered_faces = Vec::new();
                    let mut selected_edges = HashSet::new();
                    // Each step takes the face of least kind that shares the most edges
                    // with the faces already taken.
                    while !faces.is_empty() {
                        ctx.charge_work(1, "catia_mesh_quotient_iteration")?;
                        let mut position = 0usize;
                        let least = ctx.fold(
                            &faces,
                            None,
                            |least: Option<(_, usize)>, &face| {
                                let shared = ctx.fold(
                                    &domain_edges[face],
                                    0usize,
                                    |shared, edge| {
                                        Ok(shared
                                            + usize::from(ctx.contains_hash_set(
                                                &selected_edges,
                                                edge,
                                                "catia_component_shared_edge_scan",
                                            )?))
                                    },
                                    "catia_component_shared_edge_scan",
                                )?;
                                let key = face_keys[face];
                                let key = (key.0, usize::MAX - shared, key);
                                let index = position;
                                position += 1;
                                Ok(match least {
                                    Some(least) if least.0 <= key => Some(least),
                                    _ => Some((key, index)),
                                })
                            },
                            "catia_component_face_order_scan",
                        )?;
                        let Some((_, next)) = least else {
                            return Ok(None);
                        };
                        let face = faces.swap_remove(next);
                        for &edge in
                            ctx.admit_iter(&domain_edges[face], "catia_component_selected_edges")?
                        {
                            ctx.insert_hash_set(
                                &mut selected_edges,
                                edge,
                                "catia_component_selected_edges",
                            )?;
                        }
                        ctx.push_vec(&mut ordered_faces, face, "catia_component_ordered_faces")?;
                    }
                    Ok::<_, CodecError>(Some(ordered_faces))
                })?;
            let Some(ordered_faces) = ordered_faces else {
                return Ok(None);
            };
            let budget = ctx.work_budget(u64_from_index(MAX_COMPONENT_OPERATIONS));
            for round in 0..MAX_COMPONENT_ROUNDS {
                let Some(before) = quotient.monotone_measure(ctx)? else {
                    return Ok(None);
                };
                let mut cursor = 0usize;
                while cursor < ordered_faces.len() {
                    ctx.charge_work(1, "catia_mesh_quotient_iteration")?;
                    let (value, storage) =
                        ctx.with_scoped_storage("catia_component_state_storage", || {
                            let mut states = Vec::new();
                            ctx.push_vec(
                                &mut states,
                                (quotient.clone_charged(ctx)?, BTreeSet::new()),
                                "catia_component_states",
                            )?;
                            Ok::<_, CodecError>(states)
                        })?;
                    let mut states = ScopedValue {
                        value,
                        storage: Some(storage),
                    };
                    let mut processed = 0usize;
                    while let Some(&face) = ordered_faces.get(cursor + processed) {
                        ctx.charge_work(1, "catia_mesh_quotient_iteration")?;
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
                    let ScopedValue {
                        value: states,
                        storage: _state_storage,
                    } = states;
                    let (alternatives, _alternative_storage) =
                        ctx.with_scoped_storage("catia_component_alternatives", || {
                            let mut alternatives = Vec::new();
                            for (state, _) in
                                ctx.admit_iter(states, "catia_component_alternatives")?
                            {
                                ctx.push_vec(
                                    &mut alternatives,
                                    state,
                                    "catia_component_alternatives",
                                )?;
                            }
                            Ok::<_, CodecError>(alternatives)
                        })?;
                    if propagate_common_full_quotients(
                        ctx,
                        alternatives,
                        edge_candidates,
                        quotient,
                    )?
                    .is_none()
                    {
                        return Ok(None);
                    }
                    cursor += processed;
                }
                let Some(after) = quotient.monotone_measure(ctx)? else {
                    return Ok(None);
                };
                if after == before {
                    break;
                }
                if round + 1 == MAX_COMPONENT_ROUNDS {
                    return Err(ctx.refuse_codec_limit(
                        "catia_component_rounds",
                        u64_from_index(MAX_COMPONENT_ROUNDS),
                        u64_from_index(MAX_COMPONENT_ROUNDS + 1),
                    ));
                }
            }
        }
        Ok(Some(()))
    })
}

type MeshFaceSelection = Option<(usize, Vec<Vec<bool>>)>;
type MeshFaceDirectionOptions = Vec<Vec<Vec<bool>>>;
pub(super) type MeshEndpointPair = (usize, [usize; 2]);
pub(super) type MeshEndpointSolutionFilter<'a> =
    &'a dyn Fn(&[MeshEndpointPair]) -> Result<bool, CodecError>;
type MeshPartialEndpointSolutionFilter<'a> =
    &'a dyn Fn(&[Option<[usize; 2]>]) -> Result<bool, CodecError>;

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
type MeshOrientationOption<'storage> = (Vec<Vec<bool>>, MeshQuotient<'storage>);
struct CachedFaceEquations<'storage> {
    equations: ScopedValue<'storage, Vec<[usize; 2]>>,
    _key_storage: Option<ScopedReservation<'storage>>,
}
type MeshFaceEquationCache<'storage> =
    RefCell<HashMap<(usize, MeshQuotientSignature), CachedFaceEquations<'storage>>>;

fn canonical_direction_bit(row: &[bool], index: usize) -> bool {
    row[index] ^ row.first().copied().unwrap_or(false)
}

/// The least node of each node's class, indexed by node. Ascending nodes
/// meet each class first at its least node.
fn quotient_class_firsts(
    ctx: &DecodeContext<'_>,
    quotient: &MeshQuotient<'_>,
    operation: &'static str,
) -> Result<Vec<usize>, CodecError> {
    let node_count = quotient.union.len();
    let (mut first_by_root, _root_storage) =
        ctx.with_scoped_storage(operation, || ctx.alloc_filled(node_count, None, operation))?;
    let mut firsts = ctx.collection_vec(node_count, operation)?;
    for node in ctx.admit_iter(0..node_count, operation)? {
        let root = quotient.union.root(ctx, node)?;
        firsts.push(*first_by_root[root].get_or_insert(node));
    }
    Ok(firsts)
}

/// Hashes the canonical boundary directions: each row relative to its first
/// direction.
fn hash_canonical_directions(
    ctx: &DecodeContext<'_>,
    directions: &[Vec<bool>],
    hasher: &mut DefaultHasher,
    operation: &'static str,
) -> Result<(), CodecError> {
    directions.len().hash(hasher);
    for row in ctx.admit_iter(directions, operation)? {
        row.len().hash(hasher);
        for index in ctx.admit_iter(0..row.len(), operation)? {
            canonical_direction_bit(row, index).hash(hasher);
        }
    }
    Ok(())
}

fn orientation_fingerprint(
    ctx: &DecodeContext<'_>,
    quotient: &MeshQuotient<'_>,
    directions: &[Vec<bool>],
) -> Result<u64, CodecError> {
    const OPERATION: &str = "catia_orientation_fingerprint";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let firsts = storage.with_storage(|| quotient_class_firsts(ctx, quotient, OPERATION))?;
    let mut hasher = DefaultHasher::new();
    firsts.len().hash(&mut hasher);
    for (node, &first) in ctx.admit_iter(&firsts, OPERATION)?.enumerate() {
        first.hash(&mut hasher);
        if first == node {
            let root = quotient.union.root(ctx, node)?;
            ctx.hash_value(&quotient.domains[root][..], OPERATION)?
                .hash(&mut hasher);
        }
    }
    hash_canonical_directions(ctx, directions, &mut hasher, OPERATION)?;
    Ok(hasher.finish())
}

fn orientation_options_equivalent<'storage>(
    ctx: &DecodeContext<'_>,
    left_quotient: &MeshQuotient<'storage>,
    left_directions: &[Vec<bool>],
    right_quotient: &MeshQuotient<'storage>,
    right_directions: &[Vec<bool>],
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_orientation_equivalence";
    if left_quotient.union.len() != right_quotient.union.len()
        || left_directions.len() != right_directions.len()
    {
        return Ok(false);
    }
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let (left_firsts, right_firsts) = storage.with_storage(|| {
        Ok::<_, CodecError>((
            quotient_class_firsts(ctx, left_quotient, OPERATION)?,
            quotient_class_firsts(ctx, right_quotient, OPERATION)?,
        ))
    })?;
    if !ctx.equal(&left_firsts, &right_firsts, OPERATION)? {
        return Ok(false);
    }
    // Equal class firsts give both quotients the same classes; each class
    // domain is compared once, at its least node.
    for (node, &first) in ctx.admit_iter(&left_firsts, OPERATION)?.enumerate() {
        if first != node {
            continue;
        }
        let left = &left_quotient.domains[left_quotient.union.root(ctx, node)?];
        let right = &right_quotient.domains[right_quotient.union.root(ctx, node)?];
        if left.len() != right.len() || !ctx.equal(&left[..], &right[..], OPERATION)? {
            return Ok(false);
        }
    }
    ctx.all_by(
        left_directions.iter().zip(right_directions),
        |(left, right)| {
            Ok(left.len() == right.len()
                && ctx.all_by(
                    0..left.len(),
                    |index| {
                        Ok(canonical_direction_bit(left, index)
                            == canonical_direction_bit(right, index))
                    },
                    OPERATION,
                )?)
        },
        OPERATION,
    )
}

/// Keeps an orientation option unless an equivalent one is already in
/// `output`; options are bucketed by fingerprint.
fn admit_orientation_option<'storage>(
    ctx: &'storage DecodeContext<'_>,
    seen: &mut HashMap<u64, Vec<usize>>,
    seen_storage: &mut ScopedReservation<'_>,
    output: &[MeshOrientationOption<'storage>],
    directions: &[Vec<bool>],
    quotient: &MeshQuotient<'storage>,
) -> Result<bool, CodecError> {
    let fingerprint = orientation_fingerprint(ctx, quotient, directions)?;
    if let Some(indices) =
        ctx.get_hash_map(seen, &fingerprint, "catia_orientation_fingerprint_keys")?
    {
        let equivalent = ctx.any_by(
            indices,
            |&index| {
                let (prior_directions, prior_quotient) = &output[index];
                orientation_options_equivalent(
                    ctx,
                    quotient,
                    directions,
                    prior_quotient,
                    prior_directions,
                )
            },
            "catia_orientation_dedup_compare",
        )?;
        if equivalent {
            return Ok(false);
        }
    }
    seen_storage.with_storage(|| {
        ctx.push_vec(
            ctx.entry_hash_map(seen, fingerprint, "catia_orientation_fingerprint_keys")?
                .or_default(),
            output.len(),
            "catia_orientation_fingerprint_indices",
        )?;
        Ok::<_, CodecError>(())
    })?;
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
    let (normalized, _normalized_storage) =
        ctx.with_scoped_storage("catia_edge_class_normalized_rows", || {
            let mut normalized =
                ctx.collect_indexed_vec(choices.len(), "catia_edge_class_normalized_rows", |_| {
                    Ok(Vec::new())
                })?;
            for (row, pairs) in ctx
                .admit_iter(&mut normalized, "catia_edge_class_normalized_rows")?
                .zip(choices)
            {
                *row = ctx.alloc_filled(
                    pairs.len(),
                    [0usize; 2],
                    "catia_edge_class_normalized_pairs",
                )?;
                for (normalized_pair, &[left, right]) in ctx
                    .admit_iter(&mut *row, "catia_edge_class_normalized_pairs")?
                    .zip(pairs)
                {
                    *normalized_pair = [left.min(right), left.max(right)];
                }
                ctx.sort_unstable_by(
                    row,
                    |value| value,
                    Ord::cmp,
                    "catia_edge_class_normalized_pairs_sort",
                )?;
                ctx.dedup_vec(row, "catia_edge_class_normalized_pairs_dedup")?;
            }
            Ok::<_, CodecError>(normalized)
        })?;
    // Rows of one class with the same normalized pairs form a group.
    let (grouped, _group_storage) = ctx.with_scoped_storage("catia_edge_class_groups", || {
        let mut grouped = ctx.collect_vec(
            normalized
                .iter()
                .enumerate()
                .map(|(edge, row)| (edge_classes[edge], row, edge)),
            "catia_edge_class_groups",
        )?;
        ctx.sort_unstable_by(
            &mut grouped,
            |entry| entry,
            Ord::cmp,
            "catia_edge_class_groups_sort",
        )?;
        Ok::<_, CodecError>(grouped)
    })?;
    let mut active = ctx.alloc_filled(choices.len(), false, "catia_edge_class_active")?;
    let mut ordered = Vec::new();
    let mut start = 0;
    while start < grouped.len() {
        ctx.charge_work(1, "catia_edge_class_groups")?;
        let (class, row, _) = grouped[start];
        let length = ctx.partition_point(
            &grouped[start..],
            |&(other_class, other_row, _)| {
                Ok(other_class == class
                    && other_row.len() == row.len()
                    && ctx.equal(other_row, row, "catia_edge_class_groups")?)
            },
            "catia_edge_class_groups",
        )?;
        let group = &grouped[start..start + length];
        start += length;
        if row.len() < 2 || group.len() < 2 {
            continue;
        }
        for (index, &(_, _, left)) in ctx
            .admit_iter(group, "catia_edge_class_ordered_pairs")?
            .enumerate()
        {
            active[left] = true;
            for &(_, _, right) in
                ctx.admit_iter(&group[index + 1..], "catia_edge_class_ordered_pairs")?
            {
                ctx.push_vec(
                    &mut ordered,
                    (left.min(right), left.max(right)),
                    "catia_edge_class_ordered_pairs",
                )?;
            }
        }
    }
    ctx.sort_unstable_by(
        &mut ordered,
        |pair| pair,
        Ord::cmp,
        "catia_edge_class_ordered_pairs_sort",
    )?;
    Ok(Some(EdgeClassSearchConstraint { active, ordered }))
}

#[test]
fn edge_class_ordered_pairs_refuse_before_growth() {
    let choices = vec![vec![[0, 1], [1, 2]], vec![[0, 1], [1, 2]]];
    let mut refused = HashSet::new();
    for cap in 0..32 {
        match crate::test_support::with_collection_limit(cap, |ctx| {
            edge_class_search_constraint(ctx, &[0, 0], &choices)
        }) {
            Err(CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(Some(_)) => break,
            Ok(None) => panic!("identical edge classes must have a search order"),
            Err(error) => panic!("unexpected edge class refusal: {error}"),
        }
    }
    assert!(refused.contains("catia_edge_class_ordered_pairs"));
    assert_eq!(
        crate::test_support::with_service_context(|ctx| edge_class_search_constraint(
            ctx,
            &[0, 0],
            &choices
        ))
        .expect("service resource budget")
        .expect("matching edge classes")
        .ordered,
        vec![(0, 1)]
    );
}

fn changed_quotient_edges<'storage>(
    ctx: &'storage DecodeContext<'_>,
    left: &MeshQuotient<'storage>,
    right: &MeshQuotient<'storage>,
) -> Result<HashSet<usize>, CodecError> {
    const OPERATION: &str = "catia_changed_quotient_edges";
    if left.union.len() != right.union.len() {
        return Err(CodecError::malformed("quotient node counts differ"));
    }
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let (roots, mut changed_roots) = storage.with_storage(|| {
        let mut changed_roots = ctx.alloc_filled(left.union.len(), false, OPERATION)?;
        let roots = ctx.collect_indexed_vec(left.union.len(), OPERATION, |node| {
            let left_root = left.union.root(ctx, node)?;
            let right_root = right.union.root(ctx, node)?;
            if left_root != right_root {
                changed_roots[left_root] = true;
                changed_roots[right_root] = true;
            }
            Ok([left_root, right_root])
        })?;
        Ok::<_, CodecError>((roots, changed_roots))
    })?;
    // A changed member marks its two classes. An unchanged class needs one
    // domain comparison, at its representative, independent of member order.
    for (node, &[left_root, right_root]) in ctx.admit_iter(&roots, OPERATION)?.enumerate() {
        if node != left_root || left_root != right_root || changed_roots[left_root] {
            continue;
        }
        let left_domain = &left.domains[left_root];
        let right_domain = &right.domains[right_root];
        if left_domain.len() != right_domain.len()
            || !(Rc::ptr_eq(left_domain, right_domain)
                || ctx.equal(&left_domain[..], &right_domain[..], OPERATION)?)
        {
            changed_roots[left_root] = true;
        }
    }
    let mut changed = HashSet::new();
    for (node, &[left_root, right_root]) in ctx.admit_iter(&roots, OPERATION)?.enumerate() {
        if changed_roots[left_root] || changed_roots[right_root] {
            ctx.insert_hash_set(&mut changed, node / 2, OPERATION)?;
        }
    }
    Ok(changed)
}

#[test]
fn changed_quotient_edges_refuse_before_result_set_growth() {
    let points = Arc::new(HashSet::from([0]));
    let left = MeshQuotient::new(vec![Arc::clone(&points), Arc::clone(&points)]);
    let mut right = left.clone();
    assert!(
        crate::test_support::with_service_context(|ctx| right.merge_charged(ctx, 0, 1))
            .expect("service merge")
            .is_some()
    );
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
    visited_states: HashMap<MeshSelectionStateSignature, ScopedReservation<'a>>,
    outcome: SearchOutcome<(StandardTopologyDraft, Vec<usize>)>,
    face_equation_cache: MeshFaceEquationCache<'a>,
    memo_storage: RefCell<ScopedReservation<'a>>,
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
    for assignments in ctx.admit_iter(faces, "catia_possible_face_equation_faces")? {
        let mut equations = Vec::new();
        for assignment in ctx.admit_iter(assignments, "catia_possible_face_equation_work")? {
            for boundary in
                ctx.admit_iter(&assignment.boundaries, "catia_possible_face_equation_work")?
            {
                for index in
                    ctx.admit_iter(0..boundary.len(), "catia_possible_face_equation_work")?
                {
                    let left = ports(boundary[index], true);
                    let right = ports(boundary[(index + 1) % boundary.len()], false);
                    // At most two ports on each side of a corner.
                    for left in left.into_iter().flatten() {
                        for right in right.into_iter().flatten() {
                            ctx.push_vec(
                                &mut equations,
                                [left.min(right), left.max(right)],
                                "catia_possible_face_equation_values",
                            )?;
                        }
                    }
                }
            }
        }
        ctx.sort_unstable_by(
            &mut equations,
            |value| value,
            Ord::cmp,
            "catia mesh possible face equations sort",
        )?;
        ctx.dedup_vec(&mut equations, "catia_possible_face_equation_keys")?;
        ctx.push_vec(
            &mut faces_equations,
            equations,
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
    let mut faces = faces.iter().zip(face_equations);
    while let Some((assignments, fallback)) =
        ctx.next_charged(&mut faces, "catia_possible_face_choice_faces")?
    {
        let mut choices_storage = ctx.reserve_scoped(0, "catia_possible_face_choice_keys")?;
        let mut choices = Vec::new();
        let mut assignments = assignments.iter();
        while let Some(assignment) =
            ctx.next_charged(&mut assignments, "catia_possible_face_choice_keys")?
        {
            if !budget.charge() {
                return Ok(false);
            }
            let unknown = ctx.fold(
                &assignment.boundaries,
                0usize,
                |unknown, boundary| {
                    Ok(unknown
                        + ctx.fold(
                            boundary,
                            0usize,
                            |unknown, use_| Ok(unknown + usize::from(use_.reversed.is_none())),
                            "catia_possible_face_choice_unknowns",
                        )?)
                },
                "catia_possible_face_choice_unknowns",
            )?;
            let combinations = u32::try_from(unknown)
                .ok()
                .and_then(|unknown| 1usize.checked_shl(unknown));
            let Some(combinations) = combinations.filter(|combinations| *combinations <= 4_096)
            else {
                ctx.clear_vec(&mut choices, "catia_possible_face_choice_fallback_discard")?;
                let (value, storage) = ctx
                    .with_scoped_storage("catia_possible_face_choice_fallback_equations", || {
                        ctx.copy_slice(fallback, "catia_possible_face_choice_fallback_equations")
                    })?;
                choices_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut choices,
                        ScopedValue {
                            value,
                            storage: Some(storage),
                        },
                        "catia_possible_face_choice_keys",
                    )
                })?;
                break;
            };
            for mask in 0..combinations {
                if !budget.charge() {
                    return Ok(false);
                }
                let (directions, _direction_storage) = ctx.with_scoped_storage(
                    "catia_possible_face_choice_direction_storage",
                    || {
                        let mut variable = 0usize;
                        let mut directions = Vec::new();
                        for boundary in ctx.admit_iter(
                            &assignment.boundaries,
                            "catia_possible_face_choice_directions",
                        )? {
                            let mut row = Vec::new();
                            for use_ in
                                ctx.admit_iter(boundary, "catia_possible_face_choice_directions")?
                            {
                                let direction = use_.reversed.unwrap_or_else(|| {
                                    let shift = unknown - variable - 1;
                                    variable += 1;
                                    mask & (1usize << shift) != 0
                                });
                                ctx.push_vec(
                                    &mut row,
                                    direction,
                                    "catia_possible_face_choice_directions",
                                )?;
                            }
                            ctx.push_vec(
                                &mut directions,
                                row,
                                "catia_possible_face_choice_direction_rows",
                            )?;
                        }
                        Ok::<_, CodecError>(directions)
                    },
                )?;
                let (equations, storage) =
                    ctx.with_scoped_storage("catia_possible_face_choice_equation_storage", || {
                        let mut equations = Vec::new();
                        for (boundary, row) in ctx
                            .admit_iter(
                                &assignment.boundaries,
                                "catia_possible_face_choice_equations",
                            )?
                            .zip(&directions)
                        {
                            for index in ctx.admit_iter(
                                0..boundary.len(),
                                "catia_possible_face_choice_equations",
                            )? {
                                let next = (index + 1) % boundary.len();
                                let Some(left) = port(boundary[index], row[index], true) else {
                                    return Ok(None);
                                };
                                let Some(right) = port(boundary[next], row[next], false) else {
                                    return Ok(None);
                                };
                                ctx.push_vec(
                                    &mut equations,
                                    [left.min(right), left.max(right)],
                                    "catia_possible_face_choice_equations",
                                )?;
                            }
                        }
                        ctx.sort_unstable_by(
                            &mut equations,
                            |value| value,
                            Ord::cmp,
                            "catia_possible_face_choice_equations_sort",
                        )?;
                        ctx.dedup_vec(
                            &mut equations,
                            "catia_possible_face_choice_equations_dedup",
                        )?;
                        Ok::<_, CodecError>(Some(equations))
                    })?;
                if let Some(value) = equations {
                    choices_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut choices,
                            ScopedValue {
                                value,
                                storage: Some(storage),
                            },
                            "catia_possible_face_choice_keys",
                        )
                    })?;
                }
            }
        }
        ctx.sort_unstable_by(
            &mut choices,
            |choice| &choice.value,
            Ord::cmp,
            "catia_possible_face_choice_values_sort",
        )?;
        ctx.dedup_by(
            &mut choices,
            |left, right| {
                ctx.equal(
                    &left.value,
                    &right.value,
                    "catia_possible_face_choice_values",
                )
            },
            "catia_possible_face_choice_values",
        )?;
        let choices = ctx.try_collect_retained_with(
            choices,
            "catia_possible_face_choice_keys",
            |mut choice| {
                choice
                    .storage
                    .take()
                    .ok_or_else(|| CodecError::malformed("face choice owns storage"))?
                    .commit()?;
                Ok::<_, CodecError>(choice.value)
            },
        )?;
        ctx.push_vec(faces_choices, choices, "catia_possible_face_choice_faces")?;
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

/// The start of the least rotation of a cycle, by the two-candidate scan:
/// each step either extends the common prefix or discards at least one
/// candidate start, so the scan takes at most `3 n` steps.
pub(super) fn least_rotation<T: Ord>(
    ctx: &DecodeContext<'_>,
    values: &[T],
    operation: &'static str,
) -> Result<usize, CodecError> {
    let length = values.len();
    let (mut first, mut second, mut matched) = (0usize, 1usize, 0usize);
    while first < length && second < length && matched < length {
        ctx.charge_work(1, operation)?;
        let left = &values[(first + matched) % length];
        let right = &values[(second + matched) % length];
        match left.cmp(right) {
            std::cmp::Ordering::Equal => {
                matched += 1;
                continue;
            }
            std::cmp::Ordering::Greater => first += matched + 1,
            std::cmp::Ordering::Less => second += matched + 1,
        }
        if first == second {
            second += 1;
        }
        matched = 0;
    }
    Ok(first.min(second))
}

fn deduplicate_mesh_quotient_assignments(
    ctx: &DecodeContext<'_>,
    faces: &mut [Vec<MeshFaceBoundaryAssignment>],
) -> Result<(), CodecError> {
    /// The least rotation of the cycle read forward or backward.
    fn canonical_cycle(
        ctx: &DecodeContext<'_>,
        boundary: &[MeshBoundaryEdgeCandidate],
    ) -> Result<Vec<(usize, Option<bool>)>, CodecError> {
        let (forward, _forward_storage) =
            ctx.with_scoped_storage("catia_mesh_quotient_cycle_forward", || {
                ctx.collect_vec(
                    boundary.iter().map(|use_| (use_.edge, use_.reversed)),
                    "catia_mesh_quotient_cycle_forward",
                )
            })?;
        let (reversed, _reverse_storage) =
            ctx.with_scoped_storage("catia_mesh_quotient_cycle_reverse", || {
                ctx.collect_vec(
                    boundary
                        .iter()
                        .rev()
                        .map(|use_| (use_.edge, use_.reversed.map(|value| !value))),
                    "catia_mesh_quotient_cycle_reverse",
                )
            })?;
        let forward_start = least_rotation(ctx, &forward, "catia_mesh_quotient_cycle_compare")?;
        let reverse_start = least_rotation(ctx, &reversed, "catia_mesh_quotient_cycle_compare")?;
        let reverse_first = ctx
            .find_map(
                0..forward.len(),
                |offset| {
                    let order = reversed[(reverse_start + offset) % reversed.len()]
                        .cmp(&forward[(forward_start + offset) % forward.len()]);
                    Ok((!order.is_eq()).then_some(order.is_lt()))
                },
                "catia_mesh_quotient_cycle_compare",
            )?
            .unwrap_or(false);
        let (values, start) = if reverse_first {
            (&reversed, reverse_start)
        } else {
            (&forward, forward_start)
        };
        ctx.collect_vec(
            values[start..].iter().chain(&values[..start]).copied(),
            "catia_mesh_quotient_canonical_cycle",
        )
    }

    let mut scratch = ctx.reserve_scoped(0, "catia_mesh_quotient_seen_assignments")?;
    scratch.with_storage(|| {
        for assignments in ctx.admit_iter(faces, "catia_mesh_quotient_seen_assignments")? {
            let mut seen = HashSet::new();
            ctx.retain_vec(
                assignments,
                |assignment| {
                    let mut signature = Vec::new();
                    for boundary in ctx.admit_iter(
                        &assignment.boundaries,
                        "catia_mesh_quotient_signature_boundaries",
                    )? {
                        let cycle = canonical_cycle(ctx, boundary)?;
                        ctx.push_vec(
                            &mut signature,
                            cycle,
                            "catia_mesh_quotient_signature_boundaries",
                        )?;
                    }
                    ctx.sort_unstable_by(
                        &mut signature,
                        |value| value,
                        Ord::cmp,
                        "catia_mesh_quotient_signature_boundaries_sort",
                    )?;
                    ctx.insert_hash_set(
                        &mut seen,
                        signature,
                        "catia_mesh_quotient_seen_assignments",
                    )
                },
                "catia_mesh_quotient_seen_assignments",
            )?;
        }
        Ok(())
    })
}

pub(super) fn mesh_assignment_endpoint_cycles_viable_by<'a>(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    budget: Option<&WorkBudget<'_>>,
    candidates: impl Fn(usize) -> Option<MeshEndpointCandidates<'a>>,
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Result<Option<bool>, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "catia_mesh_assignment_endpoint_cycles_viable_by_scratch")?;
    scratch.with_storage(|| {
        const MAX_LOCAL_ENDPOINT_STATES: usize = 65_536;

        /// Both orientations of the allowed candidate pairs, ascending, so a
        /// point's neighbors form one run. Each examined pair consumes one unit
        /// of `budget`; at most `MAX_LOCAL_ENDPOINT_STATES` pairs are examined.
        fn endpoint_adjacency(
            ctx: &DecodeContext<'_>,
            mut next_pair: impl FnMut() -> Result<Option<[usize; 2]>, CodecError>,
            allowed: impl Fn([usize; 2]) -> bool,
            budget: Option<&WorkBudget<'_>>,
        ) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
            let mut adjacency = Vec::new();
            let mut count = 0usize;
            while let Some(pair @ [left, right]) = next_pair()? {
                if budget.is_some_and(|budget| !budget.charge()) {
                    return Ok(None);
                }
                count += 1;
                if count > MAX_LOCAL_ENDPOINT_STATES {
                    return Ok(None);
                }
                if !allowed(pair) {
                    continue;
                }
                ctx.push_vec(
                    &mut adjacency,
                    [left, right],
                    "catia_endpoint_viability_neighbors",
                )?;
                if right != left {
                    ctx.push_vec(
                        &mut adjacency,
                        [right, left],
                        "catia_endpoint_viability_neighbors",
                    )?;
                }
            }
            ctx.sort_unstable_by(
                &mut adjacency,
                |value| value,
                Ord::cmp,
                "catia_endpoint_viability_neighbors_sort",
            )?;
            ctx.dedup_vec(&mut adjacency, "catia_endpoint_viability_neighbors_dedup")?;
            Ok((!adjacency.is_empty()).then_some(adjacency))
        }

        for boundary in ctx.admit_iter(
            &assignment.boundaries,
            "catia_endpoint_viability_boundaries",
        )? {
            if boundary.is_empty() {
                return Ok(Some(false));
            }
            let (prepared, _prepared_storage) =
                ctx.with_scoped_storage("catia_endpoint_viability_prepared", || {
                    let mut prepared = HashMap::<usize, Vec<[usize; 2]>>::new();
                    for use_ in ctx.admit_iter(boundary, "catia_endpoint_viability_prepared")? {
                        if ctx.contains_key_hash_map(
                            &prepared,
                            &use_.edge,
                            "catia_endpoint_viability_prepared",
                        )? {
                            continue;
                        }
                        let Some(candidate) = candidates(use_.edge) else {
                            return Ok(None);
                        };
                        let adjacency = match candidate {
                            MeshEndpointCandidates::Explicit(values) => {
                                let mut values = values.iter().copied();
                                endpoint_adjacency(
                                    ctx,
                                    || {
                                        ctx.next_charged(
                                            &mut values,
                                            "catia_endpoint_viability_adjacency",
                                        )
                                    },
                                    |pair| allowed(use_.edge, pair),
                                    budget,
                                )
                            }
                            MeshEndpointCandidates::Implicit(mut values) => endpoint_adjacency(
                                ctx,
                                || values.next_with_context(ctx),
                                |pair| allowed(use_.edge, pair),
                                budget,
                            ),
                            MeshEndpointCandidates::Selected(value) => {
                                let mut value = Some(value);
                                endpoint_adjacency(
                                    ctx,
                                    || Ok(value.take()),
                                    |pair| allowed(use_.edge, pair),
                                    budget,
                                )
                            }
                        }?;
                        let Some(adjacency) = adjacency else {
                            return Ok(None);
                        };
                        ctx.insert_hash_map(
                            &mut prepared,
                            use_.edge,
                            adjacency,
                            "catia_endpoint_viability_prepared",
                        )?;
                    }
                    Ok::<_, CodecError>(Some(prepared))
                })?;
            let Some(prepared) = prepared else {
                return Ok(None);
            };
            let adjacency_of = |edge: usize| -> Result<&[[usize; 2]], CodecError> {
                Ok(ctx
                    .get_hash_map(&prepared, &edge, "catia_endpoint_viability_prepared")?
                    .map_or(&[][..], Vec::as_slice))
            };
            // A state is a walk's start point and its current point.
            let (states, mut state_storage) =
                ctx.with_scoped_storage("catia_endpoint_viability_states", || {
                    let mut states = BTreeSet::new();
                    for &state in ctx.admit_iter(
                        adjacency_of(boundary[0].edge)?,
                        "catia_endpoint_viability_states",
                    )? {
                        if budget.is_some_and(|budget| !budget.charge()) {
                            return Ok(None);
                        }
                        ctx.insert_btree_set(
                            &mut states,
                            (state[0], state[1]),
                            "catia_endpoint_viability_states",
                        )?;
                    }
                    if states.len() > MAX_LOCAL_ENDPOINT_STATES {
                        return Ok(None);
                    }
                    Ok::<_, CodecError>(Some(states))
                })?;
            let Some(mut states) = states else {
                return Ok(None);
            };
            for use_ in ctx.admit_iter(&boundary[1..], "catia_endpoint_viability_next_states")? {
                let adjacency = adjacency_of(use_.edge)?;
                let (next, next_storage) =
                    ctx.with_scoped_storage("catia_endpoint_viability_next_states", || {
                        let mut next = BTreeSet::new();
                        for &(start, current) in
                            ctx.admit_iter(&states, "catia_endpoint_viability_next_states")?
                        {
                            let neighbors = point_support_run(
                                ctx,
                                adjacency,
                                current,
                                "catia_endpoint_viability_next_states",
                            )?;
                            for pair in
                                ctx.admit_iter(neighbors, "catia_endpoint_viability_next_states")?
                            {
                                if budget.is_some_and(|budget| !budget.charge()) {
                                    return Ok(None);
                                }
                                ctx.insert_btree_set(
                                    &mut next,
                                    (start, pair[1]),
                                    "catia_endpoint_viability_next_states",
                                )?;
                                if next.len() > MAX_LOCAL_ENDPOINT_STATES {
                                    return Ok(None);
                                }
                            }
                        }
                        Ok::<_, CodecError>(Some(next))
                    })?;
                let Some(next) = next else {
                    return Ok(None);
                };
                states = next;
                state_storage = next_storage;
                if states.is_empty() {
                    return Ok(Some(false));
                }
            }
            let _state_storage = state_storage;
            if !ctx.any_by(
                &states,
                |&(start, current)| Ok(start == current),
                "catia_endpoint_viability_closed",
            )? {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    })
}

pub(super) fn mesh_assignment_endpoint_cycles_viable_where(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
    budget: Option<&WorkBudget<'_>>,
    allowed: impl Fn(usize, [usize; 2]) -> bool + Copy,
) -> Result<Option<bool>, CodecError> {
    mesh_assignment_endpoint_cycles_viable_by(
        ctx,
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
    /// Each edge's supported pairs, normalized and ascending.
    pub(super) by_edge: BTreeMap<usize, Vec<[usize; 2]>>,
}

/// Keeps the pairs of `retained` that `supported` also holds; both ascending.
fn retain_supported_pairs(
    ctx: &DecodeContext<'_>,
    retained: &mut Vec<[usize; 2]>,
    supported: &[[usize; 2]],
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.retain_vec(
        retained,
        |pair| Ok(ctx.binary_search(supported, pair, operation)?.is_ok()),
        operation,
    )
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
        let ends = ctx
            .entry_btree_map(relation, start, operation)?
            .or_default();
        ctx.insert_btree_set(ends, end, operation)
    }

    fn compose_relations(
        ctx: &DecodeContext<'_>,
        left: &EndpointRelation,
        right: &EndpointRelation,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Option<EndpointRelation>, CodecError> {
        const OPERATION: &str = "catia_endpoint_relation_composition";
        let mut composed = EndpointRelation::new();
        let mut state_count = 0usize;
        for (&start, middles) in ctx.admit_iter(left, OPERATION)? {
            for middle in ctx.admit_iter(middles, OPERATION)? {
                let Some(ends) = ctx.get_btree_map(right, middle, OPERATION)? else {
                    continue;
                };
                for &end in ctx.admit_iter(ends, OPERATION)? {
                    if budget.is_some_and(|budget| !budget.charge()) {
                        return Ok(None);
                    }
                    if insert_relation(ctx, &mut composed, start, end, OPERATION)? {
                        state_count += 1;
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
        operation: &'static str,
    ) -> Result<EndpointRelation, CodecError> {
        let mut identity = EndpointRelation::new();
        for &point in ctx.admit_iter(points, operation)? {
            insert_relation(ctx, &mut identity, point, point, operation)?;
        }
        Ok(identity)
    }

    let unsupported = || MeshEndpointPairSupport {
        by_edge: BTreeMap::new(),
    };
    let charge = || budget.is_none_or(WorkBudget::charge);
    let mut assignment_support = BTreeMap::<usize, Vec<[usize; 2]>>::new();
    for boundary in ctx.admit_iter(&assignment.boundaries, "catia_endpoint_layers")? {
        if boundary.is_empty() {
            return Ok(Some(unsupported()));
        }
        let (prepared, _boundary_storage) =
            ctx.with_scoped_storage("catia_endpoint_support_boundary_storage", || {
                let mut points = BTreeSet::new();
                let mut layers = Vec::<(usize, Vec<[usize; 2]>, EndpointRelation)>::new();
                for use_ in ctx.admit_iter(boundary, "catia_endpoint_layers")? {
                    let Some(values) = candidates(use_.edge) else {
                        return Ok(ControlFlow::Break(None));
                    };
                    let values = match values {
                        MeshEndpointCandidates::Explicit(values) => {
                            ctx.copy_slice(values, "catia_endpoint_layer_values")?
                        }
                        MeshEndpointCandidates::Implicit(mut values) => {
                            let mut collected = Vec::new();
                            while collected.len() <= MAX_LOCAL_ENDPOINT_STATES {
                                let Some(value) = values.next_with_context(ctx)? else {
                                    break;
                                };
                                ctx.push_vec(&mut collected, value, "catia_endpoint_layer_values")?;
                            }
                            collected
                        }
                        MeshEndpointCandidates::Selected(value) => {
                            let mut selected =
                                ctx.collection_vec(1, "catia_endpoint_layer_values")?;
                            selected.push(value);
                            selected
                        }
                    };
                    if values.len() > MAX_LOCAL_ENDPOINT_STATES {
                        return Ok(ControlFlow::Break(None));
                    }
                    let mut retained = Vec::new();
                    let mut relation = EndpointRelation::new();
                    for [left, right] in ctx.admit_iter(values, "catia_endpoint_layer_relation")? {
                        let pair = [left.min(right), left.max(right)];
                        if !allowed(use_.edge, pair) {
                            continue;
                        }
                        ctx.push_vec(&mut retained, pair, "catia_endpoint_layer_retained_pairs")?;
                        for point in pair {
                            ctx.insert_btree_set(
                                &mut points,
                                point,
                                "catia_endpoint_layer_points",
                            )?;
                        }
                        for (rank, (start, end)) in [(pair[0], pair[1]), (pair[1], pair[0])]
                            .into_iter()
                            .enumerate()
                        {
                            if rank == 1 && pair[0] == pair[1] {
                                continue;
                            }
                            if !charge() {
                                return Ok(ControlFlow::Break(None));
                            }
                            insert_relation(
                                ctx,
                                &mut relation,
                                start,
                                end,
                                "catia_endpoint_layer_relation",
                            )?;
                        }
                    }
                    ctx.sort_unstable_by(
                        &mut retained,
                        |value| value,
                        Ord::cmp,
                        "catia_endpoint_layer_retained_pairs_sort",
                    )?;
                    ctx.dedup_vec(&mut retained, "catia_endpoint_layer_retained_pairs_dedup")?;
                    if retained.is_empty() {
                        return Ok(ControlFlow::Break(Some(unsupported())));
                    }
                    ctx.push_vec(
                        &mut layers,
                        (use_.edge, retained, relation),
                        "catia_endpoint_layers",
                    )?;
                }
                if points.len() > MAX_LOCAL_ENDPOINT_STATES {
                    return Ok(ControlFlow::Break(None));
                }
                let layer_count = layers.len() + 1;
                let mut prefixes = Vec::new();
                let first = identity_relation(ctx, &points, "catia_endpoint_prefix_identity")?;
                ctx.push_vec(&mut prefixes, first, "catia_endpoint_prefixes")?;
                for (index, (_, _, relation)) in ctx
                    .admit_iter(&layers, "catia_endpoint_prefixes")?
                    .enumerate()
                {
                    let Some(composed) =
                        compose_relations(ctx, &prefixes[index], relation, budget)?
                    else {
                        return Ok(ControlFlow::Break(None));
                    };
                    ctx.push_vec(&mut prefixes, composed, "catia_endpoint_prefixes")?;
                }
                let mut suffixes =
                    ctx.collect_indexed_vec(layer_count, "catia_endpoint_suffixes", |_| {
                        Ok(EndpointRelation::new())
                    })?;
                suffixes[layers.len()] =
                    identity_relation(ctx, &points, "catia_endpoint_identity_relation")?;
                for layer in ctx
                    .admit_iter(0..layers.len(), "catia_endpoint_suffixes")?
                    .rev()
                {
                    let Some(composed) =
                        compose_relations(ctx, &layers[layer].2, &suffixes[layer + 1], budget)?
                    else {
                        return Ok(ControlFlow::Break(None));
                    };
                    suffixes[layer] = composed;
                }
                Ok::<_, CodecError>(ControlFlow::Continue((layers, prefixes, suffixes)))
            })?;
        let (layers, prefixes, suffixes) = match prepared {
            ControlFlow::Break(outcome) => return Ok(outcome),
            ControlFlow::Continue(prepared) => prepared,
        };
        let mut boundary_support_storage =
            ctx.reserve_scoped(0, "catia_endpoint_boundary_support")?;
        let mut boundary_support = BTreeMap::<usize, Vec<[usize; 2]>>::new();
        for (layer, (edge, candidates, _)) in ctx
            .admit_iter(layers, "catia_endpoint_layer_support")?
            .enumerate()
        {
            // A pair is supported when one orientation closes a cycle: some
            // anchor reaches its start through the prefix and is reached from
            // its end through the suffix.
            let mut layer_support = Vec::new();
            for pair in ctx.admit_iter(candidates, "catia_endpoint_layer_support")? {
                let mut supported = false;
                for (rank, (start, end)) in [(pair[0], pair[1]), (pair[1], pair[0])]
                    .into_iter()
                    .enumerate()
                {
                    if supported || (rank == 1 && pair[0] == pair[1]) {
                        continue;
                    }
                    let Some(anchors) = ctx.get_btree_map(
                        &suffixes[layer + 1],
                        &end,
                        "catia_endpoint_layer_support",
                    )?
                    else {
                        continue;
                    };
                    supported = ctx.any_by(
                        anchors,
                        |anchor| {
                            if !charge() {
                                return Ok(false);
                            }
                            Ok(
                                match ctx.get_btree_map(
                                    &prefixes[layer],
                                    anchor,
                                    "catia_endpoint_layer_support",
                                )? {
                                    Some(ends) => ctx.contains_btree_set(
                                        ends,
                                        &start,
                                        "catia_endpoint_layer_support",
                                    )?,
                                    None => false,
                                },
                            )
                        },
                        "catia_endpoint_layer_support",
                    )?;
                }
                if budget.is_some_and(WorkBudget::exhausted) {
                    return Ok(None);
                }
                if supported {
                    ctx.push_vec(&mut layer_support, pair, "catia_endpoint_layer_support")?;
                }
            }
            match ctx.get_mut_btree_map(
                &mut boundary_support,
                &edge,
                "catia_endpoint_boundary_support",
            )? {
                Some(retained) => retain_supported_pairs(
                    ctx,
                    retained,
                    &layer_support,
                    "catia_endpoint_boundary_support",
                )?,
                None => {
                    boundary_support_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut boundary_support,
                            edge,
                            layer_support,
                            "catia_endpoint_boundary_support",
                        )
                    })?;
                }
            }
        }
        if ctx.any_by(
            boundary,
            |use_| {
                Ok(ctx
                    .get_btree_map(
                        &boundary_support,
                        &use_.edge,
                        "catia_endpoint_boundary_support",
                    )?
                    .is_none_or(Vec::is_empty))
            },
            "catia_endpoint_boundary_support",
        )? {
            return Ok(Some(unsupported()));
        }
        for (edge, supported) in
            ctx.admit_iter(boundary_support, "catia_endpoint_assignment_support")?
        {
            match ctx.get_mut_btree_map(
                &mut assignment_support,
                &edge,
                "catia_endpoint_assignment_support",
            )? {
                Some(retained) => retain_supported_pairs(
                    ctx,
                    retained,
                    &supported,
                    "catia_endpoint_assignment_support",
                )?,
                None => {
                    ctx.insert_btree_map(
                        &mut assignment_support,
                        edge,
                        supported,
                        "catia_endpoint_assignment_support",
                    )?;
                }
            }
        }
        if ctx.any_by(
            &assignment_support,
            |(_, pairs)| Ok(pairs.is_empty()),
            "catia_endpoint_assignment_support",
        )? {
            return Ok(Some(unsupported()));
        }
    }
    Ok(Some(MeshEndpointPairSupport {
        by_edge: assignment_support,
    }))
}

fn mesh_assignment_endpoint_cycles_viable_with(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    edge_candidates: &[Vec<[usize; 2]>],
    required: Option<(usize, [usize; 2])>,
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<bool>, CodecError> {
    mesh_assignment_endpoint_cycles_viable_where(
        ctx,
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
        mesh_assignment_endpoint_cycles_viable_with(ctx, assignment, edge_candidates, None, None)
            .expect("service resource budget")
            .unwrap_or(true)
    })
}

pub(super) fn mesh_face_endpoint_configurations(
    ctx: &DecodeContext<'_>,
    assignments: &[MeshFaceBoundaryAssignment],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[Option<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> Result<Option<MeshFaceEndpointConfigurations>, CodecError> {
    /// Records `pair` for `edge` in a configuration kept ascending by edge.
    /// Returns false when the configuration already holds another pair for
    /// the edge.
    fn insert_pair(
        ctx: &DecodeContext<'_>,
        configuration: &mut MeshFaceEndpointConfiguration,
        edge: usize,
        [left, right]: [usize; 2],
    ) -> Result<bool, CodecError> {
        let pair = [left.min(right), left.max(right)];
        match ctx.binary_search_by_key(
            configuration,
            &edge,
            |(stored, _)| Ok(*stored),
            "catia_face_configuration_pairs",
        )? {
            Ok(index) => Ok(configuration[index].1 == pair),
            Err(index) => {
                ctx.insert_vec(
                    configuration,
                    index,
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
            || ctx.any_by(
                boundary,
                |use_| Ok(edge_candidates.get(use_.edge).is_none_or(Vec::is_empty)),
                "catia_face_configuration_boundary",
            )?
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
        let (states, mut state_storage) =
            ctx.with_scoped_storage("catia_face_configuration_state_storage", || {
                let mut states = Vec::new();
                for &pair @ [left, right] in ctx.admit_iter(
                    &edge_candidates[boundary[0].edge],
                    "catia_face_configuration_initial_states",
                )? {
                    if !allowed(boundary[0].edge, pair) {
                        continue;
                    }
                    let directions = [(left, right), (right, left)];
                    let direction_count = usize::from(left != right) + 1;
                    for &(start, current) in &directions[..direction_count] {
                        let (configuration, storage) =
                            ctx.with_scoped_storage("catia_face_configuration_pairs", || {
                                let mut configuration = Vec::new();
                                let inserted =
                                    insert_pair(ctx, &mut configuration, boundary[0].edge, pair)?;
                                Ok::<_, CodecError>(inserted.then_some(configuration))
                            })?;
                        if let Some(configuration) = configuration {
                            if charge(work).is_none() {
                                return Ok(None);
                            }
                            ctx.push_vec(
                                &mut states,
                                (
                                    start,
                                    current,
                                    ScopedValue {
                                        value: configuration,
                                        storage: Some(storage),
                                    },
                                ),
                                "catia_face_configuration_initial_states",
                            )?;
                        }
                    }
                }
                Ok::<_, CodecError>(Some(states))
            })?;
        let Some(mut states) = states else {
            return Ok(None);
        };
        for use_ in ctx.admit_iter(&boundary[1..], "catia_face_configuration_next_states")? {
            let (next, next_storage) =
                ctx.with_scoped_storage("catia_face_configuration_state_storage", || {
                    let mut next = Vec::new();
                    for (start, current, configuration) in
                        ctx.admit_iter(states, "catia_face_configuration_next_states")?
                    {
                        for &pair @ [left, right] in ctx.admit_iter(
                            &edge_candidates[use_.edge],
                            "catia_face_configuration_next_states",
                        )? {
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
                                let (configuration, storage) = ctx.with_scoped_storage(
                                    "catia_face_configuration_state_pairs",
                                    || {
                                        let mut copy = ctx.copy_slice(
                                            &configuration[..],
                                            "catia_face_configuration_state_pairs",
                                        )?;
                                        let inserted =
                                            insert_pair(ctx, &mut copy, use_.edge, pair)?;
                                        Ok::<_, CodecError>(inserted.then_some(copy))
                                    },
                                )?;
                                if let Some(configuration) = configuration {
                                    ctx.push_vec(
                                        &mut next,
                                        (
                                            start,
                                            endpoint,
                                            ScopedValue {
                                                value: configuration,
                                                storage: Some(storage),
                                            },
                                        ),
                                        "catia_face_configuration_next_states",
                                    )?;
                                }
                            }
                        }
                    }
                    Ok::<_, CodecError>(Some(next))
                })?;
            let Some(next) = next else {
                return Ok(None);
            };
            states = next;
            state_storage = next_storage;
            if states.is_empty() {
                return Ok(Some(Vec::new()));
            }
        }
        // Configurations are ascending by edge; closed walks give the results.
        let mut completed = Vec::new();
        for (start, current, configuration) in
            ctx.admit_iter(states, "catia_face_configuration_boundary_results")?
        {
            if start == current {
                let ScopedValue {
                    value: configuration,
                    storage,
                } = configuration;
                if let Some(storage) = storage {
                    storage.commit()?;
                }
                ctx.push_vec(
                    &mut completed,
                    configuration,
                    "catia_face_configuration_boundary_results",
                )?;
            }
        }
        drop(state_storage);
        ctx.sort_unstable_by(
            &mut completed,
            |value| value,
            Ord::cmp,
            "catia_face_configuration_sort",
        )?;
        ctx.dedup_vec(&mut completed, "catia_face_configuration_seen_keys")?;
        Ok(Some(completed))
    }

    if selected.len() != edge_candidates.len() {
        return Ok(None);
    }
    let mut work = 0usize;
    let mut results = Vec::new();
    for assignment in ctx.admit_iter(assignments, "catia_face_configuration_result_rows")? {
        let (mut combined, mut combined_storage) =
            ctx.with_scoped_storage("catia_face_configuration_combined_storage", || {
                let mut combined: Vec<ScopedValue<'_, MeshFaceEndpointConfiguration>> = Vec::new();
                ctx.push_vec(
                    &mut combined,
                    ScopedValue::default(),
                    "catia_face_configuration_combined_rows",
                )?;
                Ok::<_, CodecError>(combined)
            })?;
        for boundary in ctx.admit_iter(
            &assignment.boundaries,
            "catia_face_configuration_combined_rows",
        )? {
            let (boundary, _boundary_storage) =
                ctx.with_scoped_storage("catia_face_configuration_boundary_storage", || {
                    boundary_configurations(
                        ctx,
                        boundary,
                        edge_candidates,
                        selected,
                        &mut work,
                        budget,
                    )
                })?;
            let Some(boundary) = boundary else {
                return Ok(None);
            };
            let (next, next_storage) =
                ctx.with_scoped_storage("catia_face_configuration_combined_storage", || {
                    let mut next = Vec::new();
                    for stored in
                        ctx.admit_iter(combined, "catia_face_configuration_next_combined")?
                    {
                        for candidate in
                            ctx.admit_iter(&boundary, "catia_face_configuration_next_combined")?
                        {
                            let Some(next_work) = work.checked_add(1) else {
                                return Ok(None);
                            };
                            work = next_work;
                            if work > MAX_FACE_ENDPOINT_CONFIGURATION_WORK || !budget.charge() {
                                return Ok(None);
                            }
                            let (merged, storage) = ctx.with_scoped_storage(
                                "catia_face_configuration_combined_pairs",
                                || {
                                    let mut merged = ctx.copy_slice(
                                        &stored[..],
                                        "catia_face_configuration_combined_pairs",
                                    )?;
                                    let compatible = ctx.all_by(
                                        candidate,
                                        |&(edge, pair)| insert_pair(ctx, &mut merged, edge, pair),
                                        "catia_face_configuration_combined_pairs",
                                    )?;
                                    Ok::<_, CodecError>(compatible.then_some(merged))
                                },
                            )?;
                            if let Some(merged) = merged {
                                ctx.push_vec(
                                    &mut next,
                                    ScopedValue {
                                        value: merged,
                                        storage: Some(storage),
                                    },
                                    "catia_face_configuration_next_combined",
                                )?;
                            }
                        }
                    }
                    Ok::<_, CodecError>(Some(next))
                })?;
            let Some(next) = next else {
                return Ok(None);
            };
            combined = next;
            combined_storage = next_storage;
        }
        for configuration in ctx.admit_iter(combined, "catia_face_configuration_result_rows")? {
            let ScopedValue { value, storage } = configuration;
            if let Some(storage) = storage {
                storage.commit()?;
            }
            ctx.push_vec(&mut results, value, "catia_face_configuration_result_rows")?;
        }
        drop(combined_storage);
    }
    ctx.sort_unstable_by(
        &mut results,
        |value| value,
        Ord::cmp,
        "catia_face_configuration_result_rows_sort",
    )?;
    ctx.dedup_vec(&mut results, "catia_face_configuration_result_keys")?;
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
    const OPERATION: &str = "catia_endpoint_cycle_pair_map";
    let Some(first) = boundary.first() else {
        return Ok(None);
    };
    let Some(&first_pair) = ctx.get_hash_map(pairs, &first.edge, OPERATION)? else {
        return Ok(None);
    };
    // Each initial direction has at most one successor at each edge.
    let mut states = [Some((first_pair[0], first_pair[1])), None];
    if first_pair[0] != first_pair[1] {
        states[1] = Some((first_pair[1], first_pair[0]));
    }
    let mut remaining = boundary[1..].iter();
    while let Some(use_) = ctx.next_charged(&mut remaining, "catia_endpoint_cycle_next_states")? {
        let Some(&pair) = ctx.get_hash_map(pairs, &use_.edge, OPERATION)? else {
            return Ok(None);
        };
        for state in &mut states {
            let Some((start, current)) = *state else {
                continue;
            };
            let endpoint = if pair[0] == current {
                Some(pair[1])
            } else if pair[1] == current {
                Some(pair[0])
            } else {
                None
            };
            *state = endpoint.map(|end| (start, end));
        }
        if states.iter().all(Option::is_none) {
            return Ok(Some(false));
        }
    }
    Ok(Some(
        states
            .into_iter()
            .flatten()
            .any(|(start, current)| start == current),
    ))
}

fn endpoint_configuration_cycles_viable(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    configuration: &MeshFaceEndpointConfiguration,
) -> Result<Option<bool>, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "catia_endpoint_configuration_cycles_viable_scratch")?;
    scratch.with_storage(|| {
        let mut pairs = HashMap::new();
        for &(edge, pair) in ctx.admit_iter(configuration, "catia_endpoint_cycle_pair_map")? {
            ctx.insert_hash_map(&mut pairs, edge, pair, "catia_endpoint_cycle_pair_map")?;
        }
        if pairs.len() != configuration.len() {
            return Ok(None);
        }
        if assignment.boundaries.is_empty() {
            return Ok(Some(false));
        }
        for boundary in ctx.admit_iter(&assignment.boundaries, "catia_endpoint_cycle_boundaries")? {
            if endpoint_configuration_boundary_cycle_viable(ctx, boundary, &pairs)? != Some(true) {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    })
}

/// The configuration of an assignment under one pair per edge: its edges'
/// normalized pairs, ascending by edge.
fn endpoint_configuration_for_assignment(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    edge_pairs: &[[usize; 2]],
) -> Result<Option<MeshFaceEndpointConfiguration>, CodecError> {
    let mut configuration = Vec::new();
    for boundary in ctx.admit_iter(&assignment.boundaries, "catia_endpoint_assignment_pair_map")? {
        for use_ in ctx.admit_iter(boundary, "catia_endpoint_assignment_pair_map")? {
            let Some(&[left, right]) = edge_pairs.get(use_.edge) else {
                return Ok(None);
            };
            ctx.push_vec(
                &mut configuration,
                (use_.edge, [left.min(right), left.max(right)]),
                "catia_endpoint_assignment_configuration",
            )?;
        }
    }
    ctx.sort_unstable_by(
        &mut configuration,
        |value| value,
        Ord::cmp,
        "catia_endpoint_assignment_configuration_sort",
    )?;
    ctx.dedup_vec(
        &mut configuration,
        "catia_endpoint_assignment_configuration_dedup",
    )?;
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
    let Some(&first_pair) =
        ctx.get_hash_map(pairs, &first.edge, "catia_endpoint_configuration_pairs")?
    else {
        return Ok(Err(MeshDirectionEnumerationError::Invalid));
    };
    // A fixed endpoint pair gives each direction at most one successor.
    let mut states = [None, None];
    for direction in [false, true] {
        if first_pair[0] == first_pair[1] && direction {
            continue;
        }
        let [start, current] = if direction {
            [first_pair[1], first_pair[0]]
        } else {
            first_pair
        };
        let (directions, storage) = ctx
            .with_scoped_storage("catia_endpoint_direction_storage", || {
                ctx.alloc_filled(1, direction, "catia_endpoint_initial_direction")
            })?;
        let directions = ScopedValue {
            value: directions,
            storage: Some(storage),
        };
        states[usize::from(direction)] = Some((start, current, directions));
    }
    let mut remaining = boundary[1..].iter();
    while let Some(use_) = ctx.next_charged(&mut remaining, "catia_endpoint_direction_states")? {
        let Some(&pair) =
            ctx.get_hash_map(pairs, &use_.edge, "catia_endpoint_configuration_pairs")?
        else {
            return Ok(Err(MeshDirectionEnumerationError::Invalid));
        };
        for state in &mut states {
            let Some((start, current, mut directions)) = state.take() else {
                continue;
            };
            let next = if pair[0] == current {
                Some((false, pair[1]))
            } else if pair[1] == current {
                Some((true, pair[0]))
            } else {
                None
            };
            if let Some((direction, end)) = next {
                directions
                    .storage
                    .as_mut()
                    .ok_or_else(|| CodecError::malformed("direction owns storage"))?
                    .with_storage(|| {
                        ctx.push_vec(
                            &mut directions.value,
                            direction,
                            "catia_endpoint_direction_step",
                        )
                    })?;
                *state = Some((start, end, directions));
            }
        }
        if states.iter().all(Option::is_none) {
            return Ok(Ok(Vec::new()));
        }
    }
    let mut solutions = [None, None];
    let mut count = 0;
    for (start, current, directions) in states.into_iter().flatten() {
        if start == current {
            solutions[count] = Some(directions);
            count += 1;
        }
    }
    if count == 2 {
        let order = ctx.compare(
            &solutions[0]
                .as_ref()
                .ok_or_else(|| CodecError::malformed("first direction"))?
                .value,
            &solutions[1]
                .as_ref()
                .ok_or_else(|| CodecError::malformed("second direction"))?
                .value,
            "catia_endpoint_boundary_solutions_sort",
        )?;
        if order.is_gt() {
            solutions.swap(0, 1);
        }
        if order.is_eq() {
            solutions[1] = None;
            count = 1;
        }
    }
    if count == 2
        && ctx.all_by(
            boundary,
            |use_| Ok(use_.reversed.is_none()),
            "catia_endpoint_boundary_solutions",
        )?
    {
        // An unresolved closed boundary has two traversal orientations. They
        // are the boundary-reversal gauge; retain one deterministic member
        // before combining independent boundaries.
        solutions[1] = None;
    }
    let mut completed = Vec::new();
    for mut directions in solutions.into_iter().flatten() {
        directions
            .storage
            .take()
            .ok_or_else(|| CodecError::malformed("direction owns storage"))?
            .commit()?;
        ctx.push_vec(
            &mut completed,
            directions.value,
            "catia_endpoint_boundary_solutions",
        )?;
    }
    Ok(Ok(completed))
}

fn endpoint_configuration_directions(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    configuration: &MeshFaceEndpointConfiguration,
) -> Result<Result<MeshFaceDirectionOptions, MeshDirectionEnumerationError>, CodecError> {
    let (pairs, _pair_storage) =
        ctx.with_scoped_storage("catia_endpoint_configuration_pairs", || {
            let mut pairs = HashMap::new();
            for &(edge, pair) in
                ctx.admit_iter(configuration, "catia_endpoint_configuration_pairs")?
            {
                ctx.insert_hash_map(&mut pairs, edge, pair, "catia_endpoint_configuration_pairs")?;
            }
            if pairs.len() != configuration.len() {
                return Ok(Err(MeshDirectionEnumerationError::Invalid));
            }
            Ok::<_, CodecError>(Ok(pairs))
        })?;
    let pairs = match pairs {
        Ok(pairs) => pairs,
        Err(error) => return Ok(Err(error)),
    };
    let (value, storage) = ctx.with_scoped_storage("catia_endpoint_alternative_storage", || {
        ctx.collect_indexed_vec(1, "catia_endpoint_initial_alternatives", |_| Ok(Vec::new()))
    })?;
    let mut alternatives = ScopedValue {
        value,
        storage: Some(storage),
    };
    let mut boundaries = assignment.boundaries.iter();
    while let Some(boundary) = ctx.next_charged(&mut boundaries, "catia_endpoint_alternatives")? {
        let (boundary_options, _boundary_storage) = ctx
            .with_scoped_storage("catia_endpoint_boundary_options", || {
                endpoint_configuration_boundary_directions(ctx, boundary, &pairs)
            })?;
        let boundary_options = match boundary_options {
            Ok(options) => options,
            Err(error) => return Ok(Err(error)),
        };
        let (next, storage) =
            ctx.with_scoped_storage("catia_endpoint_alternative_storage", || {
                let mut next = Vec::new();
                for prefix in ctx.admit_iter(&*alternatives, "catia_endpoint_alternatives")? {
                    for boundary_directions in
                        ctx.admit_iter(&boundary_options, "catia_endpoint_alternatives")?
                    {
                        let mut alternative = ctx.copy_retained_rows(
                            prefix,
                            "catia_endpoint_alternative_prefix_rows",
                            "catia_endpoint_alternative_prefix_directions",
                        )?;
                        ctx.push_vec(
                            &mut alternative,
                            ctx.copy_slice(
                                boundary_directions,
                                "catia_endpoint_boundary_direction_copy",
                            )?,
                            "catia_endpoint_alternative_boundary",
                        )?;
                        ctx.push_vec(&mut next, alternative, "catia_endpoint_alternatives")?;
                        if next.len() > MAX_FACE_ENDPOINT_CONFIGURATION_WORK {
                            return Ok(Err(MeshDirectionEnumerationError::Overflow));
                        }
                    }
                }
                Ok::<_, CodecError>(Ok(next))
            })?;
        let next = match next {
            Ok(next) => next,
            Err(error) => return Ok(Err(error)),
        };
        alternatives = ScopedValue {
            value: next,
            storage: Some(storage),
        };
        if alternatives.is_empty() {
            break;
        }
    }
    alternatives
        .storage
        .take()
        .ok_or_else(|| CodecError::malformed("alternatives own storage"))?
        .commit()?;
    Ok(Ok(alternatives.value))
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

impl cadmpeg_core::decode::cost::DecodeCost for MeshEndpointRelationSelection {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        match self {
            Self::Enumerated {
                assignments,
                edge_pairs,
            } => (1_u8, assignments, edge_pairs).decode_cost(ctx, operation),
            Self::Deferred => Ok(1),
        }
    }
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
                let mut assignments =
                    ctx.copy_slice(assignments, "catia_relation_normalized_assignments")?;
                ctx.sort_unstable_by(
                    &mut assignments,
                    |value| value,
                    Ord::cmp,
                    "catia_relation_normalized_assignments_sort",
                )?;
                ctx.dedup_vec(
                    &mut assignments,
                    "catia_relation_normalized_assignments_dedup",
                )?;
                let mut edge_pairs =
                    ctx.copy_slice(edge_pairs, "catia_relation_normalized_pairs")?;
                for (_, pair) in
                    ctx.admit_iter(&mut edge_pairs, "catia_relation_normalized_pairs")?
                {
                    *pair = [pair[0].min(pair[1]), pair[0].max(pair[1])];
                }
                ctx.sort_unstable_by(
                    &mut edge_pairs,
                    |value| value,
                    Ord::cmp,
                    "catia_relation_normalized_pairs_sort",
                )?;
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
type MeshEndpointSolutionPredicate<'a> =
    dyn Fn(&[Option<[usize; 2]>]) -> Result<bool, CodecError> + 'a;
type MeshFixedDirectionOption<'storage> =
    (Vec<Vec<bool>>, MeshQuotient<'storage>, Vec<Option<bool>>);

pub(super) fn raw_endpoint_relation_state_signature(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
) -> Result<MeshEndpointRelationStateSignature, CodecError> {
    let normalized_assigned = ctx.collect_vec(
        assigned
            .iter()
            .map(|pair| pair.map(|[left, right]| [left.min(right), left.max(right)])),
        "catia_relation_signature_assigned",
    )?;
    let mut normalized_domains = Vec::new();
    for choices in ctx.admit_iter(domains, "catia_relation_signature_domains")? {
        let mut row = Vec::new();
        for choice in ctx.admit_iter(choices, "catia_relation_signature_choices")? {
            let normalized = choice.selection.normalized(ctx)?;
            ctx.push_vec(&mut row, normalized, "catia_relation_signature_choices")?;
        }
        ctx.sort_unstable_by(
            &mut row,
            |value| value,
            Ord::cmp,
            "catia_relation_signature_choices_sort",
        )?;
        ctx.push_vec(
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
    let has_unconstrained_choice = ctx.any_by(
        domains,
        |choices| {
            ctx.any_by(
                choices,
                |choice| Ok(choice.selection.is_unconstrained()),
                "catia_relation_coordinate_unconstrained",
            )
        },
        "catia_relation_coordinate_unconstrained",
    )?;
    let mut candidates = Vec::new();
    let mut possible_storage = ctx.reserve_scoped(0, "catia_relation_coordinate_possible_rows")?;
    let mut possible = Vec::new();
    for base in ctx.admit_iter(base_candidates, "catia_relation_coordinate_candidate_rows")? {
        let copy = ctx.copy_slice(base, "catia_relation_coordinate_candidate_pairs")?;
        ctx.push_vec(
            &mut candidates,
            copy,
            "catia_relation_coordinate_candidate_rows",
        )?;
        possible_storage.with_storage(|| {
            ctx.push_vec(
                &mut possible,
                Vec::<[usize; 2]>::new(),
                "catia_relation_coordinate_possible_rows",
            )?;
            Ok::<_, CodecError>(())
        })?;
    }
    if !has_unconstrained_choice
        && !possible_storage.with_storage(|| {
            for choices in ctx.admit_iter(domains, "catia_relation_coordinate_possible_pairs")? {
                for choice in ctx.admit_iter(choices, "catia_relation_coordinate_possible_pairs")? {
                    for &(edge, [left, right]) in ctx.admit_iter(
                        choice.selection.edge_pairs(),
                        "catia_relation_coordinate_possible_pairs",
                    )? {
                        let Some(row) = possible.get_mut(edge) else {
                            return Ok(false);
                        };
                        ctx.push_vec(
                            row,
                            [left.min(right), left.max(right)],
                            "catia_relation_coordinate_possible_pairs",
                        )?;
                    }
                }
            }
            for row in ctx.admit_iter(&mut possible, "catia_relation_coordinate_possible_pairs")? {
                ctx.sort_unstable_by(
                    row,
                    |pair| pair,
                    Ord::cmp,
                    "catia_relation_coordinate_possible_pairs_sort",
                )?;
            }
            Ok::<_, CodecError>(true)
        })?
    {
        return Ok(None);
    }
    for (edge, assigned) in ctx
        .admit_iter(assigned, "catia_relation_coordinate_retained_pairs")?
        .enumerate()
    {
        if let Some(pair) = assigned {
            ctx.retain_vec(
                &mut candidates[edge],
                |candidate| Ok(same_unordered_pair(*candidate, *pair)),
                "catia_relation_coordinate_retained_pairs",
            )?;
        } else if !has_unconstrained_choice && !possible[edge].is_empty() {
            let possible = &possible[edge];
            ctx.retain_vec(
                &mut candidates[edge],
                |&[left, right]| {
                    Ok(ctx
                        .binary_search(
                            possible,
                            &[left.min(right), left.max(right)],
                            "catia_relation_coordinate_retained_pairs",
                        )?
                        .is_ok())
                },
                "catia_relation_coordinate_retained_pairs",
            )?;
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
    ctx.collect_indexed_vec(directions.len(), "catia_direction_copy_rows", |row| {
        ctx.copy_slice(&directions[row], "catia_direction_copy_values")
    })
}

/// Each boundary row or its complement, whichever is lexicographically
/// smaller: the complement exactly when the row starts with `true`.
fn canonical_mesh_boundary_directions(
    ctx: &DecodeContext<'_>,
    directions: &[Vec<bool>],
) -> Result<Vec<Vec<bool>>, CodecError> {
    ctx.collect_indexed_vec(
        directions.len(),
        "catia_canonical_mesh_direction_rows",
        |index| {
            let row = &directions[index];
            let mut canonical = ctx.copy_slice(row, "catia_canonical_mesh_direction_values")?;
            if row.first() == Some(&true) {
                for direction in
                    ctx.admit_iter(&mut canonical, "catia_canonical_mesh_direction_values")?
                {
                    *direction = !*direction;
                }
            }
            Ok(canonical)
        },
    )
}

type EndpointRelationKey = Vec<Option<[usize; 2]>>;
type EndpointRelationKeys<'a> = Vec<(&'a MeshEndpointRelationChoice, EndpointRelationKey)>;

fn canonical_endpoint_relation_key(
    ctx: &DecodeContext<'_>,
    choice: &MeshEndpointRelationChoice,
    edges: &[usize],
) -> Result<EndpointRelationKey, CodecError> {
    let mut key = Vec::new();
    for &edge in ctx.admit_iter(edges, "catia_endpoint_relation_key_values")? {
        let pair = ctx
            .find_map(
                choice.selection.edge_pairs(),
                |&(candidate, pair)| Ok((candidate == edge).then_some(pair)),
                "catia_endpoint_relation_key_values",
            )?
            .map(|[left, right]| [left.min(right), left.max(right)]);
        ctx.push_vec(&mut key, pair, "catia_endpoint_relation_key_values")?;
    }
    Ok(key)
}

fn complete_endpoint_relation_keys<'a>(
    ctx: &DecodeContext<'_>,
    choices: &'a [MeshEndpointRelationChoice],
    edges: &[usize],
) -> Result<(bool, EndpointRelationKeys<'a>), CodecError> {
    let mut keys = Vec::new();
    for choice in ctx.admit_iter(choices, "catia_endpoint_relation_keys")? {
        let key = canonical_endpoint_relation_key(ctx, choice, edges)?;
        ctx.push_vec(&mut keys, (choice, key), "catia_endpoint_relation_keys")?;
    }
    let complete = ctx.all_by(
        &keys,
        |(_, key)| {
            ctx.all_by(
                key,
                |pair| Ok(pair.is_some()),
                "catia_endpoint_relation_keys",
            )
        },
        "catia_endpoint_relation_keys",
    )?;
    Ok((complete, keys))
}

fn build_endpoint_relation_constraints(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    budget: &WorkBudget<'_>,
) -> Result<Option<MeshEndpointRelationConstraints>, CodecError> {
    // (edge, face) for every edge a face's choices name, ascending.
    let mut edge_faces = Vec::new();
    for (face, choices) in ctx
        .admit_iter(domains, "catia_endpoint_relation_edge_faces")?
        .enumerate()
    {
        for choice in ctx.admit_iter(choices, "catia_endpoint_relation_edge_faces")? {
            for &(edge, _) in ctx.admit_iter(
                choice.selection.edge_pairs(),
                "catia_endpoint_relation_edge_faces",
            )? {
                ctx.push_vec(
                    &mut edge_faces,
                    (edge, face),
                    "catia_endpoint_relation_edge_faces",
                )?;
            }
        }
    }
    ctx.sort_unstable_by(
        &mut edge_faces,
        |entry| entry,
        Ord::cmp,
        "catia_endpoint_relation_sorted_faces_sort",
    )?;
    ctx.dedup_vec(&mut edge_faces, "catia_endpoint_relation_sorted_faces")?;
    // ((face, neighbor), edge) for every edge two faces share, both ways.
    let mut shared_edges = Vec::new();
    let mut start = 0;
    while start < edge_faces.len() {
        ctx.charge_work(1, "catia_endpoint_relation_shared_edges")?;
        let edge = edge_faces[start].0;
        let length = ctx.partition_point(
            &edge_faces[start..],
            |entry| Ok(entry.0 == edge),
            "catia_endpoint_relation_shared_edges",
        )?;
        let faces = &edge_faces[start..start + length];
        start += length;
        for (left_index, &(_, left)) in ctx
            .admit_iter(faces, "catia_endpoint_relation_shared_edges")?
            .enumerate()
        {
            for &(_, right) in ctx.admit_iter(
                &faces[left_index + 1..],
                "catia_endpoint_relation_shared_edges",
            )? {
                for pair in [(left, right), (right, left)] {
                    ctx.push_vec(
                        &mut shared_edges,
                        (pair, edge),
                        "catia_endpoint_relation_shared_edges",
                    )?;
                }
            }
        }
    }
    ctx.sort_unstable_by(
        &mut shared_edges,
        |entry| entry,
        Ord::cmp,
        "catia_endpoint_relation_shared_rows_sort",
    )?;
    let mut shared_rows = Vec::new();
    let mut start = 0;
    while start < shared_edges.len() {
        ctx.charge_work(1, "catia_endpoint_relation_shared_rows")?;
        let pair = shared_edges[start].0;
        let length = ctx.partition_point(
            &shared_edges[start..],
            |entry| Ok(entry.0 == pair),
            "catia_endpoint_relation_shared_rows",
        )?;
        let edges = ctx.collect_vec(
            shared_edges[start..start + length]
                .iter()
                .map(|entry| entry.1),
            "catia_endpoint_relation_shared_rows",
        )?;
        start += length;
        ctx.push_vec(
            &mut shared_rows,
            (pair, edges),
            "catia_endpoint_relation_shared_rows",
        )?;
    }
    let mut arcs =
        ctx.collect_indexed_vec(domains.len(), "catia_endpoint_relation_arcs", |_| {
            Ok(Vec::new())
        })?;
    let mut incoming =
        ctx.collect_indexed_vec(domains.len(), "catia_endpoint_relation_incoming", |_| {
            Ok(Vec::new())
        })?;
    let choice_counts = ctx.collect_vec(
        domains.iter().map(Vec::len),
        "catia_endpoint_relation_choice_counts",
    )?;
    for ((face, neighbor), edges) in ctx.admit_iter(shared_rows, "catia_endpoint_relation_arcs")? {
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
            for (choice, key) in
                ctx.admit_iter(right_choices, "catia_endpoint_relation_index_keys")?
            {
                let others = ctx
                    .entry_hash_map(&mut index, key, "catia_endpoint_relation_index_keys")?
                    .or_default();
                ctx.push_vec(others, choice.id, "catia_endpoint_relation_index_values")?;
            }
            let mut supports = Vec::new();
            for (_, key) in ctx.admit_iter(&left_choices, "catia_endpoint_relation_support_rows")? {
                let mut mask = ctx.alloc_filled(
                    bitset_words(domains[neighbor].len()),
                    0u64,
                    "catia_endpoint_relation_support_mask",
                )?;
                if let Some(others) =
                    ctx.get_hash_map(&index, key, "catia_endpoint_relation_index_keys")?
                {
                    for &other in ctx.admit_iter(others, "catia_endpoint_relation_support_mask")? {
                        mask[other / 64] |= 1u64 << (other % 64);
                    }
                }
                ctx.push_vec(&mut supports, mask, "catia_endpoint_relation_support_rows")?;
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
            for (_, left_key) in
                ctx.admit_iter(&left_choices, "catia_endpoint_relation_support_rows")?
            {
                let mut mask = ctx.alloc_filled(
                    bitset_words(domains[neighbor].len()),
                    0u64,
                    "catia_endpoint_relation_support_mask",
                )?;
                for (other, right_key) in
                    ctx.admit_iter(&right_choices, "catia_endpoint_relation_key_comparison")?
                {
                    let compatible = ctx.all_by(
                        left_key.iter().zip(right_key),
                        |(left, right)| {
                            Ok(left
                                .as_ref()
                                .zip(right.as_ref())
                                .is_none_or(|(left, right)| same_unordered_pair(*left, *right)))
                        },
                        "catia_endpoint_relation_key_comparison",
                    )?;
                    if compatible {
                        mask[other.id / 64] |= 1u64 << (other.id % 64);
                    }
                }
                ctx.push_vec(&mut supports, mask, "catia_endpoint_relation_support_rows")?;
            }
            supports
        };
        let arc_index = arcs[face].len();
        ctx.push_vec(
            &mut arcs[face],
            MeshEndpointRelationArc { neighbor, supports },
            "catia_endpoint_relation_arc_entries",
        )?;
        ctx.push_vec(
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
    let mut scratch = ctx.reserve_scoped(0, "catia_propagate_endpoint_relation_domains_scratch")?;
    scratch.with_storage(|| {
        const OPERATION: &str = "catia_endpoint_relation_propagation";
        let mut dirty_faces = Vec::new();
        for (face, choices) in ctx
            .admit_iter(&*domains, "catia_endpoint_relation_dirty_faces")?
            .enumerate()
        {
            if choices.len() != constraints.choice_counts[face] {
                ctx.push_vec(
                    &mut dirty_faces,
                    face,
                    "catia_endpoint_relation_dirty_faces",
                )?;
            }
        }
        let mut first_pass = true;
        loop {
            ctx.charge_work(1, "catia_mesh_quotient_iteration")?;
            let mut changed = false;
            let mut pass_storage = ctx.reserve_scoped(0, "catia_endpoint_relation_pass_storage")?;
            for (face, choices) in ctx.admit_iter(&mut *domains, OPERATION)?.enumerate() {
                if !budget.charge_by(work_units(choices.len())) {
                    return Ok(false);
                }
                let before = choices.len();
                ctx.retain_vec(
                    choices,
                    |choice| {
                        ctx.all_by(
                            choice.selection.edge_pairs(),
                            |&(edge, pair)| {
                                Ok(assigned[edge]
                                    .is_none_or(|selected| same_unordered_pair(selected, pair)))
                            },
                            OPERATION,
                        )
                    },
                    OPERATION,
                )?;
                if choices.is_empty() {
                    return Ok(false);
                }
                if choices.len() != before {
                    ctx.push_vec(
                        &mut dirty_faces,
                        face,
                        "catia_endpoint_relation_dirty_faces",
                    )?;
                    changed = true;
                }
            }

            let mut active = Vec::new();
            for &choice_count in ctx.admit_iter(&constraints.choice_counts, OPERATION)? {
                let mask = pass_storage.with_storage(|| {
                    ctx.alloc_filled(
                        bitset_words(choice_count),
                        0u64,
                        "catia_endpoint_relation_active_mask",
                    )
                })?;
                pass_storage.with_storage(|| {
                    ctx.push_vec(&mut active, mask, "catia_endpoint_relation_active_rows")
                })?;
            }
            for (face, choices) in ctx.admit_iter(&*domains, OPERATION)?.enumerate() {
                for choice in ctx.admit_iter(choices, OPERATION)? {
                    let Some(active_word) = active[face].get_mut(choice.id / 64) else {
                        return Ok(false);
                    };
                    *active_word |= 1u64 << (choice.id % 64);
                }
            }
            let mut queue = VecDeque::new();
            if first_pass && dirty_faces.is_empty() {
                for (face, arcs) in ctx
                    .admit_iter(&constraints.arcs, "catia_endpoint_relation_queue")?
                    .enumerate()
                {
                    for arc in ctx.admit_iter(0..arcs.len(), "catia_endpoint_relation_queue")? {
                        pass_storage.with_storage(|| {
                            ctx.push_back(&mut queue, (face, arc), "catia_endpoint_relation_queue")
                        })?;
                    }
                }
            } else {
                let mut dirty = dirty_faces.drain(..);
                while let Some(face) =
                    ctx.next_charged(&mut dirty, "catia_endpoint_relation_queue")?
                {
                    for &incoming in ctx
                        .admit_iter(&constraints.incoming[face], "catia_endpoint_relation_queue")?
                    {
                        pass_storage.with_storage(|| {
                            ctx.push_back(&mut queue, incoming, "catia_endpoint_relation_queue")
                        })?;
                    }
                }
            }
            first_pass = false;
            while let Some((face, arc_index)) = ctx.next_charged(
                &mut std::iter::from_fn(|| queue.pop_front()),
                "catia_mesh_quotient_iteration",
            )? {
                let arc = &constraints.arcs[face][arc_index];
                let neighbor_active = &active[arc.neighbor];
                let before = domains[face].len();
                ctx.retain_vec(
                    &mut domains[face],
                    |choice| {
                        let Some(supports) = arc.supports.get(choice.id) else {
                            return Ok(false);
                        };
                        if !budget.charge_by(work_units(supports.len())) {
                            return Ok(false);
                        }
                        ctx.any_by(
                            supports.iter().zip(neighbor_active),
                            |(supported, active)| Ok(supported & active != 0),
                            OPERATION,
                        )
                    },
                    OPERATION,
                )?;
                if budget.exhausted() || domains[face].is_empty() {
                    return Ok(false);
                }
                if domains[face].len() == before {
                    continue;
                }
                ctx.fill(&mut active[face], 0, OPERATION)?;
                for choice in ctx.admit_iter(&domains[face], OPERATION)? {
                    active[face][choice.id / 64] |= 1u64 << (choice.id % 64);
                }
                changed = true;
                for &incoming in
                    ctx.admit_iter(&constraints.incoming[face], "catia_endpoint_relation_queue")?
                {
                    if incoming.0 != arc.neighbor {
                        pass_storage.with_storage(|| {
                            ctx.push_back(&mut queue, incoming, "catia_endpoint_relation_queue")
                        })?;
                    }
                }
            }
            // A pair present with one value in every surviving choice of one face
            // is a forced edge relation, even when the face still has assignment
            // or boundary-direction alternatives. Record it before branching on
            // those independent alternatives.
            for choices in ctx.admit_iter(&*domains, OPERATION)? {
                if !budget.charge_by(work_units(choices.len())) {
                    return Ok(false);
                }
                let choice_count = choices.len();
                // (edge, pair) for every choice, ascending by edge.
                let mut storage = ctx.reserve_scoped(0, "catia_endpoint_relation_pair_values")?;
                let mut pairs = Vec::new();
                storage.with_storage(|| {
                    for choice in ctx.admit_iter(choices, "catia_endpoint_relation_pair_values")? {
                        for &entry in ctx.admit_iter(
                            choice.selection.edge_pairs(),
                            "catia_endpoint_relation_pair_values",
                        )? {
                            ctx.push_vec(&mut pairs, entry, "catia_endpoint_relation_pair_values")?;
                        }
                    }
                    ctx.sort_unstable_by(
                        &mut pairs,
                        |entry| entry,
                        Ord::cmp,
                        "catia_endpoint_relation_pair_keys",
                    )
                })?;
                let mut start = 0;
                while start < pairs.len() {
                    ctx.charge_work(1, "catia_endpoint_relation_pair_keys")?;
                    let (edge, pair) = pairs[start];
                    let length = ctx.partition_point(
                        &pairs[start..],
                        |entry| Ok(entry.0 == edge),
                        "catia_endpoint_relation_count_keys",
                    )?;
                    let group = &pairs[start..start + length];
                    start += length;
                    // The group is sorted, so one value means its last entry
                    // equals its first.
                    if length != choice_count || group[length - 1].1 != pair {
                        continue;
                    }
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
            for choices in ctx.admit_iter(&*domains, OPERATION)? {
                if choices.len() != 1 {
                    continue;
                }
                for &(edge, pair) in ctx.admit_iter(choices[0].selection.edge_pairs(), OPERATION)? {
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
    })
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
            assignments: ctx.copy_slice(assignments, "catia_endpoint_relation_copy_assignments")?,
            edge_pairs: ctx.copy_slice(edge_pairs, "catia_endpoint_relation_copy_edge_pairs")?,
        }),
    }
}

fn copy_endpoint_relation_branch(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    selected_face: usize,
    selected_choice: usize,
) -> Result<Vec<Vec<MeshEndpointRelationChoice>>, CodecError> {
    let mut copy = Vec::new();
    for (face, choices) in ctx
        .admit_iter(domains, "catia_endpoint_relation_copy_faces")?
        .enumerate()
    {
        let choices = if face == selected_face {
            &choices[selected_choice..selected_choice + 1]
        } else {
            choices.as_slice()
        };
        let mut row = Vec::new();
        for choice in ctx.admit_iter(choices, "catia_endpoint_relation_copy_choices")? {
            let selection = copy_endpoint_relation_selection(ctx, &choice.selection)?;
            ctx.push_vec(
                &mut row,
                MeshEndpointRelationChoice {
                    id: choice.id,
                    selection,
                },
                "catia_endpoint_relation_copy_choices",
            )?;
        }
        ctx.push_vec(&mut copy, row, "catia_endpoint_relation_copy_faces")?;
    }
    Ok(copy)
}

/// The distinct points a relation choice's edge pairs name, ascending.
fn relation_choice_points(
    ctx: &DecodeContext<'_>,
    choice: &MeshEndpointRelationChoice,
    operation: &'static str,
) -> Result<Vec<usize>, CodecError> {
    let mut points = ctx.collect_vec(
        choice
            .selection
            .edge_pairs()
            .iter()
            .flat_map(|(_, pair)| pair)
            .copied(),
        operation,
    )?;
    ctx.sort_unstable_by(&mut points, |point| point, Ord::cmp, operation)?;
    ctx.dedup_vec(&mut points, operation)?;
    Ok(points)
}

// The recursive walk keeps branch-owned domains and shared memo state explicit;
// a context object would hide which values are cloned for each branch.
struct WalkEndpointRelationDomainsInputs<
    'storage,
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
    state_memo:
        &'input4 mut HashMap<MeshEndpointRelationStateSignature, ScopedReservation<'storage>>,
    state_memo_storage: &'input4 RefCell<ScopedReservation<'storage>>,
    candidate_gauge: Option<MeshCandidateGauge<'input5>>,
    priority_edges: Option<&'input6 [bool]>,
    partial_solution_valid: Option<&'input8 MeshEndpointSolutionPredicate<'input7>>,
    coordinate_domains: Option<&'input9 MeshCoordinateRootDomains<'input9>>,
    coordinate_budget: Option<&'input11 WorkBudget<'input10>>,
    evaluate: &'input12 mut F,
}

fn walk_endpoint_relation_domains<'storage, F>(
    ctx: &'storage DecodeContext<'_>,
    inputs: WalkEndpointRelationDomainsInputs<
        'storage,
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
        state_memo_storage,
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
    let propagated = if ctx.all_by(
        &domains,
        |choices| Ok(choices.len() == 1),
        "catia_endpoint_relation_walk",
    )? {
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
        if !valid(&assigned)? {
            return Ok(false);
        }
    }
    // The final coordinate binding is surjective: every coordinate row must
    // occur in the selected endpoint pairs. When every surviving relation
    // choice has an explicit edge configuration, their point union is an
    // upper bound for every completion of this branch.
    if ctx.all_by(
        &domains,
        |choices| {
            ctx.all_by(
                choices,
                |choice| Ok(!choice.selection.is_unconstrained()),
                "catia_endpoint_relation_walk",
            )
        },
        "catia_endpoint_relation_walk",
    )? {
        let mut point_storage = ctx.reserve_scoped(0, "catia_endpoint_relation_possible_points")?;
        let possible_count = point_storage.with_storage(|| {
            let mut possible_points = HashSet::new();
            for pair in ctx.admit_iter(&assigned, "catia_endpoint_relation_possible_points")? {
                for &point in pair.iter().flatten() {
                    ctx.insert_hash_set(
                        &mut possible_points,
                        point,
                        "catia_endpoint_relation_possible_points",
                    )?;
                }
            }
            for choices in ctx.admit_iter(&domains, "catia_endpoint_relation_possible_points")? {
                for choice in ctx.admit_iter(choices, "catia_endpoint_relation_possible_points")? {
                    for (_, pair) in ctx.admit_iter(
                        choice.selection.edge_pairs(),
                        "catia_endpoint_relation_possible_points",
                    )? {
                        for &point in pair {
                            ctx.insert_hash_set(
                                &mut possible_points,
                                point,
                                "catia_endpoint_relation_possible_points",
                            )?;
                        }
                    }
                }
            }
            Ok::<_, CodecError>(possible_points.len())
        })?;
        drop(point_storage);
        if possible_count < point_count {
            return Ok(false);
        }
    }
    if let (Some(coordinate_domains), Some(coordinate_budget)) =
        (coordinate_domains, coordinate_budget)
    {
        if !coordinate_budget.exhausted() {
            let (candidates, _candidate_storage) =
                ctx.with_scoped_storage("catia_relation_coordinate_storage", || {
                    relation_coordinate_candidate_domains(
                        ctx,
                        &domains,
                        &assigned,
                        coordinate_domains.edge_candidates(),
                    )
                })?;
            let Some(candidates) = candidates else {
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
        let alternative_count = ctx.fold(
            &domains,
            Some(0usize),
            |total, choices| Ok(total.and_then(|total| total.checked_add(choices.len()))),
            "catia_endpoint_relation_walk",
        )?;
        let (signature, signature_storage) =
            ctx.with_scoped_storage("catia_endpoint_relation_state_memo", || {
                if candidate_gauge.is_some()
                    && alternative_count.is_some_and(|total| total <= MAX_GAUGE_STATE_ALTERNATIVES)
                {
                    endpoint_relation_state_signature(ctx, &domains, &assigned, candidate_gauge)
                } else {
                    Ok(Some(raw_endpoint_relation_state_signature(
                        ctx, &domains, &assigned,
                    )?))
                }
            })?;
        if let Some(signature) = signature {
            let entry = state_memo_storage.borrow_mut().with_storage(|| {
                ctx.entry_hash_map(state_memo, signature, "catia_endpoint_relation_state_memo")
            })?;
            match entry {
                std::collections::hash_map::Entry::Occupied(_) => return Ok(false),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(signature_storage);
                }
            }
        }
    }

    let (priority_counts, _priority_count_storage) =
        ctx.with_scoped_storage("catia_endpoint_relation_priority_counts", || {
            let mut priority_counts = Vec::new();
            if let Some(edges) = priority_edges {
                if ctx.any_by(
                    &domains,
                    |choices| Ok(choices.len() > 1),
                    "catia_endpoint_relation_priority_edges",
                )? {
                    for choices in
                        ctx.admit_iter(&domains, "catia_endpoint_relation_priority_counts")?
                    {
                        let mut priority_storage =
                            ctx.reserve_scoped(0, "catia_endpoint_relation_priority_edges")?;
                        let priority_count = priority_storage.with_storage(|| {
                            let mut priority = HashSet::new();
                            for choice in
                                ctx.admit_iter(choices, "catia_endpoint_relation_priority_edges")?
                            {
                                for &(edge, _) in ctx.admit_iter(
                                    choice.selection.edge_pairs(),
                                    "catia_endpoint_relation_priority_edges",
                                )? {
                                    if edges.get(edge).copied().unwrap_or(false) {
                                        ctx.insert_hash_set(
                                            &mut priority,
                                            edge,
                                            "catia_endpoint_relation_priority_edges",
                                        )?;
                                    }
                                }
                            }
                            Ok::<_, CodecError>(priority.len())
                        })?;
                        ctx.push_vec(
                            &mut priority_counts,
                            priority_count,
                            "catia_endpoint_relation_priority_counts",
                        )?;
                    }
                }
            }
            Ok::<_, CodecError>(priority_counts)
        })?;
    // Visit a face touching a monotone preference-dependent edge before an
    // unrelated face. Within each tier use minimum remaining values first;
    // relation degree is only a deterministic tie breaker.
    let mut position = 0usize;
    let least = ctx.fold(
        &domains,
        None,
        |least: Option<(_, usize)>, choices| {
            let face = position;
            position += 1;
            if choices.len() <= 1 {
                return Ok(least);
            }
            let priority_count = priority_counts.get(face).copied().unwrap_or(0);
            let key = (
                priority_count == 0,
                std::cmp::Reverse(priority_count),
                choices.len(),
                std::cmp::Reverse(constraints.arcs.get(face).map_or(0, Vec::len)),
                face,
            );
            Ok(match least {
                Some(least) if least.0 <= key => Some(least),
                _ => Some((key, face)),
            })
        },
        "catia_endpoint_relation_branch_face",
    )?;
    let Some((_, face)) = least else {
        let (completed, _completed_storage) =
            ctx.with_scoped_storage("catia_endpoint_relation_completed_storage", || {
                let mut edge_pairs = assigned;
                for choices in
                    ctx.admit_iter(&domains, "catia_endpoint_relation_completed_pairs")?
                {
                    let Some(choice) = choices.first() else {
                        continue;
                    };
                    for &(edge, pair) in ctx.admit_iter(
                        choice.selection.edge_pairs(),
                        "catia_endpoint_relation_completed_pairs",
                    )? {
                        match edge_pairs[edge] {
                            Some(selected) if !same_unordered_pair(selected, pair) => {
                                return Ok(None)
                            }
                            Some(_) => {}
                            None => edge_pairs[edge] = Some(pair),
                        }
                    }
                }
                let mut completed_pairs = Vec::new();
                for pair in ctx.admit_iter(edge_pairs, "catia_endpoint_relation_completed_pairs")? {
                    let Some(pair) = pair else {
                        return Ok(None);
                    };
                    ctx.push_vec(
                        &mut completed_pairs,
                        pair,
                        "catia_endpoint_relation_completed_pairs",
                    )?;
                }
                let mut selections = Vec::new();
                for (face, choices) in ctx
                    .admit_iter(&domains, "catia_endpoint_relation_selection_faces")?
                    .enumerate()
                {
                    let Some(choice) = choices.first() else {
                        return Ok(None);
                    };
                    let viable =
                        if let MeshEndpointRelationSelection::Enumerated { assignments, .. } =
                            &choice.selection
                        {
                            ctx.copy_slice(
                                assignments,
                                "catia_endpoint_relation_selected_assignments",
                            )?
                        } else {
                            let mut viable = Vec::new();
                            for (assignment, assignment_value) in ctx
                                .admit_iter(
                                    &face_assignments[face],
                                    "catia_endpoint_relation_deferred_assignments",
                                )?
                                .enumerate()
                            {
                                let (configuration, _configuration_storage) = ctx
                                    .with_scoped_storage(
                                        "catia_endpoint_relation_deferred_configuration_storage",
                                        || {
                                            endpoint_configuration_for_assignment(
                                                ctx,
                                                assignment_value,
                                                &completed_pairs,
                                            )
                                        },
                                    )?;
                                let Some(configuration) = configuration else {
                                    continue;
                                };
                                if endpoint_configuration_cycles_viable(
                                    ctx,
                                    assignment_value,
                                    &configuration,
                                )? != Some(true)
                                {
                                    continue;
                                }
                                ctx.push_vec(
                                    &mut viable,
                                    assignment,
                                    "catia_endpoint_relation_deferred_assignments",
                                )?;
                            }
                            if viable.is_empty() {
                                return Ok(None);
                            }
                            viable
                        };
                    ctx.push_vec(
                        &mut selections,
                        viable,
                        "catia_endpoint_relation_selection_faces",
                    )?;
                }
                Ok::<_, CodecError>(Some((selections, completed_pairs)))
            })?;
        let Some((selections, completed_pairs)) = completed else {
            return Ok(false);
        };
        return evaluate(selections, completed_pairs);
    };
    let choices = &domains[face];
    let (branch_order, _branch_order_storage) = {
        let (assigned_points, _assigned_point_storage) =
            ctx.with_scoped_storage("catia_endpoint_relation_assigned_points", || {
                let mut assigned_points = HashSet::new();
                for pair in ctx.admit_iter(&assigned, "catia_endpoint_relation_assigned_points")? {
                    for &point in pair.iter().flatten() {
                        ctx.insert_hash_set(
                            &mut assigned_points,
                            point,
                            "catia_endpoint_relation_assigned_points",
                        )?;
                    }
                }
                Ok::<_, CodecError>(assigned_points)
            })?;
        // How many choices, over all faces, name each point.
        let (point_support, _point_support_storage) =
            ctx.with_scoped_storage("catia_endpoint_relation_support_point_keys", || {
                let mut point_support = HashMap::<usize, usize>::new();
                for face_choices in
                    ctx.admit_iter(&domains, "catia_endpoint_relation_support_point_keys")?
                {
                    for choice in
                        ctx.admit_iter(face_choices, "catia_endpoint_relation_support_point_keys")?
                    {
                        let (points, _point_storage) = ctx.with_scoped_storage(
                            "catia_endpoint_relation_support_choice_points",
                            || {
                                relation_choice_points(
                                    ctx,
                                    choice,
                                    "catia_endpoint_relation_support_choice_points",
                                )
                            },
                        )?;
                        for point in
                            ctx.admit_iter(points, "catia_endpoint_relation_support_point_keys")?
                        {
                            *ctx.entry_hash_map(
                                &mut point_support,
                                point,
                                "catia_endpoint_relation_support_point_keys",
                            )?
                            .or_default() += 1;
                        }
                    }
                }
                Ok::<_, CodecError>(point_support)
            })?;
        ctx.with_scoped_storage("catia_endpoint_relation_branch_order", || {
            let mut branch_order = Vec::new();
            for (index, choice) in ctx
                .admit_iter(choices, "catia_endpoint_relation_branch_order")?
                .enumerate()
            {
                let (points, _point_storage) = ctx
                    .with_scoped_storage("catia_endpoint_relation_score_points", || {
                        relation_choice_points(ctx, choice, "catia_endpoint_relation_score_points")
                    })?;
                let score = ctx.fold(
                    &points,
                    Some(0usize),
                    |score, point| {
                        if ctx.contains_hash_set(
                            &assigned_points,
                            point,
                            "catia_endpoint_relation_score_points",
                        )? {
                            return Ok(score);
                        }
                        let support = ctx
                            .get_hash_map(
                                &point_support,
                                point,
                                "catia_endpoint_relation_score_points",
                            )?
                            .copied()
                            .unwrap_or(0);
                        Ok(score.and_then(|score| {
                            point_count
                                .checked_sub(support)?
                                .checked_add(1)
                                .and_then(|contribution| score.checked_add(contribution))
                        }))
                    },
                    "catia_endpoint_relation_score_points",
                )?;
                let Some(score) = score else {
                    return Ok(None);
                };
                ctx.push_vec(
                    &mut branch_order,
                    (score, index),
                    "catia_endpoint_relation_branch_order",
                )?;
            }
            ctx.sort_unstable_by(
                &mut branch_order,
                |value| value,
                |(left_score, left), (right_score, right)| {
                    right_score
                        .cmp(left_score)
                        .then_with(|| choices[*left].id.cmp(&choices[*right].id))
                },
                "catia mesh endpoint relation branch order sort",
            )?;
            Ok::<_, CodecError>(Some(branch_order))
        })?
    };
    let Some(branch_order) = branch_order else {
        return Ok(true);
    };
    let mut branches = branch_order.into_iter();
    while let Some((_, index)) =
        ctx.next_charged(&mut branches, "catia_endpoint_relation_branch_order")?
    {
        if budget.exhausted() {
            return Ok(true);
        }
        let ((branch, assigned_copy), _branch_storage) =
            ctx.with_scoped_storage("catia_endpoint_relation_branch_storage", || {
                Ok::<_, CodecError>((
                    copy_endpoint_relation_branch(ctx, &domains, face, index)?,
                    ctx.copy_slice(&assigned, "catia_endpoint_relation_branch_assigned")?,
                ))
            })?;
        if walk_endpoint_relation_domains(
            ctx,
            crate::solve::mesh_quotient::WalkEndpointRelationDomainsInputs {
                domains: branch,
                face_assignments,
                assigned: assigned_copy,
                constraints,
                point_count,
                budget,
                state_memo,
                state_memo_storage,
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
    ctx: &DecodeContext<'_>,
    face_assignments: &[MeshFaceBoundaryAssignment],
    face_configurations: &[Option<MeshFaceEndpointConfigurations>],
    covered: &mut [bool],
) -> Result<Option<Vec<MeshEndpointRelationChoice>>, CodecError> {
    // (configuration, assignment) for every viable configuration.
    let (prepared, _viable_storage) =
        ctx.with_scoped_storage("catia_endpoint_relation_viable_storage", || {
            let mut viable = Vec::new();
            let mut unknown = false;
            for (assignment, configurations) in ctx
                .admit_iter(
                    face_configurations,
                    "catia_endpoint_relation_configuration_keys",
                )?
                .enumerate()
            {
                let Some(configurations) = configurations else {
                    unknown = true;
                    continue;
                };
                for configuration in
                    ctx.admit_iter(configurations, "catia_endpoint_relation_configuration_keys")?
                {
                    for &(edge, _) in
                        ctx.admit_iter(configuration, "catia_endpoint_relation_covered")?
                    {
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
                    let mut relation_configuration =
                        ctx.copy_slice(configuration, "catia_endpoint_relation_config_pairs")?;
                    for (_, pair) in ctx.admit_iter(
                        &mut relation_configuration,
                        "catia_endpoint_relation_config_pairs",
                    )? {
                        *pair = [pair[0].min(pair[1]), pair[0].max(pair[1])];
                    }
                    ctx.sort_unstable_by(
                        &mut relation_configuration,
                        |value| value,
                        Ord::cmp,
                        "catia_endpoint_relation_config_pairs_sort",
                    )?;
                    ctx.push_vec(
                        &mut viable,
                        (relation_configuration, assignment),
                        "catia_endpoint_relation_configuration_assignments",
                    )?;
                }
            }
            // Equal configurations group together, ascending, each with its
            // ascending assignments; choices take that configuration order.
            ctx.sort_unstable_by(
                &mut viable,
                |entry| entry,
                Ord::cmp,
                "catia_endpoint_relation_configuration_assignments_sort",
            )?;
            ctx.dedup_vec(
                &mut viable,
                "catia_endpoint_relation_configuration_assignments",
            )?;
            Ok::<_, CodecError>(Some((viable, unknown)))
        })?;
    let Some((viable, unknown)) = prepared else {
        return Ok(None);
    };
    let mut choices = Vec::new();
    let mut start = 0;
    while start < viable.len() {
        ctx.charge_work(1, "catia_endpoint_relation_face_choices")?;
        let edge_pairs = &viable[start].0;
        let length = ctx.partition_point(
            &viable[start..],
            |(pairs, _)| {
                Ok(pairs.len() == edge_pairs.len()
                    && ctx.equal(
                        pairs,
                        edge_pairs,
                        "catia_endpoint_relation_configuration_keys",
                    )?)
            },
            "catia_endpoint_relation_configuration_keys",
        )?;
        let group = &viable[start..start + length];
        start += length;
        let assignments = ctx.collect_vec(
            group.iter().map(|(_, assignment)| *assignment),
            "catia_endpoint_relation_configuration_assignments",
        )?;
        let edge_pairs = ctx.copy_slice(&group[0].0, "catia_endpoint_relation_config_pairs")?;
        ctx.push_vec(
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
    if unknown {
        // A stopped enumeration is not evidence that the assignment has no
        // configuration. The wildcard lets the relation walker defer that
        // assignment to the complete endpoint search.
        ctx.push_vec(
            &mut choices,
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Deferred,
            },
            "catia_endpoint_relation_face_choices",
        )?;
    }
    for (id, choice) in ctx
        .admit_iter(&mut choices, "catia_endpoint_relation_face_choices")?
        .enumerate()
    {
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
    coordinate_domains: Option<&'input14 MeshCoordinateRootDomains<'input14>>,
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
        || ctx.any_by(
            edge_candidates,
            |candidates| Ok(candidates.is_empty()),
            "catia_endpoint_relation_face_domains",
        )?
    {
        return Ok(None);
    }
    let (prepared, _preparation_storage) =
        ctx.with_scoped_storage("catia_endpoint_relation_preparation_storage", || {
            let mut domains = Vec::new();
            let mut covered = ctx.alloc_filled(
                edge_candidates.len(),
                false,
                "catia_endpoint_relation_covered",
            )?;
            for (face_assignments, face_configurations) in ctx
                .admit_iter(assignments, "catia_endpoint_relation_face_domains")?
                .zip(endpoint_configurations)
            {
                if face_assignments.len() != face_configurations.len() {
                    return Ok(ControlFlow::Break(None));
                }
                let choices = collect_endpoint_relation_face_choices(
                    ctx,
                    face_assignments,
                    face_configurations,
                    &mut covered,
                )?;
                let Some(choices) = choices else {
                    return Ok(ControlFlow::Break(None));
                };
                if choices.is_empty() {
                    return Ok(ControlFlow::Break(Some(MeshSolve::Failed(
                        MeshCandidateFailure::Rejected(()),
                    ))));
                }
                if !budget.charge_by(choices.len()) {
                    return Ok(ControlFlow::Break(Some(MeshSolve::Failed(
                        MeshCandidateFailure::Exhausted(()),
                    ))));
                }
                ctx.push_vec(
                    &mut domains,
                    choices,
                    "catia_endpoint_relation_face_domains",
                )?;
            }
            if ctx.any_by(
                &covered,
                |covered| Ok(!covered),
                "catia_endpoint_relation_covered",
            )? {
                return Ok(ControlFlow::Break(None));
            }
            let Some(constraints) = build_endpoint_relation_constraints(ctx, &domains, budget)?
            else {
                return Ok(ControlFlow::Break(Some(MeshSolve::Failed(
                    MeshCandidateFailure::Exhausted(()),
                ))));
            };
            Ok::<_, CodecError>(ControlFlow::Continue((domains, constraints)))
        })?;
    let (domains, constraints) = match prepared {
        ControlFlow::Break(outcome) => return Ok(outcome),
        ControlFlow::Continue(prepared) => prepared,
    };
    let mut resolved = None;
    let relation_memo_storage = RefCell::new(ctx.reserve_scoped(0, "catia_relation_state_memo")?);
    let relation_walk_memo_storage =
        RefCell::new(ctx.reserve_scoped(0, "catia_endpoint_relation_state_memo")?);
    let mut relation_state_memo = HashMap::new();
    let mut relation_walk_state_memo = HashMap::new();
    let coordinate_budget =
        coordinate_domains.map(|_| budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS));
    let mut ambiguous = false;
    let mut exhausted = false;
    let mut evaluate = |selections: MeshEndpointRelationSelections,
                        edge_pairs: Vec<[usize; 2]>|
     -> Result<bool, CodecError> {
        let mut point_set_storage = ctx.reserve_scoped(0, "catia_relation_point_set")?;
        let point_count = point_set_storage.with_storage(|| {
            let mut point_set = HashSet::new();
            for pair in ctx.admit_iter(&edge_pairs, "catia_relation_point_set")? {
                for &point in pair {
                    ctx.insert_hash_set(&mut point_set, point, "catia_relation_point_set")?;
                }
            }
            Ok::<_, CodecError>(point_set.len())
        })?;
        drop(point_set_storage);
        if point_count != vertex_points.len() {
            return Ok(false);
        }
        if let Some(valid) = partial_solution_valid {
            let (candidate_pairs, _predicate_storage) =
                ctx.with_scoped_storage("catia_relation_partial_pairs", || {
                    ctx.collect_vec(
                        edge_pairs.iter().copied().map(Some),
                        "catia_relation_partial_pairs",
                    )
                })?;
            if !valid(&candidate_pairs)? {
                return Ok(false);
            }
        }
        if relation_state_memo.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let (key, key_storage) =
                ctx.with_scoped_storage("catia_relation_state_memo", || {
                    let canonical_pairs = if let Some(gauge) = candidate_gauge {
                        canonicalize_complete_endpoint_pairs(ctx, &edge_pairs, gauge)?
                    } else {
                        Some(ctx.copy_slice(&edge_pairs, "catia_relation_canonical_pairs")?)
                    };
                    let Some(canonical_pairs) = canonical_pairs else {
                        return Ok(None);
                    };
                    let selection_copy = ctx.copy_retained_rows(
                        &selections,
                        "catia_relation_memo_selection_rows",
                        "catia_relation_memo_selection_values",
                    )?;
                    Ok::<_, CodecError>(Some((selection_copy, canonical_pairs)))
                })?;
            let Some(key) = key else {
                return Ok(false);
            };
            let entry = relation_memo_storage.borrow_mut().with_storage(|| {
                ctx.entry_hash_map(&mut relation_state_memo, key, "catia_relation_state_memo")
            })?;
            match entry {
                std::collections::hash_map::Entry::Occupied(_) => return Ok(false),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(key_storage);
                }
            }
        }

        let (prepared, mut evaluation_storage) =
            ctx.with_scoped_storage("catia_relation_evaluation_storage", || {
                let mut assignment_domains = Vec::new();
                for (face, options) in ctx
                    .admit_iter(&selections, "catia_relation_assignment_domains")?
                    .enumerate()
                {
                    let mut row = Vec::new();
                    for &assignment in
                        ctx.admit_iter(options, "catia_relation_assignment_choices")?
                    {
                        let Some(source) = assignments[face].get(assignment) else {
                            continue;
                        };
                        let mut boundaries = Vec::new();
                        for boundary in ctx.admit_iter(
                            &source.boundaries,
                            "catia_relation_assignment_boundaries",
                        )? {
                            let copy = ctx.copy_slice(
                                boundary,
                                "catia_relation_assignment_boundary_values",
                            )?;
                            ctx.push_vec(
                                &mut boundaries,
                                copy,
                                "catia_relation_assignment_boundaries",
                            )?;
                        }
                        ctx.push_vec(
                            &mut row,
                            MeshFaceBoundaryAssignment { boundaries },
                            "catia_relation_assignment_choices",
                        )?;
                    }
                    ctx.push_vec(
                        &mut assignment_domains,
                        row,
                        "catia_relation_assignment_domains",
                    )?;
                }
                if ctx.any_by(
                    &assignment_domains,
                    |domain| Ok(domain.is_empty()),
                    "catia_relation_assignment_domains",
                )? {
                    return Ok(None);
                }
                let candidates = ctx.collect_indexed_vec(
                    edge_pairs.len(),
                    "catia_relation_candidate_rows",
                    |edge| {
                        let mut row = ctx.collection_vec(1, "catia_relation_candidate_pair")?;
                        row.push(edge_pairs[edge]);
                        Ok(row)
                    },
                )?;
                Ok::<_, CodecError>(Some((assignment_domains, candidates)))
            })?;
        let Some((assignment_domains, candidates)) = prepared else {
            return Ok(false);
        };
        let endpoint_resolution_budget = budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        let outcome = if ctx.all_by(
            &assignment_domains,
            |domain| Ok(domain.len() == 1),
            "catia_relation_assignment_domains",
        )? {
            let (selected, complete) = evaluation_storage.with_storage(|| {
                let mut selected = Vec::new();
                let mut complete = true;
                for mut domain in
                    ctx.admit_iter(assignment_domains, "catia_relation_selected_assignments")?
                {
                    if let Some(choice) = domain.pop() {
                        ctx.push_vec(&mut selected, choice, "catia_relation_selected_assignments")?;
                    } else {
                        complete = false;
                        break;
                    }
                }
                Ok::<_, CodecError>((selected, complete))
            })?;
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
                    if !valid(&candidate_pairs)? {
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
    let (assigned, _assigned_storage) =
        ctx.with_scoped_storage("catia_endpoint_relation_assigned", || {
            ctx.alloc_filled(
                edge_candidates.len(),
                None,
                "catia_endpoint_relation_assigned",
            )
        })?;
    walk_endpoint_relation_domains(
        ctx,
        crate::solve::mesh_quotient::WalkEndpointRelationDomainsInputs {
            domains,
            face_assignments: assignments,
            assigned,

            constraints: &constraints,
            point_count: vertex_points.len(),
            budget,
            state_memo: &mut relation_walk_state_memo,
            state_memo_storage: &relation_walk_memo_storage,
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
    for [start, end] in ctx.admit_iter(edge_vertices, "catia_mesh_candidate_point_pairs")? {
        let (Some(start), Some(end)) = (point_assignment.get(start), point_assignment.get(end))
        else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut pairs,
            Some([*start, *end]),
            "catia_mesh_candidate_point_pairs",
        )?;
    }
    Ok(Some(pairs))
}

fn endpoint_pairs_respect_candidate_domains(
    ctx: &DecodeContext<'_>,
    pairs: &[Option<[usize; 2]>],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_endpoint_pair_candidate_domains";
    if pairs.len() != edge_candidates.len() {
        return Ok(false);
    }
    ctx.all_by(
        pairs.iter().zip(edge_candidates),
        |(pair, candidates)| {
            let Some(pair) = pair else {
                return Ok(true);
            };
            Ok(candidates.is_empty()
                || ctx.any_by(
                    candidates,
                    |candidate| Ok(same_unordered_pair(*candidate, *pair)),
                    OPERATION,
                )?)
        },
        OPERATION,
    )
}

/// Orients each pair as its edge's only matching candidate does. A pair
/// that two differently oriented candidates match, or whose edge has no
/// candidates, keeps its orientation; a pair no candidate matches fails.
fn restore_unique_endpoint_pair_orientations(
    ctx: &DecodeContext<'_>,
    pairs: &[[usize; 2]],
    candidates: &[Vec<[usize; 2]>],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    const OPERATION: &str = "catia_oriented_endpoint_pairs";
    if pairs.len() != candidates.len() {
        return Ok(None);
    }
    let mut oriented = Vec::new();
    for (&pair, candidates) in ctx.admit_iter(pairs, OPERATION)?.zip(candidates) {
        let mut unique = None;
        // Stops at the second differently oriented match.
        let multiple = ctx.any_by(
            candidates,
            |&candidate| {
                if !same_unordered_pair(candidate, pair) {
                    return Ok(false);
                }
                if unique.is_some_and(|previous| previous != candidate) {
                    return Ok(true);
                }
                unique = Some(candidate);
                Ok(false)
            },
            OPERATION,
        )?;
        let direction = if multiple {
            pair
        } else if let Some(unique) = unique {
            unique
        } else if candidates.is_empty() {
            pair
        } else {
            return Ok(None);
        };
        ctx.push_vec(&mut oriented, direction, OPERATION)?;
    }
    Ok(Some(oriented))
}

fn materialize_boundary_domains(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    edge_pairs: &[[usize; 2]],
) -> Result<Option<Vec<Vec<MeshFaceBoundaryAssignment>>>, CodecError> {
    let mut materialized = Vec::new();
    ctx.reserve_vec(
        &mut materialized,
        domains.len(),
        "catia materialized boundary domains",
    )?;
    for domain in ctx.admit_iter(domains, "catia materialized boundary domains")? {
        let assignments = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                let mut copies = Vec::new();
                ctx.reserve_vec(
                    &mut copies,
                    assignments.len(),
                    "catia materialized ordered assignments",
                )?;
                for assignment in
                    ctx.admit_iter(assignments, "catia materialized ordered assignments")?
                {
                    let boundaries = ctx.copy_retained_rows(
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
                ctx.push_vec(
                    &mut copies,
                    assignment,
                    "catia materialized deferred boundary",
                )?;
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
                ctx.reserve_vec(
                    &mut boundary,
                    length,
                    "catia materialized unordered boundary",
                )?;
                for (index, &(edge, reversed)) in ctx
                    .admit_iter(&cycle[..], "catia materialized unordered boundary")?
                    .enumerate()
                {
                    boundary.push(MeshBoundaryEdgeCandidate {
                        edge,
                        start: index,
                        end: (index + 1) % length,
                        reversed: Some(reversed),
                    });
                }
                let mut boundaries = Vec::new();
                ctx.push_vec(
                    &mut boundaries,
                    boundary,
                    "catia materialized unordered boundary rows",
                )?;
                let mut copies = Vec::new();
                ctx.push_vec(
                    &mut copies,
                    MeshFaceBoundaryAssignment { boundaries },
                    "catia materialized unordered assignments",
                )?;
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
    ctx: &DecodeContext<'_>,
    edge_faces: &[[usize; 2]],
    domains: &[MeshFaceBoundaryDomain],
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_incident_edge_support";
    // Each face's edges, ascending, are indexed once.
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let face_edges = storage.with_storage(|| {
        let mut face_edges = Vec::new();
        for domain in ctx.admit_iter(domains, OPERATION)? {
            let edges = mesh_boundary_domain_edges(ctx, domain)?;
            ctx.push_vec(&mut face_edges, edges, OPERATION)?;
        }
        Ok::<_, CodecError>(face_edges)
    })?;
    ctx.all_by(
        edge_faces.iter().enumerate(),
        |(edge, faces)| {
            for face in faces {
                let Some(edges) = face_edges.get(*face) else {
                    return Ok(false);
                };
                if ctx.binary_search(edges, &edge, OPERATION)?.is_err() {
                    return Ok(false);
                }
            }
            Ok(true)
        },
        OPERATION,
    )
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

    fn incident_support(edge_faces: &[[usize; 2]], domains: &[MeshFaceBoundaryDomain]) -> bool {
        crate::test_support::with_service_context(|ctx| {
            mesh_domains_have_incident_edge_support(ctx, edge_faces, domains)
        })
        .expect("service resource budget")
    }

    fn pairs_respect_domains(
        pairs: &[Option<[usize; 2]>],
        edge_candidates: &[Vec<[usize; 2]>],
    ) -> bool {
        crate::test_support::with_service_context(|ctx| {
            endpoint_pairs_respect_candidate_domains(ctx, pairs, edge_candidates)
        })
        .expect("service resource budget")
    }

    #[test]
    fn every_concrete_incident_face_must_host_the_edge() {
        let edge_faces = [[0, 1]];
        let valid = vec![
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
        ];
        assert!(incident_support(&edge_faces, &valid));

        let wrong_face = vec![
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
            MeshFaceBoundaryDomain::Ordered(vec![assignment(1)]),
        ];
        assert!(!incident_support(&edge_faces, &wrong_face));

        let unordered = vec![
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
            MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0]),
        ];
        assert!(incident_support(&edge_faces, &unordered));

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
        assert!(incident_support(&[[0, 1], [0, 1]], &deferred));
        assert!(!incident_support(&[[0, 1], [0, 1], [0, 1]], &deferred));
    }

    #[test]
    fn endpoint_pairs_must_remain_inside_candidate_domains() {
        let candidates = vec![vec![[1, 2], [2, 3]], Vec::new()];
        assert!(pairs_respect_domains(&[Some([2, 1]), None], &candidates,));
        assert!(!pairs_respect_domains(&[Some([0, 2]), None], &candidates,));
        assert!(!pairs_respect_domains(&[Some([1, 2])], &candidates,));
    }

    #[test]
    fn unique_candidate_restores_endpoint_pair_orientation() {
        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                restore_unique_endpoint_pair_orientations(
                    ctx,
                    &[[0, 1], [2, 3], [4, 5]],
                    &[vec![[1, 0]], vec![[2, 3], [3, 2]], Vec::new()],
                )
            })
            .expect("service resource budget"),
            Some(vec![[1, 0], [2, 3], [4, 5]])
        );
        assert!(crate::test_support::with_service_context(|ctx| {
            restore_unique_endpoint_pair_orientations(ctx, &[[0, 1]], &[vec![[2, 3]]])
        })
        .expect("service resource budget")
        .is_none());
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
        let domains = [MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: Some(false),
                }]],
            },
        ])];
        let mut refused = HashSet::new();
        for cap in 0..8 {
            match crate::test_support::with_collection_limit(cap, |ctx| {
                materialize_boundary_domains(ctx, &domains, &[])
            }) {
                Err(CodecError::ResourceLimit(limit)) => {
                    refused.insert(limit.operation);
                }
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
        assert!(
            crate::test_support::with_service_context(|ctx| materialize_boundary_domains(
                ctx,
                &domains,
                &[]
            ))
            .expect("service resource budget")
            .is_some()
        );
    }

    #[test]
    fn unordered_boundary_copy_refuses_before_nested_rows() {
        let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];
        let pairs = [[0, 1], [1, 2], [2, 0]];
        let mut refused = HashSet::new();
        for cap in 0..32 {
            match crate::test_support::with_collection_limit(cap, |ctx| {
                materialize_boundary_domains(ctx, &domains, &pairs)
            }) {
                Err(CodecError::ResourceLimit(limit)) => {
                    refused.insert(limit.operation);
                }
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
        assert!(
            crate::test_support::with_service_context(|ctx| materialize_boundary_domains(
                ctx, &domains, &pairs
            ))
            .expect("service resource budget")
            .is_some()
        );
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
        boundaries: ctx.copy_retained_rows(
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
    ctx.collect_indexed_vec(rows.len(), "catia_mesh_edge_copy_rows", |row| {
        rows[row].clone_charged(ctx)
    })
}

fn fixed_initial_orientations(
    ctx: &DecodeContext<'_>,
    fixed: &[bool],
    operation: &'static str,
) -> Result<Vec<Option<bool>>, CodecError> {
    ctx.collect_vec(
        fixed.iter().map(|fixed| (!fixed).then_some(false)),
        operation,
    )
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
    let (assignment_domains, _assignment_domain_storage) =
        ctx.with_scoped_storage("catia_fixed_assignment_storage", || {
            let mut assignment_domains = Vec::new();
            ctx.reserve_vec(
                &mut assignment_domains,
                selected.len(),
                "catia_fixed_assignment_domain_rows",
            )?;
            for assignment in ctx.admit_iter(selected, "catia_fixed_assignment_domain_rows")? {
                let mut domain = Vec::new();
                ctx.push_vec(
                    &mut domain,
                    copy_mesh_assignment(ctx, assignment)?,
                    "catia_fixed_assignment_domain_entries",
                )?;
                assignment_domains.push(domain);
            }
            Ok::<_, CodecError>(assignment_domains)
        })?;
    if ctx.any_by(
        edge_candidates,
        |candidates| Ok(candidates.len() != 1),
        "catia_fixed_endpoint_pairs",
    )? {
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
    // Every edge holds exactly one candidate pair.
    let (prepared, _direction_storage) =
        ctx.with_scoped_storage("catia_fixed_direction_preparation_storage", || {
            let edge_pairs = ctx.collect_vec(
                edge_candidates
                    .iter()
                    .flat_map(|candidates| candidates.first().copied()),
                "catia_fixed_endpoint_pairs",
            )?;
            let (fixed_face_directions, use_fixed_direction_search) = {
                let mut row_storage = ctx.reserve_scoped(0, "catia_fixed_face_direction_rows")?;
                let mut rows = Vec::new();
                row_storage.with_storage(|| {
                    ctx.reserve_vec(&mut rows, selected.len(), "catia_fixed_face_direction_rows")
                })?;
                let mut direction_overflow = false;
                let mut assignments = selected.iter();
                while let Some(assignment) =
                    ctx.next_charged(&mut assignments, "catia_fixed_face_direction_rows")?
                {
                    let (configuration, _configuration_storage) = ctx
                        .with_scoped_storage("catia_fixed_face_configuration_storage", || {
                            endpoint_configuration_for_assignment(ctx, assignment, &edge_pairs)
                        })?;
                    let Some(configuration) = configuration else {
                        return Ok(None);
                    };
                    let (directions, storage) = ctx
                        .with_scoped_storage("catia_fixed_face_direction_storage", || {
                            endpoint_configuration_directions(ctx, assignment, &configuration)
                        })?;
                    let directions = match directions {
                        Ok(directions) => directions,
                        Err(MeshDirectionEnumerationError::Overflow) => {
                            direction_overflow = true;
                            break;
                        }
                        Err(MeshDirectionEnumerationError::Invalid) => return Ok(None),
                    };
                    if directions.is_empty() {
                        return Ok(None);
                    }
                    rows.push(ScopedValue {
                        value: directions,
                        storage: Some(storage),
                    });
                }
                if direction_overflow {
                    ctx.clear_vec(&mut rows, "catia_fixed_face_direction_discard")?;
                    drop(rows);
                    drop(row_storage);
                    (
                        ctx.collect_indexed_vec(
                            assignment_domains.len(),
                            "catia_general_mesh_fixed_face_directions",
                            |_| Ok(None),
                        )?,
                        false,
                    )
                } else {
                    let rows = ctx.try_collect_retained_with(
                        rows,
                        "catia_fixed_face_direction_rows",
                        |mut row| {
                            row.storage
                                .take()
                                .ok_or_else(|| {
                                    CodecError::malformed("fixed face directions own storage")
                                })?
                                .commit()?;
                            Ok::<_, CodecError>(Some(row.value))
                        },
                    )?;
                    (rows, true)
                }
            };
            let mut edge_has_fixed_direction = ctx.alloc_filled(
                edge_candidates.len(),
                false,
                "catia_fixed_mesh_edge_directions",
            )?;
            for assignment in ctx.admit_iter(selected, "catia_fixed_mesh_edge_directions")? {
                for boundary in
                    ctx.admit_iter(&assignment.boundaries, "catia_fixed_mesh_edge_directions")?
                {
                    for use_ in ctx.admit_iter(boundary, "catia_fixed_mesh_edge_directions")? {
                        if use_.reversed.is_some() {
                            let Some(fixed) = edge_has_fixed_direction.get_mut(use_.edge) else {
                                return Ok(None);
                            };
                            *fixed = true;
                        }
                    }
                }
            }
            Ok::<_, CodecError>(Some((
                fixed_face_directions,
                use_fixed_direction_search,
                edge_has_fixed_direction,
            )))
        })?;
    let Some((fixed_face_directions, use_fixed_direction_search, edge_has_fixed_direction)) =
        prepared
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    };
    let Some(quotient) =
        initial_mesh_quotient(ctx, edge_candidates, vertex_points.len(), port_identities)?
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    };
    let ((direct_quotient, direct_directions, direct_possible), _direct_storage) = ctx
        .with_scoped_storage("catia_fixed_direct_storage", || {
            let mut direct_quotient = quotient.clone_charged(ctx)?;
            let mut direct_orientations = fixed_initial_orientations(
                ctx,
                &edge_has_fixed_direction,
                "catia_fixed_direct_orientations",
            )?;
            let mut direct_directions = Vec::new();
            ctx.reserve_vec(
                &mut direct_directions,
                selected.len(),
                "catia_fixed_direct_direction_rows",
            )?;
            let mut direct_possible = true;
            for (assignment, direction_options) in ctx
                .admit_iter(selected, "catia_fixed_direct_direction_rows")?
                .zip(&fixed_face_directions)
            {
                let Some(direction_options) = direction_options.as_ref() else {
                    direct_possible = false;
                    break;
                };
                let Some(label_directions) = direction_options.first() else {
                    direct_possible = false;
                    break;
                };
                let mut orient = |use_: &MeshBoundaryEdgeCandidate, label_direction: bool| {
                    let Some(required) = use_.reversed else {
                        return Ok(true);
                    };
                    let Some(orientation) = direct_orientations.get_mut(use_.edge) else {
                        return Ok(false);
                    };
                    let required_orientation = required ^ label_direction;
                    Ok(match *orientation {
                        Some(existing) => existing == required_orientation,
                        None => {
                            *orientation = Some(required_orientation);
                            true
                        }
                    })
                };
                let constrained = ctx.all_by(
                    assignment.boundaries.iter().zip(label_directions),
                    |(boundary, directions)| {
                        ctx.all_by(
                            boundary.iter().zip(directions),
                            |(use_, &label_direction)| orient(use_, label_direction),
                            "catia_fixed_direct_orientations",
                        )
                    },
                    "catia_fixed_direct_orientations",
                )?;
                if !constrained {
                    direct_possible = false;
                    break;
                }
                let Some(directions) = direct_quotient.merge_label_directions_in_place(
                    ctx,
                    assignment,
                    label_directions,
                    &direct_orientations,
                    Some(budget),
                )?
                else {
                    direct_possible = false;
                    break;
                };
                direct_directions.push(directions);
            }
            Ok::<_, CodecError>((direct_quotient, direct_directions, direct_possible))
        })?;
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
    let face_work = ctx.alloc_filled(
        assignment_domains.len(),
        Some(1usize),
        "catia_fixed_face_work",
    )?;
    let fixed_edge_orientations = if use_fixed_direction_search {
        fixed_initial_orientations(
            ctx,
            &edge_has_fixed_direction,
            "catia_fixed_search_orientations",
        )?
    } else {
        Vec::new()
    };
    let (mut search, _search_storage) =
        ctx.with_scoped_storage("catia_mesh_search_field_storage", || {
            Ok::<_, CodecError>(MeshSelectionSearch {
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
                selected: ctx.collect_indexed_vec(
                    assignment_domains.len(),
                    "catia_fixed_mesh_selection",
                    |_| Ok(None),
                )?,
                visited_states: std::collections::HashMap::new(),
                memo_storage: RefCell::new(
                    (ctx).reserve_scoped(0, "catia_selection_memo_storage")?,
                ),
                outcome: SearchOutcome::Open,
                face_equation_cache: RefCell::default(),
            })
        })?;
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
        let mut assignments = assignment_domains[face].iter();
        while let Some(assignment) =
            ctx.next_charged(&mut assignments, "catia_fixed_assignment_selection")?
        {
            let (assignment, _assignment_storage) = ctx
                .with_scoped_storage("catia_fixed_assignment_copy", || {
                    copy_mesh_assignment(ctx, assignment)
                })?;
            selected.push(assignment);
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
        || edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
        || ctx.any_by(
            assignment_domains,
            |domain| Ok(domain.is_empty()),
            "catia_fixed_assignment_selection",
        )?
        || ctx.any_by(
            edge_candidates,
            |candidates| Ok(candidates.len() != 1),
            "catia_fixed_assignment_selection",
        )?
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }

    let (mut selected, _selected_storage) = ctx
        .with_scoped_storage("catia_fixed_assignment_selection", || {
            ctx.collection_vec(assignment_domains.len(), "catia_fixed_assignment_selection")
        })?;
    let mut outcome = SearchOutcome::Open;
    visit(
        ctx,
        FixedEndpointAssignmentSearch {
            face: 0,
            assignment_domains,
            selected: &mut selected,
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
    const OPERATION: &str = "catia_prune_endpoint_pair_support";
    let budget = WorkBudget::new(limit);
    'fixpoint: loop {
        ctx.charge_work(1, "catia_mesh_quotient_iteration")?;
        let mut changed = false;
        for face in ctx.admit_iter(&mut *assignments, OPERATION)? {
            let before = face.len();
            ctx.retain_vec(
                face,
                |assignment| {
                    Ok(mesh_assignment_endpoint_cycles_viable_with(
                        ctx,
                        assignment,
                        edge_candidates,
                        None,
                        Some(&budget),
                    )?
                    .unwrap_or(true))
                },
                OPERATION,
            )?;
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
        // (edge, face, assignment) for every assignment using the edge,
        // ascending, so an edge's users form one run.
        let mut storage = ctx.reserve_scoped(0, "catia_prune_incident_faces")?;
        let incidences = storage.with_storage(|| {
            let mut incidences = Vec::new();
            for (face, choices) in ctx
                .admit_iter(&*assignments, "catia_prune_incident_faces")?
                .enumerate()
            {
                for (index, assignment) in ctx
                    .admit_iter(choices, "catia_prune_incident_faces")?
                    .enumerate()
                {
                    for boundary in
                        ctx.admit_iter(&assignment.boundaries, "catia_prune_incident_faces")?
                    {
                        for use_ in ctx.admit_iter(boundary, "catia_prune_incident_faces")? {
                            ctx.push_vec(
                                &mut incidences,
                                (use_.edge, face, index),
                                "catia_prune_incident_faces",
                            )?;
                        }
                    }
                }
            }
            ctx.sort_unstable_by(
                &mut incidences,
                |entry| entry,
                Ord::cmp,
                "catia_prune_incident_faces_sort",
            )?;
            ctx.dedup_vec(&mut incidences, "catia_prune_incident_faces")?;
            Ok::<_, CodecError>(incidences)
        })?;
        for edge in ctx.admit_iter(0..edge_candidates.len(), OPERATION)? {
            if edge_candidates[edge].is_empty() {
                continue;
            }
            let start = ctx.partition_point(&incidences, |entry| Ok(entry.0 < edge), OPERATION)?;
            let length =
                ctx.partition_point(&incidences[start..], |entry| Ok(entry.0 == edge), OPERATION)?;
            let users = &incidences[start..start + length];
            // A pair survives when every incident face keeps an assignment
            // using the edge that closes its cycles with that pair.
            let pair_supported = |pair: [usize; 2]| -> Result<bool, CodecError> {
                let mut index = 0;
                while index < users.len() {
                    ctx.charge_work(1, OPERATION)?;
                    let face = users[index].1;
                    let run = ctx.partition_point(
                        &users[index..],
                        |entry| Ok(entry.1 == face),
                        OPERATION,
                    )?;
                    let supported = ctx.any_by(
                        &users[index..index + run],
                        |&(_, face, assignment)| {
                            Ok(mesh_assignment_endpoint_cycles_viable_with(
                                ctx,
                                &assignments[face][assignment],
                                edge_candidates,
                                Some((edge, pair)),
                                Some(&budget),
                            )?
                            .unwrap_or(true))
                        },
                        OPERATION,
                    )?;
                    if !supported {
                        return Ok(false);
                    }
                    index += run;
                }
                Ok(true)
            };
            let before = edge_candidates[edge].len();
            let (supported, _support_storage) =
                ctx.with_scoped_storage("catia_prune_retained_pairs", || {
                    ctx.try_collect_vec(
                        edge_candidates[edge].iter().copied().map(pair_supported),
                        "catia_prune_retained_pairs",
                    )
                })?;
            let mut position = 0usize;
            ctx.retain_vec(
                &mut edge_candidates[edge],
                |_| {
                    let keep = supported[position];
                    position += 1;
                    Ok(keep)
                },
                OPERATION,
            )?;
            let narrowed = edge_candidates[edge].len() != before;
            if budget.exhausted() {
                // Do not turn incomplete propagation into a contradiction.
                return Ok(true);
            }
            if edge_candidates[edge].is_empty() {
                return Ok(false);
            }
            if narrowed {
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
        || ctx.any_by(
            edge_candidates,
            |candidates| Ok(candidates.len() != 1),
            "catia_singleton_selected_assignments",
        )?
    {
        return Ok(None);
    }

    let (prepared, _singleton_preparation_storage) =
        ctx.with_scoped_storage("catia_singleton_selection_storage", || {
            let mut selected = Vec::new();
            let mut endpoint_labelled_directions = Vec::new();
            ctx.reserve_vec(
                &mut selected,
                assignments.len(),
                "catia_singleton_selected_assignments",
            )?;
            ctx.reserve_vec(
                &mut endpoint_labelled_directions,
                assignments.len(),
                "catia_singleton_selected_directions",
            )?;
            for face in ctx.admit_iter(assignments, "catia_singleton_selected_assignments")? {
                let mut seen = HashSet::new();
                let mut first = None;
                for assignment in ctx.admit_iter(face, "catia_singleton_signatures")? {
                    let mut directions = Vec::new();
                    ctx.reserve_vec(
                        &mut directions,
                        assignment.boundaries.len(),
                        "catia_singleton_direction_rows",
                    )?;
                    let mut valid = true;
                    for boundary in
                        ctx.admit_iter(&assignment.boundaries, "catia_singleton_direction_rows")?
                    {
                        let Some(row) = singleton_mesh_boundary_directions(
                            ctx,
                            boundary,
                            edge_candidates,
                            edge_direction_evidence,
                        )?
                        else {
                            valid = false;
                            break;
                        };
                        directions.push(row);
                    }
                    if !valid {
                        continue;
                    }
                    let Some(signature) = canonical_singleton_coordinate_cycles(
                        ctx,
                        assignment,
                        &directions,
                        edge_candidates,
                    )?
                    else {
                        continue;
                    };
                    if !ctx.insert_hash_set(&mut seen, signature, "catia_singleton_signatures")? {
                        continue;
                    }
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
            Ok::<_, CodecError>(Some((selected, endpoint_labelled_directions)))
        })?;
    let Some((selected, endpoint_labelled_directions)) = prepared else {
        return Ok(None);
    };
    if let Some(topology) = reconstruct_singleton_coordinate_topology(
        ctx,
        edge_rows,
        vertex_points,
        edge_candidates,
        &selected,
        &endpoint_labelled_directions,
    )? {
        let point_assignment =
            ctx.collect_vec(0..vertex_points.len(), "catia_singleton_identity_points")?;
        return Ok(Some(MeshSolve::Solved((topology, point_assignment))));
    }
    // An unresolved coedge direction does not select a point endpoint. It is
    // a row-orientation gauge. Let the exact coordinate binding prove the
    // resulting cycle, then try the endpoint-labelled gauge only when the
    // fixed false direction cannot bind.
    let (fixed_directions, _fixed_direction_storage) =
        ctx.with_scoped_storage("catia_singleton_fixed_storage", || {
            let mut fixed_directions = Vec::new();
            ctx.reserve_vec(
                &mut fixed_directions,
                selected.len(),
                "catia_singleton_fixed_face_rows",
            )?;
            for assignment in ctx.admit_iter(&selected, "catia_singleton_fixed_face_rows")? {
                let mut face_directions = Vec::new();
                ctx.reserve_vec(
                    &mut face_directions,
                    assignment.boundaries.len(),
                    "catia_singleton_fixed_boundary_rows",
                )?;
                for boundary in ctx.admit_iter(
                    &assignment.boundaries,
                    "catia_singleton_fixed_boundary_rows",
                )? {
                    if boundary.is_empty() {
                        return Ok(None);
                    }
                    let directions = ctx.collect_vec(
                        boundary.iter().map(|use_| use_.reversed.unwrap_or(false)),
                        "catia_singleton_fixed_directions",
                    )?;
                    face_directions.push(directions);
                }
                fixed_directions.push(face_directions);
            }
            Ok::<_, CodecError>(Some(fixed_directions))
        })?;
    let Some(fixed_directions) = fixed_directions else {
        return Ok(None);
    };
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
    'storage,
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
    prepared_quotient: Option<&'input4 MeshQuotient<'storage>>,
    edge_direction_evidence: Option<&'input5 [bool]>,
    budget: &'input7 WorkBudget<'input6>,
    partial_solution_valid: Option<&'input9 MeshEndpointSolutionPredicate<'input8>>,
    complete_solution_valid: Option<&'input11 MeshEndpointSolutionPredicate<'input10>>,
    candidate_gauge: Option<MeshCandidateGauge<'input12>>,
    priority_edges: Option<&'input13 [bool]>,
}

fn resolve_standard_mesh_endpoint_candidates<'storage>(
    ctx: &'storage DecodeContext<'_>,
    inputs: ResolveStandardMeshEndpointCandidatesInputs<
        'storage,
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
    let (mut edge_candidates, _candidate_storage) =
        ctx.with_scoped_storage("catia_standard_choice_storage", || {
            ctx.copy_retained_rows(
                edge_candidates,
                "catia_standard_endpoint_choice_rows",
                "catia_standard_endpoint_choice_pairs",
            )
        })?;
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
    let (refusal, _assignment_storage) =
        ctx.with_scoped_storage("catia_standard_assignment_storage", || {
            for face in
                ctx.admit_iter(&mut assignments, "catia_assignment_retained_face_options")?
            {
                let mut retained = Vec::new();
                for assignment in ctx.admit_iter(
                    std::mem::take(face),
                    "catia_assignment_retained_face_options",
                )? {
                    if quotient.assignment_has_option(
                        ctx,
                        &assignment,
                        &edge_candidates,
                        Some(budget),
                    )? {
                        ctx.push_vec(
                            &mut retained,
                            assignment,
                            "catia_assignment_retained_face_options",
                        )?;
                    }
                }
                *face = retained;
                if budget.exhausted() {
                    return Ok(Some(MeshCandidateFailure::Exhausted(())));
                }
                if face.is_empty() {
                    return Ok(Some(MeshCandidateFailure::Rejected(())));
                }
            }
            Ok::<_, CodecError>(None)
        })?;
    if let Some(refusal) = refusal {
        return Ok(MeshSolve::Failed(refusal));
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
    let coordinate_domains = if let Some(preparation_limit) =
        quotient.coordinate_domain_preparation_limit(ctx, vertex_points.len(), &edge_candidates)?
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
    let (prepared, _face_storage) =
        ctx.with_scoped_storage("catia_standard_face_preparation_storage", || {
            let mut face_work = Vec::new();
            ctx.reserve_vec(
                &mut face_work,
                assignments.len(),
                "catia_selection_face_work",
            )?;
            let mut total_work = 0usize;
            for face in ctx.admit_iter(&assignments, "catia_selection_face_work")? {
                let Some(next) = total_work.checked_add(face.len()) else {
                    return Ok(ControlFlow::Break(MeshCandidateFailure::Rejected(())));
                };
                total_work = next;
                face_work.push(Some(face.len()));
            }
            if total_work > MAX_SELECTION_WORK {
                return Ok(ControlFlow::Break(MeshCandidateFailure::Exhausted(())));
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
                return Ok(ControlFlow::Break(MeshCandidateFailure::Exhausted(())));
            }
            let (unselected, _unselected_storage) =
                ctx.with_scoped_storage("catia_endpoint_unselected_storage", || {
                    ctx.alloc_filled(
                        edge_candidates.len(),
                        None,
                        "catia_endpoint_unselected_edges",
                    )
                })?;
            let configuration_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
            let mut endpoint_configurations = Vec::new();
            for face in ctx.admit_iter(&assignments, "catia_face_configuration_face_rows")? {
                let mut face_configurations = Vec::new();
                for assignment in
                    ctx.admit_iter(face, "catia_face_configuration_assignment_rows")?
                {
                    let configurations = if configuration_budget.exhausted() {
                        None
                    } else {
                        let local_budget =
                            configuration_budget.child_slice(MAX_FACE_ENDPOINT_CONFIGURATION_WORK);
                        let (configurations, storage) =
                            ctx.with_scoped_storage("catia_face_configuration_storage", || {
                                mesh_face_endpoint_configurations(
                                    ctx,
                                    std::slice::from_ref(assignment),
                                    &edge_candidates,
                                    &unselected,
                                    &local_budget,
                                )
                            })?;
                        if !configuration_budget.charge_by(local_budget.consumed())
                            || local_budget.exhausted()
                        {
                            None
                        } else {
                            if configurations.is_some() {
                                storage.commit()?;
                            }
                            configurations
                        }
                    };
                    ctx.push_vec(
                        &mut face_configurations,
                        configurations,
                        "catia_face_configuration_assignment_rows",
                    )?;
                }
                ctx.push_vec(
                    &mut endpoint_configurations,
                    face_configurations,
                    "catia_face_configuration_face_rows",
                )?;
            }
            Ok::<_, CodecError>(ControlFlow::Continue((
                face_work,
                face_equations,
                face_choices,
                endpoint_configurations,
            )))
        })?;
    let (face_work, face_equations, face_choices, endpoint_configurations) = match prepared {
        ControlFlow::Break(refusal) => return Ok(MeshSolve::Failed(refusal)),
        ControlFlow::Continue(prepared) => prepared,
    };
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
    let (mut search, _search_storage) =
        ctx.with_scoped_storage("catia_mesh_search_field_storage", || {
            Ok::<_, CodecError>(MeshSelectionSearch {
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
                fixed_face_directions: ctx.collect_indexed_vec(
                    face_count,
                    "catia_mesh_fixed_face_directions",
                    |_| Ok(None),
                )?,
                fixed_edge_orientations: Vec::new(),
                edge_has_fixed_direction: Vec::new(),
                selected: ctx.collect_indexed_vec(
                    face_count,
                    "catia_mesh_selected_faces",
                    |_| Ok(None),
                )?,
                visited_states: std::collections::HashMap::new(),
                memo_storage: RefCell::new(
                    (ctx).reserve_scoped(0, "catia_selection_memo_storage")?,
                ),
                outcome: SearchOutcome::Open,
                face_equation_cache: RefCell::default(),
            })
        })?;
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
/// Errors from either predicate propagate unchanged.
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
    FP: Fn(&[Option<[usize; 2]>]) -> Result<bool, CodecError>,
    FC: Fn(&[Option<[usize; 2]>]) -> Result<bool, CodecError>,
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
    FP: Fn(&[Option<[usize; 2]>]) -> Result<bool, CodecError>,
    FC: Fn(&[Option<[usize; 2]>]) -> Result<bool, CodecError>,
{
    let budget = inputs.budget;
    let outcome = (|| -> Result<MeshCandidateSolve, CodecError> {
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

        let endpoint_budget = budget.session_child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
        let (prepared, _input_storage) = ctx.with_scoped_storage(
            "catia_mesh_input_storage",
            || -> Result<Option<_>, CodecError> {
                let Some(face_run) = largest_fbb_run(ctx, bytes)? else {
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
                let (boundary_context, _boundary_storage) =
                    ctx.with_scoped_storage("catia_mesh_boundary_context_storage", || {
                        StandardMeshBoundaryContext::parse_ports(
                            ctx,
                            bytes,
                            edge_faces,
                            global_handle_ports,
                        )
                    })?;
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
            },
        )?;
        let Some((face_count, edge_rows, vertex_points, mut mesh_domains, port_identities)) =
            prepared
        else {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
                MeshCandidateRejection::InputStructure,
            )));
        };
        let (coordinate_gauge, _gauge_storage) =
            ctx.with_scoped_storage("catia_mesh_gauge_storage", || {
                build_mesh_coordinate_gauge(
                    ctx,
                    vertex_points.len(),
                    &edge_rows,
                    edge_faces,
                    edge_geometry,
                    edge_candidates,
                    edge_identity_evidence,
                )
            })?;
        let candidate_gauge = Some(MeshCandidateGauge {
            edge_rows: &edge_rows,
            edge_faces,
            edge_geometry,
            edge_candidates,
            edge_identity_evidence,
            coordinate_gauge: Some(&coordinate_gauge),
        });
        const CARDINALITY: &str = "catia_mesh_input_cardinality";
        let dependency_outside = |dependencies: &[Vec<usize>]| -> Result<bool, CodecError> {
            Ok(dependencies.len() != edge_rows.len()
                || ctx.any_by(
                    dependencies,
                    |edges| ctx.any_by(edges, |edge| Ok(*edge >= edge_rows.len()), CARDINALITY),
                    CARDINALITY,
                )?)
        };
        let point_outside = ctx.any_by(
            edge_candidates,
            |pairs| {
                ctx.any_by(
                    pairs,
                    |pair| Ok(pair.iter().any(|point| *point >= vertex_points.len())),
                    CARDINALITY,
                )
            },
            CARDINALITY,
        )?;
        if edge_rows.len() != edge_faces.len()
            || edge_rows.len() != edge_candidates.len()
            || edge_rows.len() != edge_classes.len()
            || edge_rows.len() != edge_geometry.len()
            || edge_rows.len() != edge_direction_evidence.len()
            || edge_rows.len() != partial_constraint_edges.len()
            || edge_rows.len() != preferred_assignment_edges.len()
            || priority_edges.is_some_and(|edges| edges.len() != edge_rows.len())
            || assignment_dependencies.map_or(Ok(false), dependency_outside)?
            || point_outside
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
        for domain in ctx.admit_iter(&mut mesh_domains, "catia_mesh_domain_deduplication")? {
            if let MeshFaceBoundaryDomain::Ordered(assignments) = domain {
                deduplicate_mesh_quotient_assignments(ctx, std::slice::from_mut(assignments))?;
            }
        }
        if !mesh_domains_have_incident_edge_support(ctx, edge_faces, &mesh_domains)? {
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
        if ctx.any_by(
            edge_candidates,
            |candidates| Ok(candidates.is_empty()),
            "catia_mesh_open_edges",
        )? && propagate_common_boundary_components(
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
        let (prepared_constraints, _constraint_storage) =
            ctx.with_scoped_storage("catia_mesh_constraint_storage", || {
                let completed_edge_candidates = ctx.copy_retained_rows(
                    edge_candidates,
                    "catia_completed_edge_candidate_rows",
                    "catia_completed_edge_candidate_pairs",
                )?;
                if !mesh_quotient.edge_domains_viable(ctx, &completed_edge_candidates)? {
                    return Ok(ControlFlow::Break(
                        MeshCandidateRejection::QuotientPreparation,
                    ));
                }
                let Some(class_constraint) =
                    edge_class_search_constraint(ctx, edge_classes, &completed_edge_candidates)?
                else {
                    return Ok(ControlFlow::Break(
                        MeshCandidateRejection::EdgeClassConstraint,
                    ));
                };
                let constraint_edges = ctx.collect_vec(
                    partial_constraint_edges
                        .iter()
                        .zip(preferred_assignment_edges)
                        .zip(&class_constraint.active)
                        .map(|((partial, preferred), class)| *partial || *preferred || *class),
                    "catia_mesh_constraint_edges",
                )?;
                let mut assignment_predecessors = ctx.alloc_filled(
                    completed_edge_candidates.len(),
                    None,
                    "catia_mesh_assignment_predecessors",
                )?;
                for &(left, right) in ctx.admit_iter(
                    &class_constraint.ordered,
                    "catia_mesh_assignment_predecessors",
                )? {
                    assignment_predecessors[right] = Some(
                        assignment_predecessors[right]
                            .map_or(left, |predecessor: usize| predecessor.max(left)),
                    );
                }
                Ok::<_, CodecError>(ControlFlow::Continue((
                    completed_edge_candidates,
                    constraint_edges,
                    assignment_predecessors,
                )))
            })?;
        let (completed_edge_candidates, constraint_edges, assignment_predecessors) =
            match prepared_constraints {
                ControlFlow::Continue(prepared) => prepared,
                ControlFlow::Break(reason) => {
                    return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(reason)))
                }
            };
        let constrained_partial_solution_valid = |pairs: &[Option<[usize; 2]>]| {
            if endpoint_pairs_respect_candidate_domains(ctx, pairs, &completed_edge_candidates)? {
                partial_solution_valid(pairs)
            } else {
                Ok(false)
            }
        };
        let complete_preference_rejected = Cell::new(false);
        let constrained_complete_solution_valid = |pairs: &[Option<[usize; 2]>]| {
            let valid = if endpoint_pairs_respect_candidate_domains(
                ctx,
                pairs,
                &completed_edge_candidates,
            )? {
                complete_solution_valid(pairs)?
            } else {
                false
            };
            if !valid {
                complete_preference_rejected.set(true);
            }
            Ok(valid)
        };
        let mut incidence_solution = None;
        let mut incidence_ambiguity = None;
        let mut incidence_exhausted = false;
        let mut endpoint_memo_storage = ctx.reserve_scoped(0, "catia_endpoint_memo_entries")?;
        let mut endpoint_resolution_memo =
            HashMap::<Vec<[usize; 2]>, ScopedValue<'_, MeshEndpointResolve>>::new();
        let pair_solutions = visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy(ctx, crate::solve::incidence::VisitIncidenceEndpointPairSolutionsWithCoordinateRootPolicyInputs { edge_rows: &edge_rows, vertex_points: &vertex_points, edge_faces, edge_candidates: &completed_edge_candidates, face_count, mesh_assignments: Some(&mesh_domains), mesh_quotient: Some(&mesh_quotient), coordinate_root_policy: CoordinateRootPolicy::DeferToVisitor, partial_solution_valid: Some(MeshPartialEndpointConstraint {
            active_edges: &constraint_edges,
            coupled_edges: partial_constraint_edges,
            assignment_order: AssignmentOrder::new(
                Some(&assignment_predecessors),
                assignment_dependencies,
            ),
            valid: &constrained_partial_solution_valid,
        }), complete_solution_budget: Some(&endpoint_budget), solution_valid: &|pairs| {
            let (completed, _predicate_storage) = ctx.with_scoped_storage("catia_mesh_completed_predicate_pairs", || ctx.collect_vec(
                pairs.iter().copied().map(Some),
                "catia_mesh_completed_predicate_pairs",
            ))?;
            constrained_complete_solution_valid(&completed)
        }, visitor: &mut |pairs| -> Result<ControlFlow<()>, CodecError> {
            let endpoint_resolution = if let Some(cached) =
                ctx.get_hash_map(&endpoint_resolution_memo, pairs, "catia_endpoint_memo_key")?
            {
                copy_mesh_endpoint_resolution(ctx, cached)?
            } else {
                let (prepared, _visitor_storage) = ctx.with_scoped_storage("catia_endpoint_visitor_storage", || {
                let Some(oriented_pairs) = restore_unique_endpoint_pair_orientations(
                    ctx,
                    pairs,
                    &completed_edge_candidates,
                )?
                else {
                    return Ok(None);
                };
                let singleton = ctx.collect_indexed_vec(
                    oriented_pairs.len(),
                    "catia_mesh_singleton_pair_rows",
                    |edge| ctx.alloc_filled(1, oriented_pairs[edge], "catia_mesh_singleton_pair"),
                )?;
                let Some(mut mesh_assignments) =
                    materialize_boundary_domains(ctx, &mesh_domains, &oriented_pairs)?
                else {
                    return Ok(None);
                };
                deduplicate_mesh_quotient_assignments(ctx, &mut mesh_assignments)?;
                Ok::<_, CodecError>(Some((singleton, mesh_assignments)))
                })?;
                let Some((singleton, mesh_assignments)) = prepared else { return Ok(ControlFlow::Continue(())); };
                // This child owns the incidence-to-endpoint relation phase. The
                // complete materialization invoked by that relation takes its
                // own MAX_MESH_CONSTRAINT_OPERATIONS child slice.
                let endpoint_resolution_budget =
                    endpoint_budget.session_child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
                let resolution = resolve_standard_mesh_endpoint_candidates(ctx, crate::solve::mesh_quotient::ResolveStandardMeshEndpointCandidatesInputs { edge_rows: &edge_rows, vertex_points: &vertex_points, edge_candidates: &singleton, assignments: mesh_assignments, port_identities: &port_identities, prepared_quotient: None, edge_direction_evidence: Some(edge_direction_evidence), budget: &endpoint_resolution_budget, partial_solution_valid: Some(&constrained_partial_solution_valid), complete_solution_valid: Some(&constrained_complete_solution_valid), candidate_gauge, priority_edges })?;
                // Exhaustion is deterministic for this input and child budget.
                // The parent budget only decreases, so retrying the same key
                // cannot turn an exhausted materialization into a solution.
                if endpoint_resolution_memo.len() < MAX_ENDPOINT_RESOLUTION_MEMO_ENTRIES {
                    let ((endpoint_key, value), storage) = ctx.with_scoped_storage("catia_endpoint_memo_entries", || {
                        Ok::<_, CodecError>((ctx.copy_slice(pairs, "catia_endpoint_memo_key")?, copy_mesh_endpoint_resolution(ctx, &resolution)?))
                    })?;
                    let entry = endpoint_memo_storage.with_storage(|| ctx.entry_hash_map(&mut endpoint_resolution_memo, endpoint_key, "catia_endpoint_memo_entries"))?;
                    if let std::collections::hash_map::Entry::Vacant(entry) = entry {
                        entry.insert(ScopedValue { value, storage: Some(storage) });
                    }

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
            let (topology, assignment) = canonicalize_mesh_candidate_for_output(
                ctx,
                &topology,
                &assignment,
                candidate_gauge,
            )?
            .unwrap_or((topology, assignment));
            return Ok(MeshSolve::Solved((topology, assignment)));
        }
        let fallback = (|| -> Result<Option<MeshEndpointResolve>, CodecError> {
            let (assignments, _fallback_storage) =
                ctx.with_scoped_storage("catia_mesh_fallback_storage", || {
                    let mut assignments = Vec::new();
                    ctx.reserve_vec(
                        &mut assignments,
                        mesh_domains.len(),
                        "catia_mesh_fallback_assignment_rows",
                    )?;
                    for domain in
                        ctx.admit_iter(mesh_domains, "catia_mesh_fallback_assignment_rows")?
                    {
                        match domain {
                            MeshFaceBoundaryDomain::Ordered(face) => assignments.push(face),
                            MeshFaceBoundaryDomain::UnorderedFullCycle(_)
                            | MeshFaceBoundaryDomain::DeferredValidation(_) => return Ok(None),
                        }
                    }
                    Ok::<_, CodecError>(Some(assignments))
                })?;
            let Some(assignments) = assignments else {
                return Ok(None);
            };
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
    })()?;
    outcome.require_work(ctx, budget)
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
    let outcome = (|| -> Result<MeshFaceDomainCandidateSolve, CodecError> {
        let mut solution: Option<(Vec<[usize; 2]>, (StandardTopologyDraft, Vec<usize>))> = None;
        let mut rejection = None;
        let mut ambiguity = None;
        let mut exhaustion = None;
        let mut evaluate = |assignment: &[[usize; 2]]| -> Result<bool, CodecError> {
            if !budget.charge() {
                exhaustion = Some(MeshCandidateExhaustion::FaceDomainEnumeration);
                return Ok(false);
            }
            // A child of the same session charges the session as it works;
            // its consumption then moves into this budget's local count.
            let branch_budget = budget.session_child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
            let outcome = solve(assignment, &branch_budget)?;
            if budget.consume_child(&branch_budget).is_err() {
                exhaustion = Some(MeshCandidateExhaustion::FaceDomainEnumeration);
                return Ok(false);
            }
            match outcome {
                MeshSolve::Solved(candidate) => {
                    if let Some((_, stored)) = &solution {
                        if !mesh_candidates_identical_with_context(ctx, stored, &candidate)? {
                            ambiguity = Some(MeshCandidateAmbiguity::DistinctTopologySolutions);
                            return Ok(false);
                        }
                        return Ok(true);
                    }
                    solution = Some((
                        ctx.copy_slice(assignment, "catia_face_domain_solution_assignment")?,
                        candidate,
                    ));
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
                const OPERATION: &str = "catia_face_domain_assignments";
                let edge_count = assignments.first().map_or(0, Vec::len);
                if assignments.len() > MAX_FACE_DOMAIN_ASSIGNMENTS
                    || ctx.any_by(
                        assignments,
                        |assignment| {
                            Ok(assignment.len() != edge_count
                                || ctx.any_by(
                                    assignment,
                                    |faces| Ok(faces.iter().any(|face| *face >= face_count)),
                                    OPERATION,
                                )?)
                        },
                        OPERATION,
                    )?
                {
                    None
                } else if ctx.all_by(assignments, |assignment| evaluate(assignment), OPERATION)? {
                    Some(DuplicateFaceAssignmentVisit::Complete)
                } else {
                    Some(DuplicateFaceAssignmentVisit::Stopped)
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
            if let Some((faces, (topology, point_assignment))) = solution {
                MeshSolve::Solved((faces, topology, point_assignment))
            } else {
                MeshSolve::Failed(MeshCandidateFailure::Rejected(
                    rejection.unwrap_or(MeshCandidateRejection::InputStructure),
                ))
            },
        )
    })()?;
    outcome.require_work(ctx, budget)
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
            if limit.operation == "catia_endpoint_assignment_configuration"
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
    assert!(
        matches!(crate::test_support::with_work_refusal("catia_endpoint_cycle_next_states", boundary_result), Err(CodecError::ResourceLimit(limit)) if limit.operation == "catia_endpoint_cycle_next_states")
    );
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
        "catia_endpoint_assignment_configuration",
        "catia_endpoint_cycle_pair_map",
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
                partial_solution_valid: |_| Ok(true),
                complete_solution_valid: |_| Ok(true),
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
                    StandardTopologyDraft {
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
    let (limited_ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture input");
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
fn endpoint_configuration_relation_charges_covered_and_assigned_edges() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let edge_rows = (0..3)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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
            crate::solve::mesh_quotient::ResolveEndpointConfigurationRelationStreamingInputs {
                assignments: &assignments,
                endpoint_configurations: &configurations,
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
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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
            crate::solve::mesh_quotient::selection_search::ResolveSingletonMeshSelectionInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates: &candidates,
                selected: &selected,
                directions: &directions,
                port_identities: &identities,
                budget: &budget,
                candidate_gauge: None,
            },
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
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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
            crate::solve::mesh_quotient::ResolveStandardMeshEndpointCandidatesInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates: &candidates,
                assignments: assignments.clone(),
                port_identities: &[[0, 0], [1, 1]],
                prepared_quotient: None,
                edge_direction_evidence: None,
                budget: &budget,
                partial_solution_valid: None,
                complete_solution_valid: None,
                candidate_gauge: None,
                priority_edges: None,
            },
        )
    };
    catia_test_context!(service_ctx);
    run(&service_ctx).expect("service resource budget");

    let mut refused = HashSet::new();
    let mut limit = 0;
    let mut completed = false;
    for _ in 0..2048 {
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
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let candidates = vec![vec![[0, 1]]; edge_count];
    let identities = (0..edge_count)
        .map(|edge| {
            [
                u32::try_from(edge * 2).expect("fixture value fits u32"),
                u32::try_from(edge * 2 + 1).expect("fixture value fits u32"),
            ]
        })
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
                    if error.operation.starts_with("catia_endpoint_") =>
                {
                    low = mid + 1;
                }
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
            "catia_endpoint_relation_edge_faces",
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
        let memo_storage =
            RefCell::new(ctx.reserve_scoped(0, "catia_endpoint_relation_state_memo")?);
        let mut memo = HashMap::new();
        walk_endpoint_relation_domains(
            ctx,
            crate::solve::mesh_quotient::WalkEndpointRelationDomainsInputs {
                domains: vec![choices.clone()],
                face_assignments: &assignments,
                assigned: vec![None],
                constraints: &constraints,
                point_count: 2,
                budget: &budget,
                state_memo: &mut memo,
                state_memo_storage: &memo_storage,
                candidate_gauge: None,
                priority_edges: Some(&[true]),
                partial_solution_valid: None,
                coordinate_domains: None,
                coordinate_budget: None,
                evaluate: &mut |_, _| Ok(false),
            },
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
        "catia_endpoint_relation_pair_values",
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
        "catia_face_configuration_boundary_results",
        "catia_face_configuration_combined_rows",
        "catia_face_configuration_next_combined",
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
fn endpoint_configuration_directions_refuse_before_direction_and_result_growth() {
    use std::collections::BTreeSet;

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
    let run = |ctx: &DecodeContext<'_>| {
        endpoint_configuration_directions(ctx, &assignment, &configuration)
    };
    crate::test_support::with_service_context(|ctx| {
        assert_eq!(
            run(ctx).expect("service budget").expect("directions").len(),
            1
        );
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
        "catia_endpoint_direction_step",
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
            &ctx,
            &assignment,
            Some(&budget),
            |_| {
                Some(MeshEndpointCandidates::Implicit(
                    MeshImplicitEdgeCandidates {
                        source: MeshImplicitEdgeCandidateSource::Cartesian {
                            domains: Rc::new(ScopedValue {
                                value: vec![vec![0, 1], vec![2, 3]],
                                storage: None,
                            }),
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
        )
        .expect("service resource budget"),
        None
    );
    assert!(budget.exhausted());
}

#[test]
fn endpoint_cycle_viability_refuses_adjacency_and_state_growth() {
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
                end: 0,
                reversed: None,
            },
        ]],
    };
    let candidates = [vec![[0, 1]], vec![[1, 0]]];
    let run = |ctx: &DecodeContext<'_>| {
        mesh_assignment_endpoint_cycles_viable_where(ctx, &assignment, &candidates, None, |_, _| {
            true
        })
    };
    assert_eq!(
        crate::test_support::with_service_context(run).expect("service budget"),
        Some(true)
    );
    let mut operations = BTreeSet::new();
    let mut completed = false;
    for cap in 0..=32 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => {
                operations.insert(limit.operation);
            }
            Ok(Some(true)) => {
                completed = true;
                break;
            }
            _ => panic!("unexpected endpoint viability result"),
        }
    }
    assert!(completed);
    for operation in [
        "catia_endpoint_viability_neighbors",
        "catia_endpoint_viability_prepared",
        "catia_endpoint_viability_states",
        "catia_endpoint_viability_next_states",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
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
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        use std::collections::HashSet;

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

#[cfg(test)]
#[test]
fn boundary_component_face_keys_refuse_unadmitted_scan() {
    let use_ = MeshBoundaryEdgeCandidate {
        edge: 0,
        start: 0,
        end: 1,
        reversed: None,
    };
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_]],
        },
    ])];
    let candidates = [Vec::new()];
    let refusal = crate::test_support::with_work_refusal("catia_component_face_key_scan", |ctx| {
        let mut quotient = MeshQuotient::new(vec![
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([0, 1])),
        ]);
        propagate_common_boundary_components(ctx, &domains, &candidates, &mut quotient)
    });
    assert!(matches!(
        refusal,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_component_face_key_scan"
    ));
}

#[cfg(test)]
#[test]
fn face_domain_local_work_exhaustion_propagates_resource_refusal() {
    crate::test_support::with_service_context(|ctx| {
        let budget = ctx.work_budget(0);
        let assignments = [vec![[0, 0]]];
        let CodecError::ResourceLimit(limit) =
            parse_standard_mesh_candidate_outcome_with_face_assignments(
                ctx,
                MeshFaceAssignmentCandidates::Concrete {
                    assignments: &assignments,
                    face_count: 1,
                },
                &budget,
                |_, _| panic!("refused branch must not run"),
            )
            .expect_err("local work refusal")
        else {
            panic!("resource refusal")
        };
        assert_eq!(limit.operation, "catia_mesh_topology_work");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[cfg(test)]
#[test]
fn mesh_work_guard_preserves_the_existing_session_refusal() {
    crate::test_support::with_work_limit(0, |ctx| {
        let CodecError::ResourceLimit(original) = ctx
            .charge_work(1, "catia_mesh_test_refusal")
            .expect_err("work refusal")
        else {
            panic!("resource refusal")
        };
        let budget = ctx.work_budget(0);
        let result = MeshSolve::<()>::Failed(MeshCandidateFailure::Exhausted(
            MeshCandidateExhaustion::EndpointResolution,
        ))
        .require_work(ctx, &budget);
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[cfg(test)]
#[test]
fn ordered_face_local_ceiling_refuses_constraint_propagation() {
    crate::test_support::with_service_context(|ctx| {
        let mut quotient =
            MeshQuotient::new((0..80).map(|_| Arc::new(HashSet::from([0]))).collect());
        let domains = [MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: Vec::new(),
            },
        ])];
        let candidates = vec![vec![[0, 0]]; 40];
        let budget = ctx.work_budget(1_000_000);
        let CodecError::ResourceLimit(limit) = propagate_common_ordered_face_quotients(
            ctx,
            &domains,
            &candidates,
            &mut quotient,
            &budget,
        )
        .expect_err("face work ceiling refuses") else {
            panic!("resource refusal")
        };
        assert_eq!(limit.operation, "catia_ordered_face_constraint_work");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[cfg(test)]
#[test]
fn boundary_component_exhausted_slice_refuses_instead_of_absence() {
    crate::test_support::with_service_context(|ctx| {
        let budget = ctx.work_budget(0);
        assert!(!budget.charge());
        let domain = MeshFaceBoundaryDomain::Ordered(Vec::new());
        let CodecError::ResourceLimit(limit) =
            advance_boundary_component_states(ctx, &domain, &[], &[], 128, &budget)
                .map(|value| value.is_some())
                .expect_err("component work ceiling refuses")
        else {
            panic!("resource refusal")
        };
        assert_eq!(limit.operation, "catia_boundary_component_work");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}
