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
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
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
    for row in rows {
        let mut copied_row = BTreeMap::new();
        for (&point, &degree) in row {
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
    for row in rows {
        copy.push(row.clone_charged(ctx)?);
    }
    Ok(copy)
}

fn singleton_incidence_pairs(
    ctx: &DecodeContext<'_>,
    pairs: &[[usize; 2]],
) -> Result<Vec<Vec<[usize; 2]>>, CodecError> {
    let mut singleton = Vec::new();
    ctx.reserve_vec(
        &mut singleton,
        pairs.len(),
        "catia_incidence_singleton_rows",
    )?;
    for &pair in pairs {
        singleton.push(ctx.alloc_filled(1, pair, "catia_incidence_singleton_pair")?);
    }
    Ok(singleton)
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
    fn unique_faces(faces: [usize; 2]) -> impl Iterator<Item = usize> {
        faces
            .into_iter()
            .enumerate()
            .filter_map(move |(rank, face)| (rank == 0 || face != faces[0]).then_some(face))
    }

    fn fits(
        degrees: &[BTreeMap<usize, u8>],
        edge_faces: &[[usize; 2]],
        edge: usize,
        pair: [usize; 2],
    ) -> bool {
        unique_faces(edge_faces[edge]).all(|face| {
            pair.iter().enumerate().all(|(rank, &point)| {
                let multiplicity = 1 + usize::from(rank == 0 && pair[0] == pair[1]);
                usize::from(degrees[face].get(&point).copied().unwrap_or(0)) + multiplicity <= 2
            })
        })
    }

    fn preserves_new_degree_support(
        supports: &[BTreeMap<usize, u32>],
        edge_supports: &[HashSet<usize>],
        degrees: &[BTreeMap<usize, u8>],
        edge_faces: &[[usize; 2]],
        edge: usize,
        pair: [usize; 2],
    ) -> bool {
        unique_faces(edge_faces[edge]).all(|face| {
            pair.into_iter().enumerate().all(|(rank, point)| {
                if rank == 1 && point == pair[0] {
                    return true;
                }
                let selected_degree = 1 + u8::from(pair[0] == pair[1]);
                degrees[face].get(&point).copied().unwrap_or(0) + selected_degree != 1
                    || supports[face].get(&point).copied().unwrap_or(0)
                        > u32::from(edge_supports[edge].contains(&point))
            })
        })
    }

    fn remove_edge_support(
        ctx: &DecodeContext<'_>,
        supports: &mut [BTreeMap<usize, u32>],
        edge_supports: &mut [HashSet<usize>],
        edge_faces: &[[usize; 2]],
        edge: usize,
        retained: HashSet<usize>,
    ) -> Result<Option<()>, CodecError> {
        let mut removed = Vec::new();
        ctx.reserve_vec(
            &mut removed,
            edge_supports[edge].difference(&retained).count(),
            "catia incidence removed support points",
        )?;
        removed.extend(edge_supports[edge].difference(&retained).copied());
        for face in unique_faces(edge_faces[edge]) {
            for &point in &removed {
                let Some(count) = supports[face].get_mut(&point) else {
                    return Ok(None);
                };
                let Some(next) = count.checked_sub(1) else {
                    return Ok(None);
                };
                *count = next;
                if *count == 0 {
                    supports[face].remove(&point);
                }
            }
        }
        edge_supports[edge] = retained;
        Ok(Some(()))
    }

    fn sole_supporting_edge(
        face_edges: &[Vec<usize>],
        edge_supports: &[HashSet<usize>],
        face: usize,
        point: usize,
    ) -> Option<usize> {
        face_edges[face]
            .iter()
            .copied()
            .find(|&edge| edge_supports[edge].contains(&point))
    }

    fn choice_points(
        ctx: &DecodeContext<'_>,
        choices: &[[usize; 2]],
    ) -> Result<HashSet<usize>, CodecError> {
        let mut points = HashSet::new();
        for point in choices.iter().flatten().copied() {
            ctx.insert_hash_set(&mut points, point, "catia incidence choice points")?;
        }
        Ok(points)
    }

    fn degree_one_points(
        degrees: &[BTreeMap<usize, u8>],
        face: usize,
    ) -> impl Iterator<Item = usize> + '_ {
        degrees[face]
            .iter()
            .filter_map(|(&point, &degree)| (degree == 1).then_some(point))
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
        return Ok(None);
    }
    let mut face_edges = ctx.alloc_filled(face_count, Vec::new(), "catia_incidence_face_edges")?;
    for (edge, faces) in edge_faces.iter().copied().enumerate() {
        for face in unique_faces(faces) {
            ctx.push_vec(
                &mut face_edges[face],
                edge,
                "catia incidence face edge lists",
            )?;
        }
    }
    let mut fixed = ctx.alloc_filled(choices.len(), false, "catia_incidence_fixed_edges")?;
    let mut degrees = ctx.alloc_filled(
        face_count,
        BTreeMap::<usize, u8>::new(),
        "catia_incidence_degrees",
    )?;
    let mut edge_supports = Vec::new();
    ctx.reserve_vec(
        &mut edge_supports,
        choices.len(),
        "catia incidence edge supports",
    )?;
    for pairs in choices.iter() {
        edge_supports.push(choice_points(ctx, pairs)?);
    }
    let mut supports = ctx.alloc_filled(
        face_count,
        BTreeMap::<usize, u32>::new(),
        "catia_incidence_supports",
    )?;
    for (edge, points) in edge_supports.iter().enumerate() {
        for face in unique_faces(edge_faces[edge]) {
            for &point in points {
                ctx.admit_btree_entry(
                    &supports[face],
                    &point,
                    "catia incidence point support counts",
                )?;
                let count = supports[face].entry(point).or_default();
                let Some(next) = count.checked_add(1) else {
                    return Ok(None);
                };
                *count = next;
            }
        }
    }
    loop {
        let mut changed = false;
        for edge in 0..choices.len() {
            if fixed[edge] {
                continue;
            }
            let before = choices[edge].len();
            let mut retained = Vec::new();
            for pair in choices[edge].iter().copied() {
                if fits(&degrees, edge_faces, edge, pair)
                    && (!explicit_support_complete
                        || preserves_new_degree_support(
                            &supports,
                            &edge_supports,
                            &degrees,
                            edge_faces,
                            edge,
                            pair,
                        ))
                {
                    ctx.push_vec(&mut retained, pair, "catia incidence retained choices")?;
                }
            }
            choices[edge] = retained;
            changed |= choices[edge].len() != before;
            if remove_edge_support(
                ctx,
                &mut supports,
                &mut edge_supports,
                edge_faces,
                edge,
                choice_points(ctx, &choices[edge])?,
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
            for face in unique_faces(edge_faces[edge]) {
                for point in pair {
                    ctx.admit_btree_entry(
                        &degrees[face],
                        point,
                        "catia incidence endpoint degrees",
                    )?;
                    let degree = degrees[face].entry(*point).or_default();
                    let Some(next) = degree.checked_add(1) else {
                        return Ok(None);
                    };
                    *degree = next;
                }
            }
            if remove_edge_support(
                ctx,
                &mut supports,
                &mut edge_supports,
                edge_faces,
                edge,
                HashSet::new(),
            )?
            .is_none()
            {
                return Ok(None);
            }
            fixed[edge] = true;
            changed = true;
        }
        if explicit_support_complete {
            for face in 0..face_count {
                for point in degree_one_points(&degrees, face) {
                    match supports[face].get(&point).copied().unwrap_or(0) {
                        0 => return Ok(None),
                        1 => {
                            let Some(edge) =
                                sole_supporting_edge(&face_edges, &edge_supports, face, point)
                            else {
                                return Ok(None);
                            };
                            let before = choices[edge].len();
                            choices[edge].retain(|pair| pair.contains(&point));
                            if choices[edge].is_empty() {
                                return Ok(None);
                            }
                            changed |= choices[edge].len() != before;
                            if remove_edge_support(
                                ctx,
                                &mut supports,
                                &mut edge_supports,
                                edge_faces,
                                edge,
                                choice_points(ctx, &choices[edge])?,
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

fn incidence_choice_components<'storage>(
    ctx: &'storage DecodeContext<'_>,
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    boundary_domains: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient<'storage>>,
) -> Result<Vec<Vec<usize>>, CodecError> {
    let mut union = UnionFind::charged(ctx, choices.len(), "catia_incidence_choice_union")?;
    let mut point_nodes = HashMap::<(usize, usize), usize>::new();
    for (edge, pairs) in choices.iter().enumerate() {
        for (rank, face) in edge_faces[edge].into_iter().enumerate() {
            if rank > 0 && face == edge_faces[edge][0] {
                continue;
            }
            for point in pairs.iter().flatten().copied() {
                let next = point_nodes.len();
                if !point_nodes.contains_key(&(face, point)) {
                    ctx.insert_hash_map(
                        &mut point_nodes,
                        (face, point),
                        next,
                        "catia_incidence_point_nodes",
                    )?;
                }
            }
        }
    }
    let mut fixed_incidence =
        UnionFind::charged(ctx, point_nodes.len(), "catia_incidence_fixed_union")?;
    for (edge, pairs) in choices.iter().enumerate() {
        let [pair] = pairs.as_slice() else {
            continue;
        };
        for (rank, face) in edge_faces[edge].into_iter().enumerate() {
            if rank > 0 && face == edge_faces[edge][0] {
                continue;
            }
            fixed_incidence.union(
                ctx,
                point_nodes[&(face, pair[0])],
                point_nodes[&(face, pair[1])],
            )?;
        }
    }
    let mut owner = HashMap::<(usize, usize), usize>::new();
    let mut ambiguous = Vec::new();
    for (edge, pairs) in choices.iter().enumerate() {
        if pairs.len() > 1 || (pairs.is_empty() && mesh_quotient.is_some()) {
            ctx.push_vec(&mut ambiguous, edge, "catia_incidence_ambiguous_edges")?;
        }
    }
    for &edge in &ambiguous {
        let faces = edge_faces[edge];
        for (rank, face) in faces.into_iter().enumerate() {
            if rank > 0 && face == faces[0] {
                continue;
            }
            for point in choices[edge].iter().flatten().copied() {
                let point = fixed_incidence.find(ctx, point_nodes[&(face, point)])?;
                if let Some(&previous) = owner.get(&(face, point)) {
                    union.union(ctx, previous, edge)?;
                } else {
                    ctx.insert_hash_map(
                        &mut owner,
                        (face, point),
                        edge,
                        "catia_incidence_choice_owner",
                    )?;
                }
            }
        }
    }
    if let Some(domains) = boundary_domains {
        let mut connect = |mut edges: Vec<usize>| -> Result<(), CodecError> {
            ctx.sort_unstable_by(
                &mut edges,
                Ord::cmp,
                |_| 0,
                "catia_incidence_boundary_edges_sort",
            )?;
            edges.dedup();
            let mut ambiguous = edges.into_iter().filter(|edge| {
                choices[*edge].len() > 1 || (choices[*edge].is_empty() && mesh_quotient.is_some())
            });
            if let Some(first) = ambiguous.next() {
                for edge in ambiguous {
                    union.union(ctx, first, edge)?;
                }
            }
            Ok(())
        };
        for domain in domains {
            match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) if assignments.len() == 1 => {
                    for boundary in &assignments[0].boundaries {
                        let mut edges = Vec::new();
                        for use_ in boundary {
                            ctx.push_vec(&mut edges, use_.edge, "catia_incidence_boundary_edges")?;
                        }
                        connect(edges)?;
                    }
                }
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    let mut edges = Vec::new();
                    for use_ in assignments
                        .iter()
                        .flat_map(|assignment| assignment.boundaries.iter().flatten())
                    {
                        ctx.push_vec(&mut edges, use_.edge, "catia_incidence_boundary_edges")?;
                    }
                    connect(edges)?;
                }
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                    connect(ctx.copy_slice(edges, "catia_incidence_boundary_edges")?)?;
                }
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    let mut edges =
                        ctx.copy_slice(&domain.missing_edges, "catia_incidence_boundary_edges")?;
                    for (use_, _) in domain.cycles.iter().flat_map(|cycle| &cycle.exact_uses) {
                        ctx.push_vec(&mut edges, use_.edge, "catia_incidence_boundary_edges")?;
                    }
                    connect(edges)?;
                }
            }
        }
    }
    if let Some(mesh_quotient) = mesh_quotient {
        if choices.len().checked_mul(2) == Some(mesh_quotient.len()) {
            let mut owner = HashMap::<usize, usize>::new();
            for &edge in &ambiguous {
                for port in [edge * 2, edge * 2 + 1] {
                    let root = mesh_quotient.root(ctx, port)?;
                    for point in mesh_quotient.domains()[root].iter().copied() {
                        if let Some(&previous) = owner.get(&point) {
                            union.union(ctx, previous, edge)?;
                        } else {
                            ctx.insert_hash_map(
                                &mut owner,
                                point,
                                edge,
                                "catia_incidence_quotient_owner",
                            )?;
                        }
                    }
                }
            }
        }
    }
    let mut by_root = HashMap::<usize, Vec<usize>>::new();
    for edge in ambiguous {
        let root = union.find(ctx, edge)?;
        if let Some(group) = by_root.get_mut(&root) {
            ctx.push_vec(group, edge, "catia_incidence_component_edges")?;
        } else {
            let mut group = Vec::new();
            ctx.push_vec(&mut group, edge, "catia_incidence_component_edges")?;
            ctx.insert_hash_map(&mut by_root, root, group, "catia_incidence_component_roots")?;
        }
    }
    let mut components = Vec::new();
    for group in by_root.into_values() {
        ctx.push_vec(&mut components, group, "catia_incidence_components")?;
    }
    for component in &mut components {
        ctx.sort_unstable_by(
            &mut *component,
            Ord::cmp,
            |_| 0,
            "catia_incidence_component_sort",
        )?;
    }
    ctx.stable_sort_by(
        &mut components,
        |left, right| left[0].cmp(&right[0]),
        |_| 0,
        "catia_incidence_components_sort",
    )?;
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
    let mut component_by_edge = HashMap::new();
    for (component, edges) in components.iter().enumerate() {
        for &edge in edges {
            ctx.insert_hash_map(
                &mut component_by_edge,
                edge,
                component,
                "catia_incidence_component_index",
            )?;
        }
    }
    let mut union = UnionFind::charged(ctx, components.len(), "catia_incidence_coupling_union")?;
    let mut coupled_owner = None;
    for (edge, active) in coupled_edges.iter().copied().enumerate() {
        if !active {
            continue;
        }
        let Some(&component) = component_by_edge.get(&edge) else {
            continue;
        };
        if let Some(owner) = coupled_owner {
            union.union(ctx, owner, component)?;
        } else {
            coupled_owner = Some(component);
        }
    }
    let mut joined = HashMap::<usize, (usize, Vec<usize>)>::new();
    for (index, component) in components.into_iter().enumerate() {
        let root = union.find(ctx, index)?;
        if let Some(entry) = joined.get_mut(&root) {
            entry.0 = entry.0.min(index);
            ctx.reserve_vec(
                &mut entry.1,
                component.len(),
                "catia_incidence_joined_edges",
            )?;
            entry.1.extend(component);
        } else {
            ctx.insert_hash_map(
                &mut joined,
                root,
                (index, component),
                "catia_incidence_joined_roots",
            )?;
        }
    }
    let mut joined_values = Vec::new();
    for value in joined.into_values() {
        ctx.push_vec(&mut joined_values, value, "catia_incidence_joined_groups")?;
    }
    for (_, edges) in &mut joined_values {
        ctx.sort_unstable_by(
            &mut *edges,
            Ord::cmp,
            |_| 0,
            "catia_incidence_joined_edges_sort",
        )?;
    }
    ctx.sort_unstable_by(
        &mut joined_values,
        |left, right| left.0.cmp(&right.0),
        |_| 0,
        "catia_incidence_joined_groups_sort",
    )?;
    let mut output = Vec::new();
    for (_, edges) in joined_values {
        ctx.push_vec(&mut output, edges, "catia_incidence_joined_components")?;
    }
    Ok(output)
}

fn order_incidence_components_by_branch_width(
    ctx: &DecodeContext<'_>,
    components: &mut [Vec<usize>],
    choices: &[Vec<[usize; 2]>],
) -> Result<Option<()>, CodecError> {
    if components
        .iter()
        .flatten()
        .any(|edge| *edge >= choices.len())
    {
        return Ok(None);
    }
    let branch_width = |component: &[usize]| {
        component.iter().try_fold(1usize, |width, edge| {
            width.checked_mul(choices[*edge].len())
        })
    };
    let order_key = |component: &Vec<usize>| {
        (
            branch_width(component).is_none(),
            branch_width(component),
            component.len(),
            component.first().copied().unwrap_or_default(),
        )
    };
    ctx.stable_sort_by(
        components,
        |left, right| order_key(left).cmp(&order_key(right)),
        |_| 0,
        "catia incidence component branch width sort",
    )?;
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
    let assignment_predecessors = assignment_order.and_then(AssignmentOrder::predecessors);
    let assignment_dependencies = assignment_order.and_then(AssignmentOrder::dependencies);
    if components
        .iter()
        .flatten()
        .any(|edge| *edge >= choices.len())
        || assignment_predecessors.is_some_and(|predecessors| {
            predecessors.len() != choices.len()
                || predecessors
                    .iter()
                    .flatten()
                    .any(|edge| *edge >= choices.len())
        })
        || assignment_dependencies.is_some_and(|dependencies| {
            dependencies.len() != choices.len()
                || dependencies
                    .iter()
                    .flatten()
                    .any(|edge| *edge >= choices.len())
        })
    {
        return Ok(None);
    }
    if assignment_order.is_none() {
        return order_incidence_components_by_branch_width(ctx, components, choices);
    }

    let edge_entry_count = components
        .iter()
        .try_fold(0usize, |count, edges| count.checked_add(edges.len()))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia incidence component edge indices", u64::MAX, u64::MAX)
        })?;
    let mut component_by_edge = HashMap::new();
    ctx.reserve_map(
        &mut component_by_edge,
        edge_entry_count,
        "catia incidence component edge indices",
    )?;
    for (component, edges) in components.iter().enumerate() {
        for edge in edges.iter().copied() {
            component_by_edge.insert(edge, component);
        }
    }
    if component_by_edge.len() != edge_entry_count {
        return Ok(None);
    }
    let mut incoming =
        ctx.alloc_filled(components.len(), 0usize, "catia_incidence_component_in")?;
    let mut outgoing = ctx.alloc_filled(
        components.len(),
        Vec::<usize>::new(),
        "catia_incidence_component_out",
    )?;
    let mut local_incoming = ctx.alloc_filled(choices.len(), 0usize, "catia_incidence_local_in")?;
    let mut local_outgoing = ctx.alloc_filled(
        choices.len(),
        Vec::<usize>::new(),
        "catia_incidence_local_out",
    )?;
    let mut add_dependency =
        |target_edge: usize, prerequisite_edge: usize| -> Result<(), CodecError> {
            let (Some(&target_component), Some(&prerequisite_component)) = (
                component_by_edge.get(&target_edge),
                component_by_edge.get(&prerequisite_edge),
            ) else {
                return Ok(());
            };
            if target_component == prerequisite_component {
                if !local_outgoing[prerequisite_edge].contains(&target_edge) {
                    ctx.push_vec(
                        &mut local_outgoing[prerequisite_edge],
                        target_edge,
                        "catia incidence local dependents",
                    )?;
                    local_incoming[target_edge] += 1;
                }
                return Ok(());
            }
            if !outgoing[prerequisite_component].contains(&target_component) {
                ctx.push_vec(
                    &mut outgoing[prerequisite_component],
                    target_component,
                    "catia incidence component dependents",
                )?;
                incoming[target_component] += 1;
            }
            Ok(())
        };
    if let Some(predecessors) = assignment_predecessors {
        for (target, prerequisite) in predecessors.iter().enumerate() {
            if let Some(prerequisite) = prerequisite {
                add_dependency(target, *prerequisite)?;
            }
        }
    }
    if let Some(dependencies) = assignment_dependencies {
        for (target, prerequisites) in dependencies.iter().enumerate() {
            for prerequisite in prerequisites {
                add_dependency(target, *prerequisite)?;
            }
        }
    }
    let local_ready_count = component_by_edge
        .keys()
        .filter(|edge| local_incoming[**edge] == 0)
        .count();
    let mut local_ready = Vec::new();
    ctx.reserve_vec(
        &mut local_ready,
        local_ready_count,
        "catia incidence local ready edges",
    )?;
    local_ready.extend(
        component_by_edge
            .keys()
            .copied()
            .filter(|edge| local_incoming[*edge] == 0),
    );
    let mut local_ordered = 0usize;
    while let Some(edge) = local_ready.pop() {
        local_ordered += 1;
        for dependent in local_outgoing[edge].iter().copied() {
            local_incoming[dependent] -= 1;
            if local_incoming[dependent] == 0 {
                ctx.push_vec(
                    &mut local_ready,
                    dependent,
                    "catia incidence local ready edges",
                )?;
            }
        }
    }
    if local_ordered != component_by_edge.len() {
        return Ok(None);
    }

    let branch_width = |component: &[usize]| {
        component.iter().try_fold(1usize, |width, edge| {
            width.checked_mul(choices[*edge].len())
        })
    };
    let ready_count = (0..components.len())
        .filter(|component| incoming[*component] == 0)
        .count();
    let mut ready = Vec::new();
    ctx.reserve_vec(&mut ready, ready_count, "catia incidence ready components")?;
    ready.extend((0..components.len()).filter(|component| incoming[*component] == 0));
    let mut ordered = Vec::new();
    ctx.reserve_vec(
        &mut ordered,
        components.len(),
        "catia incidence ordered components",
    )?;
    while let Some((position, &component)) =
        ready.iter().enumerate().min_by_key(|(_, component)| {
            (
                branch_width(&components[**component]).is_none(),
                branch_width(&components[**component]),
                components[**component].len(),
                components[**component].first().copied().unwrap_or_default(),
            )
        })
    {
        ready.swap_remove(position);
        ordered.push(std::mem::take(&mut components[component]));
        for dependent in outgoing[component].iter().copied() {
            incoming[dependent] -= 1;
            if incoming[dependent] == 0 {
                ctx.push_vec(&mut ready, dependent, "catia incidence ready components")?;
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
        required_point: Option<usize>,
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

struct AppliedFaceConfiguration {
    assigned: Vec<(usize, [usize; 2], IncidenceDegreeUndo)>,
    affected_faces: Vec<usize>,
    coordinate_domains: Option<Arc<MeshCoordinateRootDomains>>,
    factor_checkpoint: Option<FaceFactorCheckpoint>,
}

struct IncidenceDegreeUndo {
    entries: Vec<(usize, usize, Option<u8>)>,
}

struct FaceFactorCheckpoint {
    active: Vec<Vec<u64>>,
}

enum FaceFactorRefinement {
    Rejected,
    Untracked,
    Tracked(FaceFactorCheckpoint),
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
    active: Option<Vec<Vec<u64>>>,
}

fn full_configuration_mask(ctx: &DecodeContext<'_>, len: usize) -> Result<Vec<u64>, CodecError> {
    let mut mask = ctx.alloc_filled(
        len.div_ceil(index_from_u32(u64::BITS)),
        u64::MAX,
        "catia_face_config_full_mask",
    )?;
    if let Some(last) = mask.last_mut() {
        let remainder = len % index_from_u32(u64::BITS);
        if remainder != 0 {
            *last = (1 << remainder) - 1;
        }
    }
    Ok(mask)
}

fn set_mask_bit<K: Eq + std::hash::Hash>(
    ctx: &DecodeContext<'_>,
    map: &mut HashMap<K, Vec<u64>>,
    key: K,
    word: usize,
    bit: u64,
    word_count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(mask) = map.get_mut(&key) {
        mask[word] |= bit;
    } else {
        let mut mask = ctx.alloc_filled(word_count, 0u64, operation)?;
        mask[word] |= bit;
        ctx.insert_hash_map(map, key, mask, "catia face configuration mask keys")?;
    }
    Ok(())
}

fn configuration_mask_contains(mask: &[u64], index: usize) -> bool {
    mask.get(index / index_from_u32(u64::BITS))
        .is_some_and(|word| word & (1 << (index % index_from_u32(u64::BITS))) != 0)
}

impl FaceFactorGraph {
    fn compile(
        ctx: &DecodeContext<'_>,
        domains: &[MeshFaceEndpointConfigurations],
        budget: &WorkBudget<'_>,
    ) -> Result<Option<Self>, CodecError> {
        let mut edge_sets = Vec::new();
        ctx.reserve_vec(&mut edge_sets, domains.len(), "catia face factor edge sets")?;
        for domain in domains {
            let mut edges = HashSet::new();
            for &(edge, _) in domain.iter().flatten() {
                ctx.insert_hash_set(&mut edges, edge, "catia face factor edges")?;
            }
            edge_sets.push(edges);
        }
        let mut right_indexes = Vec::new();
        ctx.reserve_vec(
            &mut right_indexes,
            domains.len(),
            "catia face factor right indexes",
        )?;
        for domain in domains {
            let word_count = domain.len().div_ceil(index_from_u32(u64::BITS));
            let mut present = HashMap::<usize, Vec<u64>>::new();
            let mut matching = HashMap::<(usize, [usize; 2]), Vec<u64>>::new();
            for (configuration, candidate) in domain.iter().enumerate() {
                if !budget.charge_by(work_units(candidate.len())) {
                    return Ok(None);
                }
                let word = configuration / index_from_u32(u64::BITS);
                let bit = 1 << (configuration % index_from_u32(u64::BITS));
                for &(edge, pair) in candidate {
                    set_mask_bit(
                        ctx,
                        &mut present,
                        edge,
                        word,
                        bit,
                        word_count,
                        "catia_face_config_present",
                    )?;
                    set_mask_bit(
                        ctx,
                        &mut matching,
                        (edge, pair),
                        word,
                        bit,
                        word_count,
                        "catia_face_config_matching",
                    )?;
                }
            }
            right_indexes.push((present, matching));
        }
        let mut arcs = Vec::new();
        let mut incoming =
            ctx.alloc_filled(domains.len(), Vec::new(), "catia_face_factor_incoming")?;
        for left in 0..domains.len() {
            for right in 0..domains.len() {
                if left == right || edge_sets[left].is_disjoint(&edge_sets[right]) {
                    continue;
                }
                let word_count = domains[right].len().div_ceil(index_from_u32(u64::BITS));
                let (present, matching) = &right_indexes[right];
                let mut supports = Vec::new();
                ctx.reserve_vec(
                    &mut supports,
                    domains[left].len(),
                    "catia face factor support rows",
                )?;
                for candidate in &domains[left] {
                    let Some(work) = candidate.len().checked_add(word_count) else {
                        return Ok(None);
                    };
                    if !budget.charge_by(work_units(work)) {
                        return Ok(None);
                    }
                    let mut compatible = full_configuration_mask(ctx, domains[right].len())?;
                    for &(edge, pair) in candidate {
                        let Some(edge_present) = present.get(&edge) else {
                            continue;
                        };
                        let edge_matching = matching.get(&(edge, pair));
                        for word in 0..word_count {
                            compatible[word] &=
                                !edge_present[word] | edge_matching.map_or(0, |mask| mask[word]);
                        }
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
        let mut domain_lengths = Vec::new();
        ctx.reserve_vec(
            &mut domain_lengths,
            domains.len(),
            "catia face factor domain lengths",
        )?;
        domain_lengths.extend(domains.iter().map(Vec::len));
        Ok(Some(Self {
            arcs,
            incoming,
            domain_lengths,
        }))
    }

    fn full_state(&self, ctx: &DecodeContext<'_>) -> Result<Vec<Vec<u64>>, CodecError> {
        let mut rows = Vec::new();
        ctx.reserve_vec(
            &mut rows,
            self.domain_lengths.len(),
            "catia face factor active rows",
        )?;
        for &length in &self.domain_lengths {
            rows.push(full_configuration_mask(ctx, length)?);
        }
        Ok(rows)
    }

    fn propagate(
        &self,
        ctx: &DecodeContext<'_>,
        active: &mut [Vec<u64>],
        initial: impl IntoIterator<Item = usize>,
        budget: &WorkBudget<'_>,
    ) -> Result<Option<bool>, CodecError> {
        let mut queue = VecDeque::new();
        for arc in initial {
            ctx.push_back(&mut queue, arc, "catia face factor propagation queue")?;
        }
        while let Some(arc_index) = queue.pop_front() {
            let arc = &self.arcs[arc_index];
            let mut changed = false;
            for (configuration, supports) in arc.supports.iter().enumerate() {
                if !configuration_mask_contains(&active[arc.left], configuration) {
                    continue;
                }
                if !budget.charge_by(work_units(supports.len())) {
                    return Ok(None);
                }
                if supports
                    .iter()
                    .zip(&active[arc.right])
                    .any(|(supports, active)| supports & active != 0)
                {
                    continue;
                }
                active[arc.left][configuration / index_from_u32(u64::BITS)] &=
                    !(1 << (configuration % index_from_u32(u64::BITS)));
                changed = true;
            }
            if !changed {
                continue;
            }
            if active[arc.left].iter().all(|word| *word == 0) {
                return Ok(Some(false));
            }
            for &incoming in &self.incoming[arc.left] {
                ctx.push_back(&mut queue, incoming, "catia face factor propagation queue")?;
            }
        }
        Ok(Some(true))
    }

    fn propagate_all(
        &self,
        ctx: &DecodeContext<'_>,
        active: &mut [Vec<u64>],
        budget: &WorkBudget<'_>,
    ) -> Result<Option<bool>, CodecError> {
        self.propagate(ctx, active, 0..self.arcs.len(), budget)
    }

    fn propagate_from(
        &self,
        ctx: &DecodeContext<'_>,
        domain: usize,
        active: &mut [Vec<u64>],
        budget: &WorkBudget<'_>,
    ) -> Result<Option<bool>, CodecError> {
        self.propagate(ctx, active, self.incoming[domain].iter().copied(), budget)
    }
}

impl PreparedFaceFactors {
    #[cfg(test)]
    fn domains(&self) -> &[Option<MeshFaceEndpointConfigurations>] {
        &self.domains
    }

    fn refine_edges(
        &mut self,
        ctx: &DecodeContext<'_>,
        assigned: &[(usize, [usize; 2])],
    ) -> Result<FaceFactorRefinement, CodecError> {
        let Some(active) = &mut self.active else {
            return Ok(FaceFactorRefinement::Untracked);
        };
        let checkpoint = FaceFactorCheckpoint {
            active: ctx.copy_retained_rows(
                active,
                "catia_face_factor_checkpoint_rows",
                "catia_face_factor_checkpoint_words",
            )?,
        };
        for &(edge, pair) in assigned {
            let Some(factors) = self.factors_by_edge.get(edge) else {
                continue;
            };
            for &factor in factors {
                let face = self.factor_faces[factor];
                let Some(configurations) = self.domains.get(face).and_then(Option::as_ref) else {
                    self.active = Some(checkpoint.active);
                    return Ok(FaceFactorRefinement::Rejected);
                };
                for (configuration, pairs) in configurations.iter().enumerate() {
                    if !configuration_mask_contains(&active[factor], configuration)
                        || pairs.iter().all(|(candidate_edge, candidate_pair)| {
                            *candidate_edge != edge || same_unordered_pair(*candidate_pair, pair)
                        })
                    {
                        continue;
                    }
                    active[factor][configuration / index_from_u32(u64::BITS)] &=
                        !(1 << (configuration % index_from_u32(u64::BITS)));
                }
                if active[factor].iter().all(|word| *word == 0) {
                    self.active = Some(checkpoint.active);
                    return Ok(FaceFactorRefinement::Rejected);
                }
            }
        }
        Ok(FaceFactorRefinement::Tracked(checkpoint))
    }

    fn restore(&mut self, checkpoint: Option<FaceFactorCheckpoint>) {
        if let Some(checkpoint) = checkpoint {
            self.active = Some(checkpoint.active);
        }
    }

    fn face_has_active_configuration(&self, face: usize) -> Option<bool> {
        let factor = self.factor_by_face.get(face).copied().flatten()?;
        let active = self.active.as_ref()?.get(factor)?;
        Some(active.iter().any(|word| *word != 0))
    }

    fn face_candidate_has_active_configuration(
        &self,
        face: usize,
        edge: usize,
        pair: [usize; 2],
    ) -> Option<bool> {
        let factor = self.factor_by_face.get(face).copied().flatten()?;
        if !self.factors_by_edge.get(edge)?.contains(&factor) {
            return self.face_has_active_configuration(face);
        }
        let active = self.active.as_ref()?.get(factor)?;
        let configurations = self.domains.get(face)?.as_ref()?;
        Some(
            configurations
                .iter()
                .enumerate()
                .any(|(configuration, pairs)| {
                    configuration_mask_contains(active, configuration)
                        && pairs.iter().any(|(candidate_edge, candidate_pair)| {
                            *candidate_edge == edge && same_unordered_pair(*candidate_pair, pair)
                        })
                }),
        )
    }
}

fn retain_configuration_masks(domains: &mut [MeshFaceEndpointConfigurations], active: &[Vec<u64>]) {
    for (domain, mask) in domains.iter_mut().zip(active) {
        let mut index = 0;
        domain.retain(|_| {
            let retain = configuration_mask_contains(mask, index);
            index += 1;
            retain
        });
    }
}

fn prune_face_configuration_support(
    ctx: &DecodeContext<'_>,
    domains: &mut [MeshFaceEndpointConfigurations],
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    let mut edge_sets = Vec::new();
    ctx.reserve_vec(
        &mut edge_sets,
        domains.len(),
        "catia face configuration edge sets",
    )?;
    for domain in domains.iter() {
        let mut edges = HashSet::new();
        for &(edge, _) in domain.iter().flatten() {
            ctx.insert_hash_set(&mut edges, edge, "catia face configuration edges")?;
        }
        edge_sets.push(edges);
    }
    let mut neighbors =
        ctx.alloc_filled(domains.len(), Vec::new(), "catia_face_config_neighbors")?;
    let mut queue = VecDeque::new();
    for left in 0..domains.len() {
        for right in 0..domains.len() {
            if left != right && !edge_sets[left].is_disjoint(&edge_sets[right]) {
                ctx.push_vec(
                    &mut neighbors[left],
                    right,
                    "catia face configuration neighbors",
                )?;
                ctx.push_back(&mut queue, (left, right), "catia face configuration queue")?;
            }
        }
    }
    while let Some((left, right)) = queue.pop_front() {
        let word_count = domains[right].len().div_ceil(index_from_u32(u64::BITS));
        let mut present = HashMap::<usize, Vec<u64>>::new();
        let mut matching = HashMap::<(usize, [usize; 2]), Vec<u64>>::new();
        for (configuration, candidate) in domains[right].iter().enumerate() {
            if !budget.charge_by(work_units(candidate.len())) {
                return Ok(true);
            }
            let word = configuration / index_from_u32(u64::BITS);
            let bit = 1 << (configuration % index_from_u32(u64::BITS));
            for &(edge, pair) in candidate {
                set_mask_bit(
                    ctx,
                    &mut present,
                    edge,
                    word,
                    bit,
                    word_count,
                    "catia_face_config_present",
                )?;
                set_mask_bit(
                    ctx,
                    &mut matching,
                    (edge, pair),
                    word,
                    bit,
                    word_count,
                    "catia_face_config_matching",
                )?;
            }
        }
        let mut keep = Vec::new();
        ctx.reserve_vec(
            &mut keep,
            domains[left].len(),
            "catia face configuration keep marks",
        )?;
        for candidate in &domains[left] {
            let Some(work) = candidate.len().checked_add(word_count) else {
                return Ok(true);
            };
            if !budget.charge_by(work_units(work)) {
                return Ok(true);
            }
            let mut viable = ctx.alloc_filled(word_count, u64::MAX, "catia_face_config_viable")?;
            if let Some(last) = viable.last_mut() {
                let remainder = domains[right].len() % index_from_u32(u64::BITS);
                if remainder != 0 {
                    *last = (1 << remainder) - 1;
                }
            }
            for &(edge, pair) in candidate {
                let Some(edge_present) = present.get(&edge) else {
                    continue;
                };
                let edge_matching = matching.get(&(edge, pair));
                for word in 0..word_count {
                    viable[word] &=
                        !edge_present[word] | edge_matching.map_or(0, |matching| matching[word]);
                }
                if viable.iter().all(|word| *word == 0) {
                    break;
                }
            }
            keep.push(viable.iter().any(|word| *word != 0));
        }
        if keep.iter().all(|supported| !supported) {
            return Ok(false);
        }
        if keep.iter().any(|supported| !supported) {
            let mut index = 0;
            domains[left].retain(|_| {
                let retain = keep[index];
                index += 1;
                retain
            });
            for &neighbor in &neighbors[left] {
                if neighbor != right {
                    ctx.push_back(
                        &mut queue,
                        (neighbor, left),
                        "catia face configuration queue",
                    )?;
                }
            }
        }
    }
    Ok(true)
}

fn prune_face_configuration_singleton_support(
    ctx: &DecodeContext<'_>,
    domains: &mut [MeshFaceEndpointConfigurations],
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    let Some(graph) = FaceFactorGraph::compile(ctx, domains, budget)? else {
        return Ok(true);
    };
    let mut active = graph.full_state(ctx)?;
    let active_clone_work = work_units(active.iter().map(Vec::len).sum::<usize>());
    match graph.propagate_all(ctx, &mut active, budget)? {
        Some(true) => {}
        Some(false) => return Ok(false),
        None => return Ok(true),
    }
    loop {
        let mut changed = false;
        let mut order = Vec::new();
        ctx.reserve_vec(
            &mut order,
            domains.len(),
            "catia face configuration singleton order",
        )?;
        order.extend(0..domains.len());
        ctx.sort_unstable_by(
            &mut order,
            |left, right| {
                let count = |domain: &usize| {
                    active[*domain]
                        .iter()
                        .map(|word| index_from_u32(word.count_ones()))
                        .sum::<usize>()
                };
                count(left).cmp(&count(right))
            },
            |_| 0,
            "catia face configuration singleton order sort",
        )?;
        for domain in order {
            let mut domain_changed = false;
            let active_count = active[domain]
                .iter()
                .map(|word| index_from_u32(word.count_ones()))
                .sum::<usize>();
            if active_count <= 1 {
                continue;
            }
            for configuration in 0..domains[domain].len() {
                if !configuration_mask_contains(&active[domain], configuration) {
                    continue;
                }
                if !budget.charge_by(active_clone_work) {
                    retain_configuration_masks(domains, &active);
                    return Ok(true);
                }
                let mut trial = Vec::new();
                ctx.reserve_vec(
                    &mut trial,
                    active.len(),
                    "catia face configuration trial rows",
                )?;
                for mask in &active {
                    trial.push(ctx.copy_slice(mask, "catia face configuration trial masks")?);
                }
                trial[domain].fill(0);
                trial[domain][configuration / index_from_u32(u64::BITS)] =
                    1 << (configuration % index_from_u32(u64::BITS));
                match graph.propagate_from(ctx, domain, &mut trial, budget)? {
                    Some(true) => {}
                    Some(false) => {
                        active[domain][configuration / index_from_u32(u64::BITS)] &=
                            !(1 << (configuration % index_from_u32(u64::BITS)));
                        changed = true;
                        domain_changed = true;
                    }
                    None => {
                        retain_configuration_masks(domains, &active);
                        return Ok(true);
                    }
                }
            }
            if active[domain].iter().all(|word| *word == 0) {
                return Ok(false);
            }
            if domain_changed {
                match graph.propagate_from(ctx, domain, &mut active, budget)? {
                    Some(true) => {}
                    Some(false) => return Ok(false),
                    None => {
                        retain_configuration_masks(domains, &active);
                        return Ok(true);
                    }
                }
            }
        }
        if !changed {
            retain_configuration_masks(domains, &active);
            return Ok(true);
        }
    }
}

fn prune_ordered_face_endpoint_support(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    choices: &mut [Vec<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    loop {
        let mut changed = false;
        for domain in domains {
            let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                continue;
            };
            let use_count = assignments
                .iter()
                .flat_map(|assignment| assignment.boundaries.iter())
                .try_fold(0usize, |count, boundary| count.checked_add(boundary.len()))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("catia ordered face edges", u64::MAX, u64::MAX)
                })?;
            let mut edges = Vec::new();
            ctx.reserve_vec(&mut edges, use_count, "catia ordered face edges")?;
            edges.extend(
                assignments
                    .iter()
                    .flat_map(|assignment| assignment.boundaries.iter().flatten())
                    .map(|use_| use_.edge),
            );
            ctx.sort_unstable_by(&mut edges, Ord::cmp, |_| 0, "catia ordered face edges sort")?;
            edges.dedup();
            if edges
                .iter()
                .any(|edge| choices.get(*edge).is_none_or(Vec::is_empty))
            {
                continue;
            }
            let selected = ctx.alloc_filled(choices.len(), None, "catia_ordered_face_selection")?;
            let Some(configurations) =
                mesh_face_endpoint_configurations(ctx, assignments, choices, &selected, budget)?
            else {
                if budget.exhausted() {
                    return Ok(true);
                }
                continue;
            };
            if configurations.is_empty() {
                return Ok(false);
            }
            let mut supported = HashMap::<usize, HashSet<[usize; 2]>>::new();
            for configuration in configurations {
                for (edge, pair) in configuration {
                    if !budget.charge() {
                        return Ok(true);
                    }
                    ctx.admit_hash_map_entry(
                        &mut supported,
                        &edge,
                        "catia ordered face support edges",
                    )?;
                    let pairs = supported.entry(edge).or_default();
                    ctx.insert_hash_set(pairs, pair, "catia ordered face support pairs")?;
                }
            }
            for edge in edges {
                let Some(edge_supported) = supported.get(&edge) else {
                    continue;
                };
                let mut retained = Vec::new();
                for pair in choices[edge].iter().copied() {
                    if !budget.charge() {
                        return Ok(true);
                    }
                    let mut canonical = pair;
                    ctx.sort_unstable_by(
                        &mut canonical,
                        Ord::cmp,
                        |_| 0,
                        "catia ordered face canonical pair sort",
                    )?;
                    if edge_supported.contains(&canonical) {
                        ctx.push_vec(&mut retained, pair, "catia ordered face retained pairs")?;
                    }
                }
                if retained.is_empty() {
                    return Ok(false);
                }
                if retained.len() != choices[edge].len() {
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

pub(super) fn prune_implicit_ordered_face_endpoint_support(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    choices: &mut [Vec<[usize; 2]>],
    coordinate_domains: &MeshCoordinateRootDomains,
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    loop {
        let mut changed = false;
        for domain in domains {
            let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                continue;
            };
            let mut face_support = HashMap::<usize, HashSet<[usize; 2]>>::new();
            let mut assignment_found = false;
            for assignment in assignments {
                let Some(support) = mesh_assignment_endpoint_cycle_support_by(
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
                )?
                else {
                    return Ok(true);
                };
                if support.by_edge.is_empty() {
                    continue;
                }
                assignment_found = true;
                for (edge, pairs) in support.by_edge {
                    ctx.admit_hash_map_entry(
                        &mut face_support,
                        &edge,
                        "catia implicit face support edges",
                    )?;
                    let retained = face_support.entry(edge).or_default();
                    for pair in pairs {
                        ctx.insert_hash_set(retained, pair, "catia implicit face support pairs")?;
                    }
                }
            }
            if !assignment_found {
                return Ok(false);
            }
            for (edge, supported) in face_support {
                let Some(current) = choices.get(edge) else {
                    return Ok(false);
                };
                let values = if current.is_empty() {
                    let Some(values) = coordinate_domains.implicit_edge_candidates(edge, None)
                    else {
                        return Ok(false);
                    };
                    let mut collected = Vec::new();
                    for value in values {
                        ctx.push_vec(&mut collected, value, "catia implicit face candidate pairs")?;
                    }
                    collected
                } else {
                    ctx.copy_slice(current, "catia implicit face current pairs")?
                };
                let mut retained = Vec::new();
                for mut pair in values {
                    if !budget.charge() {
                        return Ok(true);
                    }
                    ctx.sort_unstable_by(
                        &mut pair,
                        Ord::cmp,
                        |_| 0,
                        "catia implicit face pair sort",
                    )?;
                    if supported.contains(&pair) {
                        ctx.push_vec(&mut retained, pair, "catia implicit face retained pairs")?;
                    }
                }
                ctx.sort_unstable_by(
                    &mut retained,
                    Ord::cmp,
                    |_| 0,
                    "catia implicit face retained pairs sort",
                )?;
                retained.dedup();
                if retained.is_empty() {
                    return Ok(false);
                }
                if retained != choices[edge] {
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
    let Some(assignments) = assignments else {
        return Ok(None);
    };
    let mut domains = ctx.alloc_filled(assignments.len(), None, "catia_face_factor_domains")?;
    for (face, domain) in assignments.iter().enumerate() {
        let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
            continue;
        };
        let use_count = assignments
            .iter()
            .flat_map(|assignment| assignment.boundaries.iter())
            .try_fold(0usize, |count, boundary| count.checked_add(boundary.len()))
            .ok_or_else(|| ctx.refuse_codec_limit("catia face factor edges", u64::MAX, u64::MAX))?;
        let mut edges = Vec::new();
        ctx.reserve_vec(&mut edges, use_count, "catia face factor edges")?;
        edges.extend(
            assignments
                .iter()
                .flat_map(|assignment| assignment.boundaries.iter().flatten())
                .map(|use_| use_.edge)
                .filter(|edge| active.get(*edge) == Some(&true)),
        );
        ctx.sort_unstable_by(&mut edges, Ord::cmp, |_| 0, "catia face factor edges sort")?;
        edges.dedup();
        if edges.is_empty()
            || edges.iter().any(|edge| {
                selected.get(*edge).is_none()
                    || (selected[*edge].is_none() && choices[*edge].is_empty())
            })
        {
            continue;
        }
        let budget = WorkBudget::new(MAX_FACE_ENDPOINT_CONFIGURATION_WORK);
        let Some(configurations) =
            mesh_face_endpoint_configurations(ctx, assignments, choices, selected, &budget)?
        else {
            continue;
        };
        domains[face] = Some(configurations);
    }
    let mut retained_faces = Vec::new();
    ctx.reserve_vec(
        &mut retained_faces,
        domains.len(),
        "catia face factor retained faces",
    )?;
    retained_faces.extend(
        domains
            .iter()
            .enumerate()
            .filter_map(|(face, domain)| domain.as_ref().map(|_| face)),
    );
    let mut configurations = Vec::new();
    ctx.reserve_vec(
        &mut configurations,
        retained_faces.len(),
        "catia face factor configurations",
    )?;
    for face in &retained_faces {
        configurations.push(
            domains[*face]
                .as_mut()
                .map(std::mem::take)
                .unwrap_or_default(),
        );
    }
    let arc_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let viable = prune_face_configuration_support(ctx, &mut configurations, &arc_budget)?;
    let singleton_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
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
    let graph_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let graph = FaceFactorGraph::compile(ctx, &configurations, &graph_budget)?;
    let active = graph
        .as_ref()
        .map(|graph| graph.full_state(ctx))
        .transpose()?;
    let mut factor_by_face = ctx.alloc_filled(domains.len(), None, "catia_face_factor_by_face")?;
    let mut factors_by_edge =
        ctx.alloc_filled(choices.len(), Vec::new(), "catia_face_factors_by_edge")?;
    for (factor, &face) in retained_faces.iter().enumerate() {
        factor_by_face[face] = Some(factor);
        let indexed_edges = configurations[factor]
            .iter()
            .try_fold(0usize, |count, candidate| {
                count.checked_add(candidate.len())
            })
            .ok_or_else(|| {
                ctx.refuse_codec_limit("catia face factor indexed edges", u64::MAX, u64::MAX)
            })?;
        let mut edges = Vec::new();
        ctx.reserve_vec(&mut edges, indexed_edges, "catia face factor indexed edges")?;
        edges.extend(
            configurations[factor]
                .iter()
                .flatten()
                .map(|(edge, _)| *edge),
        );
        ctx.sort_unstable_by(
            &mut edges,
            Ord::cmp,
            |_| 0,
            "catia face factor indexed edges sort",
        )?;
        edges.dedup();
        for edge in edges {
            if let Some(factors) = factors_by_edge.get_mut(edge) {
                ctx.push_vec(factors, factor, "catia face factors by edge entries")?;
            }
        }
    }
    for (face, configurations) in retained_faces.iter().copied().zip(configurations) {
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
                required_point,
                next_index,
            } => {
                while let Some(&pair) = candidates.get(*next_index) {
                    *next_index += 1;
                    if required_point.is_none_or(|point| pair.contains(&point)) {
                        return Some(pair);
                    }
                }
                None
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
    let edges = match domain {
        MeshFaceBoundaryDomain::Ordered(_) => return Ok(true),
        MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
            let mut collected = Vec::new();
            ctx.reserve_vec(&mut collected, edges.len(), "catia compact viable edges")?;
            collected.extend_from_slice(edges);
            collected
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            let edge_count = domain
                .cycles
                .iter()
                .try_fold(domain.missing_edges.len(), |count, cycle| {
                    count.checked_add(cycle.exact_uses.len())
                })
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("catia compact viable edges", u64::MAX, u64::MAX)
                })?;
            let mut edges = Vec::new();
            ctx.reserve_vec(&mut edges, edge_count, "catia compact viable edges")?;
            edges.extend_from_slice(&domain.missing_edges);
            edges.extend(
                domain
                    .cycles
                    .iter()
                    .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
            );
            edges
        }
    };
    let mut selected_pairs = Vec::new();
    ctx.reserve_vec(
        &mut selected_pairs,
        edges.len(),
        "catia compact viable selected pairs",
    )?;
    selected_pairs.extend(edges.iter().copied().map(|edge| {
        selected
            .filter(|(selected_edge, _)| *selected_edge == edge)
            .map(|(_, pair)| pair)
            .or(assignment[edge])
            .map(|pair| (edge, pair))
    }));
    let complete = selected_pairs.iter().all(Option::is_some);
    if matches!(domain, MeshFaceBoundaryDomain::UnorderedFullCycle(_)) && !complete {
        let mut point_nodes = HashMap::new();
        let mut degrees = Vec::<u8>::new();
        let mut components = UnionFind::charged(ctx, 0, "catia_compact_boundary_union")?;
        for (_, pair) in selected_pairs.iter().flatten().copied() {
            let mut nodes = [0; 2];
            for (slot, point) in pair.into_iter().enumerate() {
                nodes[slot] = if let Some(&node) = point_nodes.get(&point) {
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
        let mut open_components = HashSet::new();
        for (node, degree) in degrees.into_iter().enumerate() {
            if degree < 2 {
                ctx.insert_hash_set(
                    &mut open_components,
                    components.find(ctx, node)?,
                    "catia_compact_boundary_open_components",
                )?;
            }
        }
        for node in 0..components.len() {
            if !open_components.contains(&components.find(ctx, node)?) {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    let mut complete_pairs = Vec::new();
    for pair in selected_pairs {
        let Some(pair) = pair else {
            return Ok(true);
        };
        ctx.push_vec(
            &mut complete_pairs,
            pair,
            "catia_compact_boundary_complete_pairs",
        )?;
    }
    let mut edge_points =
        ctx.alloc_filled(assignment.len(), [0; 2], "catia labeled edge points")?;
    for (edge, pair) in complete_pairs {
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

fn copy_quotient_states<'storage>(
    ctx: &'storage DecodeContext<'_>,
    states: &[MeshQuotientGaugeState<'storage>],
) -> Result<Vec<MeshQuotientGaugeState<'storage>>, CodecError> {
    let mut copy = Vec::new();
    ctx.reserve_vec(
        &mut copy,
        states.len(),
        "catia incidence quotient state rows",
    )?;
    for (quotient, oriented) in states {
        let mut oriented_copy = HashSet::new();
        ctx.reserve_set(
            &mut oriented_copy,
            oriented.len(),
            "catia incidence quotient oriented edges",
        )?;
        oriented_copy.extend(oriented.iter().copied());
        copy.push((quotient.clone_charged(ctx)?, oriented_copy));
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

    let mut ordered = Vec::<Vec<MeshFaceBoundaryAssignment>>::new();
    'domains: for domain in domains {
        let edge_count = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => assignments
                .iter()
                .flat_map(|assignment| &assignment.boundaries)
                .try_fold(0usize, |count, boundary| count.checked_add(boundary.len())),
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => Some(edges.len()),
            MeshFaceBoundaryDomain::DeferredValidation(domain) => domain
                .cycles
                .iter()
                .try_fold(domain.missing_edges.len(), |count, cycle| {
                    count.checked_add(cycle.exact_uses.len())
                }),
        }
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia compact boundary edges", u64::MAX, u64::MAX)
        })?;
        let mut edges = Vec::new();
        ctx.reserve_vec(&mut edges, edge_count, "catia compact boundary edges")?;
        match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => edges.extend(
                assignments
                    .iter()
                    .flat_map(|assignment| assignment.boundaries.iter().flatten())
                    .map(|use_| use_.edge),
            ),
            MeshFaceBoundaryDomain::UnorderedFullCycle(domain_edges) => {
                edges.extend_from_slice(domain_edges);
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                edges.extend_from_slice(&domain.missing_edges);
                edges.extend(
                    domain
                        .cycles
                        .iter()
                        .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
                );
            }
        }
        let mut edge_points = Vec::new();
        ctx.reserve_vec(
            &mut edge_points,
            edges.len(),
            "catia compact boundary selected edges",
        )?;
        for edge in edges {
            let Some(pair) = selected
                .filter(|(selected_edge, _)| *selected_edge == edge)
                .map(|(_, pair)| pair)
                .or(assignment[edge])
            else {
                continue 'domains;
            };
            edge_points.push((edge, pair));
        }
        let mut points = ctx.alloc_filled(
            assignment.len(),
            [0; 2],
            "catia compact boundary edge points",
        )?;
        for (edge, pair) in edge_points {
            points[edge] = pair;
        }
        let alternatives = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                let mut copied = Vec::new();
                ctx.reserve_vec(
                    &mut copied,
                    assignments.len(),
                    "catia compact boundary alternatives",
                )?;
                for alternative in assignments {
                    let mut boundaries = Vec::new();
                    ctx.reserve_vec(
                        &mut boundaries,
                        alternative.boundaries.len(),
                        "catia compact boundary alternative cycles",
                    )?;
                    for cycle in &alternative.boundaries {
                        boundaries.push(
                            ctx.copy_slice(cycle, "catia compact boundary alternative uses")?,
                        );
                    }
                    copied.push(MeshFaceBoundaryAssignment { boundaries });
                }
                copied
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                let Some(cycles) = incidence_cycles(ctx, edges, &points)? else {
                    return Ok(CompactBoundaryAdvanceOutcome::Rejected);
                };
                let [cycle] = cycles.as_slice() else {
                    return Ok(CompactBoundaryAdvanceOutcome::Rejected);
                };
                let mut uses = Vec::new();
                ctx.reserve_vec(
                    &mut uses,
                    cycle.len(),
                    "catia compact unordered boundary uses",
                )?;
                for (edge, _) in cycle {
                    uses.push(MeshBoundaryEdgeCandidate {
                        edge: *edge,
                        start: 0,
                        end: 0,
                        reversed: None,
                    });
                }
                let mut boundaries = Vec::new();
                ctx.push_vec(
                    &mut boundaries,
                    uses,
                    "catia compact unordered boundary cycles",
                )?;
                let mut alternatives = Vec::new();
                ctx.push_vec(
                    &mut alternatives,
                    MeshFaceBoundaryAssignment { boundaries },
                    "catia compact unordered boundary alternatives",
                )?;
                alternatives
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                let Some(materialized) = deferred_boundary_assignment(ctx, domain, &points)? else {
                    return Ok(CompactBoundaryAdvanceOutcome::Rejected);
                };
                let mut alternatives = Vec::new();
                ctx.push_vec(
                    &mut alternatives,
                    materialized,
                    "catia compact deferred alternative",
                )?;
                alternatives
            }
        };
        ctx.push_vec(
            &mut ordered,
            alternatives,
            "catia compact boundary domain rows",
        )?;
    }
    if ordered.is_empty() {
        return Ok(CompactBoundaryAdvanceOutcome::Complete(states));
    }
    ctx.charge_collection_items(
        u64_from_index(assignment.len()),
        "catia compact boundary candidate rows",
    )?;
    let candidate_count = assignment
        .iter()
        .enumerate()
        .try_fold(0usize, |count, (edge, pair)| {
            let options = if selected.is_some_and(|(selected_edge, _)| selected_edge == edge)
                || pair.is_some()
            {
                1
            } else {
                choices[edge].len()
            };
            count.checked_add(options)
        })
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia compact boundary candidate pairs", u64::MAX, u64::MAX)
        })?;
    ctx.charge_collection_items(
        u64_from_index(candidate_count),
        "catia compact boundary candidate pairs",
    )?;
    let mut candidates = Vec::new();
    for (edge, pair) in assignment.iter().enumerate() {
        let row = if let Some(pair) = selected
            .filter(|(selected_edge, _)| *selected_edge == edge)
            .map(|(_, pair)| pair)
            .or(*pair)
        {
            ctx.copy_slice(&[pair], "catia_compact_boundary_candidate_pair")?
        } else {
            ctx.copy_slice(&choices[edge], "catia_compact_boundary_candidate_pair")?
        };
        ctx.push_vec(
            &mut candidates,
            row,
            "catia_compact_boundary_candidate_rows",
        )?;
    }
    for alternatives in ordered {
        let mut next = Vec::new();
        let mut signatures = HashSet::new();
        for (state, oriented_edges) in states {
            for face in &alternatives {
                let Some(remaining) = MAX_QUOTIENT_STATES.checked_sub(next.len()) else {
                    return Ok(CompactBoundaryAdvanceOutcome::Exhausted);
                };
                for (_, mut candidate) in state.assignment_options_limited(
                    ctx,
                    face,
                    &candidates,
                    &oriented_edges,
                    remaining,
                    Some(budget),
                )? {
                    let mut next_oriented = HashSet::new();
                    for &edge in &oriented_edges {
                        ctx.insert_hash_set(
                            &mut next_oriented,
                            edge,
                            "catia_compact_boundary_oriented_copy",
                        )?;
                    }
                    for use_ in face.boundaries.iter().flatten() {
                        ctx.insert_hash_set(
                            &mut next_oriented,
                            use_.edge,
                            "catia_compact_boundary_oriented_edges",
                        )?;
                    }
                    let Some(work) = candidate
                        .signature_work(ctx)?
                        .and_then(|work| work.checked_add(work_units(next_oriented.len())))
                    else {
                        return Ok(CompactBoundaryAdvanceOutcome::Exhausted);
                    };
                    if !budget.charge_by(work) {
                        return Ok(CompactBoundaryAdvanceOutcome::Exhausted);
                    }
                    let mut oriented_signature = Vec::new();
                    for &edge in &next_oriented {
                        ctx.push_vec(
                            &mut oriented_signature,
                            edge,
                            "catia_compact_boundary_oriented_signature",
                        )?;
                    }
                    ctx.sort_unstable_by(
                        &mut oriented_signature,
                        Ord::cmp,
                        |_| 0,
                        "catia_compact_boundary_oriented_signature_sort",
                    )?;
                    if ctx.insert_hash_set(
                        &mut signatures,
                        (candidate.signature_charged(ctx)?, oriented_signature),
                        "catia_compact_boundary_signatures",
                    )? {
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

fn adjust_incidence_degrees(
    ctx: &DecodeContext<'_>,
    degrees: &mut [BTreeMap<usize, u8>],
    edge_faces: &[[usize; 2]],
    edge: usize,
    pair: [usize; 2],
) -> Result<IncidenceDegreeUndo, CodecError> {
    let mut undo = IncidenceDegreeUndo {
        entries: Vec::new(),
    };
    ctx.reserve_vec(&mut undo.entries, 4, "catia incidence degree undo entries")?;
    let faces = edge_faces[edge];
    for (rank, face) in faces.into_iter().enumerate() {
        if rank > 0 && face == faces[0] {
            continue;
        }
        for point in pair {
            let previous = degrees[face].get(&point).copied();
            ctx.admit_btree_entry(&degrees[face], &point, "catia incidence degree points")?;
            *degrees[face].entry(point).or_default() += 1;
            undo.entries.push((face, point, previous));
        }
    }
    Ok(undo)
}

fn restore_incidence_degrees(degrees: &mut [BTreeMap<usize, u8>], undo: IncidenceDegreeUndo) {
    for (face, point, previous) in undo.entries.into_iter().rev() {
        match previous {
            Some(degree) => {
                degrees[face].insert(point, degree);
            }
            None => {
                degrees[face].remove(&point);
            }
        }
    }
}

impl<'storage> IncidenceComponentSearch<'storage, '_> {
    fn candidate_pairs(
        &self,
        edge: usize,
        required_point: Option<usize>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> IncidenceCandidatePairs<'_> {
        if let Some(candidates) = coordinate_domains
            .filter(|_| self.choices[edge].is_empty())
            .and_then(|domains| domains.implicit_edge_candidates(edge, required_point))
        {
            return IncidenceCandidatePairs::Implicit(candidates);
        }
        if let Some(candidates) =
            required_point.and_then(|point| self.explicit_point_supports.get(edge)?.get(&point))
        {
            return IncidenceCandidatePairs::Options {
                candidates,
                required_point: None,
                next_index: 0,
            };
        }
        IncidenceCandidatePairs::Options {
            candidates: &self.choices[edge],
            required_point,
            next_index: 0,
        }
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

    fn degree(&self, face: usize, point: usize) -> u8 {
        self.degrees[face].get(&point).copied().unwrap_or_default()
    }

    fn remember_degree_support_witness(
        &self,
        face: usize,
        point: usize,
        witness: (usize, [usize; 2]),
    ) -> Result<(), CodecError> {
        let mut witnesses = self.degree_support_witnesses.borrow_mut();
        self.search_storage.borrow_mut().with_storage(|| {
            self.ctx.admit_hash_map_entry(
                &mut witnesses,
                &(face, point),
                "catia_incidence_witness_keys",
            )
        })?;
        let entry = witnesses.entry((face, point)).or_default();
        if !entry.contains(&witness) {
            self.search_storage.borrow_mut().with_storage(|| {
                self.ctx
                    .push_vec(entry, witness, "catia_incidence_witness_pairs")
            })?;
        }
        Ok(())
    }

    fn degree_candidate_fits(&self, edge: usize, pair: [usize; 2]) -> bool {
        let faces = self.edge_faces[edge];
        faces.into_iter().enumerate().all(|(rank, face)| {
            (rank > 0 && face == faces[0])
                || pair.iter().enumerate().all(|(point_rank, &point)| {
                    let multiplicity = 1 + usize::from(point_rank == 0 && pair[0] == pair[1]);
                    usize::from(self.degree(face, point)) + multiplicity <= 2
                })
        })
    }

    fn branch_edge_ready(&self, edge: usize) -> bool {
        let predecessor_ready = self
            .partial_solution_filter
            .and_then(|constraint| constraint.assignment_order)
            .and_then(AssignmentOrder::predecessors)
            .and_then(|predecessors| predecessors.get(edge).copied().flatten())
            .is_none_or(|predecessor| {
                self.assignment
                    .get(predecessor)
                    .is_some_and(Option::is_some)
            });
        let dependencies_ready = self
            .partial_solution_filter
            .and_then(|constraint| constraint.assignment_order)
            .and_then(AssignmentOrder::dependencies)
            .and_then(|dependencies| dependencies.get(edge))
            .is_none_or(|dependencies| {
                dependencies.iter().all(|&predecessor| {
                    self.assignment
                        .get(predecessor)
                        .is_some_and(Option::is_some)
                })
            });
        predecessor_ready && dependencies_ready
    }

    fn degree_frontiers_supported(
        &self,
        faces: &[usize],
        selected: Option<(usize, [usize; 2])>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<bool, CodecError> {
        let selected_degree = |face: usize, point: usize| {
            selected.map_or(0, |(edge, pair)| {
                let selected_faces = self.edge_faces[edge];
                usize::from(selected_faces[0] == face || selected_faces[1] == face)
                    * pair.iter().filter(|candidate| **candidate == point).count()
            })
        };
        let degree_after_selection = |face: usize, point: usize| {
            usize::from(self.degree(face, point)) + selected_degree(face, point)
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
        let supporting_point_fits = |supporting_edge: usize, point: usize| {
            let faces = self.edge_faces[supporting_edge];
            faces.into_iter().enumerate().all(|(rank, face)| {
                (rank > 0 && face == faces[0]) || degree_after_selection(face, point) < 2
            })
        };

        for &face in faces {
            let start = self
                .constraints
                .partition_point(|&(constraint_face, _)| constraint_face < face);
            let end = self.constraints[start..]
                .partition_point(|&(constraint_face, _)| constraint_face == face)
                + start;
            let constrained_points = &self.constraints[start..end];
            let support_exists = |point| -> Result<bool, CodecError> {
                let witnesses = self.degree_support_witnesses.borrow();
                for &(supporting_edge, supporting_pair) in
                    witnesses.get(&(face, point)).into_iter().flatten().rev()
                {
                    let candidate_still_available = self.choices[supporting_edge]
                        .contains(&supporting_pair)
                        || coordinate_domains
                            .filter(|_| self.choices[supporting_edge].is_empty())
                            .is_some_and(|domains| {
                                domains.supports_edge_candidate(supporting_edge, supporting_pair)
                            });
                    if !self.degree_support_budget.charge() {
                        return Ok(true);
                    }
                    if selected.is_none_or(|(edge, _)| supporting_edge != edge)
                        && self.active[supporting_edge]
                        && self.assignment[supporting_edge].is_none()
                        && candidate_still_available
                        && supporting_point_fits(supporting_edge, point)
                        && supporting_pair_fits(supporting_edge, supporting_pair)
                    {
                        return Ok(true);
                    }
                }
                drop(witnesses);
                let indexed_edges = self
                    .point_support_edges
                    .get(face)
                    .and_then(|by_point| by_point.get(&point));
                let supporting_edges =
                    indexed_edges.map_or(self.face_edges[face].as_slice(), Vec::as_slice);
                for &supporting_edge in supporting_edges {
                    if !self.degree_support_budget.charge() {
                        return Ok(true);
                    }
                    if selected.is_some_and(|(edge, _)| supporting_edge == edge)
                        || !self.active[supporting_edge]
                        || self.assignment[supporting_edge].is_some()
                        || !supporting_point_fits(supporting_edge, point)
                    {
                        continue;
                    }
                    let fits = |supporting_pair: [usize; 2]| {
                        supporting_pair_fits(supporting_edge, supporting_pair)
                    };
                    if let Some(domains) =
                        coordinate_domains.filter(|_| self.choices[supporting_edge].is_empty())
                    {
                        if let Some(witness) = domains.implicit_edge_candidate_with_point(
                            supporting_edge,
                            point,
                            Some(self.degree_support_budget),
                            fits,
                        ) {
                            self.remember_degree_support_witness(
                                face,
                                point,
                                (supporting_edge, witness),
                            )?;
                            return Ok(true);
                        }
                        if self.degree_support_budget.exhausted() {
                            return Ok(true);
                        }
                        continue;
                    }
                    for supporting_pair in self.candidate_pairs(supporting_edge, Some(point), None)
                    {
                        if !self.degree_support_budget.charge() {
                            return Ok(true);
                        }
                        if supporting_pair.contains(&point) && fits(supporting_pair) {
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
            };
            for &(_, point) in constrained_points {
                if degree_after_selection(face, point) == 1 && !support_exists(point)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn degree_support_preserved(
        &self,
        edge: usize,
        pair: [usize; 2],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<bool, CodecError> {
        let mut faces = self.edge_faces[edge];
        self.ctx.sort_unstable_by(
            &mut faces,
            Ord::cmp,
            |_| 0,
            "catia incidence degree support faces sort",
        )?;
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

    #[cfg(test)]
    fn degree_support_preserved_by_constraint_scan(
        &self,
        edge: usize,
        pair: [usize; 2],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> bool {
        let selected_faces = self.edge_faces[edge];
        let selected_degree = |face: usize, point: usize| {
            let incident = selected_faces[0] == face || selected_faces[1] == face;
            incident.then(|| pair.iter().filter(|candidate| **candidate == point).count())
        };
        let degree_after_selection = |face: usize, point: usize| {
            usize::from(self.degree(face, point)) + selected_degree(face, point).unwrap_or_default()
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
                                && self
                                    .candidate_pairs(
                                        supporting_edge,
                                        Some(point),
                                        coordinate_domains,
                                    )
                                    .any(|supporting_pair| {
                                        supporting_pair.contains(&point)
                                            && supporting_pair_fits(
                                                supporting_edge,
                                                supporting_pair,
                                            )
                                    })
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
            let mut faces = self.edge_faces[edge];
            self.ctx.sort_unstable_by(
                &mut faces,
                Ord::cmp,
                |_| 0,
                "catia incidence candidate fit faces sort",
            )?;
            let length = if faces[0] == faces[1] { 1 } else { 2 };
            for &face in &faces[..length] {
                let Some(domain) = mesh_assignments.get(face) else {
                    return Ok(false);
                };
                let mut refusal = None;
                let viable = match domain {
                    MeshFaceBoundaryDomain::Ordered(assignments) => self
                        .face_configuration_domains
                        .as_ref()
                        .and_then(|factors| {
                            factors.face_candidate_has_active_configuration(face, edge, pair)
                        })
                        .unwrap_or_else(|| {
                            assignments.iter().any(|assignment| {
                                mesh_assignment_endpoint_cycles_viable_by(
                                    self.ctx,
                                    assignment,
                                    Some(self.boundary_propagation_budget),
                                    |candidate_edge| {
                                        let selected = if candidate_edge == edge {
                                            Some(pair)
                                        } else {
                                            self.assignment.get(candidate_edge).copied().flatten()
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
                                            self.assignment.get(candidate_edge).copied().flatten()
                                        };
                                        selected.is_none_or(|selected| {
                                            same_unordered_pair(selected, candidate_pair)
                                        })
                                    },
                                )
                                .map_or_else(
                                    |error| {
                                        refusal = Some(error);
                                        true
                                    },
                                    |result| result.unwrap_or(true),
                                )
                            })
                        }),
                    _ => compact_boundary_domain_viable(
                        self.ctx,
                        domain,
                        &self.assignment,
                        Some((edge, pair)),
                    )?,
                };
                if let Some(error) = refusal {
                    return Err(error);
                }
                if !viable {
                    return Ok(false);
                }
            }
        }
        if !self.degree_candidate_fits(edge, pair) {
            return Ok(false);
        }
        if !self.degree_support_preserved(edge, pair, coordinate_domains)? {
            return Ok(false);
        }
        Ok(true)
    }

    fn constraint_options(
        &self,
        face: usize,
        point: usize,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        limit: Option<usize>,
        viability: &mut HashMap<MeshEndpointPair, bool>,
    ) -> Result<IncidenceConstraintOptions, CodecError> {
        let mut any_viable = false;
        let mut options = HashSet::new();
        for edge in self.face_edges[face]
            .iter()
            .copied()
            .filter(|&edge| self.active[edge] && self.assignment[edge].is_none())
        {
            for pair in self.candidate_pairs(edge, Some(point), coordinate_domains) {
                let viable = if let Some(&viable) = viability.get(&(edge, pair)) {
                    viable
                } else {
                    let viable = self.candidate_fits_in(edge, pair, coordinate_domains)?
                        && coordinate_domains
                            .is_none_or(|domains| domains.supports_edge_candidate(edge, pair));
                    self.search_storage.borrow_mut().with_storage(|| {
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
                if !self.branch_edge_ready(edge) {
                    continue;
                }
                self.search_storage.borrow_mut().with_storage(|| {
                    self.ctx.insert_hash_set(
                        &mut options,
                        (edge, pair),
                        "catia incidence constraint options",
                    )
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
        let mut ordered = Vec::new();
        self.search_storage.borrow_mut().with_storage(|| {
            self.ctx.reserve_vec(
                &mut ordered,
                options.len(),
                "catia incidence ordered constraint options",
            )
        })?;
        ordered.extend(options);
        self.ctx.sort_unstable_by(
            &mut ordered,
            Ord::cmp,
            |_| 0,
            "catia incidence ordered constraint options sort",
        )?;
        Ok(IncidenceConstraintOptions::Exact(ordered))
    }

    fn narrowest_edge_branch(
        &self,
        edges: impl IntoIterator<Item = usize>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<IncidenceBranch, CodecError> {
        let viable = |edge, pair| -> Result<bool, CodecError> {
            Ok(self.candidate_fits_in(edge, pair, coordinate_domains)?
                && coordinate_domains
                    .is_none_or(|domains| domains.supports_edge_candidate(edge, pair)))
        };
        let mut best = None::<(usize, usize, Option<Vec<(usize, [usize; 2])>>)>;
        let mut ordered_edges = Vec::new();
        for edge in edges {
            let width = if let Some(candidates) = coordinate_domains
                .filter(|_| self.choices[edge].is_empty())
                .and_then(|domains| domains.implicit_edge_candidates(edge, None))
            {
                candidates.width_upper_bound(self.ctx)?
            } else {
                self.choices[edge].len()
            };
            self.search_storage.borrow_mut().with_storage(|| {
                self.ctx.push_vec(
                    &mut ordered_edges,
                    (edge, width),
                    "catia_incidence_branch_edge_widths",
                )
            })?;
        }
        self.ctx.stable_sort_by(
            &mut ordered_edges,
            |left, right| left.1.cmp(&right.1),
            |_| 0,
            "catia_incidence_branch_edge_widths_sort",
        )?;
        'edges: for (edge, width) in ordered_edges {
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
            for pair in self.choices[edge].iter().copied() {
                if viable(edge, pair)? {
                    self.search_storage.borrow_mut().with_storage(|| {
                        self.ctx.push_vec(
                            &mut options,
                            (edge, pair),
                            "catia_incidence_branch_options",
                        )
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

    fn branch(
        &self,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Result<Option<IncidenceBranch>, CodecError> {
        let mut constrained = None::<Vec<(usize, [usize; 2])>>;
        let mut viability = HashMap::new();
        for &(face, point) in &self.constraints {
            if self.degree(face, point) != 1 {
                continue;
            }
            let limit = constrained.as_ref().map(Vec::len);
            let options =
                self.constraint_options(face, point, coordinate_domains, limit, &mut viability)?;
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
        if constrained.is_some() {
            return Ok(constrained
                .map(Vec::into_iter)
                .map(IncidenceBranch::Options));
        }
        if let Some(constraint) = self.partial_solution_filter {
            let mut edges = self.edges.iter().copied().filter(|&edge| {
                constraint.active_edges.get(edge) == Some(&true)
                    && self.assignment[edge].is_none()
                    && self.branch_edge_ready(edge)
            });
            if let Some(first) = edges.next() {
                return Ok(Some(self.narrowest_edge_branch(
                    std::iter::once(first).chain(edges),
                    coordinate_domains,
                )?));
            }
        }
        if !self
            .constraints
            .iter()
            .any(|&(face, point)| self.degree(face, point) == 1)
            && self
                .edges
                .iter()
                .all(|&edge| self.assignment[edge].is_some())
        {
            let mut complete = Vec::new();
            if self.solution_visitor.is_some() {
                self.search_storage.borrow_mut().with_storage(|| {
                    self.ctx.reserve_vec(
                        &mut complete,
                        self.edges.len(),
                        "catia incidence complete branch",
                    )
                })?;
            } else {
                self.ctx.reserve_vec(
                    &mut complete,
                    self.edges.len(),
                    "catia incidence complete branch",
                )?;
            }
            complete.extend(
                self.edges
                    .iter()
                    .filter_map(|&edge| self.assignment[edge].map(|pair| (edge, pair))),
            );
            return Ok(Some(IncidenceBranch::Complete(complete)));
        }
        let edges = self
            .edges
            .iter()
            .copied()
            .filter(|&edge| self.assignment[edge].is_none() && self.branch_edge_ready(edge));
        Ok(Some(self.narrowest_edge_branch(edges, coordinate_domains)?))
    }

    fn adjust(&mut self, edge: usize, pair: [usize; 2]) -> Result<IncidenceDegreeUndo, CodecError> {
        self.search_storage.borrow_mut().with_storage(|| {
            adjust_incidence_degrees(self.ctx, &mut self.degrees, self.edge_faces, edge, pair)
        })
    }

    fn restore_adjustment(&mut self, undo: IncidenceDegreeUndo) {
        restore_incidence_degrees(&mut self.degrees, undo);
    }

    fn advance_ordered_faces(
        &mut self,
        faces: impl IntoIterator<Item = usize>,
        quotient_states: Vec<MeshQuotientGaugeState<'storage>>,
    ) -> Result<Option<Vec<MeshQuotientGaugeState<'storage>>>, CodecError> {
        let Some(mesh_assignments) = self.mesh_assignments else {
            return Ok(Some(quotient_states));
        };
        let mut faces_collection = Vec::new();
        for face in faces {
            self.search_storage.borrow_mut().with_storage(|| {
                self.ctx.push_vec(
                    &mut faces_collection,
                    face,
                    "catia_incidence_advanced_faces",
                )
            })?;
        }
        let mut faces = faces_collection;
        self.ctx.sort_unstable_by(
            &mut faces,
            Ord::cmp,
            |_| 0,
            "catia_incidence_advanced_faces_sort",
        )?;
        faces.dedup();
        for &face in &faces {
            let Some(domain) = mesh_assignments.get(face) else {
                return Ok(None);
            };
            let mut refusal = None;
            let viable = match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => self
                    .face_configuration_domains
                    .as_ref()
                    .and_then(|factors| factors.face_has_active_configuration(face))
                    .unwrap_or_else(|| {
                        assignments.iter().any(|assignment| {
                            mesh_assignment_endpoint_cycles_viable_where(
                                self.ctx,
                                assignment,
                                self.choices,
                                Some(self.boundary_propagation_budget),
                                |edge, pair| {
                                    self.assignment[edge]
                                        .is_none_or(|selected| same_unordered_pair(selected, pair))
                                },
                            )
                            .map_or_else(
                                |error| {
                                    refusal = Some(error);
                                    true
                                },
                                |result| result.unwrap_or(true),
                            )
                        })
                    }),
                _ => compact_boundary_domain_viable(self.ctx, domain, &self.assignment, None)?,
            };
            if let Some(error) = refusal {
                return Err(error);
            }
            if !viable {
                return Ok(None);
            }
        }
        if quotient_states.is_empty() {
            Ok(Some(quotient_states))
        } else {
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
    }

    #[cfg(test)]
    fn ordered_faces_feasible(
        &mut self,
        faces: impl IntoIterator<Item = usize>,
    ) -> Result<bool, CodecError> {
        Ok(self.advance_ordered_faces(faces, Vec::new())?.is_some())
    }

    fn component_faces(&self) -> Result<Vec<usize>, CodecError> {
        let count = self.edges.len().checked_mul(2).ok_or_else(|| {
            self.ctx
                .refuse_codec_limit("catia incidence component faces", u64::MAX, u64::MAX)
        })?;
        let mut faces = Vec::new();
        self.search_storage.borrow_mut().with_storage(|| {
            self.ctx
                .reserve_vec(&mut faces, count, "catia incidence component faces")
        })?;
        faces.extend(self.edges.iter().flat_map(|edge| self.edge_faces[*edge]));
        self.ctx.sort_unstable_by(
            &mut faces,
            Ord::cmp,
            |_| 0,
            "catia incidence component faces sort",
        )?;
        faces.dedup();
        Ok(faces)
    }

    #[cfg(test)]
    fn face_configuration_options(
        &self,
    ) -> Result<Option<MeshFaceEndpointConfigurations>, CodecError> {
        self.face_configuration_options_for(&self.component_faces()?)
    }

    fn face_configuration_options_for(
        &self,
        component_faces: &[usize],
    ) -> Result<Option<MeshFaceEndpointConfigurations>, CodecError> {
        let Some(mesh_assignments) = self.mesh_assignments else {
            return Ok(None);
        };
        let factor_state = self
            .face_configuration_domains
            .as_ref()
            .and_then(|factors| factors.active.as_ref());
        let mut faces = Vec::new();
        self.search_storage.borrow_mut().with_storage(|| {
            self.ctx.reserve_vec(
                &mut faces,
                component_faces.len(),
                "catia face option candidates",
            )
        })?;
        faces.extend(component_faces.iter().copied().filter_map(|face| {
            let domain = mesh_assignments.get(face)?;
            let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                return None;
            };
            if self.face_edges[face].iter().any(|edge| {
                self.active[*edge]
                    && self.assignment[*edge].is_none()
                    && self.choices[*edge].is_empty()
            }) {
                return None;
            }
            let mut has_unresolved = false;
            let width = self.face_edges[face]
                .iter()
                .copied()
                .filter(|edge| self.active[*edge] && self.assignment[*edge].is_none())
                .try_fold(assignments.len().max(1), |width, edge| {
                    has_unresolved = true;
                    width.checked_mul(self.choices[edge].len())
                });
            if !has_unresolved {
                return None;
            }
            Some((width?, face, assignments))
        }));
        self.ctx.stable_sort_by(
            &mut faces,
            |left, right| (left.0, left.1).cmp(&(right.0, right.1)),
            |_| 0,
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
                for (configuration_index, configuration) in persistent.iter().enumerate() {
                    if factor_mask
                        .is_some_and(|mask| !configuration_mask_contains(mask, configuration_index))
                        || !configuration.iter().all(|(edge, pair)| {
                            self.assignment[*edge]
                                .is_none_or(|chosen| same_unordered_pair(chosen, *pair))
                        })
                    {
                        continue;
                    }
                    let copy = self.search_storage.borrow_mut().with_storage(|| {
                        self.ctx
                            .copy_slice(configuration, "catia face option configuration pairs")
                    })?;
                    self.search_storage.borrow_mut().with_storage(|| {
                        self.ctx
                            .push_vec(&mut selected, copy, "catia face option configurations")
                    })?;
                }
                selected
            } else {
                let Some(configurations) = mesh_face_endpoint_configurations(
                    self.ctx,
                    assignments,
                    self.choices,
                    &self.assignment,
                    self.boundary_propagation_budget,
                )?
                else {
                    if self.boundary_propagation_budget.exhausted() {
                        break;
                    }
                    continue;
                };
                configurations
            };
            let mut unique = HashSet::new();
            for configuration in configurations {
                let mut projection = Vec::new();
                for pair in configuration {
                    if self.active[pair.0] && self.assignment[pair.0].is_none() {
                        self.search_storage.borrow_mut().with_storage(|| {
                            self.ctx.push_vec(
                                &mut projection,
                                pair,
                                "catia face option projected pairs",
                            )
                        })?;
                    }
                }
                self.search_storage.borrow_mut().with_storage(|| {
                    self.ctx.insert_hash_set(
                        &mut unique,
                        projection,
                        "catia face option projected configurations",
                    )
                })?;
            }
            let mut projected = Vec::new();
            self.search_storage.borrow_mut().with_storage(|| {
                self.ctx.reserve_vec(
                    &mut projected,
                    unique.len(),
                    "catia face option ordered projections",
                )
            })?;
            projected.extend(unique);
            self.ctx.sort_unstable_by(
                &mut projected,
                Ord::cmp,
                |item| std::mem::size_of_val(item.as_slice()),
                "catia face option ordered projections sort",
            )?;
            if projected.is_empty() {
                return Ok(Some(Vec::new()));
            }
            if projected.iter().all(Vec::is_empty) {
                continue;
            }
            let forced = projected.len() == 1;
            self.search_storage.borrow_mut().with_storage(|| {
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
        if domains
            .iter()
            .all(|domain| domain.configurations.len() != 1)
        {
            let mut configuration_domains = Vec::new();
            self.search_storage.borrow_mut().with_storage(|| {
                self.ctx.reserve_vec(
                    &mut configuration_domains,
                    domains.len(),
                    "catia face option configuration domains",
                )
            })?;
            configuration_domains.extend(
                domains
                    .iter_mut()
                    .map(|domain| std::mem::take(&mut domain.configurations)),
            );
            let viable = prune_face_configuration_support(
                self.ctx,
                &mut configuration_domains,
                self.boundary_propagation_budget,
            )?;
            for (domain, configurations) in domains.iter_mut().zip(configuration_domains) {
                domain.configurations = configurations;
            }
            if !viable {
                return Ok(Some(Vec::new()));
            }
        }
        Ok(domains
            .into_iter()
            .min_by_key(|domain| (domain.configurations.len(), domain.width, domain.face))
            .map(|domain| domain.configurations))
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
        for option in options {
            if let Some(applied) = self.apply_face_configuration(option, coordinate_domains)? {
                if let Some(next_states) = self.advance_ordered_faces(
                    applied.affected_faces,
                    copy_quotient_states(self.ctx, quotient_states)?,
                )? {
                    self.search_with_quotient(
                        &next_states,
                        applied.coordinate_domains.as_ref(),
                        component_faces,
                    )?;
                }
                self.rollback_face_configuration(applied.assigned);
                if let Some(factors) = &mut self.face_configuration_domains {
                    factors.restore(applied.factor_checkpoint);
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
    ) -> Result<Option<AppliedFaceConfiguration>, CodecError> {
        let mut assigned = Vec::new();
        let mut affected_faces = Vec::new();
        let mut next_coordinate_domains = coordinate_domains.cloned();
        self.search_storage.borrow_mut().with_storage(|| {
            self.ctx.reserve_vec(
                &mut assigned,
                option.len(),
                "catia face applied assignments",
            )
        })?;
        let affected_count = option.len().checked_mul(2).ok_or_else(|| {
            self.ctx
                .refuse_codec_limit("catia face affected faces", u64::MAX, u64::MAX)
        })?;
        self.search_storage.borrow_mut().with_storage(|| {
            self.ctx.reserve_vec(
                &mut affected_faces,
                affected_count,
                "catia face affected faces",
            )
        })?;
        for (edge, pair) in option {
            if !self.active[edge] || self.assignment[edge].is_some() {
                continue;
            }
            if !self.degree_candidate_fits(edge, pair) {
                self.rollback_face_configuration(assigned);
                return Ok(None);
            }
            if let Some(domains) = next_coordinate_domains.take() {
                let Some(refined) = self.refine_coordinate_domains(&domains, edge, pair)? else {
                    self.rollback_face_configuration(assigned);
                    return Ok(None);
                };
                next_coordinate_domains = Some(refined);
            }
            let undo = self.adjust(edge, pair)?;
            self.assignment[edge] = Some(pair);
            assigned.push((edge, pair, undo));
            affected_faces.extend(self.edge_faces[edge]);
        }
        if assigned.is_empty()
            || self
                .partial_solution_filter
                .is_some_and(|constraint| !(constraint.valid)(&self.assignment))
        {
            self.rollback_face_configuration(assigned);
            return Ok(None);
        }
        self.ctx.sort_unstable_by(
            &mut affected_faces,
            Ord::cmp,
            |_| 0,
            "catia face configuration affected faces sort",
        )?;
        affected_faces.dedup();
        if !self.degree_frontiers_supported(
            &affected_faces,
            None,
            next_coordinate_domains.as_deref(),
        )? {
            if self.budget.exhausted() {
                self.state = IncidenceSearchState::Exhausted;
            }
            self.rollback_face_configuration(assigned);
            return Ok(None);
        }
        let mut assigned_pairs = Vec::new();
        self.search_storage.borrow_mut().with_storage(|| {
            self.ctx.reserve_vec(
                &mut assigned_pairs,
                assigned.len(),
                "catia face factor assigned pairs",
            )
        })?;
        assigned_pairs.extend(assigned.iter().map(|(edge, pair, _)| (*edge, *pair)));
        let factor_checkpoint = match &mut self.face_configuration_domains {
            Some(factors) => match factors.refine_edges(self.ctx, &assigned_pairs)? {
                FaceFactorRefinement::Tracked(checkpoint) => Some(checkpoint),
                FaceFactorRefinement::Untracked => None,
                FaceFactorRefinement::Rejected => {
                    self.rollback_face_configuration(assigned);
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
        }))
    }

    fn rollback_face_configuration(
        &mut self,
        assigned: Vec<(usize, [usize; 2], IncidenceDegreeUndo)>,
    ) {
        for (edge, _, undo) in assigned.into_iter().rev() {
            self.assignment[edge] = None;
            self.restore_adjustment(undo);
        }
    }

    fn search_forced_face_configurations(
        &mut self,
        mut option: Vec<(usize, [usize; 2])>,
        quotient_states: &[MeshQuotientGaugeState<'storage>],
        coordinate_domains: Option<&Arc<MeshCoordinateRootDomains>>,
        component_faces: &[usize],
    ) -> Result<(), CodecError> {
        let mut assigned = Vec::new();
        let mut factor_checkpoint = None;
        let mut states = copy_quotient_states(self.ctx, quotient_states)?;
        let mut domains = coordinate_domains.cloned();
        while let Some(applied) = self.apply_face_configuration(option, domains.as_ref())? {
            let applied_factor_checkpoint = applied.factor_checkpoint;
            let Some(next_states) = self.advance_ordered_faces(applied.affected_faces, states)?
            else {
                self.rollback_face_configuration(applied.assigned);
                if let Some(factors) = &mut self.face_configuration_domains {
                    factors.restore(applied_factor_checkpoint);
                }
                break;
            };
            if factor_checkpoint.is_none() {
                factor_checkpoint = applied_factor_checkpoint;
            }
            self.search_storage.borrow_mut().with_storage(|| {
                self.ctx.reserve_vec(
                    &mut assigned,
                    applied.assigned.len(),
                    "catia forced face assignments",
                )
            })?;
            assigned.extend(applied.assigned);
            states = next_states;
            domains = applied.coordinate_domains;
            let face_options = self.face_configuration_options_for(component_faces)?;
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
        self.rollback_face_configuration(assigned);
        if let Some(factors) = &mut self.face_configuration_domains {
            factors.restore(factor_checkpoint);
        }
        Ok(())
    }

    fn search(&mut self) -> Result<(), CodecError> {
        let quotient_states = Vec::new();
        let component_faces = self.component_faces()?;
        let coordinate_domains = self
            .coordinate_domains
            .map(|domains| domains.clone_charged(self.ctx).map(Arc::new))
            .transpose()?;
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
        if self.state != IncidenceSearchState::Open {
            return Ok(());
        }
        if !self.budget.charge() {
            self.state = IncidenceSearchState::Exhausted;
            return Ok(());
        }
        let mut state = Vec::new();
        self.search_storage.borrow_mut().with_storage(|| {
            self.ctx.reserve_vec(
                &mut state,
                self.edges.len(),
                "catia incidence dead state key",
            )
        })?;
        state.extend(self.edges.iter().map(|&edge| self.assignment[edge]));
        if self.dead_states.contains(&state) {
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
        let face_options = self.face_configuration_options_for(component_faces)?;
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
        let Some(branch) = branch else {
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
                    FaceFactorRefinement::Untracked => None,
                    FaceFactorRefinement::Rejected => {
                        self.assignment[edge] = None;
                        self.restore_adjustment(undo);
                        continue;
                    }
                },
                None => None,
            };
            if self
                .partial_solution_filter
                .is_none_or(|constraint| (constraint.valid)(&self.assignment))
            {
                if let Some(next_states) = self.advance_ordered_faces(
                    self.edge_faces[edge],
                    copy_quotient_states(self.ctx, quotient_states)?,
                )? {
                    self.search_with_quotient(
                        &next_states,
                        next_coordinate_domains.as_ref(),
                        component_faces,
                    )?;
                }
            }
            self.assignment[edge] = None;
            self.restore_adjustment(undo);
            if let Some(factors) = &mut self.face_configuration_domains {
                factors.restore(factor_checkpoint);
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
    if mesh.exact_uses.is_empty() {
        if incidence.len() <= mesh.length
            && incidence.iter().all(|(edge, _)| missing.contains(edge))
        {
            let slack = mesh.length - incidence.len();
            let mut start = 0usize;
            let mut boundary = Vec::new();
            ctx.reserve_vec(
                &mut boundary,
                incidence.len(),
                "catia deferred cycle unconstrained uses",
            )?;
            for (index, (edge, _)) in incidence.iter().enumerate() {
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
    let mut expected = Vec::new();
    ctx.reserve_vec(
        &mut expected,
        mesh.exact_uses.len(),
        "catia deferred cycle expected edges",
    )?;
    expected.extend(mesh.exact_uses.iter().map(|(use_, _)| use_.edge));
    for reversed in [false, true] {
        let mut actual = Vec::new();
        ctx.reserve_vec(
            &mut actual,
            incidence.len(),
            "catia deferred cycle actual edges",
        )?;
        actual.extend(incidence.iter().map(|(edge, _)| *edge));
        if reversed {
            actual.reverse();
        }
        let Some(anchor) = actual.iter().position(|edge| *edge == expected[0]) else {
            continue;
        };
        actual.rotate_left(anchor);
        let mut positions = Vec::new();
        ctx.reserve_vec(
            &mut positions,
            expected.len(),
            "catia deferred cycle positions",
        )?;
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
            let mut boundary = Vec::new();
            ctx.reserve_vec(
                &mut boundary,
                actual.len(),
                "catia deferred cycle boundary uses",
            )?;
            for index in 0..expected.len() {
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
                for offset in 1..=missing_count {
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
    Ok(deferred_boundary_cycle_assignment(ctx, mesh, incidence, missing)?.is_some())
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
        let reassigned = matched_mesh[incidence].is_none_or(|previous| {
            augment_cycle_matching(previous, compatible, seen, matched_mesh)
        });
        if reassigned {
            matched_mesh[incidence] = Some(mesh);
            return true;
        }
    }
    false
}

pub(super) fn deferred_boundary_assignment(
    ctx: &DecodeContext<'_>,
    domain: &MeshDeferredFaceBoundary,
    edge_points: &[[usize; 2]],
) -> Result<Option<MeshFaceBoundaryAssignment>, CodecError> {
    let incident_count = domain
        .cycles
        .iter()
        .try_fold(domain.missing_edges.len(), |count, cycle| {
            count.checked_add(cycle.exact_uses.len())
        })
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia deferred incident edges", u64::MAX, u64::MAX)
        })?;
    let mut temporary = ctx.reserve_scoped(0, "catia deferred boundary workspace")?;
    let mut incident = Vec::new();
    temporary.with_storage(|| {
        ctx.reserve_vec(
            &mut incident,
            incident_count,
            "catia deferred incident edges",
        )
    })?;
    incident.extend_from_slice(&domain.missing_edges);
    incident.extend(
        domain
            .cycles
            .iter()
            .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
    );
    ctx.sort_unstable_by(
        &mut incident,
        Ord::cmp,
        |_| 0,
        "catia deferred incident edges sort",
    )?;
    incident.dedup();
    let Some(incidence) =
        temporary.with_storage(|| incidence_cycles(ctx, &incident, edge_points))?
    else {
        return Ok(None);
    };
    if incidence.len() != domain.cycles.len() {
        return Ok(None);
    }
    let mut missing = HashSet::new();
    temporary.with_storage(|| {
        ctx.reserve_set(
            &mut missing,
            domain.missing_edges.len(),
            "catia deferred missing edges",
        )
    })?;
    missing.extend(domain.missing_edges.iter().copied());
    let compatibility_count = domain
        .cycles
        .len()
        .checked_mul(incidence.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia deferred compatibility cells", u64::MAX, u64::MAX)
        })?;
    let mut compatible = Vec::new();
    temporary.with_storage(|| {
        ctx.reserve_vec(
            &mut compatible,
            domain.cycles.len(),
            "catia deferred compatibility rows",
        )
    })?;
    ctx.charge_collection_items(
        u64_from_index(compatibility_count),
        "catia deferred compatibility cells",
    )?;
    for mesh in &domain.cycles {
        let mut row = Vec::new();
        temporary.with_storage(|| {
            ctx.reserve_capacity(
                &mut row,
                incidence.len(),
                "catia deferred compatibility cells",
            )
        })?;
        for candidate in &incidence {
            row.push(temporary.with_storage(|| {
                deferred_boundary_cycle_assignment(ctx, mesh, candidate, &missing)
            })?);
        }
        compatible.push(row);
    }
    let mut boolean_compatible = Vec::new();
    temporary.with_storage(|| {
        ctx.reserve_vec(
            &mut boolean_compatible,
            domain.cycles.len(),
            "catia deferred matching rows",
        )
    })?;
    ctx.charge_collection_items(
        u64_from_index(compatibility_count),
        "catia deferred matching cells",
    )?;
    for cycles in &compatible {
        let mut row = Vec::new();
        temporary.with_storage(|| {
            ctx.reserve_capacity(&mut row, cycles.len(), "catia deferred matching cells")
        })?;
        row.extend(cycles.iter().map(Option::is_some));
        boolean_compatible.push(row);
    }
    let (mut matched_mesh, _matched_mesh_storage) =
        ctx.temporary_vec(incidence.len(), "catia_deferred_match")?;
    for _ in 0..incidence.len() {
        matched_mesh.push(None);
    }
    for mesh in 0..domain.cycles.len() {
        let (mut visited, _visited_storage) =
            ctx.temporary_vec(incidence.len(), "catia_deferred_visit")?;
        visited.extend(std::iter::repeat_n(false, incidence.len()));
        if !augment_cycle_matching(mesh, &boolean_compatible, &mut visited, &mut matched_mesh) {
            return Ok(None);
        }
    }
    let (mut boundaries, _boundaries_storage) =
        ctx.temporary_vec(domain.cycles.len(), "catia_deferred_boundaries")?;
    for _ in 0..domain.cycles.len() {
        boundaries.push(None);
    }
    for (incidence, mesh) in matched_mesh.into_iter().enumerate() {
        let Some(mesh) = mesh else {
            return Ok(None);
        };
        boundaries[mesh] = compatible[mesh][incidence]
            .as_ref()
            .map(|uses| ctx.copy_slice(uses, "catia deferred copied boundary uses"))
            .transpose()?;
    }
    let mut collected = Vec::new();
    ctx.reserve_vec(
        &mut collected,
        boundaries.len(),
        "catia deferred collected boundaries",
    )?;
    for boundary in boundaries {
        let Some(boundary) = boundary else {
            return Ok(None);
        };
        collected.push(boundary);
    }
    Ok(Some(MeshFaceBoundaryAssignment {
        boundaries: collected,
    }))
}

fn deferred_boundary_closes(
    ctx: &DecodeContext<'_>,
    domain: &MeshDeferredFaceBoundary,
    edge_points: &[[usize; 2]],
) -> Result<bool, CodecError> {
    let incident_count = domain
        .cycles
        .iter()
        .try_fold(domain.missing_edges.len(), |count, cycle| {
            count.checked_add(cycle.exact_uses.len())
        })
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia deferred close incident edges", u64::MAX, u64::MAX)
        })?;
    let mut temporary = ctx.reserve_scoped(0, "catia deferred close boundary workspace")?;
    let mut incident = Vec::new();
    temporary.with_storage(|| {
        ctx.reserve_vec(
            &mut incident,
            incident_count,
            "catia deferred close incident edges",
        )
    })?;
    incident.extend_from_slice(&domain.missing_edges);
    incident.extend(
        domain
            .cycles
            .iter()
            .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
    );
    ctx.sort_unstable_by(
        &mut incident,
        Ord::cmp,
        |_| 0,
        "catia deferred close incident edges sort",
    )?;
    incident.dedup();
    let Some(incidence) =
        temporary.with_storage(|| incidence_cycles(ctx, &incident, edge_points))?
    else {
        return Ok(false);
    };
    if incidence.len() != domain.cycles.len() {
        return Ok(false);
    }
    let mut missing = HashSet::new();
    temporary.with_storage(|| {
        ctx.reserve_set(
            &mut missing,
            domain.missing_edges.len(),
            "catia deferred close missing edges",
        )
    })?;
    missing.extend(domain.missing_edges.iter().copied());
    let cells = domain
        .cycles
        .len()
        .checked_mul(incidence.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "catia deferred close compatibility cells",
                u64::MAX,
                u64::MAX,
            )
        })?;
    let mut compatible = Vec::new();
    temporary.with_storage(|| {
        ctx.reserve_vec(
            &mut compatible,
            domain.cycles.len(),
            "catia deferred close compatibility rows",
        )
    })?;
    ctx.charge_collection_items(
        u64_from_index(cells),
        "catia deferred close compatibility cells",
    )?;
    for mesh in &domain.cycles {
        let mut row = Vec::new();
        temporary.with_storage(|| {
            ctx.reserve_capacity(
                &mut row,
                incidence.len(),
                "catia deferred close compatibility cells",
            )
        })?;
        for candidate in &incidence {
            row.push(deferred_boundary_cycle_matches(
                ctx, mesh, candidate, &missing,
            )?);
        }
        compatible.push(row);
    }
    let (mut matched_mesh, _matched_mesh_storage) =
        ctx.temporary_vec(incidence.len(), "catia_deferred_close_match")?;
    for _ in 0..incidence.len() {
        matched_mesh.push(None);
    }
    for mesh in 0..domain.cycles.len() {
        let (mut visited, _visited_storage) =
            ctx.temporary_vec(incidence.len(), "catia_deferred_close_visit")?;
        visited.extend(std::iter::repeat_n(false, incidence.len()));
        if !augment_cycle_matching(mesh, &compatible, &mut visited, &mut matched_mesh) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn boundary_domains_close(
    ctx: &DecodeContext<'_>,
    domains: Option<&[MeshFaceBoundaryDomain]>,
    edge_points: &[[usize; 2]],
) -> Result<bool, CodecError> {
    let Some(domains) = domains else {
        return Ok(true);
    };
    for domain in domains {
        let closes = match domain {
            MeshFaceBoundaryDomain::Ordered(_) => true,
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                incidence_cycles(ctx, edges, edge_points)?.is_some_and(|cycles| cycles.len() == 1)
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                deferred_boundary_closes(ctx, domain, edge_points)?
            }
        };
        if !closes {
            return Ok(false);
        }
    }
    Ok(true)
}

fn component_incidence_faces_viable(
    ctx: &DecodeContext<'_>,
    faces: &HashSet<usize>,
    assignment: &[Option<[usize; 2]>],
    choices: &[Vec<[usize; 2]>],
    face_edges: &[Vec<usize>],
    domains: Option<&[MeshFaceBoundaryDomain]>,
    point_count: usize,
) -> Result<bool, CodecError> {
    for &face in faces {
        let mut temporary = ctx.reserve_scoped(0, "catia component incidence face workspace")?;
        if domains.is_none() {
            let mut degrees = HashMap::<usize, u8>::new();
            for &edge in &face_edges[face] {
                let Some(pair) = assignment[edge] else {
                    continue;
                };
                for point in pair {
                    if point >= point_count {
                        return Ok(false);
                    }
                    temporary.with_storage(|| {
                        ctx.admit_hash_map_entry(
                            &mut degrees,
                            &point,
                            "catia component incidence degree points",
                        )
                    })?;
                    let degree = degrees.entry(point).or_default();
                    let Some(next) = degree.checked_add(1) else {
                        return Ok(false);
                    };
                    *degree = next;
                }
            }
            if !degrees.into_iter().all(|(point, degree)| {
                degree <= 2
                    && (degree != 1
                        || face_edges[face].iter().copied().any(|edge| {
                            assignment[edge].is_none()
                                && choices[edge].iter().any(|pair| pair.contains(&point))
                        }))
            }) {
                return Ok(false);
            }
            continue;
        }
        let mut points = temporary.with_storage(|| {
            ctx.alloc_filled(
                assignment.len(),
                [0; 2],
                "catia component incidence edge points",
            )
        })?;
        for &edge in &face_edges[face] {
            let Some(pair) = assignment[edge] else {
                return Ok(false);
            };
            points[edge] = pair;
        }
        if temporary
            .with_storage(|| incidence_cycles(ctx, &face_edges[face], &points))?
            .is_none()
        {
            return Ok(false);
        }
        let Some(domain) = domains.and_then(|domains| domains.get(face)) else {
            continue;
        };
        let mut refusal = None;
        let viable = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                assignments.iter().any(|boundary_assignment| {
                    temporary
                        .with_storage(|| {
                            mesh_assignment_endpoint_cycles_viable_where(
                                ctx,
                                boundary_assignment,
                                choices,
                                None,
                                |edge, pair| {
                                    assignment[edge]
                                        .is_none_or(|selected| same_unordered_pair(selected, pair))
                                },
                            )
                        })
                        .map_or_else(
                            |error| {
                                refusal = Some(error);
                                true
                            },
                            |result| result.unwrap_or(true),
                        )
                })
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => temporary
                .with_storage(|| incidence_cycles(ctx, edges, &points))?
                .is_some_and(|cycles| cycles.len() == 1),
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                deferred_boundary_closes(ctx, domain, &points)?
            }
        };
        if let Some(error) = refusal {
            return Err(error);
        }
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
    if !edge_faces.iter().any(|faces| faces[0] != faces[1]) {
        return Ok(true);
    }
    let mut edge_points = Vec::new();
    ctx.reserve_vec(
        &mut edge_points,
        assignment.len(),
        "catia orientability edge points",
    )?;
    edge_points.extend(assignment.iter().map(|pair| pair.unwrap_or_default()));
    let mut edge_uses = HashMap::<usize, Vec<(usize, bool)>>::new();
    let mut boundary_count = 0usize;
    for incident in face_edges {
        let mut selected = Vec::new();
        ctx.reserve_vec(
            &mut selected,
            incident.len(),
            "catia orientability selected edges",
        )?;
        selected.extend(
            incident
                .iter()
                .copied()
                .filter(|edge| assignment[*edge].is_some()),
        );
        if selected.is_empty() {
            continue;
        }
        if !budget.charge_by(selected.len()) {
            return Ok(false);
        }
        let mut edges_at_point = HashMap::<usize, Vec<usize>>::new();
        let mut degrees = HashMap::<usize, u8>::new();
        for &edge in &selected {
            for point in edge_points[edge] {
                ctx.admit_hash_map_entry(
                    &mut edges_at_point,
                    &point,
                    "catia orientability point indexes",
                )?;
                ctx.push_vec(
                    edges_at_point.entry(point).or_default(),
                    edge,
                    "catia orientability indexed edges",
                )?;
                ctx.admit_hash_map_entry(
                    &mut degrees,
                    &point,
                    "catia orientability degree points",
                )?;
                let degree = degrees.entry(point).or_default();
                *degree = match degree.checked_add(1) {
                    Some(degree) => degree,
                    None => return Ok(false),
                };
            }
        }
        if degrees.values().any(|degree| *degree > 2) {
            return Ok(false);
        }
        let mut unseen = HashSet::new();
        ctx.reserve_set(
            &mut unseen,
            selected.len(),
            "catia orientability unseen edges",
        )?;
        unseen.extend(selected.iter().copied());
        for first in selected {
            if !unseen.contains(&first) {
                continue;
            }
            let mut stack = Vec::new();
            ctx.push_vec(&mut stack, first, "catia orientability traversal stack")?;
            let mut component = Vec::new();
            let mut points = HashSet::new();
            while let Some(edge) = stack.pop() {
                if !unseen.remove(&edge) {
                    continue;
                }
                ctx.push_vec(&mut component, edge, "catia orientability component edges")?;
                for point in edge_points[edge] {
                    ctx.insert_hash_set(
                        &mut points,
                        point,
                        "catia orientability component points",
                    )?;
                    ctx.reserve_vec(
                        &mut stack,
                        edges_at_point[&point].len(),
                        "catia orientability traversal stack",
                    )?;
                    stack.extend(edges_at_point[&point].iter().copied());
                }
            }
            ctx.sort_unstable_by(
                &mut component,
                Ord::cmp,
                |_| 0,
                "catia orientability component edges sort",
            )?;
            let trail = if points.iter().all(|point| degrees[point] == 2) {
                let Some(cycles) = incidence_cycles(ctx, &component, &edge_points)? else {
                    return Ok(false);
                };
                let [cycle] = cycles.as_slice() else {
                    return Ok(false);
                };
                ctx.copy_slice(cycle, "catia orientability closed trail")?
            } else {
                let mut endpoints = Vec::new();
                ctx.reserve_vec(
                    &mut endpoints,
                    points.len(),
                    "catia orientability endpoints",
                )?;
                endpoints.extend(points.iter().copied().filter(|point| degrees[point] == 1));
                ctx.sort_unstable_by(
                    &mut endpoints,
                    Ord::cmp,
                    |_| 0,
                    "catia orientability endpoints sort",
                )?;
                let [start, end] = endpoints.as_slice() else {
                    return Ok(false);
                };
                if points
                    .iter()
                    .any(|point| !endpoints.contains(point) && degrees[point] != 2)
                {
                    return Ok(false);
                }
                let mut remaining = HashSet::new();
                ctx.reserve_set(
                    &mut remaining,
                    component.len(),
                    "catia orientability remaining edges",
                )?;
                remaining.extend(component.iter().copied());
                let mut point = *start;
                let mut trail = Vec::new();
                ctx.reserve_vec(
                    &mut trail,
                    component.len(),
                    "catia orientability open trail",
                )?;
                while let Some(&edge) = edges_at_point[&point]
                    .iter()
                    .find(|edge| remaining.contains(edge))
                {
                    if !remaining.remove(&edge) {
                        return Ok(false);
                    }
                    let pair = edge_points[edge];
                    let reversed = pair[1] == point;
                    if !reversed && pair[0] != point {
                        return Ok(false);
                    }
                    point = pair[usize::from(!reversed)];
                    trail.push((edge, reversed));
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
            for (edge, reversed) in trail {
                ctx.admit_hash_map_entry(
                    &mut edge_uses,
                    &edge,
                    "catia orientability edge use keys",
                )?;
                ctx.push_vec(
                    edge_uses.entry(edge).or_default(),
                    (boundary, reversed),
                    "catia orientability edge uses",
                )?;
            }
        }
    }
    if !budget.charge_by(edge_uses.values().map(Vec::len).sum()) {
        return Ok(false);
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
        let mut constraints = HashSet::<(usize, usize)>::new();
        let mut point_support_edges = component_storage.with_storage(|| {
            ctx.alloc_filled(
                face_edges.len(),
                HashMap::<usize, Vec<usize>>::new(),
                "catia incidence point support edges",
            )
        })?;
        let mut component_faces = HashSet::new();
        for &edge in component {
            active[edge] = true;
            let faces = edge_faces[edge];
            for (rank, face) in faces.into_iter().enumerate() {
                if rank > 0 && face == faces[0] {
                    continue;
                }
                component_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut component_faces,
                        face,
                        "catia_incidence_component_faces",
                    )
                })?;
                let candidate_points = if let Some(domains) =
                    coordinate_domains.filter(|_| choices[edge].is_empty())
                {
                    domains.edge_candidate_points(ctx, edge)?
                } else {
                    None
                };
                let mut points = if let Some(points) = candidate_points {
                    points
                } else {
                    let mut points = Vec::new();
                    for point in choices[edge].iter().flatten().copied() {
                        component_storage.with_storage(|| {
                            ctx.push_vec(&mut points, point, "catia_incidence_candidate_points")
                        })?;
                    }
                    points
                };
                ctx.sort_unstable_by(
                    &mut points,
                    Ord::cmp,
                    |_| 0,
                    "catia_incidence_candidate_points_sort",
                )?;
                points.dedup();
                for point in points {
                    component_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut constraints,
                            (face, point),
                            "catia_incidence_point_constraints",
                        )
                    })?;
                    if !point_support_edges[face].contains_key(&point) {
                        component_storage.with_storage(|| {
                            ctx.insert_hash_map(
                                &mut point_support_edges[face],
                                point,
                                Vec::new(),
                                "catia_incidence_point_support_keys",
                            )
                        })?;
                    }
                    if let Some(support) = point_support_edges[face].get_mut(&point) {
                        component_storage.with_storage(|| {
                            ctx.push_vec(support, edge, "catia_incidence_point_support_entries")
                        })?;
                    }
                }
            }
        }
        let mut sorted_constraints = Vec::new();
        component_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut sorted_constraints,
                constraints.len(),
                "catia_incidence_sorted_constraints",
            )
        })?;
        sorted_constraints.extend(constraints);
        let mut constraints = sorted_constraints;
        ctx.sort_unstable_by(
            &mut constraints,
            Ord::cmp,
            |_| 0,
            "catia_incidence_sorted_constraints_sort",
        )?;
        let mut explicit_point_supports = Vec::new();
        component_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut explicit_point_supports,
                choices.len(),
                "catia_incidence_explicit_support_rows",
            )
        })?;
        for pairs in choices {
            let mut supports = HashMap::<usize, Vec<[usize; 2]>>::new();
            for &pair in pairs {
                for point in [Some(pair[0]), (pair[1] != pair[0]).then_some(pair[1])]
                    .into_iter()
                    .flatten()
                {
                    component_storage.with_storage(|| {
                        ctx.admit_hash_map_entry(
                            &mut supports,
                            &point,
                            "catia_incidence_explicit_support_keys",
                        )
                    })?;
                    component_storage.with_storage(|| {
                        ctx.push_vec(
                            supports.entry(point).or_default(),
                            pair,
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
            for &(edge, pair) in solution {
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
            if partial_solution_valid.is_some_and(|constraint| !(constraint.valid)(&completed)) {
                return Ok(false);
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
            if assignment.iter().any(Option::is_none) {
                return Err(IncidenceVisitError::Exhausted);
            }
            let mut pairs = Vec::new();
            ctx.reserve_vec(
                &mut pairs,
                assignment.len(),
                "catia_incidence_completed_pairs",
            )?;
            for pair in assignment.iter_mut().flatten() {
                pairs.push(*pair);
            }
            let boundary_closed = boundary_domains_close(ctx, mesh_assignments, &pairs)?;
            let solution_accepted = boundary_closed && solution_valid(&pairs)?;
            if !solution_accepted {
                return Ok(ControlFlow::Continue(()));
            }
            if let Some(quotient) = mesh_quotient {
                let singleton = singleton_incidence_pairs(ctx, &pairs)?;
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
                for &(edge, pair) in solution {
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
                    for (edge, pair) in assignment.iter().enumerate() {
                        candidates.push(if let Some(pair) = pair {
                            ctx.alloc_filled(1, *pair, "catia_incidence_fixed_candidate")?
                        } else {
                            solution_storage.with_storage(|| {
                                ctx.copy_slice(&choices[edge], "catia_incidence_open_candidates")
                            })?
                        });
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
                for (edge, undo) in degree_undo.into_iter().rev() {
                    assignment[edge] = None;
                    restore_incidence_degrees(degrees, undo);
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
        let mut coordinate_domains = if choices.iter().any(|candidates| candidates.len() != 1) {
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
        let mut components =
            incidence_choice_components(ctx, choices, edge_faces, mesh_assignments, mesh_quotient)?;
        if let Some(constraint) = partial_solution_valid {
            components =
                join_incidence_components_by_coupling(ctx, components, constraint.coupled_edges)?;
        }
        let ordered = order_incidence_components_by_constraints(
            ctx,
            &mut components,
            choices,
            partial_solution_valid.and_then(|constraint| constraint.assignment_order),
        )?;
        if ordered.is_none() {
            return Ok(None);
        }
        let mut face_edges =
            ctx.alloc_filled(face_count, Vec::new(), "catia incidence face edges")?;
        for (edge, faces) in edge_faces.iter().copied().enumerate() {
            for (rank, face) in faces.into_iter().enumerate() {
                if (rank == 0 || face != faces[0]) && !face_edges[face].contains(&edge) {
                    ctx.push_vec(
                        &mut face_edges[face],
                        edge,
                        "catia_incidence_face_edge_entries",
                    )?;
                }
            }
        }
        let mut fixed = ctx.alloc_filled(choices.len(), None, "catia incidence fixed edges")?;
        let mut degrees = ctx.alloc_filled(
            face_count,
            BTreeMap::<usize, u8>::new(),
            "catia incidence face degrees",
        )?;
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
                    ctx.admit_btree_entry(
                        &degrees[face],
                        point,
                        "catia_incidence_fixed_degree_points",
                    )?;
                    let degree = degrees[face].entry(*point).or_default();
                    let Some(next) = degree.checked_add(1) else {
                        return Ok(None);
                    };
                    *degree = next;
                }
            }
        }
        if components.is_empty() {
            rejection = IncidenceRejection::FixedAssignment;
            if fixed.iter().any(Option::is_none) {
                return Ok(None);
            }
            let mut pairs = Vec::new();
            ctx.reserve_vec(&mut pairs, fixed.len(), "catia_incidence_fixed_pairs")?;
            for pair in fixed.iter().flatten() {
                pairs.push(*pair);
            }
            let boundary_closed = boundary_domains_close(ctx, mesh_assignments, &pairs)?;
            let solution_valid = solution_valid(&pairs)?;
            if !boundary_closed || !solution_valid {
                return Ok(None);
            }
            if let Some(quotient) = mesh_quotient {
                let singleton = singleton_incidence_pairs(ctx, &pairs)?;
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
        for component in &components {
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
                    let mut completed =
                        ctx.copy_slice(&fixed, "catia_incidence_preflight_assignment")?;
                    for &(edge, pair) in solution {
                        completed[edge] = Some(pair);
                    }
                    let coordinate_feasible = if let Some(domains) = coordinate_domains.as_ref() {
                        let mut candidates = Vec::new();
                        ctx.reserve_vec(
                            &mut candidates,
                            completed.len(),
                            "catia_incidence_preflight_candidate_rows",
                        )?;
                        for (edge, pair) in completed.iter().enumerate() {
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
                        domains
                            .refine_candidates(
                                ctx,
                                &candidates,
                                Some(&coordinate_preflight_budget),
                            )?
                            .is_some()
                            || coordinate_preflight_budget.exhausted()
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
                let oriented;
                let pairs = if let Some(ports) = edge_ports {
                    let mut selected_pairs = Vec::new();
                    ctx.reserve_vec(
                        &mut selected_pairs,
                        pairs.len(),
                        "catia_incidence_port_selected_pairs",
                    )?;
                    selected_pairs.extend(pairs.iter().copied().map(Some));
                    let Some(propagated) = propagate_edge_port_points(ctx, ports, &selected_pairs)?
                    else {
                        invalid = true;
                        return Ok(ControlFlow::Break(()));
                    };
                    if propagated.iter().any(Option::is_none) {
                        invalid = true;
                        return Ok(ControlFlow::Break(()));
                    }
                    let mut completed = Vec::new();
                    ctx.reserve_vec(
                        &mut completed,
                        propagated.len(),
                        "catia_incidence_port_completed_pairs",
                    )?;
                    for pair in propagated.into_iter().flatten() {
                        completed.push(pair);
                    }
                    oriented = completed;
                    oriented.as_slice()
                } else {
                    pairs
                };
                if let Some(stored) = &solution_pairs {
                    if stored.as_slice() != pairs {
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
    for candidates in &mut choices {
        for pair in candidates.iter_mut() {
            ctx.sort_unstable_by(pair, Ord::cmp, |_| 0, "catia incidence choice pair sort")?;
        }
        ctx.sort_unstable_by(
            candidates,
            Ord::cmp,
            |_| 0,
            "catia incidence choice pairs sort",
        )?;
        candidates.dedup();
    }
    let valid = if choices.iter().any(Vec::is_empty) {
        mesh_quotient.is_some()
            && choices.len() == edge_faces.len()
            && !edge_faces.iter().flatten().any(|face| *face >= face_count)
            && !choices
                .iter()
                .flatten()
                .flatten()
                .any(|point| *point >= vertex_points.len())
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
        if complete_solution_budget.is_some_and(|budget| !budget.charge_by(edge_rows.len())) {
            return Ok(true);
        }
        let preferred = solution_valid(points)?;
        if !preferred {
            return Ok(false);
        }
        Ok(reconstruct_incidence(
            ctx,
            copy_incidence_edge_rows(ctx, edge_rows)?,
            ctx.copy_slice(vertex_points, "catia_incidence_validation_points")?,
            edge_faces,
            points,
            face_count,
        )?
        .is_some())
    };
    let mut budgeted_visitor = |points: &[[usize; 2]]| {
        if complete_solution_budget.is_some_and(WorkBudget::exhausted) {
            Ok(ControlFlow::Break(()))
        } else {
            visitor(points)
        }
    };
    let fallback_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
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
