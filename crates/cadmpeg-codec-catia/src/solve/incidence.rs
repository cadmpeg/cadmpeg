//! Incidence backtracking constraint solver for standard B-rep topology.
//!
//! Reconstructs face/edge incidence from serialized boundary domains.

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;

use crate::families::standard::topology::{
    incidence_cycles, reconstruct_incidence, solve_boundary_orientation_constraints, EdgeRow,
    StandardTopologyDraft,
};
use crate::solve::mesh_quotient::{
    initial_mesh_quotient, mesh_assignment_endpoint_cycle_support_by,
    mesh_assignment_endpoint_cycles_viable_by, mesh_assignment_endpoint_cycles_viable_where,
    mesh_face_endpoint_configurations, AssignmentOrder, MeshCandidateFailure,
    MeshCoordinateRootDomains, MeshEndpointCandidates, MeshEndpointPair,
    MeshEndpointSolutionFilter, MeshFaceEndpointConfigurations, MeshImplicitEdgeCandidates,
    MeshIncidenceBoundary, MeshPartialEndpointConstraint, MeshQuotient, MeshQuotientGaugeState,
    MeshSolve, MAX_FACE_ENDPOINT_CONFIGURATION_WORK, MAX_MESH_CONSTRAINT_OPERATIONS,
};
use crate::solve::missing_edge::{
    propagate_edge_port_points, same_unordered_pair, MeshBoundaryEdgeCandidate,
    MeshDeferredBoundaryCycle, MeshDeferredFaceBoundary, MeshFaceBoundaryAssignment,
    MeshFaceBoundaryDomain,
};
use crate::solve::union_find::UnionFind;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::ops::ControlFlow;
use std::sync::Arc;

type MeshEndpointSolutionVisitor<'a> =
    &'a mut dyn FnMut(&[MeshEndpointPair]) -> Result<ControlFlow<()>, CodecError>;
type DegreeSupportWitnesses = RefCell<HashMap<(usize, usize), Vec<(usize, [usize; 2])>>>;

fn copy_incidence_degree_rows(
    ctx: &DecodeContext<'_>,
    rows: &[BTreeMap<usize, u8>],
) -> Result<Vec<BTreeMap<usize, u8>>, CodecError> {
    let mut copy = Vec::new();
    ctx.reserve_vec(&mut copy, rows.len(), "catia_incidence_degree_copy_rows")?;
    for row in ctx.admit_iter(rows, "catia_incidence_degree_copy_rows")? {
        let mut copied_row = BTreeMap::new();
        for (&point, &degree) in ctx.admit_iter(row, "catia_incidence_degree_copy_entries")? {
            ctx.insert_btree_map(
                &mut copied_row,
                point,
                degree,
                "catia_incidence_degree_copy_entries",
            )?;
        }
        copy.push(copied_row);
    }
    Ok(copy)
}

fn copy_incidence_edge_rows(
    ctx: &DecodeContext<'_>,
    rows: &[EdgeRow],
) -> Result<Vec<EdgeRow>, CodecError> {
    let mut copy = Vec::new();
    ctx.reserve_vec(&mut copy, rows.len(), "catia_incidence_edge_copy_rows")?;
    for row in ctx.admit_iter(rows, "catia_incidence_edge_copy_rows")? {
        copy.push(row.clone_charged(ctx)?);
    }
    Ok(copy)
}

/// One single-pair candidate row per edge, held in scoped storage.
fn singleton_incidence_pairs<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    pairs: &[[usize; 2]],
) -> Result<
    (
        Vec<Vec<[usize; 2]>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("catia_incidence_singleton_rows", || {
        let mut singleton = Vec::new();
        ctx.reserve_vec(
            &mut singleton,
            pairs.len(),
            "catia_incidence_singleton_rows",
        )?;
        for &pair in ctx.admit_iter(pairs, "catia_incidence_singleton_rows")? {
            singleton.push(ctx.alloc_filled(1, pair, "catia_incidence_singleton_pair")?);
        }
        Ok::<_, CodecError>(singleton)
    })
}

fn prune_incidence_choices(
    ctx: &DecodeContext<'_>,
    choices: &mut [Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
) -> Result<Option<()>, CodecError> {
    prune_incidence_choices_with_explicit_support(
        ctx,
        choices,
        edge_faces,
        face_count,
        point_count,
        true,
    )
}

/// Prune endpoint pairs by face valency while retaining degree-one endpoints.
///
/// Mesh boundary domains may complete that support with an endpoint that is
/// not present in the explicit candidate set. The mesh-aware caller checks
/// those deferred supports after it has prepared the coordinate-root domains.
fn prune_incidence_choices_with_deferred_support(
    ctx: &DecodeContext<'_>,
    choices: &mut [Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
) -> Result<Option<()>, CodecError> {
    prune_incidence_choices_with_explicit_support(
        ctx,
        choices,
        edge_faces,
        face_count,
        point_count,
        false,
    )
}

fn prune_incidence_choices_with_explicit_support(
    ctx: &DecodeContext<'_>,
    choices: &mut [Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    explicit_support_complete: bool,
) -> Result<Option<()>, CodecError> {
    const VALIDATION: &str = "catia incidence choice validation";
    if choices.len() != edge_faces.len()
        || ctx.any_by(&*choices, |pairs| Ok(pairs.is_empty()), VALIDATION)?
        || ctx.any_by(
            edge_faces,
            |faces| Ok(faces[0] >= face_count || faces[1] >= face_count),
            VALIDATION,
        )?
        || ctx.any_by(
            &*choices,
            |pairs| {
                ctx.any_by(
                    pairs,
                    |pair| Ok(pair[0] >= point_count || pair[1] >= point_count),
                    VALIDATION,
                )
            },
            VALIDATION,
        )?
    {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "catia incidence pruning scratch")?;
    scratch.with_storage(|| {
        prune_incidence_choice_rounds(
            ctx,
            choices,
            edge_faces,
            face_count,
            explicit_support_complete,
        )
    })
}

/// The distinct faces of an edge, first face first.
fn unique_incidence_faces(faces: [usize; 2]) -> impl Iterator<Item = usize> {
    faces
        .into_iter()
        .enumerate()
        .filter_map(move |(rank, face)| (rank == 0 || face != faces[0]).then_some(face))
}

fn incidence_point_degree(
    ctx: &DecodeContext<'_>,
    degrees: &BTreeMap<usize, u8>,
    point: usize,
) -> Result<u8, CodecError> {
    Ok(ctx
        .get_btree_map(degrees, &point, "catia incidence degree lookups")?
        .copied()
        .unwrap_or(0))
}

/// Whether a pair keeps every point of the edge's faces at degree two or less.
fn pair_fits_degrees(
    ctx: &DecodeContext<'_>,
    degrees: &[BTreeMap<usize, u8>],
    faces: [usize; 2],
    pair: [usize; 2],
) -> Result<bool, CodecError> {
    for face in unique_incidence_faces(faces) {
        for (rank, &point) in pair.iter().enumerate() {
            let multiplicity = 1 + usize::from(rank == 0 && pair[0] == pair[1]);
            if usize::from(incidence_point_degree(ctx, &degrees[face], point)?) + multiplicity > 2 {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The ascending distinct points of candidate pairs.
fn incidence_choice_points(
    ctx: &DecodeContext<'_>,
    pairs: &[[usize; 2]],
) -> Result<Vec<usize>, CodecError> {
    const OPERATION: &str = "catia incidence choice points";
    let mut points = ctx.collect_vec(pairs.iter().flatten().copied(), OPERATION)?;
    ctx.sort_unstable_by(&mut points, |value| value, Ord::cmp, OPERATION)?;
    ctx.dedup_vec(&mut points, OPERATION)?;
    Ok(points)
}

/// Fixed-point pruning of endpoint choices by face valency. Each round
/// visits every open edge; a round that changes nothing ends the search.
fn prune_incidence_choice_rounds(
    ctx: &DecodeContext<'_>,
    choices: &mut [Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    explicit_support_complete: bool,
) -> Result<Option<()>, CodecError> {
    /// A pair may not leave a degree-one point whose only support is this edge.
    fn preserves_new_degree_support(
        ctx: &DecodeContext<'_>,
        supports: &[BTreeMap<usize, u32>],
        edge_support: &[usize],
        degrees: &[BTreeMap<usize, u8>],
        faces: [usize; 2],
        pair: [usize; 2],
    ) -> Result<bool, CodecError> {
        for face in unique_incidence_faces(faces) {
            for (rank, point) in pair.into_iter().enumerate() {
                if rank == 1 && point == pair[0] {
                    continue;
                }
                let selected_degree = 1 + u8::from(pair[0] == pair[1]);
                if incidence_point_degree(ctx, &degrees[face], point)? + selected_degree != 1 {
                    continue;
                }
                let support = ctx
                    .get_btree_map(&supports[face], &point, "catia incidence support lookups")?
                    .copied()
                    .unwrap_or(0);
                let own = ctx
                    .binary_search(edge_support, &point, "catia incidence support lookups")?
                    .is_ok();
                if support <= u32::from(own) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Narrows an edge's support points to those of its retained pairs; each
    /// dropped point loses one support on each face of the edge.
    fn retain_edge_support(
        ctx: &DecodeContext<'_>,
        supports: &mut [BTreeMap<usize, u32>],
        edge_support: &mut Vec<usize>,
        faces: [usize; 2],
        retained_pairs: &[[usize; 2]],
    ) -> Result<Option<()>, CodecError> {
        const OPERATION: &str = "catia incidence removed support points";
        let (retained, _retained_storage) =
            ctx.with_scoped_storage(OPERATION, || incidence_choice_points(ctx, retained_pairs))?;
        let mut consistent = true;
        ctx.retain_vec(
            edge_support,
            |point| {
                if !consistent || ctx.binary_search(&retained, point, OPERATION)?.is_ok() {
                    return Ok(true);
                }
                for face in unique_incidence_faces(faces) {
                    let Some(count) =
                        ctx.get_mut_btree_map(&mut supports[face], point, OPERATION)?
                    else {
                        consistent = false;
                        return Ok(true);
                    };
                    let Some(next) = count.checked_sub(1) else {
                        consistent = false;
                        return Ok(true);
                    };
                    *count = next;
                    if next == 0 {
                        ctx.remove_btree_map(&mut supports[face], point, OPERATION)?;
                    }
                }
                Ok(false)
            },
            OPERATION,
        )?;
        Ok(consistent.then_some(()))
    }

    let mut face_edges =
        ctx.collect_indexed_vec(face_count, "catia_incidence_face_edges", |_| Ok(Vec::new()))?;
    for (edge, &faces) in ctx
        .admit_iter(edge_faces, "catia incidence face edge lists")?
        .enumerate()
    {
        for face in unique_incidence_faces(faces) {
            ctx.push_vec(
                &mut face_edges[face],
                edge,
                "catia incidence face edge lists",
            )?;
        }
    }
    let mut fixed = ctx.alloc_filled(choices.len(), false, "catia_incidence_fixed_edges")?;
    let mut degrees = ctx.collect_indexed_vec(face_count, "catia_incidence_degrees", |_| {
        Ok(BTreeMap::<usize, u8>::new())
    })?;
    let mut edge_supports = ctx.collection_vec(choices.len(), "catia incidence edge supports")?;
    for pairs in ctx.admit_iter(&*choices, "catia incidence edge supports")? {
        edge_supports.push(incidence_choice_points(ctx, pairs)?);
    }
    let mut supports = ctx.collect_indexed_vec(face_count, "catia_incidence_supports", |_| {
        Ok(BTreeMap::<usize, u32>::new())
    })?;
    for (edge, points) in ctx
        .admit_iter(&edge_supports, "catia incidence point support counts")?
        .enumerate()
    {
        for face in unique_incidence_faces(edge_faces[edge]) {
            for &point in ctx.admit_iter(points, "catia incidence point support counts")? {
                let count = ctx
                    .entry_btree_map(
                        &mut supports[face],
                        point,
                        "catia incidence point support counts",
                    )?
                    .or_default();
                let Some(next) = count.checked_add(1) else {
                    return Ok(None);
                };
                *count = next;
            }
        }
    }
    loop {
        let mut changed = false;
        for edge in ctx.admit_iter(&(0..choices.len()), "catia_incidence_iteration")? {
            if fixed[edge] {
                continue;
            }
            let faces = edge_faces[edge];
            let before = choices[edge].len();
            ctx.retain_vec(
                &mut choices[edge],
                |pair| {
                    Ok(pair_fits_degrees(ctx, &degrees, faces, *pair)?
                        && (!explicit_support_complete
                            || preserves_new_degree_support(
                                ctx,
                                &supports,
                                &edge_supports[edge],
                                &degrees,
                                faces,
                                *pair,
                            )?))
                },
                "catia incidence retained choices",
            )?;
            changed |= choices[edge].len() != before;
            if retain_edge_support(
                ctx,
                &mut supports,
                &mut edge_supports[edge],
                faces,
                &choices[edge],
            )?
            .is_none()
            {
                return Ok(None);
            }
            let [pair] = choices[edge].as_slice() else {
                if choices[edge].is_empty() {
                    return Ok(None);
                }
                continue;
            };
            let pair = *pair;
            for face in unique_incidence_faces(faces) {
                for point in pair {
                    let degree = ctx
                        .entry_btree_map(
                            &mut degrees[face],
                            point,
                            "catia incidence endpoint degrees",
                        )?
                        .or_default();
                    let Some(next) = degree.checked_add(1) else {
                        return Ok(None);
                    };
                    *degree = next;
                }
            }
            if retain_edge_support(ctx, &mut supports, &mut edge_supports[edge], faces, &[])?
                .is_none()
            {
                return Ok(None);
            }
            fixed[edge] = true;
            changed = true;
        }
        if explicit_support_complete {
            for face in ctx.admit_iter(&(0..face_count), "catia_incidence_iteration")? {
                for (&point, &degree) in
                    ctx.admit_iter(&degrees[face], "catia incidence degree-one points")?
                {
                    if degree != 1 {
                        continue;
                    }
                    let support = ctx
                        .get_btree_map(&supports[face], &point, "catia incidence support lookups")?
                        .copied()
                        .unwrap_or(0);
                    match support {
                        0 => return Ok(None),
                        1 => {
                            let Some(edge) = ctx.find_by(
                                face_edges[face].iter().copied(),
                                |&edge| {
                                    Ok(ctx
                                        .binary_search(
                                            &edge_supports[edge],
                                            &point,
                                            "catia incidence sole supports",
                                        )?
                                        .is_ok())
                                },
                                "catia incidence sole supports",
                            )?
                            else {
                                return Ok(None);
                            };
                            let before = choices[edge].len();
                            ctx.retain_vec(
                                &mut choices[edge],
                                |pair| Ok(pair.contains(&point)),
                                "catia incidence sole support choices",
                            )?;
                            if choices[edge].is_empty() {
                                return Ok(None);
                            }
                            changed |= choices[edge].len() != before;
                            if retain_edge_support(
                                ctx,
                                &mut supports,
                                &mut edge_supports[edge],
                                edge_faces[edge],
                                &choices[edge],
                            )?
                            .is_none()
                            {
                                return Ok(None);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if !changed {
            return Ok(Some(()));
        }
    }
}

/// Appends the edge of every use in the boundary assignments, in use order.
fn push_assignment_edges(
    ctx: &DecodeContext<'_>,
    assignments: &[MeshFaceBoundaryAssignment],
    edges: &mut Vec<usize>,
    operation: &'static str,
) -> Result<(), CodecError> {
    for assignment in ctx.admit_iter(assignments, operation)? {
        for boundary in ctx.admit_iter(&assignment.boundaries, operation)? {
            ctx.reserve_vec(edges, boundary.len(), operation)?;
            for use_ in ctx.admit_iter(boundary, operation)? {
                edges.push(use_.edge);
            }
        }
    }
    Ok(())
}

/// Appends a deferred face's missing edges, then the edge of each exact use.
fn push_deferred_edges(
    ctx: &DecodeContext<'_>,
    domain: &MeshDeferredFaceBoundary,
    edges: &mut Vec<usize>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.extend_vec(edges, &domain.missing_edges, operation)?;
    for cycle in ctx.admit_iter(&domain.cycles, operation)? {
        ctx.reserve_vec(edges, cycle.exact_uses.len(), operation)?;
        for (use_, _) in ctx.admit_iter(&cycle.exact_uses, operation)? {
            edges.push(use_.edge);
        }
    }
    Ok(())
}

/// Groups the open edges whose choices can interact: edges that share a face
/// endpoint class, a boundary cycle, or a quotient point. Components are in
/// first-edge order and list their edges in ascending order.
fn incidence_choice_components<'storage>(
    ctx: &'storage DecodeContext<'_>,
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    boundary_domains: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient<'storage>>,
) -> Result<Vec<Vec<usize>>, CodecError> {
    let is_open = |edge: usize| {
        choices
            .get(edge)
            .is_some_and(|pairs| pairs.len() > 1 || (pairs.is_empty() && mesh_quotient.is_some()))
    };
    let mut union = UnionFind::charged(ctx, choices.len(), "catia_incidence_choice_union")?;
    let mut scratch = ctx.reserve_scoped(0, "catia incidence component scratch")?;
    let mut point_nodes = HashMap::<(usize, usize), usize>::new();
    for (edge, pairs) in ctx
        .admit_iter(choices, "catia_incidence_point_nodes")?
        .enumerate()
    {
        for face in unique_incidence_faces(edge_faces[edge]) {
            for &point in ctx
                .admit_iter(pairs, "catia_incidence_point_nodes")?
                .flatten()
            {
                let next = point_nodes.len();
                scratch.with_storage(|| {
                    ctx.entry_hash_map(
                        &mut point_nodes,
                        (face, point),
                        "catia_incidence_point_nodes",
                    )
                    .map(|entry| {
                        entry.or_insert(next);
                    })
                })?;
            }
        }
    }
    let node = |face: usize, point: usize| -> Result<usize, CodecError> {
        ctx.get_hash_map(&point_nodes, &(face, point), "catia_incidence_point_nodes")?
            .copied()
            .ok_or_else(|| CodecError::malformed("incidence point node is missing"))
    };
    let mut fixed_incidence =
        UnionFind::charged(ctx, point_nodes.len(), "catia_incidence_fixed_union")?;
    for (edge, pairs) in ctx
        .admit_iter(choices, "catia_incidence_fixed_pairs")?
        .enumerate()
    {
        let [pair] = pairs.as_slice() else {
            continue;
        };
        for face in unique_incidence_faces(edge_faces[edge]) {
            fixed_incidence.union(ctx, node(face, pair[0])?, node(face, pair[1])?)?;
        }
    }
    let mut ambiguous = Vec::new();
    for edge in ctx.admit_iter(&(0..choices.len()), "catia_incidence_ambiguous_edges")? {
        if is_open(edge) {
            scratch.with_storage(|| {
                ctx.push_vec(&mut ambiguous, edge, "catia_incidence_ambiguous_edges")
            })?;
        }
    }
    let mut owner = HashMap::<(usize, usize), usize>::new();
    for &edge in ctx.admit_iter(&ambiguous, "catia_incidence_choice_owner")? {
        for face in unique_incidence_faces(edge_faces[edge]) {
            for &point in ctx
                .admit_iter(&choices[edge], "catia_incidence_choice_owner")?
                .flatten()
            {
                let point = fixed_incidence.find(ctx, node(face, point)?)?;
                if let Some(&previous) =
                    ctx.get_hash_map(&owner, &(face, point), "catia_incidence_choice_owner")?
                {
                    union.union(ctx, previous, edge)?;
                } else {
                    scratch.with_storage(|| {
                        ctx.insert_hash_map(
                            &mut owner,
                            (face, point),
                            edge,
                            "catia_incidence_choice_owner",
                        )
                    })?;
                }
            }
        }
    }
    if let Some(domains) = boundary_domains {
        const OPERATION: &str = "catia_incidence_boundary_edges";
        let mut connect = |edges: &mut Vec<usize>| -> Result<(), CodecError> {
            ctx.sort_unstable_by(edges, |value| value, Ord::cmp, OPERATION)?;
            ctx.dedup_vec(edges, OPERATION)?;
            let mut first = None;
            for &edge in ctx.admit_iter(&*edges, OPERATION)? {
                if !is_open(edge) {
                    continue;
                }
                match first {
                    Some(first) => union.union(ctx, first, edge)?,
                    None => first = Some(edge),
                }
            }
            Ok(())
        };
        for domain in ctx.admit_iter(domains, OPERATION)? {
            match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) if assignments.len() == 1 => {
                    for boundary in ctx.admit_iter(&assignments[0].boundaries, OPERATION)? {
                        let (mut edges, _edges_storage) = ctx
                            .with_scoped_storage(OPERATION, || {
                                ctx.collect_vec(boundary.iter().map(|use_| use_.edge), OPERATION)
                            })?;
                        connect(&mut edges)?;
                    }
                }
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    let (mut edges, _edges_storage) = ctx.with_scoped_storage(OPERATION, || {
                        let mut edges = Vec::new();
                        push_assignment_edges(ctx, assignments, &mut edges, OPERATION)?;
                        Ok::<_, CodecError>(edges)
                    })?;
                    connect(&mut edges)?;
                }
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                    let (mut edges, _edges_storage) =
                        ctx.with_scoped_storage(OPERATION, || ctx.copy_slice(edges, OPERATION))?;
                    connect(&mut edges)?;
                }
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    let (mut edges, _edges_storage) = ctx.with_scoped_storage(OPERATION, || {
                        let mut edges = Vec::new();
                        push_deferred_edges(ctx, domain, &mut edges, OPERATION)?;
                        Ok::<_, CodecError>(edges)
                    })?;
                    connect(&mut edges)?;
                }
            }
        }
    }
    if let Some(mesh_quotient) = mesh_quotient {
        if choices.len().checked_mul(2) == Some(mesh_quotient.len()) {
            const OPERATION: &str = "catia_incidence_quotient_owner";
            let mut point_owner = HashMap::<usize, usize>::new();
            for &edge in ctx.admit_iter(&ambiguous, OPERATION)? {
                for port in [edge * 2, edge * 2 + 1] {
                    let root = mesh_quotient.root(ctx, port)?;
                    let domain = &mesh_quotient.domains()[root];
                    ctx.charge_work(u64_from_index(domain.len()), OPERATION)?;
                    for &point in domain.iter() {
                        if let Some(&previous) =
                            ctx.get_hash_map(&point_owner, &point, OPERATION)?
                        {
                            union.union(ctx, previous, edge)?;
                        } else {
                            scratch.with_storage(|| {
                                ctx.insert_hash_map(&mut point_owner, point, edge, OPERATION)
                            })?;
                        }
                    }
                }
            }
        }
    }
    // Groups open in first-edge order, so the components need no sort.
    let mut group_of_root = scratch.with_storage(|| {
        ctx.alloc_filled(choices.len(), None, "catia_incidence_component_roots")
    })?;
    let mut components: Vec<Vec<usize>> = Vec::new();
    for &edge in ctx.admit_iter(&ambiguous, "catia_incidence_component_edges")? {
        let root = union.find(ctx, edge)?;
        let group = if let Some(group) = group_of_root[root] {
            group
        } else {
            let group = components.len();
            ctx.push_vec(&mut components, Vec::new(), "catia_incidence_components")?;
            group_of_root[root] = Some(group);
            group
        };
        ctx.push_vec(
            &mut components[group],
            edge,
            "catia_incidence_component_edges",
        )?;
    }
    Ok(components)
}

/// Merge components whose assignments participate in one shared partial
/// constraint. Evaluation-order constraints stay as edges between components
/// so independent domains do not inherit each other's branch alternatives.
fn join_incidence_components_by_coupling(
    ctx: &DecodeContext<'_>,
    components: Vec<Vec<usize>>,
    coupled_edges: &[bool],
) -> Result<Vec<Vec<usize>>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia incidence coupling scratch")?;
    let mut component_of_edge = scratch.with_storage(|| {
        ctx.alloc_filled(coupled_edges.len(), None, "catia_incidence_component_index")
    })?;
    for (component, edges) in ctx
        .admit_iter(&components, "catia_incidence_component_index")?
        .enumerate()
    {
        for &edge in ctx.admit_iter(edges, "catia_incidence_component_index")? {
            if let Some(slot) = component_of_edge.get_mut(edge) {
                *slot = Some(component);
            }
        }
    }
    let mut union = UnionFind::charged(ctx, components.len(), "catia_incidence_coupling_union")?;
    let mut coupled_owner = None;
    for (edge, &active) in ctx
        .admit_iter(coupled_edges, "catia_incidence_coupled_edges")?
        .enumerate()
    {
        let Some(component) = component_of_edge[edge].filter(|_| active) else {
            continue;
        };
        if let Some(owner) = coupled_owner {
            union.union(ctx, owner, component)?;
        } else {
            coupled_owner = Some(component);
        }
    }
    // Groups open in the order of their first component.
    let mut group_of_root = scratch.with_storage(|| {
        ctx.alloc_filled(components.len(), None, "catia_incidence_joined_roots")
    })?;
    let mut joined: Vec<Vec<usize>> = Vec::new();
    for (index, mut component) in ctx
        .admit_iter(components, "catia_incidence_joined_groups")?
        .enumerate()
    {
        let root = union.find(ctx, index)?;
        if let Some(group) = group_of_root[root] {
            ctx.append_vec(
                &mut joined[group],
                &mut component,
                "catia_incidence_joined_edges",
            )?;
        } else {
            group_of_root[root] = Some(joined.len());
            ctx.push_vec(&mut joined, component, "catia_incidence_joined_components")?;
        }
    }
    for edges in ctx.admit_iter(&mut joined, "catia_incidence_joined_edges_sort")? {
        ctx.sort_unstable_by(
            edges,
            |value| value,
            Ord::cmp,
            "catia_incidence_joined_edges_sort",
        )?;
    }
    Ok(joined)
}

/// Narrowest-first order of a component: an overflowing branch width sorts
/// last, then branch width, edge count and first edge.
type ComponentOrderKey = (bool, Option<usize>, usize, usize);

fn component_order_key(
    ctx: &DecodeContext<'_>,
    component: &[usize],
    choices: &[Vec<[usize; 2]>],
) -> Result<ComponentOrderKey, CodecError> {
    let mut width = 1usize;
    let bounded = ctx.all_by(
        component,
        |edge| {
            Ok(match width.checked_mul(choices[*edge].len()) {
                Some(next) => {
                    width = next;
                    true
                }
                None => false,
            })
        },
        "catia incidence component branch width",
    )?;
    let width = bounded.then_some(width);
    Ok((
        width.is_none(),
        width,
        component.len(),
        component.first().copied().unwrap_or_default(),
    ))
}

fn components_reference_unknown_edges(
    ctx: &DecodeContext<'_>,
    components: &[Vec<usize>],
    edge_count: usize,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia incidence component validation";
    ctx.any_by(
        components,
        |edges| ctx.any_by(edges, |edge| Ok(*edge >= edge_count), OPERATION),
        OPERATION,
    )
}

fn order_incidence_components_by_branch_width(
    ctx: &DecodeContext<'_>,
    components: &mut [Vec<usize>],
    choices: &[Vec<[usize; 2]>],
) -> Result<Option<()>, CodecError> {
    const OPERATION: &str = "catia incidence component branch width sort";
    if components_reference_unknown_edges(ctx, components, choices.len())? {
        return Ok(None);
    }
    let (mut keyed, _keyed_storage) = ctx.with_scoped_storage(OPERATION, || {
        let mut keyed = ctx.collection_vec(components.len(), OPERATION)?;
        for component in ctx.admit_iter(&mut *components, OPERATION)? {
            keyed.push((
                component_order_key(ctx, component, choices)?,
                std::mem::take(component),
            ));
        }
        Ok::<_, CodecError>(keyed)
    })?;
    ctx.stable_sort_by_key(&mut keyed, |value| value.0, Ord::cmp, OPERATION)?;
    for (slot, (_, component)) in ctx.admit_iter(&mut *components, OPERATION)?.zip(keyed) {
        *slot = component;
    }
    Ok(Some(()))
}

/// Order incidence components while preserving prerequisites between their
/// assignments. A prerequisite is an evaluation order, not an incidence
/// relation: joining the two components would make an otherwise independent
/// search share every branch. Components in a prerequisite cycle have no
/// valid order and are rejected by the caller.
fn order_incidence_components_by_constraints(
    ctx: &DecodeContext<'_>,
    components: &mut Vec<Vec<usize>>,
    choices: &[Vec<[usize; 2]>],
    assignment_order: Option<AssignmentOrder<'_>>,
) -> Result<Option<()>, CodecError> {
    const VALIDATION: &str = "catia incidence component order validation";
    const OPERATION: &str = "catia incidence component order";
    let assignment_predecessors = assignment_order.and_then(AssignmentOrder::predecessors);
    let assignment_dependencies = assignment_order.and_then(AssignmentOrder::dependencies);
    if components_reference_unknown_edges(ctx, components, choices.len())? {
        return Ok(None);
    }
    if let Some(predecessors) = assignment_predecessors {
        if predecessors.len() != choices.len()
            || ctx.any_by(
                predecessors,
                |predecessor| Ok(predecessor.is_some_and(|edge| edge >= choices.len())),
                VALIDATION,
            )?
        {
            return Ok(None);
        }
    }
    if let Some(dependencies) = assignment_dependencies {
        if dependencies.len() != choices.len()
            || components_reference_unknown_edges(ctx, dependencies, choices.len())?
        {
            return Ok(None);
        }
    }
    if assignment_order.is_none() {
        return order_incidence_components_by_branch_width(ctx, components, choices);
    }

    let mut scratch = ctx.reserve_scoped(0, "catia incidence component order scratch")?;
    let mut component_of_edge = scratch.with_storage(|| {
        ctx.alloc_filled(
            choices.len(),
            None,
            "catia incidence component edge indices",
        )
    })?;
    for (component, edges) in ctx.admit_iter(&*components, OPERATION)?.enumerate() {
        for &edge in ctx.admit_iter(edges, OPERATION)? {
            if component_of_edge[edge].replace(component).is_some() {
                return Ok(None);
            }
        }
    }
    let mut incoming = scratch.with_storage(|| {
        ctx.alloc_filled(components.len(), 0usize, "catia_incidence_component_in")
    })?;
    let mut outgoing = scratch.with_storage(|| {
        ctx.collect_indexed_vec(components.len(), "catia_incidence_component_out", |_| {
            Ok(BTreeSet::<usize>::new())
        })
    })?;
    let mut local_incoming = scratch
        .with_storage(|| ctx.alloc_filled(choices.len(), 0usize, "catia_incidence_local_in"))?;
    let mut local_outgoing = scratch.with_storage(|| {
        ctx.collect_indexed_vec(choices.len(), "catia_incidence_local_out", |_| {
            Ok(BTreeSet::<usize>::new())
        })
    })?;
    let mut add_dependency =
        |target_edge: usize, prerequisite_edge: usize| -> Result<(), CodecError> {
            let (Some(target_component), Some(prerequisite_component)) = (
                component_of_edge[target_edge],
                component_of_edge[prerequisite_edge],
            ) else {
                return Ok(());
            };
            if target_component == prerequisite_component {
                if scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut local_outgoing[prerequisite_edge],
                        target_edge,
                        "catia incidence local dependents",
                    )
                })? {
                    local_incoming[target_edge] += 1;
                }
                return Ok(());
            }
            if scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut outgoing[prerequisite_component],
                    target_component,
                    "catia incidence component dependents",
                )
            })? {
                incoming[target_component] += 1;
            }
            Ok(())
        };
    if let Some(predecessors) = assignment_predecessors {
        for (target, prerequisite) in ctx.admit_iter(predecessors, OPERATION)?.enumerate() {
            if let Some(prerequisite) = *prerequisite {
                add_dependency(target, prerequisite)?;
            }
        }
    }
    if let Some(dependencies) = assignment_dependencies {
        for (target, prerequisites) in ctx.admit_iter(dependencies, OPERATION)?.enumerate() {
            for &prerequisite in ctx.admit_iter(prerequisites, OPERATION)? {
                add_dependency(target, prerequisite)?;
            }
        }
    }
    let mut local_ready = Vec::new();
    let mut edge_count = 0usize;
    for edges in ctx.admit_iter(&*components, OPERATION)? {
        for &edge in ctx.admit_iter(edges, OPERATION)? {
            edge_count += 1;
            if local_incoming[edge] == 0 {
                scratch.with_storage(|| {
                    ctx.push_vec(&mut local_ready, edge, "catia incidence local ready edges")
                })?;
            }
        }
    }
    let mut local_ordered = 0usize;
    while let Some(edge) = local_ready.pop() {
        ctx.charge_work(1, "catia_incidence_iteration")?;
        local_ordered += 1;
        for &dependent in ctx.admit_iter(&local_outgoing[edge], OPERATION)? {
            local_incoming[dependent] -= 1;
            if local_incoming[dependent] == 0 {
                scratch.with_storage(|| {
                    ctx.push_vec(
                        &mut local_ready,
                        dependent,
                        "catia incidence local ready edges",
                    )
                })?;
            }
        }
    }
    if local_ordered != edge_count {
        return Ok(None);
    }

    // Kahn's order that always takes the narrowest ready component.
    let mut keys = scratch.with_storage(|| {
        ctx.collection_vec(components.len(), "catia incidence component order keys")
    })?;
    for component in ctx.admit_iter(&*components, OPERATION)? {
        keys.push(component_order_key(ctx, component, choices)?);
    }
    let mut ready = BTreeSet::new();
    for component in ctx.admit_iter(&(0..components.len()), OPERATION)? {
        if incoming[component] == 0 {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut ready,
                    (keys[component], component),
                    "catia incidence ready components",
                )
            })?;
        }
    }
    let mut ordered = Vec::new();
    while let Some(&next) = ready.first() {
        ctx.remove_btree_set(&mut ready, &next, "catia incidence ready components")?;
        let (_, component) = next;
        ctx.push_vec(
            &mut ordered,
            std::mem::take(&mut components[component]),
            "catia incidence ordered components",
        )?;
        for &dependent in ctx.admit_iter(&outgoing[component], OPERATION)? {
            incoming[dependent] -= 1;
            if incoming[dependent] == 0 {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut ready,
                        (keys[dependent], dependent),
                        "catia incidence ready components",
                    )
                })?;
            }
        }
    }
    if ordered.len() != components.len() {
        return Ok(None);
    }
    *components = ordered;
    Ok(Some(()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncidenceSearchState {
    Open,
    Exhausted,
    Stopped,
}

enum IncidenceVisitError {
    Exhausted,
    Resource(CodecError),
}

impl From<CodecError> for IncidenceVisitError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for IncidenceVisitError {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(limit.into())
    }
}

struct IncidenceComponentSearch<'a, 'v> {
    ctx: &'a DecodeContext<'a>,
    pub(crate) choices: &'a [Vec<[usize; 2]>],
    pub(crate) explicit_point_supports: Vec<HashMap<usize, Vec<[usize; 2]>>>,
    pub(crate) point_support_edges: Vec<HashMap<usize, Vec<usize>>>,
    pub(crate) degree_support_witnesses: DegreeSupportWitnesses,
    pub(crate) edge_faces: &'a [[usize; 2]],
    pub(crate) face_edges: &'a [Vec<usize>],
    pub(crate) mesh_assignments: Option<&'a [MeshFaceBoundaryDomain]>,
    pub(crate) face_configuration_domains: Option<PreparedFaceFactors>,
    pub(crate) coordinate_domains: Option<&'a MeshCoordinateRootDomains>,
    pub(crate) active: Vec<bool>,
    pub(crate) edges: &'a [usize],
    pub(crate) constraints: Vec<(usize, usize)>,
    pub(crate) assignment: Vec<Option<[usize; 2]>>,
    pub(crate) degrees: Vec<BTreeMap<usize, u8>>,
    pub(crate) solutions: Vec<Vec<(usize, [usize; 2])>>,
    pub(crate) solution_filter: Option<MeshEndpointSolutionFilter<'a>>,
    pub(crate) solution_visitor: Option<MeshEndpointSolutionVisitor<'v>>,
    pub(crate) partial_solution_filter: Option<MeshPartialEndpointConstraint<'a>>,
    pub(crate) dead_states: HashSet<Vec<Option<[usize; 2]>>>,
    pub(crate) budget: &'a WorkBudget<'a>,
    pub(crate) degree_support_budget: &'a WorkBudget<'a>,
    pub(crate) coordinate_propagation_budget: &'a WorkBudget<'a>,
    pub(crate) boundary_propagation_budget: &'a WorkBudget<'a>,
    pub(crate) state: IncidenceSearchState,
    search_storage: RefCell<cadmpeg_core::decode::ScopedReservation<'a>>,
}

enum IncidenceBranch {
    Options(std::vec::IntoIter<(usize, [usize; 2])>),
    Implicit {
        edge: usize,
        candidates: MeshImplicitEdgeCandidates,
    },
    Complete(Vec<(usize, [usize; 2])>),
}

enum IncidenceCandidatePairs<'a> {
    Options {
        candidates: &'a [[usize; 2]],
        next_index: usize,
    },
    Implicit(MeshImplicitEdgeCandidates),
}

enum IncidenceConstraintOptions {
    Unsupported,
    Deferred,
    AtLeastLimit,
    Exact(Vec<MeshEndpointPair>),
}

struct AppliedFaceConfiguration<'storage> {
    assigned: Vec<(usize, [usize; 2], IncidenceDegreeUndo)>,
    affected_faces: Vec<usize>,
    coordinate_domains: Option<Arc<MeshCoordinateRootDomains>>,
    factor_checkpoint: Option<MaskUndo<'storage>>,
    /// Holds the assignment and face lists until the configuration is undone.
    _storage: cadmpeg_core::decode::ScopedReservation<'storage>,
}

/// The previous degrees of at most two faces times two points.
struct IncidenceDegreeUndo {
    entries: [(usize, usize, Option<u8>); 4],
    len: usize,
}

/// Previous words of changed configuration masks, newest last, held in
/// scoped storage until the masks are restored.
struct MaskUndo<'storage> {
    entries: Vec<(usize, usize, u64)>,
    storage: cadmpeg_core::decode::ScopedReservation<'storage>,
}

impl<'storage> MaskUndo<'storage> {
    fn new(ctx: &'storage DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError> {
        Ok(Self {
            entries: Vec::new(),
            storage: ctx.reserve_scoped(0, operation)?,
        })
    }

    /// Records a mask word before it changes.
    fn record(
        &mut self,
        ctx: &DecodeContext<'_>,
        mask: usize,
        word: usize,
        value: u64,
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "catia face configuration mask undo";
        let entries = &mut self.entries;
        self.storage
            .with_storage(|| ctx.push_vec(entries, (mask, word, value), OPERATION))
    }

    fn restore(self, ctx: &DecodeContext<'_>, active: &mut [Vec<u64>]) -> Result<(), CodecError> {
        for (mask, word, value) in ctx
            .admit_iter(self.entries, "catia face configuration mask restore")?
            .rev()
        {
            active[mask][word] = value;
        }
        Ok(())
    }
}

enum FaceFactorRefinement<'storage> {
    Rejected,
    Tracked(MaskUndo<'storage>),
}

struct FaceConfigurationDomain {
    width: usize,
    face: usize,
    configurations: MeshFaceEndpointConfigurations,
}

struct FaceFactorArc {
    left: usize,
    right: usize,
    supports: Vec<Vec<u64>>,
}

struct FaceFactorGraph {
    arcs: Vec<FaceFactorArc>,
    incoming: Vec<Vec<usize>>,
    domain_lengths: Vec<usize>,
}

struct PreparedFaceFactors {
    domains: Vec<Option<MeshFaceEndpointConfigurations>>,
    factor_faces: Vec<usize>,
    factor_by_face: Vec<Option<usize>>,
    factors_by_edge: Vec<Vec<usize>>,
    active: Vec<Vec<u64>>,
}

const MASK_WORD_BITS: usize = 64;

fn mask_word_count(len: usize) -> usize {
    len.div_ceil(MASK_WORD_BITS)
}

fn mask_bit(index: usize) -> u64 {
    1 << (index % MASK_WORD_BITS)
}

/// Sets every word of `mask` to select the first `len` configurations.
fn fill_configuration_mask(
    ctx: &DecodeContext<'_>,
    mask: &mut [u64],
    len: usize,
) -> Result<(), CodecError> {
    ctx.fill(mask, u64::MAX, "catia_face_config_full_mask")?;
    if let Some(last) = mask.last_mut() {
        let remainder = len % MASK_WORD_BITS;
        if remainder != 0 {
            *last = (1 << remainder) - 1;
        }
    }
    Ok(())
}

fn full_configuration_mask(ctx: &DecodeContext<'_>, len: usize) -> Result<Vec<u64>, CodecError> {
    let mut mask = ctx.alloc_filled(
        mask_word_count(len),
        u64::MAX,
        "catia_face_config_full_mask",
    )?;
    if let Some(last) = mask.last_mut() {
        let remainder = len % MASK_WORD_BITS;
        if remainder != 0 {
            *last = (1 << remainder) - 1;
        }
    }
    Ok(mask)
}

fn set_mask_bit<K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    map: &mut HashMap<K, Vec<u64>>,
    key: K,
    configuration: usize,
    word_count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let word = configuration / MASK_WORD_BITS;
    if let Some(mask) = ctx.get_mut_hash_map(map, &key, operation)? {
        mask[word] |= mask_bit(configuration);
    } else {
        let mut mask = ctx.alloc_filled(word_count, 0u64, operation)?;
        mask[word] |= mask_bit(configuration);
        ctx.insert_hash_map(map, key, mask, "catia face configuration mask keys")?;
    }
    Ok(())
}

fn configuration_mask_contains(mask: &[u64], index: usize) -> bool {
    mask.get(index / MASK_WORD_BITS)
        .is_some_and(|word| word & mask_bit(index) != 0)
}

fn active_configuration_count(ctx: &DecodeContext<'_>, mask: &[u64]) -> Result<usize, CodecError> {
    let mut count = 0usize;
    for word in ctx.admit_iter(mask, "catia face configuration active counts")? {
        count += index_from_u32(word.count_ones());
    }
    Ok(count)
}

fn mask_is_empty(ctx: &DecodeContext<'_>, mask: &[u64]) -> Result<bool, CodecError> {
    ctx.all_by(
        mask,
        |word| Ok(*word == 0),
        "catia face configuration empty masks",
    )
}

/// For one face's configurations: which configurations use each edge, and
/// which use each edge with each endpoint pair.
struct ConfigurationIndex {
    present: HashMap<usize, Vec<u64>>,
    matching: HashMap<(usize, [usize; 2]), Vec<u64>>,
    word_count: usize,
}

/// Indexes a face's configurations; `None` when the budget refuses.
fn index_configurations(
    ctx: &DecodeContext<'_>,
    domain: &MeshFaceEndpointConfigurations,
    budget: &WorkBudget<'_>,
) -> Result<Option<ConfigurationIndex>, CodecError> {
    let word_count = mask_word_count(domain.len());
    let mut index = ConfigurationIndex {
        present: HashMap::new(),
        matching: HashMap::new(),
        word_count,
    };
    for (configuration, candidate) in ctx
        .admit_iter(domain, "catia_face_config_index")?
        .enumerate()
    {
        // One local unit visits one configuration's edge/pair entry.
        for &(edge, pair) in candidate {
            if !budget.charge() {
                return Ok(None);
            }
            set_mask_bit(
                ctx,
                &mut index.present,
                edge,
                configuration,
                word_count,
                "catia_face_config_present",
            )?;
            set_mask_bit(
                ctx,
                &mut index.matching,
                (edge, pair),
                configuration,
                word_count,
                "catia_face_config_matching",
            )?;
        }
    }
    Ok(Some(index))
}

/// Narrows `mask`, which starts with every configuration of the indexed face,
/// to those that agree with `candidate` on each shared edge. Returns whether
/// any configuration remains, or `None` when the budget refuses.
fn restrict_to_compatible(
    ctx: &DecodeContext<'_>,
    candidate: &[MeshEndpointPair],
    right: &ConfigurationIndex,
    mask: &mut [u64],
    budget: &WorkBudget<'_>,
) -> Result<Option<bool>, CodecError> {
    const OPERATION: &str = "catia face configuration compatibility";
    let mut nonempty = !mask.is_empty();
    let mut exhausted = false;
    ctx.all_by(
        candidate,
        |&(edge, pair)| {
            let Some(edge_present) = ctx.get_hash_map(&right.present, &edge, OPERATION)? else {
                return Ok(true);
            };
            let edge_matching = ctx.get_hash_map(&right.matching, &(edge, pair), OPERATION)?;
            // One local unit updates one mask word; this pass visits every word.
            if !budget.charge_by(mask.len()) {
                exhausted = true;
                return Ok(false);
            }
            let mut remaining = 0;
            for (word, value) in mask.iter_mut().enumerate() {
                *value &= !edge_present[word] | edge_matching.map_or(0, |matching| matching[word]);
                remaining |= *value;
            }
            nonempty = remaining != 0;
            Ok(nonempty)
        },
        OPERATION,
    )?;
    Ok((!exhausted).then_some(nonempty))
}

/// For each face domain, the other domains that share one of its edges, in
/// ascending order.
fn configuration_neighbors(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceEndpointConfigurations],
) -> Result<Vec<Vec<usize>>, CodecError> {
    const OPERATION: &str = "catia face configuration neighbors";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut edge_sets = scratch.with_storage(|| ctx.collection_vec(domains.len(), OPERATION))?;
    let mut holders = BTreeMap::<usize, Vec<usize>>::new();
    for (index, domain) in ctx.admit_iter(domains, OPERATION)?.enumerate() {
        let edges = scratch.with_storage(|| {
            let mut edges = Vec::new();
            for configuration in ctx.admit_iter(domain, OPERATION)? {
                ctx.reserve_vec(&mut edges, configuration.len(), OPERATION)?;
                for &(edge, _) in ctx.admit_iter(configuration, OPERATION)? {
                    edges.push(edge);
                }
            }
            ctx.sort_unstable_by(&mut edges, |value| value, Ord::cmp, OPERATION)?;
            ctx.dedup_vec(&mut edges, OPERATION)?;
            for &edge in ctx.admit_iter(&edges, OPERATION)? {
                ctx.push_btree_group(&mut holders, edge, index, OPERATION, OPERATION)?;
            }
            Ok::<_, CodecError>(edges)
        })?;
        edge_sets.push(edges);
    }
    let mut neighbors = ctx.collection_vec(domains.len(), OPERATION)?;
    for (index, edges) in ctx.admit_iter(&edge_sets, OPERATION)?.enumerate() {
        let mut adjacent = Vec::new();
        for edge in ctx.admit_iter(edges, OPERATION)? {
            let Some(sharing) = ctx.get_btree_map(&holders, edge, OPERATION)? else {
                continue;
            };
            for &other in ctx.admit_iter(sharing, OPERATION)? {
                if other != index {
                    ctx.push_vec(&mut adjacent, other, OPERATION)?;
                }
            }
        }
        ctx.sort_unstable_by(&mut adjacent, |value| value, Ord::cmp, OPERATION)?;
        ctx.dedup_vec(&mut adjacent, OPERATION)?;
        neighbors.push(adjacent);
    }
    Ok(neighbors)
}

impl FaceFactorGraph {
    fn compile(
        ctx: &DecodeContext<'_>,
        domains: &[MeshFaceEndpointConfigurations],
        budget: &WorkBudget<'_>,
    ) -> Result<Option<Self>, CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "catia face factor indexes")?;
        let neighbors = scratch.with_storage(|| configuration_neighbors(ctx, domains))?;
        let mut right_indexes = scratch.with_storage(|| {
            ctx.collection_vec(domains.len(), "catia face factor right indexes")
        })?;
        for domain in ctx.admit_iter(domains, "catia face factor right indexes")? {
            let Some(index) = scratch.with_storage(|| index_configurations(ctx, domain, budget))?
            else {
                return Ok(None);
            };
            right_indexes.push(index);
        }
        let mut arcs = Vec::new();
        let mut incoming =
            ctx.collect_indexed_vec(domains.len(), "catia_face_factor_incoming", |_| {
                Ok(Vec::new())
            })?;
        for (left, adjacent) in ctx
            .admit_iter(&neighbors, "catia face factor arcs")?
            .enumerate()
        {
            for &right in ctx.admit_iter(adjacent, "catia face factor arcs")? {
                let mut supports =
                    ctx.collection_vec(domains[left].len(), "catia face factor support rows")?;
                for candidate in ctx.admit_iter(&domains[left], "catia face factor support rows")? {
                    let mut compatible = full_configuration_mask(ctx, domains[right].len())?;
                    if restrict_to_compatible(
                        ctx,
                        candidate,
                        &right_indexes[right],
                        &mut compatible,
                        budget,
                    )?
                    .is_none()
                    {
                        return Ok(None);
                    }
                    supports.push(compatible);
                }
                let arc = arcs.len();
                ctx.push_vec(
                    &mut arcs,
                    FaceFactorArc {
                        left,
                        right,
                        supports,
                    },
                    "catia face factor arcs",
                )?;
                ctx.push_vec(&mut incoming[right], arc, "catia face factor incoming arcs")?;
            }
        }
        let mut domain_lengths =
            ctx.collection_vec(domains.len(), "catia face factor domain lengths")?;
        for domain in ctx.admit_iter(domains, "catia face factor domain lengths")? {
            domain_lengths.push(domain.len());
        }
        Ok(Some(Self {
            arcs,
            incoming,
            domain_lengths,
        }))
    }

    fn full_state(&self, ctx: &DecodeContext<'_>) -> Result<Vec<Vec<u64>>, CodecError> {
        let mut rows =
            ctx.collection_vec(self.domain_lengths.len(), "catia face factor active rows")?;
        for &length in ctx.admit_iter(&self.domain_lengths, "catia face factor active rows")? {
            rows.push(full_configuration_mask(ctx, length)?);
        }
        Ok(rows)
    }

    /// Arc-consistency propagation from the queued arcs (every arc when
    /// `initial` is `None`). An arc already waiting in the queue is not queued
    /// twice. Returns `Some(false)` when a face loses every configuration and
    /// `None` when the budget refuses. Changed words are recorded in `undo`.
    fn propagate(
        &self,
        ctx: &DecodeContext<'_>,
        active: &mut [Vec<u64>],
        initial: Option<&[usize]>,
        budget: &WorkBudget<'_>,
        mut undo: Option<&mut MaskUndo<'_>>,
    ) -> Result<Option<bool>, CodecError> {
        const OPERATION: &str = "catia face factor propagation queue";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut queued =
            storage.with_storage(|| ctx.alloc_filled(self.arcs.len(), false, OPERATION))?;
        let mut queue = VecDeque::new();
        let mut enqueue = |queued: &mut [bool],
                           queue: &mut VecDeque<usize>,
                           arc: usize|
         -> Result<(), CodecError> {
            if !std::mem::replace(&mut queued[arc], true) {
                storage.with_storage(|| ctx.push_back(queue, arc, OPERATION))?;
            }
            Ok(())
        };
        match initial {
            None => {
                for arc in ctx.admit_iter(&(0..self.arcs.len()), OPERATION)? {
                    enqueue(&mut queued, &mut queue, arc)?;
                }
            }
            Some(arcs) => {
                for &arc in ctx.admit_iter(arcs, OPERATION)? {
                    enqueue(&mut queued, &mut queue, arc)?;
                }
            }
        }
        while let Some(arc_index) = queue.pop_front() {
            ctx.charge_work(1, "catia_incidence_iteration")?;
            queued[arc_index] = false;
            let arc = &self.arcs[arc_index];
            let mut changed = false;
            // One local unit visits one configuration. The overlap query
            // charges its visited mask words separately.
            for (configuration, supports) in arc.supports.iter().enumerate() {
                if !budget.charge() {
                    return Ok(None);
                }
                if !configuration_mask_contains(&active[arc.left], configuration) {
                    continue;
                }
                if ctx.any_by(
                    supports.iter().zip(&active[arc.right]),
                    |(supports, active)| Ok(supports & active != 0),
                    "catia face factor propagation",
                )? {
                    continue;
                }
                let word = configuration / MASK_WORD_BITS;
                if let Some(undo) = undo.as_deref_mut() {
                    undo.record(ctx, arc.left, word, active[arc.left][word])?;
                }
                active[arc.left][word] &= !mask_bit(configuration);
                changed = true;
            }
            if !changed {
                continue;
            }
            if mask_is_empty(ctx, &active[arc.left])? {
                return Ok(Some(false));
            }
            for &incoming in ctx.admit_iter(&self.incoming[arc.left], OPERATION)? {
                enqueue(&mut queued, &mut queue, incoming)?;
            }
        }
        Ok(Some(true))
    }
}

impl PreparedFaceFactors {
    #[cfg(test)]
    fn domains(&self) -> &[Option<MeshFaceEndpointConfigurations>] {
        &self.domains
    }

    /// Clears the configurations that disagree with the assigned pairs. The
    /// returned undo restores the masks; a rejected refinement is undone here.
    fn refine_edges<'storage>(
        &mut self,
        ctx: &'storage DecodeContext<'_>,
        assigned: &[(usize, [usize; 2])],
    ) -> Result<FaceFactorRefinement<'storage>, CodecError> {
        const OPERATION: &str = "catia face factor refinement";
        let active = &mut self.active;
        let mut undo = MaskUndo::new(ctx, "catia_face_factor_checkpoint_rows")?;
        let mut rejected = false;
        'assigned: for &(edge, pair) in ctx.admit_iter(assigned, OPERATION)? {
            let Some(factors) = self.factors_by_edge.get(edge) else {
                continue;
            };
            for &factor in ctx.admit_iter(factors, OPERATION)? {
                let face = self.factor_faces[factor];
                let Some(configurations) = self.domains.get(face).and_then(Option::as_ref) else {
                    rejected = true;
                    break 'assigned;
                };
                for (configuration, pairs) in ctx.admit_iter(configurations, OPERATION)?.enumerate()
                {
                    if !configuration_mask_contains(&active[factor], configuration)
                        || !ctx.any_by(
                            pairs,
                            |(candidate_edge, candidate_pair)| {
                                Ok(*candidate_edge == edge
                                    && !same_unordered_pair(*candidate_pair, pair))
                            },
                            OPERATION,
                        )?
                    {
                        continue;
                    }
                    let word = configuration / MASK_WORD_BITS;
                    undo.record(ctx, factor, word, active[factor][word])?;
                    active[factor][word] &= !mask_bit(configuration);
                }
                if mask_is_empty(ctx, &active[factor])? {
                    rejected = true;
                    break 'assigned;
                }
            }
        }
        if rejected {
            undo.restore(ctx, active)?;
            return Ok(FaceFactorRefinement::Rejected);
        }
        Ok(FaceFactorRefinement::Tracked(undo))
    }

    fn restore(
        &mut self,
        ctx: &DecodeContext<'_>,
        checkpoint: Option<MaskUndo<'_>>,
    ) -> Result<(), CodecError> {
        if let Some(checkpoint) = checkpoint {
            checkpoint.restore(ctx, &mut self.active)?;
        }
        Ok(())
    }

    fn face_has_active_configuration(
        &self,
        ctx: &DecodeContext<'_>,
        face: usize,
    ) -> Result<Option<bool>, CodecError> {
        let Some(active) = self
            .factor_by_face
            .get(face)
            .copied()
            .flatten()
            .and_then(|factor| self.active.get(factor))
        else {
            return Ok(None);
        };
        Ok(Some(!mask_is_empty(ctx, active)?))
    }

    fn face_candidate_has_active_configuration(
        &self,
        ctx: &DecodeContext<'_>,
        face: usize,
        edge: usize,
        pair: [usize; 2],
    ) -> Result<Option<bool>, CodecError> {
        const OPERATION: &str = "catia face factor candidate support";
        let Some(factor) = self.factor_by_face.get(face).copied().flatten() else {
            return Ok(None);
        };
        let Some(factors) = self.factors_by_edge.get(edge) else {
            return Ok(None);
        };
        if ctx.binary_search(factors, &factor, OPERATION)?.is_err() {
            return self.face_has_active_configuration(ctx, face);
        }
        let Some(active) = self.active.get(factor) else {
            return Ok(None);
        };
        let Some(configurations) = self.domains.get(face).and_then(Option::as_ref) else {
            return Ok(None);
        };
        Ok(Some(ctx.any_by(
            configurations.iter().enumerate(),
            |(configuration, pairs)| {
                Ok(configuration_mask_contains(active, configuration)
                    && ctx.any_by(
                        pairs,
                        |(candidate_edge, candidate_pair)| {
                            Ok(*candidate_edge == edge
                                && same_unordered_pair(*candidate_pair, pair))
                        },
                        OPERATION,
                    )?)
            },
            OPERATION,
        )?))
    }
}

fn retain_configuration_masks(
    ctx: &DecodeContext<'_>,
    domains: &mut [MeshFaceEndpointConfigurations],
    active: &[Vec<u64>],
) -> Result<(), CodecError> {
    const OPERATION: &str = "catia face configuration retained masks";
    for (domain, mask) in ctx.admit_iter(domains, OPERATION)?.zip(active) {
        let mut index = 0;
        ctx.retain_vec(
            domain,
            |_| {
                let retain = configuration_mask_contains(mask, index);
                index += 1;
                Ok(retain)
            },
            OPERATION,
        )?;
    }
    Ok(())
}

fn prune_face_configuration_support(
    ctx: &DecodeContext<'_>,
    domains: &mut [MeshFaceEndpointConfigurations],
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia face configuration queue";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let neighbors = scratch.with_storage(|| configuration_neighbors(ctx, domains))?;
    let mut queue = VecDeque::new();
    for (left, adjacent) in ctx.admit_iter(&neighbors, OPERATION)?.enumerate() {
        for &right in ctx.admit_iter(adjacent, OPERATION)? {
            scratch.with_storage(|| ctx.push_back(&mut queue, (left, right), OPERATION))?;
        }
    }
    while let Some((left, right)) = queue.pop_front() {
        ctx.charge_work(1, "catia_incidence_iteration")?;
        let (keep, _keep_storage) = ctx.with_scoped_storage(
            "catia face configuration keep marks",
            || -> Result<Option<Vec<bool>>, CodecError> {
                let Some(index) = index_configurations(ctx, &domains[right], budget)? else {
                    return Ok(None);
                };
                let mut viable =
                    ctx.alloc_filled(index.word_count, 0u64, "catia_face_config_viable")?;
                let mut keep =
                    ctx.collection_vec(domains[left].len(), "catia face configuration keep marks")?;
                for candidate in
                    ctx.admit_iter(&domains[left], "catia face configuration keep marks")?
                {
                    fill_configuration_mask(ctx, &mut viable, domains[right].len())?;
                    let Some(supported) =
                        restrict_to_compatible(ctx, candidate, &index, &mut viable, budget)?
                    else {
                        return Ok(None);
                    };
                    keep.push(supported);
                }
                Ok(Some(keep))
            },
        )?;
        let Some(keep) = keep else {
            return Ok(true);
        };
        if !ctx.any_by(&keep, |supported| Ok(*supported), OPERATION)? {
            return Ok(false);
        }
        if ctx.any_by(&keep, |supported| Ok(!*supported), OPERATION)? {
            let mut index = 0;
            ctx.retain_vec(
                &mut domains[left],
                |_| {
                    let retain = keep[index];
                    index += 1;
                    Ok(retain)
                },
                OPERATION,
            )?;
            for &neighbor in ctx.admit_iter(&neighbors[left], OPERATION)? {
                if neighbor != right {
                    scratch
                        .with_storage(|| ctx.push_back(&mut queue, (neighbor, left), OPERATION))?;
                }
            }
        }
    }
    Ok(true)
}

/// Singleton arc consistency: a configuration survives only when keeping it
/// alone on its face still leaves every face a configuration. Each trial
/// changes the masks in place and restores them from its undo record.
fn prune_face_configuration_singleton_support(
    ctx: &DecodeContext<'_>,
    domains: &mut [MeshFaceEndpointConfigurations],
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia face configuration singleton order";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let Some(graph) = scratch.with_storage(|| FaceFactorGraph::compile(ctx, domains, budget))?
    else {
        return Ok(true);
    };
    let mut active = scratch.with_storage(|| graph.full_state(ctx))?;
    match graph.propagate(ctx, &mut active, None, budget, None)? {
        Some(true) => {}
        Some(false) => return Ok(false),
        None => return Ok(true),
    }
    let mut counts = scratch.with_storage(|| ctx.alloc_filled(domains.len(), 0usize, OPERATION))?;
    let mut order = scratch.with_storage(|| ctx.alloc_filled(domains.len(), 0usize, OPERATION))?;
    loop {
        ctx.charge_work(1, "catia_incidence_iteration")?;
        let mut changed = false;
        for (domain, (count, slot)) in ctx
            .admit_iter(&mut counts, OPERATION)?
            .zip(order.iter_mut())
            .enumerate()
        {
            *count = active_configuration_count(ctx, &active[domain])?;
            *slot = domain;
        }
        ctx.sort_unstable_by_key(
            &mut order,
            |domain| counts[*domain],
            Ord::cmp,
            "catia face configuration singleton order sort",
        )?;
        for &domain in ctx.admit_iter(&order, OPERATION)? {
            let mut domain_changed = false;
            if active_configuration_count(ctx, &active[domain])? <= 1 {
                continue;
            }
            for configuration in ctx.admit_iter(&(0..domains[domain].len()), OPERATION)? {
                if !configuration_mask_contains(&active[domain], configuration) {
                    continue;
                }
                let mut undo = MaskUndo::new(ctx, "catia face configuration trial masks")?;
                for word in ctx.admit_iter(&(0..active[domain].len()), OPERATION)? {
                    undo.record(ctx, domain, word, active[domain][word])?;
                    active[domain][word] = if word == configuration / MASK_WORD_BITS {
                        mask_bit(configuration)
                    } else {
                        0
                    };
                }
                let outcome = graph.propagate(
                    ctx,
                    &mut active,
                    Some(&graph.incoming[domain]),
                    budget,
                    Some(&mut undo),
                )?;
                undo.restore(ctx, &mut active)?;
                match outcome {
                    Some(true) => {}
                    Some(false) => {
                        active[domain][configuration / MASK_WORD_BITS] &= !mask_bit(configuration);
                        changed = true;
                        domain_changed = true;
                    }
                    None => {
                        retain_configuration_masks(ctx, domains, &active)?;
                        return Ok(true);
                    }
                }
            }
            if mask_is_empty(ctx, &active[domain])? {
                return Ok(false);
            }
            if domain_changed {
                match graph.propagate(
                    ctx,
                    &mut active,
                    Some(&graph.incoming[domain]),
                    budget,
                    None,
                )? {
                    Some(true) => {}
                    Some(false) => return Ok(false),
                    None => {
                        retain_configuration_masks(ctx, domains, &active)?;
                        return Ok(true);
                    }
                }
            }
        }
        if !changed {
            retain_configuration_masks(ctx, domains, &active)?;
            return Ok(true);
        }
    }
}

/// An endpoint pair with its smaller point first.
fn canonical_pair(pair: [usize; 2]) -> [usize; 2] {
    [pair[0].min(pair[1]), pair[0].max(pair[1])]
}

/// Replaces an edge's candidate row with a narrower one in its own storage.
fn narrow_choice_row(
    ctx: &DecodeContext<'_>,
    row: &mut Vec<[usize; 2]>,
    retained: &[[usize; 2]],
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.clear_vec(row, operation)?;
    ctx.extend_vec(row, retained, operation)
}

fn prune_ordered_face_endpoint_support(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    choices: &mut [Vec<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia ordered face edges";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    // The edges of each ordered face do not change between rounds.
    let face_edges = scratch.with_storage(|| -> Result<Vec<Option<Vec<usize>>>, CodecError> {
        let mut face_edges = ctx.collection_vec(domains.len(), OPERATION)?;
        for domain in ctx.admit_iter(domains, OPERATION)? {
            face_edges.push(match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    let mut edges = Vec::new();
                    push_assignment_edges(ctx, assignments, &mut edges, OPERATION)?;
                    ctx.sort_unstable_by(&mut edges, |value| value, Ord::cmp, OPERATION)?;
                    ctx.dedup_vec(&mut edges, OPERATION)?;
                    Some(edges)
                }
                _ => None,
            });
        }
        Ok(face_edges)
    })?;
    let selected = scratch
        .with_storage(|| ctx.alloc_filled(choices.len(), None, "catia_ordered_face_selection"))?;
    loop {
        ctx.charge_work(1, "catia_incidence_iteration")?;
        let mut changed = false;
        for (domain, edges) in ctx.admit_iter(domains, OPERATION)?.zip(&face_edges) {
            let (MeshFaceBoundaryDomain::Ordered(assignments), Some(edges)) = (domain, edges)
            else {
                continue;
            };
            if ctx.any_by(
                edges,
                |edge| Ok(choices.get(*edge).is_none_or(Vec::is_empty)),
                OPERATION,
            )? {
                continue;
            }
            let (configurations, _configurations_storage) = ctx
                .with_scoped_storage("catia ordered face configurations", || {
                    mesh_face_endpoint_configurations(ctx, assignments, choices, &selected, budget)
                })?;
            let Some(configurations) = configurations else {
                if budget.exhausted() {
                    return Ok(true);
                }
                continue;
            };
            if configurations.is_empty() {
                return Ok(false);
            }
            let (supported, _supported_storage) = ctx.with_scoped_storage(
                "catia ordered face support edges",
                || -> Result<Option<HashMap<usize, HashSet<[usize; 2]>>>, CodecError> {
                    let mut supported = HashMap::<usize, HashSet<[usize; 2]>>::new();
                    for configuration in
                        ctx.admit_iter(&configurations, "catia ordered face support edges")?
                    {
                        for &(edge, pair) in configuration {
                            if !budget.charge() {
                                return Ok(None);
                            }
                            let pairs = ctx
                                .entry_hash_map(
                                    &mut supported,
                                    edge,
                                    "catia ordered face support edges",
                                )?
                                .or_default();
                            ctx.insert_hash_set(pairs, pair, "catia ordered face support pairs")?;
                        }
                    }
                    Ok(Some(supported))
                },
            )?;
            let Some(supported) = supported else {
                return Ok(true);
            };
            for &edge in ctx.admit_iter(edges, OPERATION)? {
                let Some(edge_supported) =
                    ctx.get_hash_map(&supported, &edge, "catia ordered face support edges")?
                else {
                    continue;
                };
                let (retained, _retained_storage) = ctx.with_scoped_storage(
                    "catia ordered face retained pairs",
                    || -> Result<Option<Vec<[usize; 2]>>, CodecError> {
                        let mut retained = Vec::new();
                        for &pair in &choices[edge] {
                            if !budget.charge() {
                                return Ok(None);
                            }
                            if ctx.contains_hash_set(
                                edge_supported,
                                &canonical_pair(pair),
                                "catia ordered face support pairs",
                            )? {
                                ctx.push_vec(
                                    &mut retained,
                                    pair,
                                    "catia ordered face retained pairs",
                                )?;
                            }
                        }
                        Ok(Some(retained))
                    },
                )?;
                let Some(retained) = retained else {
                    return Ok(true);
                };
                if retained.is_empty() {
                    return Ok(false);
                }
                if retained.len() != choices[edge].len() {
                    narrow_choice_row(
                        ctx,
                        &mut choices[edge],
                        &retained,
                        "catia ordered face retained pairs",
                    )?;
                    changed = true;
                }
            }
        }
        if !changed {
            return Ok(true);
        }
    }
}

pub(super) fn prune_implicit_ordered_face_endpoint_support(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    choices: &mut [Vec<[usize; 2]>],
    coordinate_domains: &MeshCoordinateRootDomains,
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia implicit face support edges";
    loop {
        ctx.charge_work(1, "catia_incidence_iteration")?;
        let mut changed = false;
        for domain in ctx.admit_iter(domains, OPERATION)? {
            let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                continue;
            };
            let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
            // The supports of the assignments that keep some endpoint pair.
            let mut supports = Vec::new();
            for assignment in ctx.admit_iter(assignments, OPERATION)? {
                let Some(support) = scratch.with_storage(|| {
                    mesh_assignment_endpoint_cycle_support_by(
                        ctx,
                        assignment,
                        Some(budget),
                        |edge| {
                            choices
                                .get(edge)
                                .filter(|values| !values.is_empty())
                                .map(|values| MeshEndpointCandidates::Explicit(values.as_slice()))
                                .or_else(|| {
                                    coordinate_domains
                                        .implicit_edge_candidates(edge, None)
                                        .map(MeshEndpointCandidates::Implicit)
                                })
                        },
                        |_, _| true,
                    )
                })?
                else {
                    return Ok(true);
                };
                if support.by_edge.is_empty() {
                    continue;
                }
                scratch.with_storage(|| {
                    ctx.push_vec(&mut supports, (assignment, support), OPERATION)
                })?;
            }
            if supports.is_empty() {
                return Ok(false);
            }
            let edges = scratch.with_storage(|| {
                let mut edges = Vec::new();
                for (assignment, _) in ctx.admit_iter(&supports, OPERATION)? {
                    push_assignment_edges(
                        ctx,
                        std::slice::from_ref(*assignment),
                        &mut edges,
                        OPERATION,
                    )?;
                }
                ctx.sort_unstable_by(&mut edges, |value| value, Ord::cmp, OPERATION)?;
                ctx.dedup_vec(&mut edges, OPERATION)?;
                Ok::<_, CodecError>(edges)
            })?;
            for &edge in ctx.admit_iter(&edges, OPERATION)? {
                if !ctx.any_by(
                    &supports,
                    |(_, support)| {
                        Ok(ctx
                            .get_hash_map(&support.by_edge, &edge, OPERATION)?
                            .is_some())
                    },
                    OPERATION,
                )? {
                    continue;
                }
                let Some(current) = choices.get(edge) else {
                    return Ok(false);
                };
                let (values, _values_storage) = ctx.with_scoped_storage(
                    "catia implicit face candidate pairs",
                    || -> Result<Option<Vec<[usize; 2]>>, CodecError> {
                        if current.is_empty() {
                            let Some(values) =
                                coordinate_domains.implicit_edge_candidates(edge, None)
                            else {
                                return Ok(None);
                            };
                            return Ok(Some(
                                ctx.collect_vec(values, "catia implicit face candidate pairs")?,
                            ));
                        }
                        Ok(Some(ctx.copy_slice(
                            current,
                            "catia implicit face current pairs",
                        )?))
                    },
                )?;
                let Some(values) = values else {
                    return Ok(false);
                };
                let mut retained = Vec::new();
                for &pair in &values {
                    if !budget.charge() {
                        return Ok(true);
                    }
                    let pair = canonical_pair(pair);
                    let supported = ctx.any_by(
                        &supports,
                        |(_, support)| {
                            Ok(
                                match ctx.get_hash_map(&support.by_edge, &edge, OPERATION)? {
                                    Some(pairs) => ctx.contains_hash_set(
                                        pairs,
                                        &pair,
                                        "catia implicit face support pairs",
                                    )?,
                                    None => false,
                                },
                            )
                        },
                        OPERATION,
                    )?;
                    if supported {
                        ctx.push_vec(&mut retained, pair, "catia implicit face retained pairs")?;
                    }
                }
                ctx.sort_unstable_by(
                    &mut retained,
                    |value| value,
                    Ord::cmp,
                    "catia implicit face retained pairs sort",
                )?;
                ctx.dedup_vec(&mut retained, "catia implicit face retained pairs")?;
                if retained.is_empty() {
                    return Ok(false);
                }
                if !ctx.equal(
                    &retained,
                    &choices[edge],
                    "catia implicit face retained pairs",
                )? {
                    choices[edge] = retained;
                    changed = true;
                }
            }
        }
        if !changed {
            return Ok(true);
        }
    }
}

fn prepare_face_configuration_domains(
    ctx: &DecodeContext<'_>,
    assignments: Option<&[MeshFaceBoundaryDomain]>,
    choices: &[Vec<[usize; 2]>],
    selected: &[Option<[usize; 2]>],
    active: &[bool],
) -> Result<Option<PreparedFaceFactors>, CodecError> {
    const OPERATION: &str = "catia face factor edges";
    let Some(assignments) = assignments else {
        return Ok(None);
    };
    let mut domains =
        ctx.collect_indexed_vec(assignments.len(), "catia_face_factor_domains", |_| Ok(None))?;
    for (face, domain) in ctx.admit_iter(assignments, OPERATION)?.enumerate() {
        let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
            continue;
        };
        let (edges, _edges_storage) = ctx.with_scoped_storage(OPERATION, || {
            let mut edges = Vec::new();
            push_assignment_edges(ctx, assignments, &mut edges, OPERATION)?;
            ctx.retain_vec(
                &mut edges,
                |edge| Ok(active.get(*edge) == Some(&true)),
                OPERATION,
            )?;
            ctx.sort_unstable_by(&mut edges, |value| value, Ord::cmp, OPERATION)?;
            ctx.dedup_vec(&mut edges, OPERATION)?;
            Ok::<_, CodecError>(edges)
        })?;
        if edges.is_empty()
            || ctx.any_by(
                &edges,
                |edge| {
                    Ok(selected
                        .get(*edge)
                        .is_none_or(|pair| pair.is_none() && choices[*edge].is_empty()))
                },
                OPERATION,
            )?
        {
            continue;
        }
        let budget = ctx.work_budget(u64_from_index(MAX_FACE_ENDPOINT_CONFIGURATION_WORK));
        let Some(configurations) =
            mesh_face_endpoint_configurations(ctx, assignments, choices, selected, &budget)?
        else {
            continue;
        };
        domains[face] = Some(configurations);
    }
    let mut retained_faces = Vec::new();
    for (face, domain) in ctx
        .admit_iter(&domains, "catia face factor retained faces")?
        .enumerate()
    {
        if domain.is_some() {
            ctx.push_vec(
                &mut retained_faces,
                face,
                "catia face factor retained faces",
            )?;
        }
    }
    let mut configurations =
        ctx.collection_vec(retained_faces.len(), "catia face factor configurations")?;
    for &face in ctx.admit_iter(&retained_faces, "catia face factor configurations")? {
        configurations.push(
            domains[face]
                .as_mut()
                .map(std::mem::take)
                .unwrap_or_default(),
        );
    }
    let arc_budget = ctx.work_budget(u64_from_index(MAX_MESH_CONSTRAINT_OPERATIONS));
    let viable = prune_face_configuration_support(ctx, &mut configurations, &arc_budget)?;
    let singleton_budget = ctx.work_budget(u64_from_index(MAX_MESH_CONSTRAINT_OPERATIONS));
    if !viable
        || (!arc_budget.exhausted()
            && !prune_face_configuration_singleton_support(
                ctx,
                &mut configurations,
                &singleton_budget,
            )?)
    {
        if let Some(domain) = configurations.first_mut() {
            domain.clear();
        }
    }
    let mut active = ctx.collection_vec(configurations.len(), "catia face factor active rows")?;
    for domain in ctx.admit_iter(&configurations, "catia face factor active rows")? {
        active.push(full_configuration_mask(ctx, domain.len())?);
    }
    let mut factor_by_face = ctx.alloc_filled(domains.len(), None, "catia_face_factor_by_face")?;
    let mut factors_by_edge =
        ctx.collect_indexed_vec(choices.len(), "catia_face_factors_by_edge", |_| {
            Ok(Vec::new())
        })?;
    for (factor, &face) in ctx
        .admit_iter(&retained_faces, "catia face factor indexed edges")?
        .enumerate()
    {
        factor_by_face[face] = Some(factor);
        let (edges, _edges_storage) =
            ctx.with_scoped_storage("catia face factor indexed edges", || {
                let mut edges = Vec::new();
                for configuration in
                    ctx.admit_iter(&configurations[factor], "catia face factor indexed edges")?
                {
                    ctx.reserve_vec(
                        &mut edges,
                        configuration.len(),
                        "catia face factor indexed edges",
                    )?;
                    for &(edge, _) in
                        ctx.admit_iter(configuration, "catia face factor indexed edges")?
                    {
                        edges.push(edge);
                    }
                }
                ctx.sort_unstable_by(
                    &mut edges,
                    |value| value,
                    Ord::cmp,
                    "catia face factor indexed edges sort",
                )?;
                ctx.dedup_vec(&mut edges, "catia face factor indexed edges")?;
                Ok::<_, CodecError>(edges)
            })?;
        for &edge in ctx.admit_iter(&edges, "catia face factors by edge entries")? {
            if let Some(factors) = factors_by_edge.get_mut(edge) {
                ctx.push_vec(factors, factor, "catia face factors by edge entries")?;
            }
        }
    }
    for (face, configurations) in ctx
        .admit_iter(&retained_faces, "catia face factor configurations")?
        .copied()
        .zip(configurations)
    {
        if let Some(domain) = &mut domains[face] {
            *domain = configurations;
        }
    }
    Ok(Some(PreparedFaceFactors {
        domains,
        factor_faces: retained_faces,
        factor_by_face,
        factors_by_edge,
        active,
    }))
}

impl Iterator for IncidenceCandidatePairs<'_> {
    type Item = [usize; 2];

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Options {
                candidates,
                next_index,
            } => {
                let pair = *candidates.get(*next_index)?;
                *next_index += 1;
                Some(pair)
            }
            Self::Implicit(candidates) => candidates.next(),
        }
    }
}

impl Iterator for IncidenceBranch {
    type Item = (usize, [usize; 2]);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Options(options) => options.next(),
            Self::Implicit { edge, candidates } => candidates.next().map(|pair| (*edge, pair)),
            Self::Complete(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum IncidenceSolve<T> {
    Solved(T),
    Rejected(IncidenceRejection),
    Ambiguous,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CoordinateRootPolicy {
    RequireUnique,
    DeferToVisitor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncidenceRejection {
    InputShape,
    ChoicePruning,
    FixedAssignment,
    ComponentDomain,
    ComponentComposition,
}

#[cfg(test)]
impl<T> IncidenceSolve<T> {
    fn into_option(self) -> Option<T> {
        match self {
            Self::Solved(value) => Some(value),
            Self::Rejected(_) | Self::Ambiguous | Self::Exhausted => None,
        }
    }
}

pub(super) fn compact_boundary_domain_viable(
    ctx: &DecodeContext<'_>,
    domain: &MeshFaceBoundaryDomain,
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
) -> Result<bool, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia compact viable edges")?;
    scratch.with_storage(|| compact_boundary_domain_viable_in(ctx, domain, assignment, selected))
}

fn compact_boundary_domain_viable_in(
    ctx: &DecodeContext<'_>,
    domain: &MeshFaceBoundaryDomain,
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia compact viable edges";
    let mut edges = Vec::new();
    match domain {
        MeshFaceBoundaryDomain::Ordered(_) => return Ok(true),
        MeshFaceBoundaryDomain::UnorderedFullCycle(domain_edges) => {
            ctx.extend_vec(&mut edges, domain_edges, OPERATION)?;
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            push_deferred_edges(ctx, domain, &mut edges, OPERATION)?;
        }
    }
    let mut known_pairs = ctx.collection_vec(edges.len(), "catia compact viable selected pairs")?;
    let mut complete = true;
    for &edge in ctx.admit_iter(&edges, "catia compact viable selected pairs")? {
        match selected
            .filter(|(selected_edge, _)| *selected_edge == edge)
            .map(|(_, pair)| pair)
            .or(assignment[edge])
        {
            Some(pair) => known_pairs.push((edge, pair)),
            None => complete = false,
        }
    }
    if matches!(domain, MeshFaceBoundaryDomain::UnorderedFullCycle(_)) && !complete {
        // A partial cycle keeps every point at degree two or less and leaves
        // every connected run open.
        let mut point_nodes = HashMap::new();
        let mut degrees = Vec::<u8>::new();
        let mut components = UnionFind::charged(ctx, 0, "catia_compact_boundary_union")?;
        for &(_, pair) in ctx.admit_iter(&known_pairs, "catia_compact_boundary_points")? {
            let mut nodes = [0; 2];
            for (slot, point) in pair.into_iter().enumerate() {
                nodes[slot] = if let Some(&node) =
                    ctx.get_hash_map(&point_nodes, &point, "catia_compact_boundary_points")?
                {
                    node
                } else {
                    let node = components.push_charged(ctx, "catia_compact_boundary_nodes")?;
                    ctx.push_vec(&mut degrees, 0, "catia_compact_boundary_degrees")?;
                    ctx.insert_hash_map(
                        &mut point_nodes,
                        point,
                        node,
                        "catia_compact_boundary_points",
                    )?;
                    node
                };
            }
            for &node in &nodes {
                if degrees[node] >= 2 {
                    return Ok(false);
                }
                degrees[node] += 1;
            }
            if nodes[0] != nodes[1] {
                components.union(ctx, nodes[0], nodes[1])?;
            }
        }
        let mut open = ctx.alloc_filled(
            degrees.len(),
            false,
            "catia_compact_boundary_open_components",
        )?;
        for (node, &degree) in ctx
            .admit_iter(&degrees, "catia_compact_boundary_open_components")?
            .enumerate()
        {
            if degree < 2 {
                let root = components.find(ctx, node)?;
                open[root] = true;
            }
        }
        return ctx.all_by(
            0..components.len(),
            |node| Ok(open[components.find(ctx, node)?]),
            "catia_compact_boundary_open_components",
        );
    }
    if !complete {
        return Ok(true);
    }
    let mut edge_points =
        ctx.alloc_filled(assignment.len(), [0; 2], "catia labeled edge points")?;
    for &(edge, pair) in ctx.admit_iter(&known_pairs, "catia labeled edge points")? {
        edge_points[edge] = pair;
    }
    Ok(match domain {
        MeshFaceBoundaryDomain::Ordered(_) => true,
        MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
            incidence_cycles(ctx, edges, &edge_points)?.is_some_and(|cycles| cycles.len() == 1)
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            deferred_boundary_closes(ctx, domain, &edge_points)?
        }
    })
}

enum CompactBoundaryAdvanceOutcome<'storage> {
    Complete(Vec<MeshQuotientGaugeState<'storage>>),
    Rejected,
    Exhausted,
}

/// Copies the endpoint sets of a gauge state. The set type belongs to the
/// quotient state; its traversal is charged before it runs.
fn copy_oriented_edges(
    ctx: &DecodeContext<'_>,
    oriented: &HashSet<usize>,
    operation: &'static str,
) -> Result<HashSet<usize>, CodecError> {
    let mut copy = HashSet::new();
    ctx.reserve_set(&mut copy, oriented.len(), operation)?;
    ctx.charge_work(u64_from_index(oriented.len()), operation)?;
    copy.extend(oriented.iter().copied());
    Ok(copy)
}

fn copy_quotient_states<'storage>(
    ctx: &'storage DecodeContext<'_>,
    states: &[MeshQuotientGaugeState<'storage>],
) -> Result<Vec<MeshQuotientGaugeState<'storage>>, CodecError> {
    let mut copy = ctx.collection_vec(states.len(), "catia incidence quotient state rows")?;
    for (quotient, oriented) in ctx.admit_iter(states, "catia incidence quotient state rows")? {
        copy.push((
            quotient.clone_charged(ctx)?,
            copy_oriented_edges(ctx, oriented, "catia incidence quotient oriented edges")?,
        ));
    }
    Ok(copy)
}

fn advance_compact_boundary_domains<'storage, 'a>(
    ctx: &'storage DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a MeshFaceBoundaryDomain>,
    choices: &[Vec<[usize; 2]>],
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
    mut states: Vec<MeshQuotientGaugeState<'storage>>,
    budget: &WorkBudget<'_>,
) -> Result<CompactBoundaryAdvanceOutcome<'storage>, CodecError> {
    const MAX_QUOTIENT_STATES: usize = 4_096;
    const OPERATION: &str = "catia compact boundary edges";

    let selected_pair = |edge: usize| {
        selected
            .filter(|(selected_edge, _)| *selected_edge == edge)
            .map(|(_, pair)| pair)
            .or(assignment[edge])
    };
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    // One edge-point table serves every domain: a domain writes the points of
    // its own edges before it reads them.
    let mut points = scratch.with_storage(|| {
        ctx.alloc_filled(
            assignment.len(),
            [0; 2],
            "catia compact boundary edge points",
        )
    })?;
    let mut ordered = Vec::<Vec<MeshFaceBoundaryAssignment>>::new();
    let mut domains = domains.into_iter();
    'domains: while let Some(domain) = ctx.next_charged(&mut domains, OPERATION)? {
        let (edges, _edges_storage) = ctx.with_scoped_storage(OPERATION, || {
            let mut edges = Vec::new();
            match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    push_assignment_edges(ctx, assignments, &mut edges, OPERATION)?;
                }
                MeshFaceBoundaryDomain::UnorderedFullCycle(domain_edges) => {
                    ctx.extend_vec(&mut edges, domain_edges, OPERATION)?;
                }
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    push_deferred_edges(ctx, domain, &mut edges, OPERATION)?;
                }
            }
            Ok::<_, CodecError>(edges)
        })?;
        for &edge in ctx.admit_iter(&edges, "catia compact boundary selected edges")? {
            let Some(pair) = selected_pair(edge) else {
                continue 'domains;
            };
            points[edge] = pair;
        }
        let alternatives = scratch.with_storage(|| -> Result<_, CodecError> {
            Ok(match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    let mut copied = ctx
                        .collection_vec(assignments.len(), "catia compact boundary alternatives")?;
                    for alternative in
                        ctx.admit_iter(assignments, "catia compact boundary alternatives")?
                    {
                        let mut boundaries = ctx.collection_vec(
                            alternative.boundaries.len(),
                            "catia compact boundary alternative cycles",
                        )?;
                        for cycle in ctx.admit_iter(
                            &alternative.boundaries,
                            "catia compact boundary alternative cycles",
                        )? {
                            boundaries.push(
                                ctx.copy_slice(cycle, "catia compact boundary alternative uses")?,
                            );
                        }
                        copied.push(MeshFaceBoundaryAssignment { boundaries });
                    }
                    Some(copied)
                }
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                    let Some(cycles) = incidence_cycles(ctx, edges, &points)? else {
                        return Ok(None);
                    };
                    let [cycle] = cycles.as_slice() else {
                        return Ok(None);
                    };
                    let mut uses =
                        ctx.collection_vec(cycle.len(), "catia compact unordered boundary uses")?;
                    for &(edge, _) in
                        ctx.admit_iter(&cycle[..], "catia compact unordered boundary uses")?
                    {
                        uses.push(MeshBoundaryEdgeCandidate {
                            edge,
                            start: 0,
                            end: 0,
                            reversed: None,
                        });
                    }
                    let mut boundaries =
                        ctx.collection_vec(1, "catia compact unordered boundary cycles")?;
                    boundaries.push(uses);
                    let mut alternatives =
                        ctx.collection_vec(1, "catia compact unordered boundary alternatives")?;
                    alternatives.push(MeshFaceBoundaryAssignment { boundaries });
                    Some(alternatives)
                }
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    let Some(materialized) = deferred_boundary_assignment(ctx, domain, &points)?
                    else {
                        return Ok(None);
                    };
                    let mut alternatives =
                        ctx.collection_vec(1, "catia compact deferred alternative")?;
                    alternatives.push(materialized);
                    Some(alternatives)
                }
            })
        })?;
        let Some(alternatives) = alternatives else {
            return Ok(CompactBoundaryAdvanceOutcome::Rejected);
        };
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut ordered,
                alternatives,
                "catia compact boundary domain rows",
            )
        })?;
    }
    if ordered.is_empty() {
        return Ok(CompactBoundaryAdvanceOutcome::Complete(states));
    }
    let candidates = scratch.with_storage(|| -> Result<Vec<Vec<[usize; 2]>>, CodecError> {
        let mut candidates =
            ctx.collection_vec(assignment.len(), "catia_compact_boundary_candidate_rows")?;
        for edge in ctx.admit_iter(
            &(0..assignment.len()),
            "catia_compact_boundary_candidate_rows",
        )? {
            candidates.push(match selected_pair(edge) {
                Some(pair) => ctx.copy_slice(&[pair], "catia_compact_boundary_candidate_pair")?,
                None => ctx.copy_slice(&choices[edge], "catia_compact_boundary_candidate_pair")?,
            });
        }
        Ok(candidates)
    })?;
    for alternatives in ctx.admit_iter(&ordered, "catia compact boundary domain rows")? {
        let mut next = Vec::new();
        let mut signature_storage = ctx.reserve_scoped(0, "catia_compact_boundary_signatures")?;
        let mut signatures = HashSet::new();
        for (state, oriented_edges) in ctx.admit_iter(states, "catia_compact_boundary_states")? {
            for face in ctx.admit_iter(alternatives, "catia_compact_boundary_states")? {
                let Some(remaining) = MAX_QUOTIENT_STATES.checked_sub(next.len()) else {
                    return Ok(CompactBoundaryAdvanceOutcome::Exhausted);
                };
                let options = state.assignment_options_limited(
                    ctx,
                    face,
                    &candidates,
                    &oriented_edges,
                    remaining,
                    Some(budget),
                )?;
                for (_, mut candidate) in
                    ctx.admit_iter(options, "catia_compact_boundary_states")?
                {
                    let mut next_oriented = copy_oriented_edges(
                        ctx,
                        &oriented_edges,
                        "catia_compact_boundary_oriented_copy",
                    )?;
                    for boundary in
                        ctx.admit_iter(&face.boundaries, "catia_compact_boundary_oriented_edges")?
                    {
                        for use_ in
                            ctx.admit_iter(boundary, "catia_compact_boundary_oriented_edges")?
                        {
                            ctx.insert_hash_set(
                                &mut next_oriented,
                                use_.edge,
                                "catia_compact_boundary_oriented_edges",
                            )?;
                        }
                    }
                    // One unit per signature entry pays for building both signatures.
                    let Some(work) = candidate
                        .signature_work(ctx)?
                        .and_then(|work| work.checked_add(work_units(next_oriented.len())))
                    else {
                        return Ok(CompactBoundaryAdvanceOutcome::Exhausted);
                    };
                    if !budget.charge_by(work) {
                        return Ok(CompactBoundaryAdvanceOutcome::Exhausted);
                    }
                    let mut oriented_signature = ctx.collection_vec(
                        next_oriented.len(),
                        "catia_compact_boundary_oriented_signature",
                    )?;
                    oriented_signature.extend(next_oriented.iter().copied());
                    ctx.sort_unstable_by(
                        &mut oriented_signature,
                        |value| value,
                        Ord::cmp,
                        "catia_compact_boundary_oriented_signature_sort",
                    )?;
                    let signature = candidate.signature_charged(ctx)?;
                    if signature_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut signatures,
                            (signature, oriented_signature),
                            "catia_compact_boundary_signatures",
                        )
                    })? {
                        ctx.push_vec(
                            &mut next,
                            (candidate, next_oriented),
                            "catia_compact_boundary_next_states",
                        )?;
                    }
                    if next.len() == MAX_QUOTIENT_STATES {
                        break;
                    }
                }
                if next.len() == MAX_QUOTIENT_STATES || budget.exhausted() {
                    break;
                }
            }
            if next.len() == MAX_QUOTIENT_STATES || budget.exhausted() {
                break;
            }
        }
        if next.len() == MAX_QUOTIENT_STATES || budget.exhausted() {
            return Ok(CompactBoundaryAdvanceOutcome::Exhausted);
        }
        if next.is_empty() {
            return Ok(CompactBoundaryAdvanceOutcome::Rejected);
        }
        states = next;
    }
    Ok(CompactBoundaryAdvanceOutcome::Complete(states))
}

#[cfg(test)]
pub(super) fn compact_boundary_domains_jointly_viable<'storage, 'a>(
    ctx: &'storage DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a MeshFaceBoundaryDomain>,
    choices: &[Vec<[usize; 2]>],
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
    quotient: &MeshQuotient<'storage>,
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    let mut initial = Vec::new();
    ctx.reserve_vec(&mut initial, 1, "catia compact initial quotient state")?;
    initial.push((quotient.clone_charged(ctx)?, HashSet::new()));
    Ok(matches!(
        advance_compact_boundary_domains(
            ctx, domains, choices, assignment, selected, initial, budget,
        )?,
        CompactBoundaryAdvanceOutcome::Complete(_)
    ))
}

/// Adds an edge's endpoint pair to the degrees of its distinct faces and
/// returns the previous degrees for `restore_incidence_degrees`.
fn adjust_incidence_degrees(
    ctx: &DecodeContext<'_>,
    degrees: &mut [BTreeMap<usize, u8>],
    edge_faces: &[[usize; 2]],
    edge: usize,
    pair: [usize; 2],
) -> Result<IncidenceDegreeUndo, CodecError> {
    const OPERATION: &str = "catia incidence degree points";
    let mut undo = IncidenceDegreeUndo {
        entries: [(0, 0, None); 4],
        len: 0,
    };
    for face in unique_incidence_faces(edge_faces[edge]) {
        for point in pair {
            let previous = if let Some(degree) =
                ctx.get_mut_btree_map(&mut degrees[face], &point, OPERATION)?
            {
                let previous = *degree;
                *degree += 1;
                Some(previous)
            } else {
                ctx.insert_btree_map(&mut degrees[face], point, 1, OPERATION)?;
                None
            };
            undo.entries[undo.len] = (face, point, previous);
            undo.len += 1;
        }
    }
    Ok(undo)
}

/// Writes back the degrees an adjustment changed, newest first.
fn restore_incidence_degrees(
    ctx: &DecodeContext<'_>,
    degrees: &mut [BTreeMap<usize, u8>],
    undo: IncidenceDegreeUndo,
) -> Result<(), CodecError> {
    const OPERATION: &str = "catia incidence degree restore";
    // A point the adjustment added keeps its entry at degree zero, so a
    // later adjustment of it changes a value instead of growing the tree.
    for &(face, point, previous) in undo.entries[..undo.len].iter().rev() {
        if let Some(stored) = ctx.get_mut_btree_map(&mut degrees[face], &point, OPERATION)? {
            *stored = previous.unwrap_or(0);
        }
    }
    Ok(())
}

impl<'storage> IncidenceComponentSearch<'storage, '_> {
    /// The candidate pairs of an edge; with a required point, only the pairs
    /// through that point. An indexed edge lists every pair under each of its
    /// points, so a point without an index entry has no pair; an edge without
    /// an index is scanned.
    fn candidate_pairs(
        &self,
        edge: usize,
        required_point: Option<usize>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<IncidenceCandidatePairs<'_>, CodecError> {
        if let Some(candidates) = coordinate_domains
            .filter(|_| self.choices[edge].is_empty())
            .and_then(|domains| domains.implicit_edge_candidates(edge, required_point))
        {
            return Ok(IncidenceCandidatePairs::Implicit(candidates));
        }
        let Some(point) = required_point else {
            return Ok(IncidenceCandidatePairs::Options {
                candidates: &self.choices[edge],
                next_index: 0,
            });
        };
        let Some(supports) = self.explicit_point_supports.get(edge) else {
            let candidates = self.choices[edge].as_slice();
            return Ok(IncidenceCandidatePairs::Options {
                candidates,
                next_index: 0,
            });
        };
        let candidates = self
            .ctx
            .get_hash_map(supports, &point, "catia incidence explicit supports")?
            .map_or(&[][..], Vec::as_slice);
        Ok(IncidenceCandidatePairs::Options {
            candidates,
            next_index: 0,
        })
    }

    fn refine_coordinate_domains(
        &self,
        domains: &Arc<MeshCoordinateRootDomains>,
        edge: usize,
        pair: [usize; 2],
    ) -> Result<Option<Arc<MeshCoordinateRootDomains>>, CodecError> {
        if self.coordinate_propagation_budget.exhausted() {
            return Ok(Some(Arc::clone(domains)));
        }
        if domains
            .edge_candidates()
            .get(edge)
            .is_some_and(|candidates| candidates.as_slice() == [pair])
        {
            return Ok(Some(Arc::clone(domains)));
        }
        let refined = domains.refine_edge_candidate_arc(
            self.ctx,
            edge,
            pair,
            Some(self.coordinate_propagation_budget),
        )?;
        Ok(refined.map(Arc::new).or_else(|| {
            self.coordinate_propagation_budget
                .exhausted()
                .then(|| Arc::clone(domains))
        }))
    }

    fn degree(&self, face: usize, point: usize) -> Result<u8, CodecError> {
        incidence_point_degree(self.ctx, &self.degrees[face], point)
    }

    fn remember_degree_support_witness(
        &self,
        face: usize,
        point: usize,
        witness: (usize, [usize; 2]),
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "catia_incidence_witness_pairs";
        let mut witnesses = self.degree_support_witnesses.borrow_mut();
        let mut storage = self.search_storage.borrow_mut();
        if self
            .ctx
            .get_hash_map(&witnesses, &(face, point), "catia_incidence_witness_keys")?
            .is_none()
        {
            storage.with_storage(|| {
                self.ctx.insert_hash_map(
                    &mut witnesses,
                    (face, point),
                    Vec::new(),
                    "catia_incidence_witness_keys",
                )
            })?;
        }
        let Some(entry) = self.ctx.get_mut_hash_map(
            &mut witnesses,
            &(face, point),
            "catia_incidence_witness_keys",
        )?
        else {
            return Ok(());
        };
        if !self.ctx.contains(entry, &witness, OPERATION)? {
            storage.with_storage(|| self.ctx.push_vec(entry, witness, OPERATION))?;
        }
        Ok(())
    }

    fn degree_candidate_fits(&self, edge: usize, pair: [usize; 2]) -> Result<bool, CodecError> {
        pair_fits_degrees(self.ctx, &self.degrees, self.edge_faces[edge], pair)
    }

    /// Whether the assignment order lets an edge branch now: its predecessor
    /// and every dependency are already assigned.
    fn branch_edge_ready(&self, edge: usize) -> Result<bool, CodecError> {
        let order = self
            .partial_solution_filter
            .and_then(|constraint| constraint.assignment_order);
        let assigned = |edge: usize| self.assignment.get(edge).is_some_and(Option::is_some);
        if order
            .and_then(AssignmentOrder::predecessors)
            .and_then(|predecessors| predecessors.get(edge).copied().flatten())
            .is_some_and(|predecessor| !assigned(predecessor))
        {
            return Ok(false);
        }
        match order
            .and_then(AssignmentOrder::dependencies)
            .and_then(|dependencies| dependencies.get(edge))
        {
            Some(dependencies) => self.ctx.all_by(
                dependencies,
                |&predecessor| Ok(assigned(predecessor)),
                "catia incidence branch readiness",
            ),
            None => Ok(true),
        }
    }

    /// A point's degree on a face, counting the selected pair as assigned.
    fn degree_after_selection(
        &self,
        selected: Option<(usize, [usize; 2])>,
        face: usize,
        point: usize,
    ) -> Result<usize, CodecError> {
        let selected_degree = selected.map_or(0, |(edge, pair)| {
            let selected_faces = self.edge_faces[edge];
            usize::from(selected_faces[0] == face || selected_faces[1] == face)
                * pair.iter().filter(|candidate| **candidate == point).count()
        });
        Ok(usize::from(self.degree(face, point)?) + selected_degree)
    }

    fn supporting_pair_fits(
        &self,
        selected: Option<(usize, [usize; 2])>,
        supporting_edge: usize,
        supporting_pair: [usize; 2],
    ) -> Result<bool, CodecError> {
        for face in unique_incidence_faces(self.edge_faces[supporting_edge]) {
            for (rank, &point) in supporting_pair.iter().enumerate() {
                let multiplicity =
                    1 + usize::from(rank == 0 && supporting_pair[0] == supporting_pair[1]);
                if self.degree_after_selection(selected, face, point)? + multiplicity > 2 {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn supporting_point_fits(
        &self,
        selected: Option<(usize, [usize; 2])>,
        supporting_edge: usize,
        point: usize,
    ) -> Result<bool, CodecError> {
        for face in unique_incidence_faces(self.edge_faces[supporting_edge]) {
            if self.degree_after_selection(selected, face, point)? >= 2 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn degree_frontiers_supported(
        &self,
        faces: &[usize],
        selected: Option<(usize, [usize; 2])>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "catia incidence degree frontiers";
        for &face in self.ctx.admit_iter(faces, OPERATION)? {
            let start = self.ctx.partition_point(
                &self.constraints,
                |&(constraint_face, _)| Ok(constraint_face < face),
                OPERATION,
            )?;
            let end = start
                + self.ctx.partition_point(
                    &self.constraints[start..],
                    |&(constraint_face, _)| Ok(constraint_face == face),
                    OPERATION,
                )?;
            for &(_, point) in self
                .ctx
                .admit_iter(&self.constraints[start..end], OPERATION)?
            {
                if self.degree_after_selection(selected, face, point)? == 1
                    && !self.degree_support_exists(face, point, selected, coordinate_domains)?
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Whether an open edge can still raise a degree-one point to two. The
    /// degree support budget pays for each candidate it examines; an
    /// exhausted budget assumes support.
    fn degree_support_exists(
        &self,
        face: usize,
        point: usize,
        selected: Option<(usize, [usize; 2])>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "catia incidence degree support";
        {
            let witnesses = self.degree_support_witnesses.borrow();
            if let Some(remembered) =
                self.ctx
                    .get_hash_map(&witnesses, &(face, point), OPERATION)?
            {
                for &(supporting_edge, supporting_pair) in remembered.iter().rev() {
                    if !self.degree_support_budget.charge() {
                        return Ok(true);
                    }
                    let candidate_still_available = self.ctx.contains(
                        &self.choices[supporting_edge],
                        &supporting_pair,
                        OPERATION,
                    )? || coordinate_domains
                        .filter(|_| self.choices[supporting_edge].is_empty())
                        .is_some_and(|domains| {
                            domains.supports_edge_candidate(supporting_edge, supporting_pair)
                        });
                    if selected.is_none_or(|(edge, _)| supporting_edge != edge)
                        && self.active[supporting_edge]
                        && self.assignment[supporting_edge].is_none()
                        && candidate_still_available
                        && self.supporting_point_fits(selected, supporting_edge, point)?
                        && self.supporting_pair_fits(selected, supporting_edge, supporting_pair)?
                    {
                        return Ok(true);
                    }
                }
            }
        }
        let indexed_edges = match self.point_support_edges.get(face) {
            Some(by_point) => self.ctx.get_hash_map(by_point, &point, OPERATION)?,
            None => None,
        };
        let supporting_edges =
            indexed_edges.map_or(self.face_edges[face].as_slice(), Vec::as_slice);
        for &supporting_edge in supporting_edges {
            if !self.degree_support_budget.charge() {
                return Ok(true);
            }
            if selected.is_some_and(|(edge, _)| supporting_edge == edge)
                || !self.active[supporting_edge]
                || self.assignment[supporting_edge].is_some()
                || !self.supporting_point_fits(selected, supporting_edge, point)?
            {
                continue;
            }
            if let Some(domains) =
                coordinate_domains.filter(|_| self.choices[supporting_edge].is_empty())
            {
                let mut refusal = None;
                let witness = domains.implicit_edge_candidate_with_point(
                    supporting_edge,
                    point,
                    Some(self.degree_support_budget),
                    |pair| {
                        self.supporting_pair_fits(selected, supporting_edge, pair)
                            .unwrap_or_else(|error| {
                                refusal = Some(error);
                                false
                            })
                    },
                );
                if let Some(error) = refusal {
                    return Err(error);
                }
                if let Some(witness) = witness {
                    self.remember_degree_support_witness(face, point, (supporting_edge, witness))?;
                    return Ok(true);
                }
                if self.degree_support_budget.exhausted() {
                    return Ok(true);
                }
                continue;
            }
            for supporting_pair in self.candidate_pairs(supporting_edge, Some(point), None)? {
                if !self.degree_support_budget.charge() {
                    return Ok(true);
                }
                if supporting_pair.contains(&point)
                    && self.supporting_pair_fits(selected, supporting_edge, supporting_pair)?
                {
                    self.remember_degree_support_witness(
                        face,
                        point,
                        (supporting_edge, supporting_pair),
                    )?;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn degree_support_preserved(
        &self,
        edge: usize,
        pair: [usize; 2],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<bool, CodecError> {
        let [first, second] = self.edge_faces[edge];
        let faces = [first.min(second), first.max(second)];
        let length = if faces[0] == faces[1] { 1 } else { 2 };
        let preserved = self.degree_frontiers_supported(
            &faces[..length],
            Some((edge, pair)),
            coordinate_domains,
        )?;
        #[cfg(test)]
        if !self.degree_support_budget.exhausted() {
            assert_eq!(
                preserved,
                self.degree_support_preserved_by_constraint_scan(edge, pair, coordinate_domains)
            );
        }
        Ok(preserved)
    }

    /// Reference scan over every constraint of the selected faces, outside
    /// the decode budget.
    #[cfg(test)]
    fn degree_support_preserved_by_constraint_scan(
        &self,
        edge: usize,
        pair: [usize; 2],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> bool {
        let selected_faces = self.edge_faces[edge];
        let degree = |face: usize, point: usize| {
            usize::from(self.degrees[face].get(&point).copied().unwrap_or_default())
        };
        let selected_degree = |face: usize, point: usize| {
            let incident = selected_faces[0] == face || selected_faces[1] == face;
            incident.then(|| pair.iter().filter(|candidate| **candidate == point).count())
        };
        let degree_after_selection = |face: usize, point: usize| {
            degree(face, point) + selected_degree(face, point).unwrap_or_default()
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
        let candidate_pairs = |supporting_edge: usize, point: usize| -> Vec<[usize; 2]> {
            if let Some(candidates) = coordinate_domains
                .filter(|_| self.choices[supporting_edge].is_empty())
                .and_then(|domains| domains.implicit_edge_candidates(supporting_edge, Some(point)))
            {
                return candidates.collect();
            }
            self.choices[supporting_edge]
                .iter()
                .copied()
                .filter(|candidate| candidate.contains(&point))
                .collect()
        };

        selected_faces.into_iter().enumerate().all(|(rank, face)| {
            if rank > 0 && face == selected_faces[0] {
                return true;
            }
            let start = self
                .constraints
                .partition_point(|&(constraint_face, _)| constraint_face < face);
            let end = self.constraints[start..]
                .partition_point(|&(constraint_face, _)| constraint_face == face)
                + start;
            self.constraints[start..end].iter().all(|&(_, point)| {
                degree_after_selection(face, point) != 1
                    || self.face_edges[face]
                        .iter()
                        .copied()
                        .any(|supporting_edge| {
                            supporting_edge != edge
                                && self.active[supporting_edge]
                                && self.assignment[supporting_edge].is_none()
                                && candidate_pairs(supporting_edge, point).into_iter().any(
                                    |supporting_pair| {
                                        supporting_pair.contains(&point)
                                            && supporting_pair_fits(
                                                supporting_edge,
                                                supporting_pair,
                                            )
                                    },
                                )
                        })
            })
        })
    }

    #[cfg(test)]
    fn candidate_fits(&self, edge: usize, pair: [usize; 2]) -> Result<bool, CodecError> {
        self.candidate_fits_in(edge, pair, self.coordinate_domains)
    }

    fn candidate_fits_in(
        &self,
        edge: usize,
        pair: [usize; 2],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<bool, CodecError> {
        if let Some(mesh_assignments) = self.mesh_assignments {
            let [first, second] = self.edge_faces[edge];
            let faces = [first.min(second), first.max(second)];
            let length = if faces[0] == faces[1] { 1 } else { 2 };
            for &face in &faces[..length] {
                let Some(domain) = mesh_assignments.get(face) else {
                    return Ok(false);
                };
                let viable = match domain {
                    MeshFaceBoundaryDomain::Ordered(assignments) => {
                        let tracked = match self.face_configuration_domains.as_ref() {
                            Some(factors) => factors.face_candidate_has_active_configuration(
                                self.ctx, face, edge, pair,
                            )?,
                            None => None,
                        };
                        match tracked {
                            Some(viable) => viable,
                            None => self.ctx.any_by(
                                assignments,
                                |assignment| {
                                    Ok(mesh_assignment_endpoint_cycles_viable_by(
                                        self.ctx,
                                        assignment,
                                        Some(self.boundary_propagation_budget),
                                        |candidate_edge| {
                                            let selected = if candidate_edge == edge {
                                                Some(pair)
                                            } else {
                                                self.assignment
                                                    .get(candidate_edge)
                                                    .copied()
                                                    .flatten()
                                            };
                                            if let Some(selected) = selected {
                                                return Some(MeshEndpointCandidates::Selected(
                                                    selected,
                                                ));
                                            }
                                            self.choices
                                                .get(candidate_edge)
                                                .filter(|candidates| !candidates.is_empty())
                                                .map(|candidates| {
                                                    MeshEndpointCandidates::Explicit(
                                                        candidates.as_slice(),
                                                    )
                                                })
                                                .or_else(|| {
                                                    coordinate_domains
                                                        .and_then(|domains| {
                                                            domains.implicit_edge_candidates(
                                                                candidate_edge,
                                                                None,
                                                            )
                                                        })
                                                        .map(MeshEndpointCandidates::Implicit)
                                                })
                                        },
                                        |candidate_edge, candidate_pair| {
                                            let selected = if candidate_edge == edge {
                                                Some(pair)
                                            } else {
                                                self.assignment
                                                    .get(candidate_edge)
                                                    .copied()
                                                    .flatten()
                                            };
                                            selected.is_none_or(|selected| {
                                                same_unordered_pair(selected, candidate_pair)
                                            })
                                        },
                                    )?
                                    .unwrap_or(true))
                                },
                                "catia incidence ordered face viability",
                            )?,
                        }
                    }
                    _ => compact_boundary_domain_viable(
                        self.ctx,
                        domain,
                        &self.assignment,
                        Some((edge, pair)),
                    )?,
                };
                if !viable {
                    return Ok(false);
                }
            }
        }
        if !self.degree_candidate_fits(edge, pair)? {
            return Ok(false);
        }
        self.degree_support_preserved(edge, pair, coordinate_domains)
    }

    fn candidate_viable(
        &self,
        edge: usize,
        pair: [usize; 2],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<bool, CodecError> {
        Ok(self.candidate_fits_in(edge, pair, coordinate_domains)?
            && coordinate_domains.is_none_or(|domains| domains.supports_edge_candidate(edge, pair)))
    }

    fn constraint_options(
        &self,
        face: usize,
        point: usize,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        limit: Option<usize>,
        viability: &mut HashMap<MeshEndpointPair, bool>,
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<IncidenceConstraintOptions, CodecError> {
        const OPERATION: &str = "catia incidence constraint options";
        let mut any_viable = false;
        let mut options = BTreeSet::new();
        let mut edges = self.face_edges[face].iter();
        while let Some(&edge) = self.ctx.next_charged(&mut edges, OPERATION)? {
            if !self.active[edge] || self.assignment[edge].is_some() {
                continue;
            }
            let mut pairs = self.candidate_pairs(edge, Some(point), coordinate_domains)?;
            while let Some(pair) = self.ctx.next_charged(&mut pairs, OPERATION)? {
                if !pair.contains(&point) {
                    continue;
                }
                let viable = if let Some(&viable) = self.ctx.get_hash_map(
                    viability,
                    &(edge, pair),
                    "catia incidence constraint viability",
                )? {
                    viable
                } else {
                    let viable = self.candidate_viable(edge, pair, coordinate_domains)?;
                    storage.with_storage(|| {
                        self.ctx.insert_hash_map(
                            viability,
                            (edge, pair),
                            viable,
                            "catia incidence constraint viability",
                        )
                    })?;
                    viable
                };
                if !viable {
                    continue;
                }
                any_viable = true;
                if !self.branch_edge_ready(edge)? {
                    continue;
                }
                storage.with_storage(|| {
                    self.ctx
                        .insert_btree_set(&mut options, (edge, pair), OPERATION)
                })?;
                if limit == Some(options.len()) {
                    return Ok(IncidenceConstraintOptions::AtLeastLimit);
                }
            }
        }
        if !any_viable {
            return Ok(IncidenceConstraintOptions::Unsupported);
        }
        if options.is_empty() {
            return Ok(IncidenceConstraintOptions::Deferred);
        }
        let ordered = storage.with_storage(|| {
            self.ctx
                .collect_vec(options, "catia incidence ordered constraint options")
        })?;
        Ok(IncidenceConstraintOptions::Exact(ordered))
    }

    fn narrowest_edge_branch(
        &self,
        edges: &[usize],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<IncidenceBranch, CodecError> {
        const OPERATION: &str = "catia_incidence_branch_options";
        let mut ordered_edges = storage.with_storage(|| {
            self.ctx
                .collection_vec(edges.len(), "catia_incidence_branch_edge_widths")
        })?;
        for &edge in self
            .ctx
            .admit_iter(edges, "catia_incidence_branch_edge_widths")?
        {
            let width = if let Some(candidates) = coordinate_domains
                .filter(|_| self.choices[edge].is_empty())
                .and_then(|domains| domains.implicit_edge_candidates(edge, None))
            {
                candidates.width_upper_bound(self.ctx)?
            } else {
                self.choices[edge].len()
            };
            ordered_edges.push((edge, width));
        }
        self.ctx.stable_sort_by(
            &mut ordered_edges,
            |value| &value.1,
            Ord::cmp,
            "catia_incidence_branch_edge_widths_sort",
        )?;
        let mut best = None::<(usize, usize, Option<Vec<(usize, [usize; 2])>>)>;
        let mut ordered = ordered_edges.iter();
        'edges: while let Some(&(edge, width)) = self.ctx.next_charged(&mut ordered, OPERATION)? {
            if coordinate_domains
                .filter(|_| self.choices[edge].is_empty())
                .and_then(|domains| domains.implicit_edge_candidates(edge, None))
                .is_some()
            {
                if best.as_ref().is_none_or(|(_, best, _)| width < *best) {
                    best = Some((edge, width, None));
                    if width == 0 {
                        break;
                    }
                }
                continue;
            }
            let limit = best.as_ref().map(|(_, width, _)| *width);
            let mut options = Vec::new();
            let mut pairs = self.choices[edge].iter();
            while let Some(&pair) = self.ctx.next_charged(&mut pairs, OPERATION)? {
                if self.candidate_viable(edge, pair, coordinate_domains)? {
                    storage.with_storage(|| {
                        self.ctx.push_vec(&mut options, (edge, pair), OPERATION)
                    })?;
                    if limit == Some(options.len()) {
                        continue 'edges;
                    }
                }
                if self.budget.exhausted() {
                    break 'edges;
                }
            }
            best = Some((edge, options.len(), Some(options)));
            if best.as_ref().is_some_and(|(_, width, _)| *width == 0) {
                break;
            }
        }
        Ok(match best {
            Some((_, _, Some(options))) => IncidenceBranch::Options(options.into_iter()),
            Some((edge, _, None)) => coordinate_domains
                .and_then(|domains| domains.implicit_edge_candidates(edge, None))
                .map_or_else(
                    || IncidenceBranch::Options(Vec::new().into_iter()),
                    |candidates| IncidenceBranch::Implicit { edge, candidates },
                ),
            None => IncidenceBranch::Options(Vec::new().into_iter()),
        })
    }

    /// The next branch and the scoped storage that holds its options.
    fn branch(
        &self,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<
        Option<(
            IncidenceBranch,
            cadmpeg_core::decode::ScopedReservation<'storage>,
        )>,
        CodecError,
    > {
        let mut storage = self
            .ctx
            .reserve_scoped(0, "catia incidence branch storage")?;
        let branch = self.branch_in(coordinate_domains, &mut storage)?;
        Ok(branch.map(|branch| (branch, storage)))
    }

    fn branch_in(
        &self,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<Option<IncidenceBranch>, CodecError> {
        const OPERATION: &str = "catia incidence branch edges";
        let mut constrained = None::<Vec<(usize, [usize; 2])>>;
        let mut viability = HashMap::new();
        let mut constraints = self.constraints.iter();
        while let Some(&(face, point)) = self
            .ctx
            .next_charged(&mut constraints, "catia incidence branch constraints")?
        {
            if self.degree(face, point)? != 1 {
                continue;
            }
            let limit = constrained.as_ref().map(Vec::len);
            let options = self.constraint_options(
                face,
                point,
                coordinate_domains,
                limit,
                &mut viability,
                storage,
            )?;
            match options {
                IncidenceConstraintOptions::Unsupported => return Ok(None),
                IncidenceConstraintOptions::Deferred | IncidenceConstraintOptions::AtLeastLimit => {
                }
                IncidenceConstraintOptions::Exact(options) => {
                    let singleton = options.len() == 1;
                    constrained = Some(options);
                    if singleton {
                        break;
                    }
                }
            }
        }
        if let Some(options) = constrained {
            return Ok(Some(IncidenceBranch::Options(options.into_iter())));
        }
        if let Some(constraint) = self.partial_solution_filter {
            let mut ready = Vec::new();
            for &edge in self.ctx.admit_iter(self.edges, OPERATION)? {
                if constraint.active_edges.get(edge) == Some(&true)
                    && self.assignment[edge].is_none()
                    && self.branch_edge_ready(edge)?
                {
                    storage.with_storage(|| self.ctx.push_vec(&mut ready, edge, OPERATION))?;
                }
            }
            if !ready.is_empty() {
                return Ok(Some(self.narrowest_edge_branch(
                    &ready,
                    coordinate_domains,
                    storage,
                )?));
            }
        }
        if !self.ctx.any_by(
            &self.constraints,
            |&(face, point)| Ok(self.degree(face, point)? == 1),
            OPERATION,
        )? && self.ctx.all_by(
            self.edges,
            |&edge| Ok(self.assignment[edge].is_some()),
            OPERATION,
        )? {
            // A solution kept in `solutions` outlives the branch storage.
            let mut complete = if self.solution_visitor.is_some() {
                storage.with_storage(|| {
                    self.ctx
                        .collection_vec(self.edges.len(), "catia incidence complete branch")
                })?
            } else {
                self.ctx
                    .collection_vec(self.edges.len(), "catia incidence complete branch")?
            };
            for &edge in self
                .ctx
                .admit_iter(self.edges, "catia incidence complete branch")?
            {
                if let Some(pair) = self.assignment[edge] {
                    complete.push((edge, pair));
                }
            }
            return Ok(Some(IncidenceBranch::Complete(complete)));
        }
        let mut open = Vec::new();
        for &edge in self.ctx.admit_iter(self.edges, OPERATION)? {
            if self.assignment[edge].is_none() && self.branch_edge_ready(edge)? {
                storage.with_storage(|| self.ctx.push_vec(&mut open, edge, OPERATION))?;
            }
        }
        Ok(Some(self.narrowest_edge_branch(
            &open,
            coordinate_domains,
            storage,
        )?))
    }

    fn adjust(&mut self, edge: usize, pair: [usize; 2]) -> Result<IncidenceDegreeUndo, CodecError> {
        self.search_storage.borrow_mut().with_storage(|| {
            adjust_incidence_degrees(self.ctx, &mut self.degrees, self.edge_faces, edge, pair)
        })
    }

    fn restore_adjustment(&mut self, undo: IncidenceDegreeUndo) -> Result<(), CodecError> {
        restore_incidence_degrees(self.ctx, &mut self.degrees, undo)
    }

    fn advance_ordered_faces(
        &mut self,
        faces: &[usize],
        quotient_states: Vec<MeshQuotientGaugeState<'storage>>,
    ) -> Result<Option<Vec<MeshQuotientGaugeState<'storage>>>, CodecError> {
        const OPERATION: &str = "catia_incidence_advanced_faces";
        let Some(mesh_assignments) = self.mesh_assignments else {
            return Ok(Some(quotient_states));
        };
        let mut storage = self.ctx.reserve_scoped(0, OPERATION)?;
        let mut faces = storage.with_storage(|| self.ctx.copy_slice(faces, OPERATION))?;
        self.ctx.sort_unstable_by(
            &mut faces,
            |value| value,
            Ord::cmp,
            "catia_incidence_advanced_faces_sort",
        )?;
        self.ctx.dedup_vec(&mut faces, OPERATION)?;
        for &face in self.ctx.admit_iter(&faces, OPERATION)? {
            let Some(domain) = mesh_assignments.get(face) else {
                return Ok(None);
            };
            let viable = match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    let tracked = match self.face_configuration_domains.as_ref() {
                        Some(factors) => factors.face_has_active_configuration(self.ctx, face)?,
                        None => None,
                    };
                    match tracked {
                        Some(viable) => viable,
                        None => self.ctx.any_by(
                            assignments,
                            |assignment| {
                                Ok(mesh_assignment_endpoint_cycles_viable_where(
                                    self.ctx,
                                    assignment,
                                    self.choices,
                                    Some(self.boundary_propagation_budget),
                                    |edge, pair| {
                                        self.assignment[edge].is_none_or(|selected| {
                                            same_unordered_pair(selected, pair)
                                        })
                                    },
                                )?
                                .unwrap_or(true))
                            },
                            OPERATION,
                        )?,
                    }
                }
                _ => compact_boundary_domain_viable(self.ctx, domain, &self.assignment, None)?,
            };
            if !viable {
                return Ok(None);
            }
        }
        if quotient_states.is_empty() {
            return Ok(Some(quotient_states));
        }
        match advance_compact_boundary_domains(
            self.ctx,
            faces.iter().filter_map(|face| mesh_assignments.get(*face)),
            self.choices,
            &self.assignment,
            None,
            quotient_states,
            self.boundary_propagation_budget,
        )? {
            CompactBoundaryAdvanceOutcome::Complete(states) => Ok(Some(states)),
            CompactBoundaryAdvanceOutcome::Rejected => Ok(None),
            CompactBoundaryAdvanceOutcome::Exhausted => {
                self.state = IncidenceSearchState::Exhausted;
                Ok(None)
            }
        }
    }

    #[cfg(test)]
    fn ordered_faces_feasible(
        &mut self,
        faces: impl IntoIterator<Item = usize>,
    ) -> Result<bool, CodecError> {
        let faces = faces.into_iter().collect::<Vec<_>>();
        Ok(self.advance_ordered_faces(&faces, Vec::new())?.is_some())
    }

    fn component_faces(&self) -> Result<Vec<usize>, CodecError> {
        const OPERATION: &str = "catia incidence component faces";
        let count = self
            .edges
            .len()
            .checked_mul(2)
            .ok_or_else(|| self.ctx.refuse_codec_limit(OPERATION, u64::MAX, u64::MAX))?;
        let mut faces = self.ctx.collection_vec(count, OPERATION)?;
        for &edge in self.ctx.admit_iter(self.edges, OPERATION)? {
            faces.extend(self.edge_faces[edge]);
        }
        self.ctx.sort_unstable_by(
            &mut faces,
            |value| value,
            Ord::cmp,
            "catia incidence component faces sort",
        )?;
        self.ctx.dedup_vec(&mut faces, OPERATION)?;
        Ok(faces)
    }

    #[cfg(test)]
    fn face_configuration_options(
        &self,
    ) -> Result<Option<MeshFaceEndpointConfigurations>, CodecError> {
        let mut storage = self.ctx.reserve_scoped(0, "catia face option domains")?;
        self.face_configuration_options_for(&self.component_faces()?, &mut storage)
    }

    /// The configurations of the most constrained ordered face whose open
    /// edges all have explicit choices, projected onto its open edges. The
    /// result and the work behind it are held in `storage`.
    fn face_configuration_options_for(
        &self,
        component_faces: &[usize],
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<Option<MeshFaceEndpointConfigurations>, CodecError> {
        const OPERATION: &str = "catia face option candidates";
        let Some(mesh_assignments) = self.mesh_assignments else {
            return Ok(None);
        };
        let factor_state = self
            .face_configuration_domains
            .as_ref()
            .map(|factors| &factors.active);
        let mut faces = Vec::new();
        for &face in self.ctx.admit_iter(component_faces, OPERATION)? {
            let Some(MeshFaceBoundaryDomain::Ordered(assignments)) = mesh_assignments.get(face)
            else {
                continue;
            };
            if self.ctx.any_by(
                &self.face_edges[face],
                |&edge| {
                    Ok(self.active[edge]
                        && self.assignment[edge].is_none()
                        && self.choices[edge].is_empty())
                },
                OPERATION,
            )? {
                continue;
            }
            let mut has_unresolved = false;
            let mut width = Some(assignments.len().max(1));
            for &edge in self.ctx.admit_iter(&self.face_edges[face], OPERATION)? {
                if self.active[edge] && self.assignment[edge].is_none() {
                    has_unresolved = true;
                    width = width.and_then(|width| width.checked_mul(self.choices[edge].len()));
                }
            }
            let (true, Some(width)) = (has_unresolved, width) else {
                continue;
            };
            storage.with_storage(|| {
                self.ctx
                    .push_vec(&mut faces, (width, face, assignments), OPERATION)
            })?;
        }
        self.ctx.stable_sort_by_key(
            &mut faces,
            |value| (value.0, value.1),
            Ord::cmp,
            "catia face option candidates sort",
        )?;
        let mut domains = Vec::new();
        for (width, face, assignments) in faces {
            if !self.boundary_propagation_budget.charge() {
                break;
            }
            let factor_mask = self
                .face_configuration_domains
                .as_ref()
                .and_then(|factors| factors.factor_by_face.get(face))
                .copied()
                .flatten()
                .zip(factor_state)
                .map(|(factor, state)| state[factor].as_slice());
            let configurations = if let Some(persistent) = self
                .face_configuration_domains
                .as_ref()
                .and_then(|factors| factors.domains.get(face))
                .and_then(Option::as_ref)
            {
                let mut selected = Vec::new();
                for (configuration_index, configuration) in self
                    .ctx
                    .admit_iter(persistent, "catia face option configurations")?
                    .enumerate()
                {
                    if factor_mask
                        .is_some_and(|mask| !configuration_mask_contains(mask, configuration_index))
                        || !self.ctx.all_by(
                            configuration,
                            |(edge, pair)| {
                                Ok(self.assignment[*edge]
                                    .is_none_or(|chosen| same_unordered_pair(chosen, *pair)))
                            },
                            "catia face option configurations",
                        )?
                    {
                        continue;
                    }
                    storage.with_storage(|| {
                        let copy = self
                            .ctx
                            .copy_slice(configuration, "catia face option configuration pairs")?;
                        self.ctx
                            .push_vec(&mut selected, copy, "catia face option configurations")
                    })?;
                }
                selected
            } else {
                let Some(configurations) = storage.with_storage(|| {
                    mesh_face_endpoint_configurations(
                        self.ctx,
                        assignments,
                        self.choices,
                        &self.assignment,
                        self.boundary_propagation_budget,
                    )
                })?
                else {
                    if self.boundary_propagation_budget.exhausted() {
                        break;
                    }
                    continue;
                };
                configurations
            };
            let mut unique = BTreeSet::new();
            for configuration in self
                .ctx
                .admit_iter(configurations, "catia face option projected configurations")?
            {
                let mut projection = Vec::new();
                for pair in self
                    .ctx
                    .admit_iter(configuration, "catia face option projected pairs")?
                {
                    if self.active[pair.0] && self.assignment[pair.0].is_none() {
                        storage.with_storage(|| {
                            self.ctx.push_vec(
                                &mut projection,
                                pair,
                                "catia face option projected pairs",
                            )
                        })?;
                    }
                }
                storage.with_storage(|| {
                    self.ctx.insert_btree_set(
                        &mut unique,
                        projection,
                        "catia face option projected configurations",
                    )
                })?;
            }
            let projected = storage.with_storage(|| {
                self.ctx
                    .collect_vec(unique, "catia face option ordered projections")
            })?;
            if projected.is_empty() {
                return Ok(Some(Vec::new()));
            }
            if self.ctx.all_by(
                &projected,
                |projection| Ok(projection.is_empty()),
                "catia face option ordered projections",
            )? {
                continue;
            }
            let forced = projected.len() == 1;
            storage.with_storage(|| {
                self.ctx.push_vec(
                    &mut domains,
                    FaceConfigurationDomain {
                        width,
                        face,
                        configurations: projected,
                    },
                    "catia face option domains",
                )
            })?;
            if forced {
                break;
            }
        }
        if domains.is_empty() {
            return Ok(None);
        }
        if self.ctx.all_by(
            &domains,
            |domain| Ok(domain.configurations.len() != 1),
            "catia face option domains",
        )? {
            let mut configuration_domains = storage.with_storage(|| {
                self.ctx
                    .collection_vec(domains.len(), "catia face option configuration domains")
            })?;
            for domain in self
                .ctx
                .admit_iter(&mut domains, "catia face option configuration domains")?
            {
                configuration_domains.push(std::mem::take(&mut domain.configurations));
            }
            let viable = prune_face_configuration_support(
                self.ctx,
                &mut configuration_domains,
                self.boundary_propagation_budget,
            )?;
            for (domain, configurations) in self
                .ctx
                .admit_iter(&mut domains, "catia face option configuration domains")?
                .zip(configuration_domains)
            {
                domain.configurations = configurations;
            }
            if !viable {
                return Ok(Some(Vec::new()));
            }
        }
        let mut best = 0;
        for (index, domain) in self
            .ctx
            .admit_iter(&domains, "catia face option domains")?
            .enumerate()
        {
            let key = |domain: &FaceConfigurationDomain| {
                (domain.configurations.len(), domain.width, domain.face)
            };
            if key(domain) < key(&domains[best]) {
                best = index;
            }
        }
        Ok(Some(domains.swap_remove(best).configurations))
    }

    fn search_face_configurations(
        &mut self,
        mut options: MeshFaceEndpointConfigurations,
        quotient_states: &[MeshQuotientGaugeState<'storage>],
        coordinate_domains: Option<&Arc<MeshCoordinateRootDomains>>,
        component_faces: &[usize],
    ) -> Result<(), CodecError> {
        if options.len() == 1 {
            let Some(option) = options.pop() else {
                return Ok(());
            };
            self.search_forced_face_configurations(
                option,
                quotient_states,
                coordinate_domains,
                component_faces,
            )?;
            return Ok(());
        }
        for option in self
            .ctx
            .admit_iter(options, "catia face configuration options")?
        {
            if let Some(applied) = self.apply_face_configuration(option, coordinate_domains)? {
                let mut branch_storage = self
                    .ctx
                    .reserve_scoped(0, "catia incidence branch quotient states")?;
                let next_states = branch_storage.with_storage(|| {
                    let copies = copy_quotient_states(self.ctx, quotient_states)?;
                    self.advance_ordered_faces(&applied.affected_faces, copies)
                })?;
                if let Some(next_states) = next_states {
                    self.search_with_quotient(
                        &next_states,
                        applied.coordinate_domains.as_ref(),
                        component_faces,
                    )?;
                }
                self.rollback_face_configuration(applied.assigned)?;
                if let Some(factors) = &mut self.face_configuration_domains {
                    factors.restore(self.ctx, applied.factor_checkpoint)?;
                }
            }
            if self.state != IncidenceSearchState::Open {
                return Ok(());
            }
        }
        Ok(())
    }

    fn apply_face_configuration(
        &mut self,
        option: Vec<(usize, [usize; 2])>,
        coordinate_domains: Option<&Arc<MeshCoordinateRootDomains>>,
    ) -> Result<Option<AppliedFaceConfiguration<'storage>>, CodecError> {
        let mut storage = self
            .ctx
            .reserve_scoped(0, "catia face applied assignments")?;
        let mut assigned = storage.with_storage(|| {
            self.ctx
                .collection_vec(option.len(), "catia face applied assignments")
        })?;
        let affected_count = option.len().checked_mul(2).ok_or_else(|| {
            self.ctx
                .refuse_codec_limit("catia face affected faces", u64::MAX, u64::MAX)
        })?;
        let mut affected_faces = storage.with_storage(|| {
            self.ctx
                .collection_vec(affected_count, "catia face affected faces")
        })?;
        let mut next_coordinate_domains = coordinate_domains.cloned();
        for (edge, pair) in self
            .ctx
            .admit_iter(option, "catia face applied assignments")?
        {
            if !self.active[edge] || self.assignment[edge].is_some() {
                continue;
            }
            if !self.degree_candidate_fits(edge, pair)? {
                self.rollback_face_configuration(assigned)?;
                return Ok(None);
            }
            if let Some(domains) = next_coordinate_domains.take() {
                let Some(refined) = self.refine_coordinate_domains(&domains, edge, pair)? else {
                    self.rollback_face_configuration(assigned)?;
                    return Ok(None);
                };
                next_coordinate_domains = Some(refined);
            }
            let undo = self.adjust(edge, pair)?;
            self.assignment[edge] = Some(pair);
            assigned.push((edge, pair, undo));
            affected_faces.extend(self.edge_faces[edge]);
        }
        if assigned.is_empty() {
            return Ok(None);
        }
        let partial_constraint_valid = match self.partial_solution_filter {
            Some(constraint) => match (constraint.valid)(&self.assignment) {
                Ok(valid) => valid,
                Err(error) => {
                    self.rollback_face_configuration(assigned)?;
                    return Err(error);
                }
            },
            None => true,
        };
        if !partial_constraint_valid {
            self.rollback_face_configuration(assigned)?;
            return Ok(None);
        }
        self.ctx.sort_unstable_by(
            &mut affected_faces,
            |value| value,
            Ord::cmp,
            "catia face configuration affected faces sort",
        )?;
        self.ctx
            .dedup_vec(&mut affected_faces, "catia face affected faces")?;
        if !self.degree_frontiers_supported(
            &affected_faces,
            None,
            next_coordinate_domains.as_deref(),
        )? {
            if self.budget.exhausted() {
                self.state = IncidenceSearchState::Exhausted;
            }
            self.rollback_face_configuration(assigned)?;
            return Ok(None);
        }
        let mut assigned_pairs = storage.with_storage(|| {
            self.ctx
                .collection_vec(assigned.len(), "catia face factor assigned pairs")
        })?;
        for &(edge, pair, _) in self
            .ctx
            .admit_iter(&assigned, "catia face factor assigned pairs")?
        {
            assigned_pairs.push((edge, pair));
        }
        let factor_checkpoint = match &mut self.face_configuration_domains {
            Some(factors) => match factors.refine_edges(self.ctx, &assigned_pairs)? {
                FaceFactorRefinement::Tracked(checkpoint) => Some(checkpoint),
                FaceFactorRefinement::Rejected => {
                    self.rollback_face_configuration(assigned)?;
                    return Ok(None);
                }
            },
            None => None,
        };
        Ok(Some(AppliedFaceConfiguration {
            assigned,
            affected_faces,
            coordinate_domains: next_coordinate_domains,
            factor_checkpoint,
            _storage: storage,
        }))
    }

    fn rollback_face_configuration(
        &mut self,
        assigned: Vec<(usize, [usize; 2], IncidenceDegreeUndo)>,
    ) -> Result<(), CodecError> {
        for (edge, _, undo) in self
            .ctx
            .admit_iter(assigned, "catia face applied assignments")?
            .rev()
        {
            self.assignment[edge] = None;
            self.restore_adjustment(undo)?;
        }
        Ok(())
    }

    fn search_forced_face_configurations(
        &mut self,
        mut option: Vec<(usize, [usize; 2])>,
        quotient_states: &[MeshQuotientGaugeState<'storage>],
        coordinate_domains: Option<&Arc<MeshCoordinateRootDomains>>,
        component_faces: &[usize],
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "catia forced face assignments";
        let mut forced_storage = self.ctx.reserve_scoped(0, OPERATION)?;
        let mut assigned = Vec::new();
        let mut factor_checkpoints = Vec::new();
        let mut states =
            forced_storage.with_storage(|| copy_quotient_states(self.ctx, quotient_states))?;
        let mut domains = coordinate_domains.cloned();
        while let Some(applied) = self.apply_face_configuration(option, domains.as_ref())? {
            self.ctx.charge_work(1, "catia_incidence_iteration")?;
            let AppliedFaceConfiguration {
                assigned: applied_assigned,
                affected_faces,
                coordinate_domains: applied_domains,
                factor_checkpoint,
                _storage,
            } = applied;
            let next_states = forced_storage
                .with_storage(|| self.advance_ordered_faces(&affected_faces, states))?;
            let Some(next_states) = next_states else {
                self.rollback_face_configuration(applied_assigned)?;
                if let Some(factors) = &mut self.face_configuration_domains {
                    factors.restore(self.ctx, factor_checkpoint)?;
                }
                break;
            };
            forced_storage.with_storage(|| {
                self.ctx.push_vec(
                    &mut factor_checkpoints,
                    factor_checkpoint,
                    "catia forced face checkpoints",
                )?;
                self.ctx
                    .append_vec(&mut assigned, &mut { applied_assigned }, OPERATION)
            })?;
            states = next_states;
            domains = applied_domains;
            let face_options =
                self.face_configuration_options_for(component_faces, &mut forced_storage)?;
            if self.budget.exhausted() {
                self.state = IncidenceSearchState::Exhausted;
                break;
            }
            match face_options {
                Some(options) if options.is_empty() => break,
                Some(mut options) if options.len() == 1 => {
                    let Some(next) = options.pop() else {
                        break;
                    };
                    option = next;
                }
                Some(options) => {
                    self.search_face_configurations(
                        options,
                        &states,
                        domains.as_ref(),
                        component_faces,
                    )?;
                    break;
                }
                None => {
                    self.search_edge_state(&states, domains.as_ref(), component_faces)?;
                    break;
                }
            }
            if self.state != IncidenceSearchState::Open {
                break;
            }
        }
        self.rollback_face_configuration(assigned)?;
        if let Some(factors) = &mut self.face_configuration_domains {
            while let Some(checkpoint) = factor_checkpoints.pop() {
                factors.restore(self.ctx, checkpoint)?;
            }
        }
        Ok(())
    }

    fn search(&mut self) -> Result<(), CodecError> {
        let quotient_states = Vec::new();
        let mut storage = self
            .ctx
            .reserve_scoped(0, "catia incidence component faces")?;
        let component_faces = storage.with_storage(|| self.component_faces())?;
        let coordinate_domains = storage.with_storage(|| {
            self.coordinate_domains
                .map(|domains| domains.clone_charged(self.ctx).map(Arc::new))
                .transpose()
        })?;
        self.search_with_quotient(
            &quotient_states,
            coordinate_domains.as_ref(),
            &component_faces,
        )?;
        if self.budget.exhausted() {
            self.state = IncidenceSearchState::Exhausted;
        }
        Ok(())
    }

    fn search_with_quotient(
        &mut self,
        quotient_states: &[MeshQuotientGaugeState<'storage>],
        coordinate_domains: Option<&Arc<MeshCoordinateRootDomains>>,
        component_faces: &[usize],
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "catia incidence dead state key";
        if self.state != IncidenceSearchState::Open {
            return Ok(());
        }
        if !self.budget.charge() {
            self.state = IncidenceSearchState::Exhausted;
            return Ok(());
        }
        // The key is kept with the dead states, so it is held in search storage.
        let mut state = self
            .search_storage
            .borrow_mut()
            .with_storage(|| self.ctx.collection_vec(self.edges.len(), OPERATION))?;
        for &edge in self.ctx.admit_iter(self.edges, OPERATION)? {
            state.push(self.assignment[edge]);
        }
        if self
            .ctx
            .contains_hash_set(&self.dead_states, &state, "catia incidence dead states")?
        {
            return Ok(());
        }
        let solutions_before = self.solutions.len();
        self.search_state(quotient_states, coordinate_domains, component_faces)?;
        if self.state == IncidenceSearchState::Open && self.solutions.len() == solutions_before {
            self.search_storage.borrow_mut().with_storage(|| {
                self.ctx.insert_hash_set(
                    &mut self.dead_states,
                    state,
                    "catia incidence dead states",
                )
            })?;
        }
        Ok(())
    }

    fn search_state(
        &mut self,
        quotient_states: &[MeshQuotientGaugeState<'storage>],
        coordinate_domains: Option<&Arc<MeshCoordinateRootDomains>>,
        component_faces: &[usize],
    ) -> Result<(), CodecError> {
        const MAX_SOLUTIONS: usize = 256;
        if self.state != IncidenceSearchState::Open {
            return Ok(());
        }
        if self.solution_visitor.is_none() && self.solutions.len() >= MAX_SOLUTIONS {
            self.state = IncidenceSearchState::Exhausted;
            return Ok(());
        }
        let mut options_storage = self.ctx.reserve_scoped(0, "catia face option domains")?;
        let face_options =
            self.face_configuration_options_for(component_faces, &mut options_storage)?;
        if self.budget.exhausted() {
            self.state = IncidenceSearchState::Exhausted;
            return Ok(());
        }
        if let Some(options) = face_options {
            if !options.is_empty() {
                self.search_face_configurations(
                    options,
                    quotient_states,
                    coordinate_domains,
                    component_faces,
                )?;
            }
            return Ok(());
        }
        self.search_edge_state(quotient_states, coordinate_domains, component_faces)
    }

    fn search_edge_state(
        &mut self,
        quotient_states: &[MeshQuotientGaugeState<'storage>],
        coordinate_domains: Option<&Arc<MeshCoordinateRootDomains>>,
        component_faces: &[usize],
    ) -> Result<(), CodecError> {
        let branch = self.branch(coordinate_domains.map(Arc::as_ref))?;
        if self.budget.exhausted() {
            self.state = IncidenceSearchState::Exhausted;
            return Ok(());
        }
        let Some((branch, _branch_storage)) = branch else {
            return Ok(());
        };
        let mut options = match branch {
            IncidenceBranch::Complete(solution) => {
                if let Some(filter) = self.solution_filter {
                    if !filter(&solution)? {
                        return Ok(());
                    }
                }
                if let Some(visitor) = self.solution_visitor.as_deref_mut() {
                    if (visitor)(&solution)?.is_break() {
                        self.state = IncidenceSearchState::Stopped;
                    }
                } else {
                    self.ctx.push_vec(
                        &mut self.solutions,
                        solution,
                        "catia incidence solutions",
                    )?;
                }
                return Ok(());
            }
            branch => branch,
        };
        let Some(first_option) = options.next() else {
            return Ok(());
        };
        for (edge, pair) in std::iter::once(first_option).chain(options) {
            if !self.budget.charge() {
                self.state = IncidenceSearchState::Exhausted;
                return Ok(());
            }
            if self.assignment[edge].is_some() {
                continue;
            }
            if !self.candidate_fits_in(edge, pair, coordinate_domains.map(Arc::as_ref))? {
                if self.budget.exhausted() {
                    self.state = IncidenceSearchState::Exhausted;
                    return Ok(());
                }
                continue;
            }
            let next_coordinate_domains = if let Some(domains) = coordinate_domains.as_ref() {
                let Some(refined) = self.refine_coordinate_domains(domains, edge, pair)? else {
                    continue;
                };
                Some(refined)
            } else {
                None
            };
            let undo = self.adjust(edge, pair)?;
            self.assignment[edge] = Some(pair);
            let factor_checkpoint = match &mut self.face_configuration_domains {
                Some(factors) => match factors.refine_edges(self.ctx, &[(edge, pair)])? {
                    FaceFactorRefinement::Tracked(checkpoint) => Some(checkpoint),
                    FaceFactorRefinement::Rejected => {
                        self.assignment[edge] = None;
                        self.restore_adjustment(undo)?;
                        continue;
                    }
                },
                None => None,
            };
            let partial_constraint_valid = match self.partial_solution_filter {
                Some(constraint) => match (constraint.valid)(&self.assignment) {
                    Ok(valid) => valid,
                    Err(error) => {
                        self.assignment[edge] = None;
                        self.restore_adjustment(undo)?;
                        if let Some(factors) = &mut self.face_configuration_domains {
                            factors.restore(self.ctx, factor_checkpoint)?;
                        }
                        return Err(error);
                    }
                },
                None => true,
            };
            if partial_constraint_valid {
                let faces = self.edge_faces[edge];
                let mut branch_storage = self
                    .ctx
                    .reserve_scoped(0, "catia incidence branch quotient states")?;
                let next_states = branch_storage.with_storage(|| {
                    let copies = copy_quotient_states(self.ctx, quotient_states)?;
                    self.advance_ordered_faces(&faces, copies)
                })?;
                if let Some(next_states) = next_states {
                    self.search_with_quotient(
                        &next_states,
                        next_coordinate_domains.as_ref(),
                        component_faces,
                    )?;
                }
            }
            self.assignment[edge] = None;
            self.restore_adjustment(undo)?;
            if let Some(factors) = &mut self.face_configuration_domains {
                factors.restore(self.ctx, factor_checkpoint)?;
            }
            if self.state != IncidenceSearchState::Open {
                return Ok(());
            }
        }
        Ok(())
    }
}

fn deferred_boundary_cycle_assignment(
    ctx: &DecodeContext<'_>,
    mesh: &MeshDeferredBoundaryCycle,
    incidence: &[(usize, bool)],
    missing: &HashSet<usize>,
) -> Result<Option<Vec<MeshBoundaryEdgeCandidate>>, CodecError> {
    const OPERATION: &str = "catia deferred cycle edges";
    let is_missing = |edge: &usize| ctx.contains_hash_set(missing, edge, OPERATION);
    if mesh.exact_uses.is_empty() {
        if incidence.len() <= mesh.length
            && ctx.all_by(incidence, |(edge, _)| is_missing(edge), OPERATION)?
        {
            let slack = mesh.length - incidence.len();
            let mut start = 0usize;
            let mut boundary =
                ctx.collection_vec(incidence.len(), "catia deferred cycle unconstrained uses")?;
            for (index, (edge, _)) in ctx
                .admit_iter(incidence, "catia deferred cycle unconstrained uses")?
                .enumerate()
            {
                let span = 1 + usize::from(index == 0) * slack;
                boundary.push(MeshBoundaryEdgeCandidate {
                    edge: *edge,
                    start,
                    end: (start + span) % mesh.length,
                    reversed: None,
                });
                start = (start + span) % mesh.length;
            }
            return Ok(Some(boundary));
        }
        return Ok(None);
    }
    let mut expected =
        ctx.collection_vec(mesh.exact_uses.len(), "catia deferred cycle expected edges")?;
    for (use_, _) in ctx.admit_iter(&mesh.exact_uses, "catia deferred cycle expected edges")? {
        expected.push(use_.edge);
    }
    for reversed in [false, true] {
        let mut actual =
            ctx.collection_vec(incidence.len(), "catia deferred cycle actual edges")?;
        for (edge, _) in ctx.admit_iter(incidence, "catia deferred cycle actual edges")? {
            actual.push(*edge);
        }
        if reversed {
            ctx.reverse(&mut actual, "catia deferred cycle actual edges")?;
        }
        let Some(anchor) = ctx.position_by(&actual, |edge| Ok(*edge == expected[0]), OPERATION)?
        else {
            continue;
        };
        ctx.rotate_left(&mut actual, anchor, "catia deferred cycle actual edges")?;
        let mut positions = ctx.collection_vec(expected.len(), "catia deferred cycle positions")?;
        let mut after = 0usize;
        let mut valid = true;
        for edge in ctx.admit_iter(&expected, "catia deferred cycle positions")? {
            let Some(offset) =
                ctx.position_by(&actual[after..], |actual| Ok(actual == edge), OPERATION)?
            else {
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
        for index in ctx.admit_iter(&(0..expected.len()), OPERATION)? {
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
            if ctx.any_by(
                1..=between,
                |offset| {
                    Ok(!is_missing(
                        &actual[(left_position + offset) % actual.len()],
                    )?)
                },
                OPERATION,
            )? {
                valid = false;
                break;
            }
        }
        if valid {
            let mut boundary =
                ctx.collection_vec(actual.len(), "catia deferred cycle boundary uses")?;
            for index in
                ctx.admit_iter(&(0..expected.len()), "catia deferred cycle boundary uses")?
            {
                let left_position = positions[index];
                let right_position = if index + 1 == expected.len() {
                    positions[0] + actual.len()
                } else {
                    positions[index + 1]
                };
                let (left, left_span) = mesh.exact_uses[index];
                boundary.push(left);
                let missing_count = right_position - left_position - 1;
                let right = mesh.exact_uses[(index + 1) % expected.len()].0;
                let mut start = (left.start + left_span) % mesh.length;
                let capacity = (right.start + mesh.length - start) % mesh.length;
                let slack = capacity - missing_count;
                for offset in
                    ctx.admit_iter(&(1..=missing_count), "catia deferred cycle boundary uses")?
                {
                    let span = 1 + usize::from(offset == 1) * slack;
                    let edge = actual[(left_position + offset) % actual.len()];
                    boundary.push(MeshBoundaryEdgeCandidate {
                        edge,
                        start,
                        end: (start + span) % mesh.length,
                        reversed: None,
                    });
                    start = (start + span) % mesh.length;
                }
            }
            return Ok(Some(boundary));
        }
    }
    Ok(None)
}

pub(super) fn deferred_boundary_cycle_matches(
    ctx: &DecodeContext<'_>,
    mesh: &MeshDeferredBoundaryCycle,
    incidence: &[(usize, bool)],
    missing: &HashSet<usize>,
) -> Result<bool, CodecError> {
    let (assignment, _assignment_storage) = ctx
        .with_scoped_storage("catia deferred cycle boundary uses", || {
            deferred_boundary_cycle_assignment(ctx, mesh, incidence, missing)
        })?;
    Ok(assignment.is_some())
}

/// Kuhn's augmenting-path matching of mesh cycles to incidence cycles,
/// trying meshes in order and incidences in ascending order, with an explicit
/// stack instead of recursion. Returns the mesh matched to each incidence, or
/// `None` when some mesh cycle has no partner.
fn match_deferred_cycles(
    ctx: &DecodeContext<'_>,
    compatible: &[Vec<bool>],
    incidence_count: usize,
) -> Result<Option<Vec<Option<usize>>>, CodecError> {
    const OPERATION: &str = "catia_deferred_match";
    let mut matched_mesh = ctx.alloc_filled(incidence_count, None, OPERATION)?;
    let mut seen = ctx.alloc_filled(incidence_count, 0usize, "catia_deferred_visit")?;
    // Each frame is a mesh, the next incidence it tries and the incidence it
    // is about to take.
    let mut frames = Vec::<(usize, usize, usize)>::new();
    for mesh in ctx.admit_iter(&(0..compatible.len()), OPERATION)? {
        let generation = mesh + 1;
        frames.clear();
        ctx.push_vec(&mut frames, (mesh, 0, 0), OPERATION)?;
        let mut augmented = false;
        while let Some(frame) = frames.last_mut() {
            ctx.charge_work(1, OPERATION)?;
            let (current, next, _) = *frame;
            let row = &compatible[current];
            let Some(offset) = ctx.position_by(
                &row[next.min(row.len())..],
                |compatible| Ok(*compatible),
                OPERATION,
            )?
            else {
                frames.pop();
                continue;
            };
            let incidence = next + offset;
            frame.1 = incidence + 1;
            if seen[incidence] == generation {
                continue;
            }
            seen[incidence] = generation;
            frame.2 = incidence;
            match matched_mesh[incidence] {
                Some(previous) => ctx.push_vec(&mut frames, (previous, 0, 0), OPERATION)?,
                None => {
                    for &(mesh, _, incidence) in ctx.admit_iter(&frames, OPERATION)? {
                        matched_mesh[incidence] = Some(mesh);
                    }
                    augmented = true;
                    break;
                }
            }
        }
        if !augmented {
            return Ok(None);
        }
    }
    Ok(Some(matched_mesh))
}

pub(super) fn deferred_boundary_assignment(
    ctx: &DecodeContext<'_>,
    domain: &MeshDeferredFaceBoundary,
    edge_points: &[[usize; 2]],
) -> Result<Option<MeshFaceBoundaryAssignment>, CodecError> {
    let mut temporary = ctx.reserve_scoped(0, "catia deferred boundary workspace")?;
    let Some((compatible, matched_mesh)) =
        temporary.with_storage(|| deferred_boundary_matching(ctx, domain, edge_points))?
    else {
        return Ok(None);
    };
    let (mut boundaries, _boundaries_storage) =
        ctx.temporary_vec(domain.cycles.len(), "catia_deferred_boundaries")?;
    for _ in ctx.admit_iter(&(0..domain.cycles.len()), "catia_deferred_boundaries")? {
        boundaries.push(None);
    }
    for (incidence, mesh) in ctx
        .admit_iter(&matched_mesh, "catia_deferred_boundaries")?
        .enumerate()
    {
        let Some(mesh) = *mesh else {
            return Ok(None);
        };
        boundaries[mesh] = compatible[mesh][incidence]
            .as_ref()
            .map(|uses| ctx.copy_slice(uses, "catia deferred copied boundary uses"))
            .transpose()?;
    }
    let mut collected =
        ctx.collection_vec(boundaries.len(), "catia deferred collected boundaries")?;
    for boundary in ctx.admit_iter(boundaries, "catia deferred collected boundaries")? {
        let Some(boundary) = boundary else {
            return Ok(None);
        };
        collected.push(boundary);
    }
    Ok(Some(MeshFaceBoundaryAssignment {
        boundaries: collected,
    }))
}

type DeferredCompatibility = Vec<Vec<Option<Vec<MeshBoundaryEdgeCandidate>>>>;

/// The boundary each mesh cycle would take along each incidence cycle, and a
/// matching of every mesh cycle to a distinct compatible incidence cycle.
fn deferred_boundary_matching(
    ctx: &DecodeContext<'_>,
    domain: &MeshDeferredFaceBoundary,
    edge_points: &[[usize; 2]],
) -> Result<Option<(DeferredCompatibility, Vec<Option<usize>>)>, CodecError> {
    const OPERATION: &str = "catia deferred incident edges";
    let mut incident = Vec::new();
    push_deferred_edges(ctx, domain, &mut incident, OPERATION)?;
    ctx.sort_unstable_by(
        &mut incident,
        |value| value,
        Ord::cmp,
        "catia deferred incident edges sort",
    )?;
    ctx.dedup_vec(&mut incident, OPERATION)?;
    let Some(incidence) = incidence_cycles(ctx, &incident, edge_points)? else {
        return Ok(None);
    };
    if incidence.len() != domain.cycles.len() {
        return Ok(None);
    }
    let mut missing = HashSet::new();
    ctx.extend_hash_set(
        &mut missing,
        domain.missing_edges.iter().copied(),
        "catia deferred missing edges",
    )?;
    let mut compatible =
        ctx.collection_vec(domain.cycles.len(), "catia deferred compatibility rows")?;
    for mesh in ctx.admit_iter(&domain.cycles, "catia deferred compatibility rows")? {
        let mut row = ctx.collection_vec(incidence.len(), "catia deferred compatibility cells")?;
        for candidate in ctx.admit_iter(&incidence, "catia deferred compatibility cells")? {
            row.push(deferred_boundary_cycle_assignment(
                ctx, mesh, candidate, &missing,
            )?);
        }
        compatible.push(row);
    }
    let mut boolean_compatible =
        ctx.collection_vec(domain.cycles.len(), "catia deferred matching rows")?;
    for cycles in ctx.admit_iter(&compatible, "catia deferred matching rows")? {
        let mut row = ctx.collection_vec(cycles.len(), "catia deferred matching cells")?;
        for cycle in ctx.admit_iter(cycles, "catia deferred matching cells")? {
            row.push(cycle.is_some());
        }
        boolean_compatible.push(row);
    }
    let Some(matched_mesh) = match_deferred_cycles(ctx, &boolean_compatible, incidence.len())?
    else {
        return Ok(None);
    };
    Ok(Some((compatible, matched_mesh)))
}

/// Whether every mesh cycle of the deferred face matches a distinct closed
/// incidence cycle.
fn deferred_boundary_closes(
    ctx: &DecodeContext<'_>,
    domain: &MeshDeferredFaceBoundary,
    edge_points: &[[usize; 2]],
) -> Result<bool, CodecError> {
    let mut temporary = ctx.reserve_scoped(0, "catia deferred close boundary workspace")?;
    Ok(temporary
        .with_storage(|| deferred_boundary_matching(ctx, domain, edge_points))?
        .is_some())
}

fn boundary_domains_close(
    ctx: &DecodeContext<'_>,
    domains: Option<&[MeshFaceBoundaryDomain]>,
    edge_points: &[[usize; 2]],
) -> Result<bool, CodecError> {
    let Some(domains) = domains else {
        return Ok(true);
    };
    ctx.all_by(
        domains,
        |domain| {
            Ok(match domain {
                MeshFaceBoundaryDomain::Ordered(_) => true,
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                    incidence_cycles(ctx, edges, edge_points)?
                        .is_some_and(|cycles| cycles.len() == 1)
                }
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    deferred_boundary_closes(ctx, domain, edge_points)?
                }
            })
        },
        "catia incidence boundary closure",
    )
}

/// Whether each face of a component closes under a candidate solution: on a
/// face without boundary domains every point has degree at most two and a
/// degree-one point can still be completed; otherwise the face's edges form
/// closed cycles that satisfy its boundary domain.
fn component_incidence_faces_viable(
    ctx: &DecodeContext<'_>,
    faces: &[usize],
    assignment: &[Option<[usize; 2]>],
    choices: &[Vec<[usize; 2]>],
    face_edges: &[Vec<usize>],
    domains: Option<&[MeshFaceBoundaryDomain]>,
    point_count: usize,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia component incidence faces";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    // One edge-point table serves every face: a face writes the points of its
    // own edges before it reads them.
    let mut points = if domains.is_some() {
        scratch.with_storage(|| {
            ctx.alloc_filled(
                assignment.len(),
                [0; 2],
                "catia component incidence edge points",
            )
        })?
    } else {
        Vec::new()
    };
    for &face in ctx.admit_iter(faces, OPERATION)? {
        if domains.is_none() {
            let (degrees, _degrees_storage) = ctx.with_scoped_storage(
                "catia component incidence degree points",
                || -> Result<Option<BTreeMap<usize, u8>>, CodecError> {
                    let mut degrees = BTreeMap::<usize, u8>::new();
                    for &edge in ctx.admit_iter(&face_edges[face], OPERATION)? {
                        let Some(pair) = assignment[edge] else {
                            continue;
                        };
                        for point in pair {
                            if point >= point_count {
                                return Ok(None);
                            }
                            let degree = ctx
                                .entry_btree_map(
                                    &mut degrees,
                                    point,
                                    "catia component incidence degree points",
                                )?
                                .or_default();
                            let Some(next) = degree.checked_add(1) else {
                                return Ok(None);
                            };
                            *degree = next;
                        }
                    }
                    Ok(Some(degrees))
                },
            )?;
            let Some(degrees) = degrees else {
                return Ok(false);
            };
            let closed = ctx.all_by(
                &degrees,
                |(&point, &degree)| {
                    Ok(degree <= 2
                        && (degree != 1
                            || ctx.any_by(
                                &face_edges[face],
                                |&edge| {
                                    Ok(assignment[edge].is_none()
                                        && ctx.any_by(
                                            &choices[edge],
                                            |pair| Ok(pair.contains(&point)),
                                            OPERATION,
                                        )?)
                                },
                                OPERATION,
                            )?))
                },
                OPERATION,
            )?;
            if !closed {
                return Ok(false);
            }
            continue;
        }
        for &edge in ctx.admit_iter(&face_edges[face], OPERATION)? {
            let Some(pair) = assignment[edge] else {
                return Ok(false);
            };
            points[edge] = pair;
        }
        if incidence_cycles(ctx, &face_edges[face], &points)?.is_none() {
            return Ok(false);
        }
        let Some(domain) = domains.and_then(|domains| domains.get(face)) else {
            continue;
        };
        let viable = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => ctx.any_by(
                assignments,
                |boundary_assignment| {
                    Ok(mesh_assignment_endpoint_cycles_viable_where(
                        ctx,
                        boundary_assignment,
                        choices,
                        None,
                        |edge, pair| {
                            assignment[edge]
                                .is_none_or(|selected| same_unordered_pair(selected, pair))
                        },
                    )?
                    .unwrap_or(true))
                },
                OPERATION,
            )?,
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                incidence_cycles(ctx, edges, &points)?.is_some_and(|cycles| cycles.len() == 1)
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                deferred_boundary_closes(ctx, domain, &points)?
            }
        };
        if !viable {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn partial_face_orientability_viable(
    ctx: &DecodeContext<'_>,
    assignment: &[Option<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_edges: &[Vec<usize>],
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    if !ctx.any_by(
        edge_faces,
        |faces| Ok(faces[0] != faces[1]),
        "catia orientability faces",
    )? {
        return Ok(true);
    }
    let mut scratch = ctx.reserve_scoped(0, "catia orientability workspace")?;
    scratch.with_storage(|| orientability_in(ctx, assignment, face_edges, budget))
}

/// Orders each face's assigned edges into trails and asks whether the trails
/// admit consistent orientations. Every point has at most two incident edge
/// ends, so each trail step chooses among at most two edges.
fn orientability_in(
    ctx: &DecodeContext<'_>,
    assignment: &[Option<[usize; 2]>],
    face_edges: &[Vec<usize>],
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia orientability trails";
    let mut edge_points =
        ctx.collection_vec(assignment.len(), "catia orientability edge points")?;
    for pair in ctx.admit_iter(assignment, "catia orientability edge points")? {
        edge_points.push(pair.unwrap_or_default());
    }
    let mut edge_uses = BTreeMap::<usize, Vec<(usize, bool)>>::new();
    let mut boundary_count = 0usize;
    for incident in ctx.admit_iter(face_edges, OPERATION)? {
        let mut face_storage = ctx.reserve_scoped(0, OPERATION)?;
        let selected = face_storage.with_storage(|| {
            let mut selected = Vec::new();
            for &edge in ctx.admit_iter(incident, "catia orientability selected edges")? {
                if assignment[edge].is_some() {
                    ctx.push_vec(&mut selected, edge, "catia orientability selected edges")?;
                }
            }
            Ok::<_, CodecError>(selected)
        })?;
        if selected.is_empty() {
            continue;
        }
        let mut edges_at_point = HashMap::<usize, Vec<usize>>::new();
        let mut degrees = HashMap::<usize, u8>::new();
        let mut unseen = HashSet::new();
        let indexed = face_storage.with_storage(|| -> Result<bool, CodecError> {
            // One local unit indexes one selected edge. Key operations
            // charge their own work through the context.
            for &edge in &selected {
                if !budget.charge() {
                    return Ok(false);
                }
                for point in edge_points[edge] {
                    ctx.push_hash_group(
                        &mut edges_at_point,
                        point,
                        edge,
                        "catia orientability point indexes",
                        "catia orientability indexed edges",
                    )?;
                    let degree = ctx
                        .entry_hash_map(&mut degrees, point, "catia orientability degree points")?
                        .or_default();
                    match degree.checked_add(1) {
                        Some(next) if next <= 2 => *degree = next,
                        _ => return Ok(false),
                    }
                }
            }
            ctx.extend_hash_set(
                &mut unseen,
                selected.iter().copied(),
                "catia orientability unseen edges",
            )?;
            Ok(true)
        })?;
        if !indexed {
            return Ok(false);
        }
        let degree_of = |point: usize| -> Result<u8, CodecError> {
            Ok(ctx
                .get_hash_map(&degrees, &point, "catia orientability degree points")?
                .copied()
                .unwrap_or(0))
        };
        let incident_edges = |point: usize| -> Result<&[usize], CodecError> {
            Ok(ctx
                .get_hash_map(&edges_at_point, &point, "catia orientability point indexes")?
                .map_or(&[][..], Vec::as_slice))
        };
        for &first in ctx.admit_iter(&selected, OPERATION)? {
            if !ctx.contains_hash_set(&unseen, &first, "catia orientability unseen edges")? {
                continue;
            }
            let mut component_storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut stack = Vec::new();
            let mut component = Vec::new();
            let mut points = Vec::new();
            component_storage.with_storage(|| {
                ctx.push_vec(&mut stack, first, "catia orientability traversal stack")?;
                while let Some(edge) = stack.pop() {
                    ctx.charge_work(1, "catia_incidence_iteration")?;
                    if !ctx.remove_hash_set(
                        &mut unseen,
                        &edge,
                        "catia orientability unseen edges",
                    )? {
                        continue;
                    }
                    ctx.push_vec(&mut component, edge, "catia orientability component edges")?;
                    for point in edge_points[edge] {
                        ctx.push_vec(&mut points, point, "catia orientability component points")?;
                        ctx.extend_vec(
                            &mut stack,
                            incident_edges(point)?,
                            "catia orientability traversal stack",
                        )?;
                    }
                }
                ctx.sort_unstable_by(
                    &mut component,
                    |value| value,
                    Ord::cmp,
                    "catia orientability component edges sort",
                )?;
                ctx.sort_unstable_by(
                    &mut points,
                    |value| value,
                    Ord::cmp,
                    "catia orientability component points",
                )?;
                ctx.dedup_vec(&mut points, "catia orientability component points")
            })?;
            let mut endpoints = Vec::new();
            for &point in ctx.admit_iter(&points, "catia orientability endpoints")? {
                match degree_of(point)? {
                    1 => component_storage.with_storage(|| {
                        ctx.push_vec(&mut endpoints, point, "catia orientability endpoints")
                    })?,
                    2 => {}
                    _ => return Ok(false),
                }
            }
            let trail = if endpoints.is_empty() {
                let Some(cycles) = incidence_cycles(ctx, &component, &edge_points)? else {
                    return Ok(false);
                };
                let [cycle] = cycles.as_slice() else {
                    return Ok(false);
                };
                component_storage
                    .with_storage(|| ctx.copy_slice(cycle, "catia orientability closed trail"))?
            } else {
                let [start, end] = endpoints.as_slice() else {
                    return Ok(false);
                };
                // Walk from one endpoint, each step taking the unused edge at
                // the current point.
                let mut remaining = HashSet::new();
                let mut trail = Vec::new();
                component_storage.with_storage(|| {
                    ctx.extend_hash_set(
                        &mut remaining,
                        component.iter().copied(),
                        "catia orientability remaining edges",
                    )?;
                    trail =
                        ctx.vector_storage(component.len(), "catia orientability open trail")?;
                    Ok::<_, CodecError>(())
                })?;
                let mut point = *start;
                let mut steps = 0..component.len();
                while !remaining.is_empty() {
                    if ctx
                        .next_charged(&mut steps, "catia orientability open trail")?
                        .is_none()
                    {
                        return Ok(false);
                    }
                    // Degree admission bounds this list to two edges.
                    let mut next = None;
                    for &edge in incident_edges(point)? {
                        if ctx.contains_hash_set(
                            &remaining,
                            &edge,
                            "catia orientability remaining edges",
                        )? {
                            next = Some(edge);
                            break;
                        }
                    }
                    let Some(edge) = next else { break };
                    ctx.remove_hash_set(
                        &mut remaining,
                        &edge,
                        "catia orientability remaining edges",
                    )?;
                    let pair = edge_points[edge];
                    let reversed = pair[1] == point;
                    if !reversed && pair[0] != point {
                        return Ok(false);
                    }
                    point = pair[usize::from(!reversed)];
                    ctx.push_vec(
                        &mut trail,
                        (edge, reversed),
                        "catia orientability open trail",
                    )?;
                }
                if point != *end || !remaining.is_empty() {
                    return Ok(false);
                }
                trail
            };
            let boundary = boundary_count;
            boundary_count = match boundary_count.checked_add(1) {
                Some(count) => count,
                None => return Ok(false),
            };
            for (edge, reversed) in ctx.admit_iter(trail, "catia orientability edge uses")? {
                ctx.push_btree_group(
                    &mut edge_uses,
                    edge,
                    (boundary, reversed),
                    "catia orientability edge use keys",
                    "catia orientability edge uses",
                )?;
            }
        }
    }
    Ok(solve_boundary_orientation_constraints(ctx, boundary_count, &edge_uses, false)?.is_some())
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn component_incidence_pair_solutions<'storage, F>(
    ctx: &'storage DecodeContext<'_>,
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient<'storage>>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
) -> Result<Option<Vec<Vec<[usize; 2]>>>, CodecError>
where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
{
    component_incidence_pair_solution_outcome(
        ctx,
        choices,
        edge_faces,
        face_count,
        point_count,
        mesh_assignments,
        mesh_quotient,
        partial_solution_valid,
        solution_valid,
    )
    .map(IncidenceSolve::into_option)
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(super) fn component_incidence_pair_solution_outcome<'storage, F>(
    ctx: &'storage DecodeContext<'_>,
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient<'storage>>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
) -> Result<IncidenceSolve<Vec<Vec<[usize; 2]>>>, CodecError>
where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
{
    const MAX_PAIR_SOLUTIONS: usize = 256;
    let mut solutions = Vec::new();
    let mut result_limit_exhausted = false;
    let outcome = visit_component_incidence_pair_solutions(
        ctx,
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
                Ok(ControlFlow::Break(()))
            } else {
                solutions.push(pairs.to_vec());
                Ok(ControlFlow::Continue(()))
            }
        },
    )?;
    Ok(match outcome {
        IncidenceSolve::Solved(_) if result_limit_exhausted => IncidenceSolve::Exhausted,
        IncidenceSolve::Solved(_) => IncidenceSolve::Solved(solutions),
        IncidenceSolve::Rejected(rejection) => IncidenceSolve::Rejected(rejection),
        IncidenceSolve::Ambiguous => IncidenceSolve::Ambiguous,
        IncidenceSolve::Exhausted => IncidenceSolve::Exhausted,
    })
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn visit_component_incidence_pair_solutions<'storage, F, V>(
    ctx: &'storage DecodeContext<'_>,
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient<'storage>>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
    visitor: &mut V,
) -> Result<IncidenceSolve<usize>, CodecError>
where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    visit_component_incidence_pair_solutions_with_coordinate_root_policy(ctx, crate::solve::incidence::VisitComponentIncidencePairSolutionsWithCoordinateRootPolicyInputs { choices, edge_faces, face_count, point_count, mesh_assignments, mesh_quotient, coordinate_root_policy: CoordinateRootPolicy::RequireUnique, partial_solution_valid, solution_valid, visitor, session_budget: &budget })
}

struct VisitComponentIncidencePairSolutionsWithCoordinateRootPolicyInputs<
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
    F,
    V,
> where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    choices: &'input0 [Vec<[usize; 2]>],
    edge_faces: &'input1 [[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&'input2 [MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&'input3 MeshQuotient<'storage>>,
    coordinate_root_policy: CoordinateRootPolicy,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'input4>>,
    solution_valid: &'input5 F,
    visitor: &'input6 mut V,
    session_budget: &'input8 WorkBudget<'input7>,
}

fn visit_component_incidence_pair_solutions_with_coordinate_root_policy<'storage, F, V>(
    ctx: &'storage DecodeContext<'_>,
    inputs: VisitComponentIncidencePairSolutionsWithCoordinateRootPolicyInputs<
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
        F,
        V,
    >,
) -> Result<IncidenceSolve<usize>, CodecError>
where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    struct SolveComponentDomainInputs<
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
        'input17,
    > {
        component: &'input0 [usize],
        choices: &'input1 [Vec<[usize; 2]>],
        edge_faces: &'input2 [[usize; 2]],
        face_edges: &'input3 [Vec<usize>],
        mesh_assignments: Option<&'input4 [MeshFaceBoundaryDomain]>,
        coordinate_domains: Option<&'input5 MeshCoordinateRootDomains>,
        partial_solution_valid: Option<MeshPartialEndpointConstraint<'input6>>,
        assignment: &'input7 [Option<[usize; 2]>],
        degrees: &'input8 [BTreeMap<usize, u8>],
        point_count: usize,
        budget: &'input10 WorkBudget<'input9>,
        coordinate_propagation_budget: &'input12 WorkBudget<'input11>,
        boundary_propagation_budget: &'input14 WorkBudget<'input13>,
        orientation_budget: &'input16 WorkBudget<'input15>,
        solution_visitor: Option<MeshEndpointSolutionVisitor<'input17>>,
    }
    fn solve_component_domain(
        ctx: &DecodeContext<'_>,
        inputs: SolveComponentDomainInputs<
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
            '_,
        >,
    ) -> Result<bool, CodecError> {
        let SolveComponentDomainInputs {
            component,
            choices,
            edge_faces,
            face_edges,
            mesh_assignments,
            coordinate_domains,
            partial_solution_valid,
            assignment,
            degrees,
            point_count,
            budget,
            coordinate_propagation_budget,
            boundary_propagation_budget,
            orientation_budget,
            solution_visitor,
        } = inputs;

        let mut component_storage = ctx.reserve_scoped(0, "CATIA component incidence workspace")?;
        let mut active = component_storage.with_storage(|| {
            ctx.alloc_filled(choices.len(), false, "catia incidence active edges")
        })?;
        let mut constraints = BTreeSet::<(usize, usize)>::new();
        let mut point_support_edges = component_storage.with_storage(|| {
            ctx.collect_indexed_vec(
                face_edges.len(),
                "catia incidence point support edges",
                |_| Ok(HashMap::<usize, Vec<usize>>::new()),
            )
        })?;
        let mut component_faces = Vec::new();
        for &edge in ctx.admit_iter(component, "catia_incidence_component_faces")? {
            active[edge] = true;
            for face in unique_incidence_faces(edge_faces[edge]) {
                component_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut component_faces,
                        face,
                        "catia_incidence_component_faces",
                    )
                })?;
                let (points, _points_storage) = ctx.with_scoped_storage(
                    "catia_incidence_candidate_points",
                    || -> Result<Vec<usize>, CodecError> {
                        let candidate_points = if let Some(domains) =
                            coordinate_domains.filter(|_| choices[edge].is_empty())
                        {
                            domains.edge_candidate_points(ctx, edge)?
                        } else {
                            None
                        };
                        let mut points = match candidate_points {
                            Some(points) => points,
                            None => ctx.collect_vec(
                                choices[edge].iter().flatten().copied(),
                                "catia_incidence_candidate_points",
                            )?,
                        };
                        ctx.sort_unstable_by(
                            &mut points,
                            |value| value,
                            Ord::cmp,
                            "catia_incidence_candidate_points_sort",
                        )?;
                        ctx.dedup_vec(&mut points, "catia_incidence_candidate_points")?;
                        Ok(points)
                    },
                )?;
                for &point in ctx.admit_iter(&points, "catia_incidence_point_constraints")? {
                    component_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut constraints,
                            (face, point),
                            "catia_incidence_point_constraints",
                        )
                    })?;
                    component_storage.with_storage(|| {
                        ctx.push_hash_group(
                            &mut point_support_edges[face],
                            point,
                            edge,
                            "catia_incidence_point_support_keys",
                            "catia_incidence_point_support_entries",
                        )
                    })?;
                }
            }
        }
        ctx.sort_unstable_by(
            &mut component_faces,
            |value| value,
            Ord::cmp,
            "catia_incidence_component_faces",
        )?;
        ctx.dedup_vec(&mut component_faces, "catia_incidence_component_faces")?;
        let constraints = component_storage
            .with_storage(|| ctx.collect_vec(constraints, "catia_incidence_sorted_constraints"))?;
        let mut explicit_point_supports = component_storage.with_storage(|| {
            ctx.collection_vec(choices.len(), "catia_incidence_explicit_support_rows")
        })?;
        for pairs in ctx.admit_iter(choices, "catia_incidence_explicit_support_rows")? {
            let mut supports = HashMap::<usize, Vec<[usize; 2]>>::new();
            for &pair in ctx.admit_iter(pairs, "catia_incidence_explicit_support_pairs")? {
                for point in [Some(pair[0]), (pair[1] != pair[0]).then_some(pair[1])]
                    .into_iter()
                    .flatten()
                {
                    component_storage.with_storage(|| {
                        ctx.push_hash_group(
                            &mut supports,
                            point,
                            pair,
                            "catia_incidence_explicit_support_keys",
                            "catia_incidence_explicit_support_pairs",
                        )
                    })?;
                }
            }
            explicit_point_supports.push(supports);
        }
        let face_configuration_domains = component_storage.with_storage(|| {
            prepare_face_configuration_domains(ctx, mesh_assignments, choices, assignment, &active)
        })?;
        let filter = |solution: &[MeshEndpointPair]| -> Result<bool, CodecError> {
            let (mut completed, _filter_storage) =
                ctx.copy_temporary_slice(assignment, "catia_incidence_filter_assignment")?;
            for &(edge, pair) in ctx.admit_iter(solution, "catia_incidence_filter_assignment")? {
                completed[edge] = Some(pair);
            }
            let locally_closed = component_incidence_faces_viable(
                ctx,
                &component_faces,
                &completed,
                choices,
                face_edges,
                mesh_assignments,
                point_count,
            )?;
            if !locally_closed {
                return Ok(false);
            }
            let orientable = if orientation_budget.exhausted() {
                true
            } else {
                partial_face_orientability_viable(
                    ctx,
                    &completed,
                    edge_faces,
                    face_edges,
                    orientation_budget,
                )? || orientation_budget.exhausted()
            };
            if !orientable {
                return Ok(false);
            }
            if let Some(constraint) = partial_solution_valid {
                if !(constraint.valid)(&completed)? {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        let degree_support_budget = budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        let mut search = IncidenceComponentSearch {
            ctx,
            search_storage: RefCell::new(ctx.reserve_scoped(0, "catia incidence search storage")?),
            choices,
            explicit_point_supports,
            point_support_edges,
            degree_support_witnesses: RefCell::new(HashMap::new()),
            edge_faces,
            face_edges,
            mesh_assignments,
            face_configuration_domains,
            coordinate_domains,
            active,
            edges: component,
            constraints,
            assignment: component_storage
                .with_storage(|| ctx.copy_slice(assignment, "catia_incidence_search_assignment"))?,
            degrees: component_storage.with_storage(|| copy_incidence_degree_rows(ctx, degrees))?,
            solutions: Vec::new(),
            solution_filter: Some(&filter),
            solution_visitor,
            partial_solution_filter: partial_solution_valid,
            dead_states: HashSet::new(),
            budget,
            degree_support_budget: &degree_support_budget,
            coordinate_propagation_budget,
            boundary_propagation_budget,
            state: IncidenceSearchState::Open,
        };
        search.search()?;
        Ok(search.state == IncidenceSearchState::Exhausted)
    }
    struct VisitComponentsInputs<
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
        'input14,
        'input15,
        'input16,
        'input17,
        'input18,
        'input19,
        'input20,
        'input21,
        'input22,
        'input23,
        'input24,
        'input25,
        F,
        V,
    >
    where
        F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
        V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
    {
        component_index: usize,
        choices: &'input0 [Vec<[usize; 2]>],
        edge_faces: &'input1 [[usize; 2]],
        face_edges: &'input2 [Vec<usize>],
        mesh_assignments: Option<&'input3 [MeshFaceBoundaryDomain]>,
        mesh_quotient: Option<&'input4 MeshQuotient<'storage>>,
        coordinate_domains: Option<&'input5 MeshCoordinateRootDomains>,
        coordinate_root_policy: CoordinateRootPolicy,
        partial_solution_valid: Option<MeshPartialEndpointConstraint<'input6>>,
        solution_valid: &'input7 F,
        assignment: &'input8 mut [Option<[usize; 2]>],
        degrees: &'input9 mut [BTreeMap<usize, u8>],
        point_count: usize,
        budget: &'input11 WorkBudget<'input10>,
        visitor: &'input12 mut V,
        visited: &'input13 mut usize,
        ambiguous: &'input14 mut bool,
        components: &'input15 [Vec<usize>],
        component_budget: &'input17 WorkBudget<'input16>,
        orientation_budget: &'input19 WorkBudget<'input18>,
        coordinate_propagation_budget: &'input21 WorkBudget<'input20>,
        boundary_propagation_budget: &'input23 WorkBudget<'input22>,
        session_budget: &'input25 WorkBudget<'input24>,
    }
    fn visit_components<'storage, F, V>(
        ctx: &'storage DecodeContext<'_>,
        inputs: VisitComponentsInputs<
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
            V,
        >,
    ) -> Result<ControlFlow<()>, IncidenceVisitError>
    where
        F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
        V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
    {
        let VisitComponentsInputs {
            component_index,
            choices,
            edge_faces,
            face_edges,
            mesh_assignments,
            mesh_quotient,
            coordinate_domains,
            coordinate_root_policy,
            partial_solution_valid,
            solution_valid,
            assignment,
            degrees,
            point_count,
            budget,
            visitor,
            visited,
            ambiguous,
            components,
            component_budget,
            orientation_budget,
            coordinate_propagation_budget,
            boundary_propagation_budget,
            session_budget,
        } = inputs;

        let Some(component) = components.get(component_index) else {
            if ctx.any_by(
                &*assignment,
                |pair| Ok(pair.is_none()),
                "catia_incidence_completed_pairs",
            )? {
                return Err(IncidenceVisitError::Exhausted);
            }
            let mut completed_storage = ctx.reserve_scoped(0, "catia_incidence_completed_pairs")?;
            let mut pairs = completed_storage.with_storage(|| {
                ctx.collection_vec(assignment.len(), "catia_incidence_completed_pairs")
            })?;
            for pair in ctx.admit_iter(&*assignment, "catia_incidence_completed_pairs")? {
                pairs.extend(*pair);
            }
            let boundary_closed = boundary_domains_close(ctx, mesh_assignments, &pairs)?;
            let solution_accepted = boundary_closed && solution_valid(&pairs)?;
            if !solution_accepted {
                return Ok(ControlFlow::Continue(()));
            }
            if let Some(quotient) = mesh_quotient {
                let (singleton, _singleton_storage) = singleton_incidence_pairs(ctx, &pairs)?;
                let mut quotient = quotient.clone_charged(ctx)?;
                let Some(domains) = mesh_assignments else {
                    if !quotient.point_assignment_exists(
                        ctx,
                        point_count,
                        &singleton,
                        Some(budget),
                    )? {
                        if budget.exhausted() {
                            return Err(IncidenceVisitError::Exhausted);
                        }
                        return Ok(ControlFlow::Continue(()));
                    }
                    *visited = visited
                        .checked_add(1)
                        .ok_or(IncidenceVisitError::Exhausted)?;
                    return Ok(visitor(&pairs)?);
                };
                let Some(closure_limit) =
                    quotient.coordinate_domain_preparation_limit(ctx, point_count, &singleton)?
                else {
                    return Ok(ControlFlow::Continue(()));
                };
                let closure_budget = session_budget.session_child_slice(closure_limit);
                let outcome = quotient.coordinate_root_closure_outcome_for_incidence(
                    ctx,
                    point_count,
                    &singleton,
                    MeshIncidenceBoundary {
                        edge_faces,
                        face_count: domains.len(),
                        domains,
                    },
                    Some(&closure_budget),
                )?;
                match outcome {
                    MeshSolve::Solved(_) => {}
                    MeshSolve::Failed(MeshCandidateFailure::Ambiguous(()))
                        if coordinate_root_policy == CoordinateRootPolicy::DeferToVisitor => {}
                    MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => {
                        *ambiguous = true;
                        return Ok(ControlFlow::Continue(()));
                    }
                    MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => {
                        return Err(IncidenceVisitError::Exhausted)
                    }
                    MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {
                        return Ok(ControlFlow::Continue(()))
                    }
                }
            }
            *visited = visited
                .checked_add(1)
                .ok_or(IncidenceVisitError::Exhausted)?;
            return Ok(visitor(&pairs)?);
        };

        let mut base_storage = ctx.reserve_scoped(0, "catia incidence component snapshots")?;
        let base_assignment = base_storage
            .with_storage(|| ctx.copy_slice(assignment, "catia_incidence_base_assignment"))?;
        let base_degrees =
            base_storage.with_storage(|| copy_incidence_degree_rows(ctx, degrees))?;
        let mut downstream_control = Ok(ControlFlow::Continue(()));
        let mut visit_solution =
            |solution: &[MeshEndpointPair]| -> Result<ControlFlow<()>, CodecError> {
                if !budget.charge() {
                    downstream_control = Err(IncidenceVisitError::Exhausted);
                    return Ok(ControlFlow::Break(()));
                }
                let mut solution_storage =
                    ctx.reserve_scoped(0, "catia incidence component solution workspace")?;
                let mut degree_undo = Vec::new();
                solution_storage.with_storage(|| {
                    ctx.reserve_vec(
                        &mut degree_undo,
                        solution.len(),
                        "catia_incidence_degree_undo",
                    )
                })?;
                for &(edge, pair) in ctx.admit_iter(solution, "catia_incidence_degree_undo")? {
                    assignment[edge] = Some(pair);
                    degree_undo.push((
                        edge,
                        solution_storage.with_storage(|| {
                            adjust_incidence_degrees(ctx, degrees, edge_faces, edge, pair)
                        })?,
                    ));
                }
                let candidates = if coordinate_domains.is_some() {
                    let mut candidates = Vec::new();
                    solution_storage.with_storage(|| {
                        ctx.reserve_vec(
                            &mut candidates,
                            assignment.len(),
                            "catia_incidence_candidate_rows",
                        )
                    })?;
                    for (edge, pair) in ctx
                        .admit_iter(&*assignment, "catia_incidence_candidate_rows")?
                        .enumerate()
                    {
                        candidates.push(solution_storage.with_storage(|| {
                            if let Some(pair) = pair {
                                ctx.alloc_filled(1, *pair, "catia_incidence_fixed_candidate")
                            } else {
                                ctx.copy_slice(&choices[edge], "catia_incidence_open_candidates")
                            }
                        })?);
                    }
                    Some(candidates)
                } else {
                    None
                };
                let control = (|| -> Result<ControlFlow<()>, IncidenceVisitError> {
                    // Refinement only narrows later component searches. A complete
                    // assignment is checked against the quotient before visitation.
                    let refined_domains = if let (Some(domains), Some(candidates)) =
                        (coordinate_domains, candidates.as_ref())
                    {
                        if coordinate_propagation_budget.exhausted() {
                            None
                        } else {
                            domains.refine_candidates(
                                ctx,
                                candidates,
                                Some(coordinate_propagation_budget),
                            )?
                        }
                    } else {
                        None
                    };
                    let propagation_skipped = coordinate_propagation_budget.exhausted();
                    let feasible = coordinate_domains.is_none()
                        || refined_domains.is_some()
                        || propagation_skipped;
                    if feasible {
                        visit_components(
                            ctx,
                            VisitComponentsInputs {
                                component_index: component_index + 1,
                                choices,
                                edge_faces,
                                face_edges,
                                mesh_assignments,
                                mesh_quotient,
                                coordinate_domains: refined_domains.as_ref().or(coordinate_domains),
                                coordinate_root_policy,
                                partial_solution_valid,
                                solution_valid,
                                assignment,
                                degrees,
                                point_count,
                                budget,
                                visitor,
                                visited,
                                ambiguous,
                                components,
                                component_budget,
                                orientation_budget,
                                coordinate_propagation_budget,
                                boundary_propagation_budget,
                                session_budget,
                            },
                        )
                    } else {
                        Ok(ControlFlow::Continue(()))
                    }
                })();
                for (edge, undo) in ctx
                    .admit_iter(degree_undo, "catia_incidence_degree_undo")?
                    .rev()
                {
                    assignment[edge] = None;
                    restore_incidence_degrees(ctx, degrees, undo)?;
                }
                match control {
                    Ok(ControlFlow::Continue(())) => Ok(ControlFlow::Continue(())),
                    terminal => {
                        downstream_control = terminal;
                        Ok(ControlFlow::Break(()))
                    }
                }
            };
        let narrowed_choices =
            coordinate_domains.map_or(choices, MeshCoordinateRootDomains::edge_candidates);
        let component_exhausted = solve_component_domain(
            ctx,
            SolveComponentDomainInputs {
                component,
                choices: narrowed_choices,
                edge_faces,
                face_edges,
                mesh_assignments,
                coordinate_domains,
                partial_solution_valid,
                assignment: &base_assignment,
                degrees: &base_degrees,
                point_count,
                budget: component_budget,
                coordinate_propagation_budget,
                boundary_propagation_budget,
                orientation_budget,
                solution_visitor: Some(&mut visit_solution),
            },
        )?;
        match downstream_control {
            Ok(ControlFlow::Continue(())) if component_exhausted => {
                Err(IncidenceVisitError::Exhausted)
            }
            control => control,
        }
    }

    let VisitComponentIncidencePairSolutionsWithCoordinateRootPolicyInputs {
        choices,
        edge_faces,
        face_count,
        point_count,
        mesh_assignments,
        mesh_quotient,
        coordinate_root_policy,
        partial_solution_valid,
        solution_valid,
        visitor,
        session_budget,
    } = inputs;

    let mut choice_storage = ctx.reserve_scoped(0, "catia incidence choice snapshots")?;
    let mut exhausted = false;
    let mut ambiguous = false;
    let mut visited = 0usize;
    let mut rejection = IncidenceRejection::InputShape;
    let result = (|| -> Result<Option<()>, CodecError> {
        if partial_solution_valid.is_some_and(|constraint| {
            constraint.active_edges.len() != choices.len()
                || constraint.coupled_edges.len() != choices.len()
                || constraint
                    .assignment_order
                    .and_then(AssignmentOrder::predecessors)
                    .is_some_and(|predecessors| predecessors.len() != choices.len())
        }) {
            return Ok(None);
        }
        let mut coordinate_domains = if ctx.any_by(
            choices,
            |candidates| Ok(candidates.len() != 1),
            "catia incidence open choices",
        )? {
            if let Some(quotient) = mesh_quotient {
                let mut quotient = quotient.clone_charged(ctx)?;
                let Some(preparation_limit) =
                    quotient.coordinate_domain_preparation_limit(ctx, point_count, choices)?
                else {
                    return Ok(None);
                };
                let preparation_budget = session_budget.session_child_slice(preparation_limit);
                let Some(domains) = quotient.prepare_coordinate_root_domains(
                    ctx,
                    point_count,
                    choices,
                    Some(&preparation_budget),
                )?
                else {
                    exhausted = preparation_budget.exhausted();
                    rejection = IncidenceRejection::ComponentDomain;
                    return Ok(None);
                };
                Some(domains)
            } else {
                None
            }
        } else {
            None
        };
        let base_choices = choice_storage.with_storage(|| {
            ctx.copy_retained_rows(
                coordinate_domains
                    .as_ref()
                    .map_or(choices, MeshCoordinateRootDomains::edge_candidates),
                "catia_incidence_base_choice_rows",
                "catia_incidence_base_choice_pairs",
            )
        })?;
        let mut narrowed_choices = choice_storage.with_storage(|| {
            ctx.copy_retained_rows(
                &base_choices,
                "catia_incidence_narrow_choice_rows",
                "catia_incidence_narrow_choice_pairs",
            )
        })?;
        if let Some(domains) = mesh_assignments {
            let implicit_support_budget =
                session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
            let implicit_support_viable = if let Some(coordinate) = coordinate_domains.as_ref() {
                prune_implicit_ordered_face_endpoint_support(
                    ctx,
                    domains,
                    &mut narrowed_choices,
                    coordinate,
                    &implicit_support_budget,
                )?
            } else {
                true
            };
            if !implicit_support_viable {
                rejection = IncidenceRejection::ChoicePruning;
                return Ok(None);
            }
            if implicit_support_budget.exhausted() {
                narrowed_choices = choice_storage.with_storage(|| {
                    ctx.copy_retained_rows(
                        &base_choices,
                        "catia_incidence_restore_choice_rows",
                        "catia_incidence_restore_choice_pairs",
                    )
                })?;
            }
            let implicit_choices = choice_storage.with_storage(|| {
                ctx.copy_retained_rows(
                    &narrowed_choices,
                    "catia_incidence_implicit_choice_rows",
                    "catia_incidence_implicit_choice_pairs",
                )
            })?;
            let face_support_budget =
                session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
            if !prune_ordered_face_endpoint_support(
                ctx,
                domains,
                &mut narrowed_choices,
                &face_support_budget,
            )? {
                rejection = IncidenceRejection::ChoicePruning;
                return Ok(None);
            }
            if face_support_budget.exhausted() {
                narrowed_choices = implicit_choices;
            } else if let Some(domains) = coordinate_domains.take() {
                let refinement_budget =
                    session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
                match domains.refine_candidates(ctx, &narrowed_choices, Some(&refinement_budget))? {
                    Some(refined) => {
                        narrowed_choices = choice_storage.with_storage(|| {
                            ctx.copy_retained_rows(
                                refined.edge_candidates(),
                                "catia_incidence_refined_choice_rows",
                                "catia_incidence_refined_choice_pairs",
                            )
                        })?;
                        coordinate_domains = Some(refined);
                    }
                    None if refinement_budget.exhausted() => {
                        narrowed_choices = choice_storage.with_storage(|| {
                            ctx.copy_retained_rows(
                                &base_choices,
                                "catia_incidence_restore_choice_rows",
                                "catia_incidence_restore_choice_pairs",
                            )
                        })?;
                        coordinate_domains = Some(domains);
                    }
                    None => {
                        rejection = IncidenceRejection::ChoicePruning;
                        return Ok(None);
                    }
                }
            }
        }
        let choices = narrowed_choices.as_slice();
        let mut components = choice_storage.with_storage(|| {
            let components = incidence_choice_components(
                ctx,
                choices,
                edge_faces,
                mesh_assignments,
                mesh_quotient,
            )?;
            match partial_solution_valid {
                Some(constraint) => {
                    join_incidence_components_by_coupling(ctx, components, constraint.coupled_edges)
                }
                None => Ok(components),
            }
        })?;
        let ordered = order_incidence_components_by_constraints(
            ctx,
            &mut components,
            choices,
            partial_solution_valid.and_then(|constraint| constraint.assignment_order),
        )?;
        if ordered.is_none() {
            return Ok(None);
        }
        // Edges are visited in ascending order, so an edge already listed on
        // a face is that face's last entry.
        let mut face_edges = choice_storage.with_storage(|| {
            ctx.collect_indexed_vec(face_count, "catia incidence face edges", |_| Ok(Vec::new()))
        })?;
        for (edge, &faces) in ctx
            .admit_iter(edge_faces, "catia_incidence_face_edge_entries")?
            .enumerate()
        {
            for face in unique_incidence_faces(faces) {
                if face_edges[face].last() != Some(&edge) {
                    choice_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut face_edges[face],
                            edge,
                            "catia_incidence_face_edge_entries",
                        )
                    })?;
                }
            }
        }
        let mut fixed = choice_storage.with_storage(|| {
            ctx.alloc_filled(choices.len(), None, "catia incidence fixed edges")
        })?;
        let mut degrees = choice_storage.with_storage(|| {
            ctx.collect_indexed_vec(face_count, "catia incidence face degrees", |_| {
                Ok(BTreeMap::<usize, u8>::new())
            })
        })?;
        for (edge, pairs) in ctx
            .admit_iter(choices, "catia_incidence_fixed_degree_points")?
            .enumerate()
        {
            let [pair] = pairs.as_slice() else {
                continue;
            };
            fixed[edge] = Some(*pair);
            for face in unique_incidence_faces(edge_faces[edge]) {
                for &point in pair {
                    let Some(next) =
                        incidence_point_degree(ctx, &degrees[face], point)?.checked_add(1)
                    else {
                        return Ok(None);
                    };
                    choice_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut degrees[face],
                            point,
                            next,
                            "catia_incidence_fixed_degree_points",
                        )
                    })?;
                }
            }
        }
        if components.is_empty() {
            rejection = IncidenceRejection::FixedAssignment;
            if ctx.any_by(
                &fixed,
                |pair| Ok(pair.is_none()),
                "catia_incidence_fixed_pairs",
            )? {
                return Ok(None);
            }
            let mut pairs = choice_storage
                .with_storage(|| ctx.collection_vec(fixed.len(), "catia_incidence_fixed_pairs"))?;
            for pair in ctx.admit_iter(&fixed, "catia_incidence_fixed_pairs")? {
                pairs.extend(*pair);
            }
            let boundary_closed = boundary_domains_close(ctx, mesh_assignments, &pairs)?;
            let solution_valid = solution_valid(&pairs)?;
            if !boundary_closed || !solution_valid {
                return Ok(None);
            }
            if let Some(quotient) = mesh_quotient {
                let (singleton, _singleton_storage) = singleton_incidence_pairs(ctx, &pairs)?;
                let mut quotient = quotient.clone_charged(ctx)?;
                let Some(closure_limit) =
                    quotient.coordinate_domain_preparation_limit(ctx, point_count, &singleton)?
                else {
                    return Ok(None);
                };
                let budget = session_budget.session_child_slice(closure_limit);
                let Some(domains) = mesh_assignments else {
                    if !quotient.point_assignment_exists(
                        ctx,
                        point_count,
                        &singleton,
                        Some(&budget),
                    )? {
                        exhausted = budget.exhausted();
                        return Ok(None);
                    }
                    visited = 1;
                    // This fixed assignment is the last solution to visit, so a
                    // request to stop and a request to continue end the
                    // traversal alike.
                    let (ControlFlow::Continue(()) | ControlFlow::Break(())) = visitor(&pairs)?;
                    return Ok(Some(()));
                };
                let outcome = quotient.coordinate_root_closure_outcome_for_incidence(
                    ctx,
                    point_count,
                    &singleton,
                    MeshIncidenceBoundary {
                        edge_faces,
                        face_count,
                        domains,
                    },
                    Some(&budget),
                )?;
                match outcome {
                    MeshSolve::Solved(_) => {}
                    MeshSolve::Failed(MeshCandidateFailure::Ambiguous(()))
                        if coordinate_root_policy == CoordinateRootPolicy::DeferToVisitor => {}
                    MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => {
                        ambiguous = true;
                        return Ok(Some(()));
                    }
                    MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => {
                        exhausted = true;
                        return Ok(None);
                    }
                    MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => return Ok(None),
                }
            }
            visited = 1;
            // This fixed assignment is the last solution to visit, so a request
            // to stop and a request to continue end the traversal alike.
            let (ControlFlow::Continue(()) | ControlFlow::Break(())) = visitor(&pairs)?;
            return Ok(Some(()));
        }
        for component in ctx.admit_iter(&components, "catia incidence component preflight")? {
            // Preflight is a rejection shortcut only. Give each independent
            // component its own bounded slice so a large component cannot
            // consume the composition budget needed by all later components.
            // Exhaustion leaves the component unknown; the complete composition
            // below remains the sound decision point.
            let component_preflight_budget =
                session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
            let preflight_orientation_budget =
                session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
            let coordinate_preflight_budget =
                session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
            let boundary_preflight_budget =
                session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
            let mut found = false;
            let mut accept_first =
                |solution: &[MeshEndpointPair]| -> Result<ControlFlow<()>, CodecError> {
                    let mut preflight_storage =
                        ctx.reserve_scoped(0, "catia_incidence_preflight_assignment")?;
                    let mut completed = preflight_storage.with_storage(|| {
                        ctx.copy_slice(&fixed, "catia_incidence_preflight_assignment")
                    })?;
                    for &(edge, pair) in
                        ctx.admit_iter(solution, "catia_incidence_preflight_assignment")?
                    {
                        completed[edge] = Some(pair);
                    }
                    let coordinate_feasible = if let Some(domains) = coordinate_domains.as_ref() {
                        let (refined, _refined_storage) = ctx.with_scoped_storage(
                            "catia_incidence_preflight_candidate_rows",
                            || {
                                let mut candidates = ctx.collection_vec(
                                    completed.len(),
                                    "catia_incidence_preflight_candidate_rows",
                                )?;
                                for (edge, pair) in ctx
                                    .admit_iter(
                                        &completed,
                                        "catia_incidence_preflight_candidate_rows",
                                    )?
                                    .enumerate()
                                {
                                    candidates.push(if let Some(pair) = pair {
                                        ctx.alloc_filled(
                                            1,
                                            *pair,
                                            "catia_incidence_preflight_fixed_candidate",
                                        )?
                                    } else {
                                        ctx.copy_slice(
                                            &choices[edge],
                                            "catia_incidence_preflight_open_candidates",
                                        )?
                                    });
                                }
                                domains.refine_candidates(
                                    ctx,
                                    &candidates,
                                    Some(&coordinate_preflight_budget),
                                )
                            },
                        )?;
                        refined.is_some() || coordinate_preflight_budget.exhausted()
                    } else {
                        true
                    };
                    if !coordinate_feasible {
                        return Ok(ControlFlow::Continue(()));
                    }
                    found = true;
                    Ok(ControlFlow::Break(()))
                };
            let component_exhausted = solve_component_domain(
                ctx,
                SolveComponentDomainInputs {
                    component,
                    choices,
                    edge_faces,
                    face_edges: &face_edges,
                    mesh_assignments,
                    coordinate_domains: coordinate_domains.as_ref(),
                    partial_solution_valid,
                    assignment: &fixed,
                    degrees: &degrees,
                    point_count,
                    budget: &component_preflight_budget,
                    coordinate_propagation_budget: &coordinate_preflight_budget,
                    boundary_propagation_budget: &boundary_preflight_budget,
                    orientation_budget: &preflight_orientation_budget,
                    solution_visitor: Some(&mut accept_first),
                },
            )?;
            if component_exhausted {
                continue;
            }
            if !found {
                rejection = IncidenceRejection::ComponentDomain;
                return Ok(None);
            }
        }
        rejection = IncidenceRejection::ComponentComposition;
        let orientation_budget = session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        // These deductions can reduce composition work but cannot establish a
        // solution. Keep their exhaustion independent of the exact search budget.
        let coordinate_propagation_budget =
            session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        let boundary_propagation_budget =
            session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        match visit_components(
            ctx,
            VisitComponentsInputs {
                component_index: 0,
                choices,
                edge_faces,
                face_edges: &face_edges,
                mesh_assignments,
                mesh_quotient,
                coordinate_domains: coordinate_domains.as_ref(),
                coordinate_root_policy,
                partial_solution_valid,
                solution_valid,
                assignment: &mut fixed,
                degrees: &mut degrees,
                point_count,
                budget: session_budget,
                visitor,
                visited: &mut visited,
                ambiguous: &mut ambiguous,
                components: &components,
                component_budget: session_budget,
                orientation_budget: &orientation_budget,
                coordinate_propagation_budget: &coordinate_propagation_budget,
                boundary_propagation_budget: &boundary_propagation_budget,
                session_budget,
            },
        ) {
            Err(IncidenceVisitError::Resource(error)) => return Err(error),
            Err(IncidenceVisitError::Exhausted) => {
                exhausted = true;
                return Ok(None);
            }
            Ok(_) => {}
        }
        Ok(Some(()))
    })()?;
    Ok(match result {
        Some(()) if visited > 0 => IncidenceSolve::Solved(visited),
        Some(()) if ambiguous => IncidenceSolve::Ambiguous,
        Some(()) => IncidenceSolve::Rejected(rejection),
        None if exhausted => IncidenceSolve::Exhausted,
        None => IncidenceSolve::Rejected(rejection),
    })
}

#[derive(Clone, Copy)]
pub(crate) struct IncidenceEndpointDomains<'a> {
    pub(crate) candidates: &'a [Vec<[usize; 2]>],
    pub(crate) ports: Option<&'a [[u32; 2]]>,
}

pub(crate) fn reconstruct_incidence_candidates(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    endpoints: IncidenceEndpointDomains<'_>,
    face_count: usize,
    budget: &WorkBudget<'_>,
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    const MAX_TOPOLOGY_ASSIGNMENTS: usize = 256;

    let IncidenceEndpointDomains {
        candidates: edge_candidates,
        ports: edge_ports,
    } = endpoints;

    if edge_ports.is_some_and(|ports| ports.len() != edge_candidates.len()) {
        return Ok(None);
    }
    let quotient = match edge_ports {
        Some(ports) => {
            let Some(quotient) =
                initial_mesh_quotient(ctx, edge_candidates, vertex_points.len(), ports)?
            else {
                return Ok(None);
            };
            Some(quotient)
        }
        None => None,
    };
    let mut solution_pairs: Option<Vec<[usize; 2]>> = None;
    let mut assignment_count = 0usize;
    let mut invalid = false;
    let outcome = visit_incidence_endpoint_pair_solutions(
        ctx,
        crate::solve::incidence::VisitIncidenceEndpointPairSolutionsInputs {
            edge_rows,
            vertex_points,
            edge_faces,
            edge_candidates,
            face_count,
            mesh_assignments: None,
            mesh_quotient: quotient.as_ref(),
            partial_solution_valid: None,
            complete_solution_budget: Some(budget),
            solution_valid: &|_| Ok(true),
            visitor: &mut |pairs| -> Result<ControlFlow<()>, CodecError> {
                if assignment_count == MAX_TOPOLOGY_ASSIGNMENTS {
                    invalid = true;
                    return Ok(ControlFlow::Break(()));
                }
                assignment_count += 1;
                let mut port_storage =
                    ctx.reserve_scoped(0, "catia_incidence_port_selected_pairs")?;
                let oriented;
                let pairs = if let Some(ports) = edge_ports {
                    let completed = port_storage.with_storage(
                        || -> Result<Option<Vec<[usize; 2]>>, CodecError> {
                            let mut selected_pairs = ctx.collection_vec(
                                pairs.len(),
                                "catia_incidence_port_selected_pairs",
                            )?;
                            for &pair in
                                ctx.admit_iter(pairs, "catia_incidence_port_selected_pairs")?
                            {
                                selected_pairs.push(Some(pair));
                            }
                            let Some(propagated) =
                                propagate_edge_port_points(ctx, ports, &selected_pairs)?
                            else {
                                return Ok(None);
                            };
                            let mut completed = ctx.collection_vec(
                                propagated.len(),
                                "catia_incidence_port_completed_pairs",
                            )?;
                            for pair in
                                ctx.admit_iter(&propagated, "catia_incidence_port_completed_pairs")?
                            {
                                let Some(pair) = *pair else {
                                    return Ok(None);
                                };
                                completed.push(pair);
                            }
                            Ok(Some(completed))
                        },
                    )?;
                    let Some(completed) = completed else {
                        invalid = true;
                        return Ok(ControlFlow::Break(()));
                    };
                    oriented = completed;
                    oriented.as_slice()
                } else {
                    pairs
                };
                if let Some(stored) = &solution_pairs {
                    if !ctx.equal(stored.as_slice(), pairs, "catia_incidence_solution_pairs")? {
                        invalid = true;
                        return Ok(ControlFlow::Break(()));
                    }
                    return Ok(ControlFlow::Continue(()));
                }
                solution_pairs = Some(ctx.copy_slice(pairs, "catia_incidence_solution_pairs")?);
                Ok(ControlFlow::Continue(()))
            },
        },
    )?;
    if invalid || !matches!(outcome, IncidenceSolve::Solved(_)) {
        return Ok(None);
    }
    let Some(solution_pairs) = solution_pairs else {
        return Ok(None);
    };
    reconstruct_incidence(
        ctx,
        copy_incidence_edge_rows(ctx, edge_rows)?,
        ctx.copy_slice(vertex_points, "catia_incidence_vertex_points")?,
        edge_faces,
        &solution_pairs,
        face_count,
    )
}

struct VisitIncidenceEndpointPairSolutionsInputs<
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
    F,
    V,
> where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    edge_rows: &'input0 [EdgeRow],
    vertex_points: &'input1 [[f64; 3]],
    edge_faces: &'input2 [[usize; 2]],
    edge_candidates: &'input3 [Vec<[usize; 2]>],
    face_count: usize,
    mesh_assignments: Option<&'input4 [MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&'input5 MeshQuotient<'storage>>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'input6>>,
    complete_solution_budget: Option<&'input8 WorkBudget<'input7>>,
    solution_valid: &'input9 F,
    visitor: &'input10 mut V,
}

fn visit_incidence_endpoint_pair_solutions<'storage, F, V>(
    ctx: &'storage DecodeContext<'_>,
    inputs: VisitIncidenceEndpointPairSolutionsInputs<
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
        F,
        V,
    >,
) -> Result<IncidenceSolve<usize>, CodecError>
where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    let VisitIncidenceEndpointPairSolutionsInputs {
        edge_rows,
        vertex_points,
        edge_faces,
        edge_candidates,
        face_count,
        mesh_assignments,
        mesh_quotient,
        partial_solution_valid,
        complete_solution_budget,
        solution_valid,
        visitor,
    } = inputs;

    visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy(ctx, crate::solve::incidence::VisitIncidenceEndpointPairSolutionsWithCoordinateRootPolicyInputs { edge_rows, vertex_points, edge_faces, edge_candidates, face_count, mesh_assignments, mesh_quotient, coordinate_root_policy: CoordinateRootPolicy::RequireUnique, partial_solution_valid, complete_solution_budget, solution_valid, visitor })
}

pub(super) struct VisitIncidenceEndpointPairSolutionsWithCoordinateRootPolicyInputs<
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
    F,
    V,
> where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    pub(super) edge_rows: &'input0 [EdgeRow],
    pub(super) vertex_points: &'input1 [[f64; 3]],
    pub(super) edge_faces: &'input2 [[usize; 2]],
    pub(super) edge_candidates: &'input3 [Vec<[usize; 2]>],
    pub(super) face_count: usize,
    pub(super) mesh_assignments: Option<&'input4 [MeshFaceBoundaryDomain]>,
    pub(super) mesh_quotient: Option<&'input5 MeshQuotient<'storage>>,
    pub(super) coordinate_root_policy: CoordinateRootPolicy,
    pub(super) partial_solution_valid: Option<MeshPartialEndpointConstraint<'input6>>,
    pub(super) complete_solution_budget: Option<&'input8 WorkBudget<'input7>>,
    pub(super) solution_valid: &'input9 F,
    pub(super) visitor: &'input10 mut V,
}

pub(super) fn visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy<'storage, F, V>(
    ctx: &'storage DecodeContext<'_>,
    inputs: VisitIncidenceEndpointPairSolutionsWithCoordinateRootPolicyInputs<
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
        F,
        V,
    >,
) -> Result<IncidenceSolve<usize>, CodecError>
where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    let VisitIncidenceEndpointPairSolutionsWithCoordinateRootPolicyInputs {
        edge_rows,
        vertex_points,
        edge_faces,
        edge_candidates,
        face_count,
        mesh_assignments,
        mesh_quotient,
        coordinate_root_policy,
        partial_solution_valid,
        complete_solution_budget,
        solution_valid,
        visitor,
    } = inputs;

    let mut choice_storage = ctx.reserve_scoped(0, "catia endpoint choice snapshots")?;
    let mut choices = choice_storage.with_storage(|| {
        ctx.copy_retained_rows(
            edge_candidates,
            "catia incidence choice rows",
            "catia incidence choice pairs",
        )
    })?;
    for candidates in ctx.admit_iter(&mut choices, "catia incidence choice pairs sort")? {
        for pair in ctx.admit_iter(&mut *candidates, "catia incidence choice pair sort")? {
            *pair = canonical_pair(*pair);
        }
        ctx.sort_unstable_by(
            candidates,
            |value| value,
            Ord::cmp,
            "catia incidence choice pairs sort",
        )?;
        ctx.dedup_vec(candidates, "catia incidence choice pairs sort")?;
    }
    const VALIDATION: &str = "catia incidence choice validation";
    let valid = if ctx.any_by(&choices, |pairs| Ok(pairs.is_empty()), VALIDATION)? {
        mesh_quotient.is_some()
            && choices.len() == edge_faces.len()
            && !ctx.any_by(
                edge_faces,
                |faces| Ok(faces[0] >= face_count || faces[1] >= face_count),
                VALIDATION,
            )?
            && !ctx.any_by(
                &choices,
                |pairs| {
                    ctx.any_by(
                        pairs,
                        |pair| Ok(pair[0] >= vertex_points.len() || pair[1] >= vertex_points.len()),
                        VALIDATION,
                    )
                },
                VALIDATION,
            )?
    } else if mesh_assignments.is_some() {
        prune_incidence_choices_with_deferred_support(
            ctx,
            &mut choices,
            edge_faces,
            face_count,
            vertex_points.len(),
        )?
        .is_some()
    } else {
        prune_incidence_choices(
            ctx,
            &mut choices,
            edge_faces,
            face_count,
            vertex_points.len(),
        )?
        .is_some()
    };
    if !valid {
        return Ok(IncidenceSolve::Rejected(IncidenceRejection::ChoicePruning));
    }
    let complete_valid = |points: &[[usize; 2]]| -> Result<bool, CodecError> {
        // One local unit visits one complete candidate; its validation and
        // reconstruction admit their input-sized work separately.
        if complete_solution_budget.is_some_and(|budget| !budget.charge()) {
            return Ok(true);
        }
        let preferred = solution_valid(points)?;
        if !preferred {
            return Ok(false);
        }
        let (draft, _draft_storage) =
            ctx.with_scoped_storage("catia_incidence_validation_points", || {
                reconstruct_incidence(
                    ctx,
                    copy_incidence_edge_rows(ctx, edge_rows)?,
                    ctx.copy_slice(vertex_points, "catia_incidence_validation_points")?,
                    edge_faces,
                    points,
                    face_count,
                )
            })?;
        Ok(draft.is_some())
    };
    let mut budgeted_visitor = |points: &[[usize; 2]]| {
        if complete_solution_budget.is_some_and(WorkBudget::exhausted) {
            Ok(ControlFlow::Break(()))
        } else {
            visitor(points)
        }
    };
    let fallback_budget = ctx.work_budget(u64_from_index(MAX_MESH_CONSTRAINT_OPERATIONS));
    let session_budget = complete_solution_budget.unwrap_or(&fallback_budget);
    let outcome = visit_component_incidence_pair_solutions_with_coordinate_root_policy(ctx, crate::solve::incidence::VisitComponentIncidencePairSolutionsWithCoordinateRootPolicyInputs { choices: &choices, edge_faces, face_count, point_count: vertex_points.len(), mesh_assignments, mesh_quotient, coordinate_root_policy, partial_solution_valid, solution_valid: &complete_valid, visitor: &mut budgeted_visitor, session_budget })?;
    if complete_solution_budget.is_some_and(WorkBudget::exhausted) {
        Ok(IncidenceSolve::Exhausted)
    } else {
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
