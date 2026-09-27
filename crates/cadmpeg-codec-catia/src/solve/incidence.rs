//! Incidence backtracking constraint solver for standard B-rep topology.
//!
//! Reconstructs face/edge incidence from serialized boundary domains.

use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;

use crate::families::standard::topology::{
    incidence_cycles, reconstruct_incidence, solve_boundary_orientation_constraints, EdgeRow,
    StandardTopology,
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

fn charge_collection_items(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let count =
        u64::try_from(count).map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_collection_items(count, operation)
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
        charge_collection_items(
            ctx,
            edge_supports[edge].difference(&retained).count(),
            "catia incidence removed support points",
        )?;
        let removed = edge_supports[edge]
            .difference(&retained)
            .copied()
            .collect::<Vec<_>>();
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
            if !points.contains(&point) {
                charge_collection_items(ctx, 1, "catia incidence choice points")?;
                points.insert(point);
            }
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
            crate::resource::push(
                ctx,
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
    charge_collection_items(ctx, choices.len(), "catia incidence edge supports")?;
    let mut edge_supports = choices
        .iter()
        .map(|pairs| choice_points(ctx, pairs))
        .collect::<Result<Vec<_>, _>>()?;
    let mut supports = ctx.alloc_filled(
        face_count,
        BTreeMap::<usize, u32>::new(),
        "catia_incidence_supports",
    )?;
    for (edge, points) in edge_supports.iter().enumerate() {
        for face in unique_faces(edge_faces[edge]) {
            for &point in points {
                if !supports[face].contains_key(&point) {
                    charge_collection_items(ctx, 1, "catia incidence point support counts")?;
                }
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
                    charge_collection_items(ctx, 1, "catia incidence retained choices")?;
                    retained.push(pair);
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
                    if !degrees[face].contains_key(point) {
                        charge_collection_items(ctx, 1, "catia incidence endpoint degrees")?;
                    }
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

fn incidence_choice_components(
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    boundary_domains: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
) -> Vec<Vec<usize>> {
    let mut union = UnionFind::new(choices.len());
    let mut point_nodes = HashMap::<(usize, usize), usize>::new();
    for (edge, pairs) in choices.iter().enumerate() {
        for (rank, face) in edge_faces[edge].into_iter().enumerate() {
            if rank > 0 && face == edge_faces[edge][0] {
                continue;
            }
            for point in pairs.iter().flatten().copied() {
                let next = point_nodes.len();
                point_nodes.entry((face, point)).or_insert(next);
            }
        }
    }
    let mut fixed_incidence = UnionFind::new(point_nodes.len());
    for (edge, pairs) in choices.iter().enumerate() {
        let [pair] = pairs.as_slice() else {
            continue;
        };
        for (rank, face) in edge_faces[edge].into_iter().enumerate() {
            if rank > 0 && face == edge_faces[edge][0] {
                continue;
            }
            fixed_incidence.union(point_nodes[&(face, pair[0])], point_nodes[&(face, pair[1])]);
        }
    }
    let mut owner = HashMap::<(usize, usize), usize>::new();
    let ambiguous = choices
        .iter()
        .enumerate()
        .filter_map(|(edge, pairs)| {
            (pairs.len() > 1 || (pairs.is_empty() && mesh_quotient.is_some())).then_some(edge)
        })
        .collect::<Vec<_>>();
    for &edge in &ambiguous {
        let faces = edge_faces[edge];
        for (rank, face) in faces.into_iter().enumerate() {
            if rank > 0 && face == faces[0] {
                continue;
            }
            for point in choices[edge].iter().flatten().copied() {
                let point = fixed_incidence.find(point_nodes[&(face, point)]);
                match owner.entry((face, point)) {
                    std::collections::hash_map::Entry::Occupied(entry) => {
                        union.union(*entry.get(), edge);
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(edge);
                    }
                }
            }
        }
    }
    if let Some(domains) = boundary_domains {
        let mut connect = |mut edges: Vec<usize>| {
            edges.sort_unstable();
            edges.dedup();
            let mut ambiguous = edges.into_iter().filter(|edge| {
                choices[*edge].len() > 1 || (choices[*edge].is_empty() && mesh_quotient.is_some())
            });
            if let Some(first) = ambiguous.next() {
                for edge in ambiguous {
                    union.union(first, edge);
                }
            }
        };
        for domain in domains {
            match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) if assignments.len() == 1 => {
                    for boundary in &assignments[0].boundaries {
                        connect(boundary.iter().map(|use_| use_.edge).collect());
                    }
                }
                MeshFaceBoundaryDomain::Ordered(assignments) => connect(
                    assignments
                        .iter()
                        .flat_map(|assignment| assignment.boundaries.iter().flatten())
                        .map(|use_| use_.edge)
                        .collect(),
                ),
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => connect(edges.clone()),
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    let mut edges = domain.missing_edges.clone();
                    edges.extend(
                        domain
                            .cycles
                            .iter()
                            .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
                    );
                    connect(edges);
                }
            }
        }
    }
    if let Some(mesh_quotient) = mesh_quotient {
        let mut quotient = mesh_quotient.clone();
        if quotient.len() == choices.len().saturating_mul(2) {
            let mut owner = HashMap::<usize, usize>::new();
            for &edge in &ambiguous {
                for port in [edge * 2, edge * 2 + 1] {
                    let root = quotient.find(port);
                    for point in quotient.domains()[root].iter().copied() {
                        match owner.entry(point) {
                            std::collections::hash_map::Entry::Occupied(entry) => {
                                union.union(*entry.get(), edge);
                            }
                            std::collections::hash_map::Entry::Vacant(entry) => {
                                entry.insert(edge);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut by_root = HashMap::<usize, Vec<usize>>::new();
    for edge in ambiguous {
        by_root.entry(union.find(edge)).or_default().push(edge);
    }
    let mut components = by_root.into_values().collect::<Vec<_>>();
    for component in &mut components {
        component.sort_unstable();
    }
    components.sort_by_key(|component| component[0]);
    components
}

/// Merge components whose assignments participate in one shared partial
/// constraint. Evaluation-order constraints stay as edges between components
/// so independent domains do not inherit each other's branch alternatives.
fn join_incidence_components_by_coupling(
    components: Vec<Vec<usize>>,
    coupled_edges: &[bool],
) -> Vec<Vec<usize>> {
    let component_by_edge = components
        .iter()
        .enumerate()
        .flat_map(|(component, edges)| edges.iter().copied().map(move |edge| (edge, component)))
        .collect::<HashMap<_, _>>();
    let mut union = UnionFind::new(components.len());
    let mut coupled_owner = None;
    for (edge, active) in coupled_edges.iter().copied().enumerate() {
        if !active {
            continue;
        }
        let Some(&component) = component_by_edge.get(&edge) else {
            continue;
        };
        if let Some(owner) = coupled_owner {
            union.union(owner, component);
        } else {
            coupled_owner = Some(component);
        }
    }
    let mut joined = HashMap::<usize, (usize, Vec<usize>)>::new();
    for (index, component) in components.into_iter().enumerate() {
        let root = union.find(index);
        let entry = joined.entry(root).or_insert_with(|| (index, Vec::new()));
        entry.0 = entry.0.min(index);
        entry.1.extend(component);
    }
    let mut joined = joined.into_values().collect::<Vec<_>>();
    for (_, edges) in &mut joined {
        edges.sort_unstable();
    }
    joined.sort_unstable_by_key(|(first, _)| *first);
    joined.into_iter().map(|(_, edges)| edges).collect()
}

fn order_incidence_components_by_branch_width(
    components: &mut [Vec<usize>],
    choices: &[Vec<[usize; 2]>],
) -> Option<()> {
    if components
        .iter()
        .flatten()
        .any(|edge| *edge >= choices.len())
    {
        return None;
    }
    let branch_width = |component: &[usize]| {
        component.iter().fold(1usize, |width, edge| {
            width.saturating_mul(choices[*edge].len())
        })
    };
    components.sort_by_key(|component| {
        (
            branch_width(component),
            component.len(),
            component.first().copied().unwrap_or_default(),
        )
    });
    Some(())
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
        return Ok(order_incidence_components_by_branch_width(
            components, choices,
        ));
    }

    let edge_entry_count = components
        .iter()
        .try_fold(0usize, |count, edges| count.checked_add(edges.len()))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia incidence component edge indices", u64::MAX, u64::MAX)
        })?;
    charge_collection_items(
        ctx,
        edge_entry_count,
        "catia incidence component edge indices",
    )?;
    let component_by_edge = components
        .iter()
        .enumerate()
        .flat_map(|(component, edges)| edges.iter().copied().map(move |edge| (edge, component)))
        .collect::<HashMap<_, _>>();
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
                    charge_collection_items(ctx, 1, "catia incidence local dependents")?;
                    local_outgoing[prerequisite_edge].push(target_edge);
                    local_incoming[target_edge] += 1;
                }
                return Ok(());
            }
            if !outgoing[prerequisite_component].contains(&target_component) {
                charge_collection_items(ctx, 1, "catia incidence component dependents")?;
                outgoing[prerequisite_component].push(target_component);
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
    charge_collection_items(
        ctx,
        component_by_edge
            .keys()
            .filter(|edge| local_incoming[**edge] == 0)
            .count(),
        "catia incidence local ready edges",
    )?;
    let mut local_ready = component_by_edge
        .keys()
        .copied()
        .filter(|edge| local_incoming[*edge] == 0)
        .collect::<Vec<_>>();
    let mut local_ordered = 0usize;
    while let Some(edge) = local_ready.pop() {
        local_ordered += 1;
        for dependent in local_outgoing[edge].iter().copied() {
            local_incoming[dependent] -= 1;
            if local_incoming[dependent] == 0 {
                charge_collection_items(ctx, 1, "catia incidence local ready edges")?;
                local_ready.push(dependent);
            }
        }
    }
    if local_ordered != component_by_edge.len() {
        return Ok(None);
    }

    let branch_width = |component: &[usize]| {
        component.iter().fold(1usize, |width, edge| {
            width.saturating_mul(choices[*edge].len())
        })
    };
    charge_collection_items(
        ctx,
        (0..components.len())
            .filter(|component| incoming[*component] == 0)
            .count(),
        "catia incidence ready components",
    )?;
    let mut ready = (0..components.len())
        .filter(|component| incoming[*component] == 0)
        .collect::<Vec<_>>();
    charge_collection_items(ctx, components.len(), "catia incidence ordered components")?;
    let mut ordered = Vec::with_capacity(components.len());
    while let Some((position, &component)) =
        ready.iter().enumerate().min_by_key(|(_, component)| {
            (
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
                charge_collection_items(ctx, 1, "catia incidence ready components")?;
                ready.push(dependent);
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
}

enum IncidenceBranch {
    Options(std::vec::IntoIter<(usize, [usize; 2])>),
    Implicit {
        edge: usize,
        candidates: MeshImplicitEdgeCandidates,
    },
    Complete(Vec<(usize, [usize; 2])>),
}

enum IncidenceCandidatePairs {
    Options(std::vec::IntoIter<[usize; 2]>),
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
        len.div_ceil(u64::BITS as usize),
        u64::MAX,
        "catia_face_config_full_mask",
    )?;
    if let Some(last) = mask.last_mut() {
        let remainder = len % u64::BITS as usize;
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
    match map.entry(key) {
        std::collections::hash_map::Entry::Occupied(mut occupied) => {
            occupied.get_mut()[word] |= bit;
        }
        std::collections::hash_map::Entry::Vacant(vacant) => {
            let mut mask = ctx.alloc_filled(word_count, 0u64, operation)?;
            mask[word] |= bit;
            charge_collection_items(ctx, 1, "catia face configuration mask keys")?;
            vacant.insert(mask);
        }
    }
    Ok(())
}

fn configuration_mask_contains(mask: &[u64], index: usize) -> bool {
    mask.get(index / u64::BITS as usize)
        .is_some_and(|word| word & (1 << (index % u64::BITS as usize)) != 0)
}

impl FaceFactorGraph {
    fn compile(
        ctx: &DecodeContext<'_>,
        domains: &[MeshFaceEndpointConfigurations],
        budget: &WorkBudget<'_>,
    ) -> Result<Option<Self>, CodecError> {
        charge_collection_items(ctx, domains.len(), "catia face factor edge sets")?;
        let mut edge_sets = Vec::new();
        for domain in domains {
            let mut edges = HashSet::new();
            for &(edge, _) in domain.iter().flatten() {
                if !edges.contains(&edge) {
                    charge_collection_items(ctx, 1, "catia face factor edges")?;
                    edges.insert(edge);
                }
            }
            edge_sets.push(edges);
        }
        charge_collection_items(ctx, domains.len(), "catia face factor right indexes")?;
        let mut right_indexes = Vec::with_capacity(domains.len());
        for domain in domains {
            let word_count = domain.len().div_ceil(u64::BITS as usize);
            let mut present = HashMap::<usize, Vec<u64>>::new();
            let mut matching = HashMap::<(usize, [usize; 2]), Vec<u64>>::new();
            for (configuration, candidate) in domain.iter().enumerate() {
                if !budget.charge_by(work_units(candidate.len())) {
                    return Ok(None);
                }
                let word = configuration / u64::BITS as usize;
                let bit = 1 << (configuration % u64::BITS as usize);
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
                let word_count = domains[right].len().div_ceil(u64::BITS as usize);
                let (present, matching) = &right_indexes[right];
                let mut supports = Vec::new();
                crate::resource::reserve_vec(
                    ctx,
                    &mut supports,
                    domains[left].len(),
                    "catia face factor support rows",
                )?;
                for candidate in &domains[left] {
                    if !budget.charge_by(work_units(candidate.len().saturating_add(word_count))) {
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
                crate::resource::push(
                    ctx,
                    &mut arcs,
                    FaceFactorArc {
                        left,
                        right,
                        supports,
                    },
                    "catia face factor arcs",
                )?;
                crate::resource::push(
                    ctx,
                    &mut incoming[right],
                    arc,
                    "catia face factor incoming arcs",
                )?;
            }
        }
        let mut domain_lengths = Vec::new();
        crate::resource::reserve_vec(
            ctx,
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
        charge_collection_items(
            ctx,
            self.domain_lengths.len(),
            "catia face factor active rows",
        )?;
        self.domain_lengths
            .iter()
            .map(|length| full_configuration_mask(ctx, *length))
            .collect()
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
            charge_collection_items(ctx, 1, "catia face factor propagation queue")?;
            queue.push_back(arc);
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
                active[arc.left][configuration / u64::BITS as usize] &=
                    !(1 << (configuration % u64::BITS as usize));
                changed = true;
            }
            if !changed {
                continue;
            }
            if active[arc.left].iter().all(|word| *word == 0) {
                return Ok(Some(false));
            }
            for &incoming in &self.incoming[arc.left] {
                charge_collection_items(ctx, 1, "catia face factor propagation queue")?;
                queue.push_back(incoming);
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
        assigned: &[(usize, [usize; 2])],
    ) -> Result<Option<FaceFactorCheckpoint>, ()> {
        let Some(active) = &mut self.active else {
            return Ok(None);
        };
        let checkpoint = FaceFactorCheckpoint {
            active: active.clone(),
        };
        for &(edge, pair) in assigned {
            let Some(factors) = self.factors_by_edge.get(edge) else {
                continue;
            };
            for &factor in factors {
                let face = self.factor_faces[factor];
                let Some(configurations) = self.domains.get(face).and_then(Option::as_ref) else {
                    active.clone_from(&checkpoint.active);
                    return Err(());
                };
                for (configuration, pairs) in configurations.iter().enumerate() {
                    if !configuration_mask_contains(&active[factor], configuration)
                        || pairs.iter().all(|(candidate_edge, candidate_pair)| {
                            *candidate_edge != edge || same_unordered_pair(*candidate_pair, pair)
                        })
                    {
                        continue;
                    }
                    active[factor][configuration / u64::BITS as usize] &=
                        !(1 << (configuration % u64::BITS as usize));
                }
                if active[factor].iter().all(|word| *word == 0) {
                    active.clone_from(&checkpoint.active);
                    return Err(());
                }
            }
        }
        Ok(Some(checkpoint))
    }

    fn restore_checkpoint(&mut self, checkpoint: &FaceFactorCheckpoint) {
        self.active = Some(checkpoint.active.clone());
    }

    fn restore(&mut self, checkpoint: Option<FaceFactorCheckpoint>) {
        if let Some(checkpoint) = checkpoint {
            self.restore_checkpoint(&checkpoint);
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
    charge_collection_items(ctx, domains.len(), "catia face configuration edge sets")?;
    let mut edge_sets = Vec::new();
    for domain in domains.iter() {
        let mut edges = HashSet::new();
        for &(edge, _) in domain.iter().flatten() {
            if !edges.contains(&edge) {
                charge_collection_items(ctx, 1, "catia face configuration edges")?;
                edges.insert(edge);
            }
        }
        edge_sets.push(edges);
    }
    let mut neighbors =
        ctx.alloc_filled(domains.len(), Vec::new(), "catia_face_config_neighbors")?;
    let mut queue = VecDeque::new();
    for left in 0..domains.len() {
        for right in 0..domains.len() {
            if left != right && !edge_sets[left].is_disjoint(&edge_sets[right]) {
                crate::resource::push(
                    ctx,
                    &mut neighbors[left],
                    right,
                    "catia face configuration neighbors",
                )?;
                crate::resource::push_back(
                    ctx,
                    &mut queue,
                    (left, right),
                    "catia face configuration queue",
                )?;
            }
        }
    }
    while let Some((left, right)) = queue.pop_front() {
        let word_count = domains[right].len().div_ceil(u64::BITS as usize);
        let mut present = HashMap::<usize, Vec<u64>>::new();
        let mut matching = HashMap::<(usize, [usize; 2]), Vec<u64>>::new();
        for (configuration, candidate) in domains[right].iter().enumerate() {
            if !budget.charge_by(work_units(candidate.len())) {
                return Ok(true);
            }
            let word = configuration / u64::BITS as usize;
            let bit = 1 << (configuration % u64::BITS as usize);
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
        charge_collection_items(
            ctx,
            domains[left].len(),
            "catia face configuration keep marks",
        )?;
        let mut keep = Vec::with_capacity(domains[left].len());
        for candidate in &domains[left] {
            if !budget.charge_by(work_units(candidate.len().saturating_add(word_count))) {
                return Ok(true);
            }
            let mut viable = ctx.alloc_filled(word_count, u64::MAX, "catia_face_config_viable")?;
            if let Some(last) = viable.last_mut() {
                let remainder = domains[right].len() % u64::BITS as usize;
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
                    charge_collection_items(ctx, 1, "catia face configuration queue")?;
                    queue.push_back((neighbor, left));
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
        charge_collection_items(
            ctx,
            domains.len(),
            "catia face configuration singleton order",
        )?;
        let mut order = (0..domains.len()).collect::<Vec<_>>();
        order.sort_unstable_by_key(|domain| {
            active[*domain]
                .iter()
                .map(|word| word.count_ones() as usize)
                .sum::<usize>()
        });
        for domain in order {
            let mut domain_changed = false;
            let active_count = active[domain]
                .iter()
                .map(|word| word.count_ones() as usize)
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
                charge_collection_items(ctx, active.len(), "catia face configuration trial rows")?;
                for mask in &active {
                    charge_collection_items(
                        ctx,
                        mask.len(),
                        "catia face configuration trial masks",
                    )?;
                }
                let mut trial = active.clone();
                trial[domain].fill(0);
                trial[domain][configuration / u64::BITS as usize] =
                    1 << (configuration % u64::BITS as usize);
                match graph.propagate_from(ctx, domain, &mut trial, budget)? {
                    Some(true) => {}
                    Some(false) => {
                        active[domain][configuration / u64::BITS as usize] &=
                            !(1 << (configuration % u64::BITS as usize));
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
            charge_collection_items(ctx, use_count, "catia ordered face edges")?;
            let mut edges = assignments
                .iter()
                .flat_map(|assignment| assignment.boundaries.iter().flatten())
                .map(|use_| use_.edge)
                .collect::<Vec<_>>();
            edges.sort_unstable();
            edges.dedup();
            if edges
                .iter()
                .any(|edge| choices.get(*edge).is_none_or(Vec::is_empty))
            {
                continue;
            }
            let selected = ctx.alloc_filled(choices.len(), None, "catia_ordered_face_selection")?;
            let Some(configurations) =
                mesh_face_endpoint_configurations(assignments, choices, &selected, budget)
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
                    if !supported.contains_key(&edge) {
                        charge_collection_items(ctx, 1, "catia ordered face support edges")?;
                    }
                    let pairs = supported.entry(edge).or_default();
                    if !pairs.contains(&pair) {
                        charge_collection_items(ctx, 1, "catia ordered face support pairs")?;
                        pairs.insert(pair);
                    }
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
                    canonical.sort_unstable();
                    if edge_supported.contains(&canonical) {
                        charge_collection_items(ctx, 1, "catia ordered face retained pairs")?;
                        retained.push(pair);
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
                    face_support.entry(edge).or_default().extend(pairs);
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
                    values.collect::<Vec<_>>()
                } else {
                    current.clone()
                };
                let mut retained = Vec::new();
                for mut pair in values {
                    if !budget.charge() {
                        return Ok(true);
                    }
                    pair.sort_unstable();
                    if supported.contains(&pair) {
                        retained.push(pair);
                    }
                }
                retained.sort_unstable();
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
        charge_collection_items(ctx, use_count, "catia face factor edges")?;
        let mut edges = assignments
            .iter()
            .flat_map(|assignment| assignment.boundaries.iter().flatten())
            .map(|use_| use_.edge)
            .filter(|edge| active.get(*edge) == Some(&true))
            .collect::<Vec<_>>();
        edges.sort_unstable();
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
            mesh_face_endpoint_configurations(assignments, choices, selected, &budget)
        else {
            continue;
        };
        domains[face] = Some(configurations);
    }
    charge_collection_items(ctx, domains.len(), "catia face factor retained faces")?;
    let retained_faces = domains
        .iter()
        .enumerate()
        .filter_map(|(face, domain)| domain.as_ref().map(|_| face))
        .collect::<Vec<_>>();
    charge_collection_items(
        ctx,
        retained_faces.len(),
        "catia face factor configurations",
    )?;
    let mut configurations = retained_faces
        .iter()
        .map(|face| {
            domains[*face]
                .as_mut()
                .map(std::mem::take)
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
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
        crate::resource::reserve_vec(
            ctx,
            &mut edges,
            indexed_edges,
            "catia face factor indexed edges",
        )?;
        edges.extend(
            configurations[factor]
                .iter()
                .flatten()
                .map(|(edge, _)| *edge),
        );
        edges.sort_unstable();
        edges.dedup();
        for edge in edges {
            if let Some(factors) = factors_by_edge.get_mut(edge) {
                crate::resource::push(ctx, factors, factor, "catia face factors by edge entries")?;
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

impl Iterator for IncidenceCandidatePairs {
    type Item = [usize; 2];

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Options(options) => options.next(),
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
            crate::resource::reserve_vec(
                ctx,
                &mut collected,
                edges.len(),
                "catia compact viable edges",
            )?;
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
            crate::resource::reserve_vec(
                ctx,
                &mut edges,
                edge_count,
                "catia compact viable edges",
            )?;
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
    crate::resource::reserve_vec(
        ctx,
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
                    crate::resource::push(ctx, &mut degrees, 0, "catia_compact_boundary_degrees")?;
                    crate::resource::insert_map(
                        ctx,
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
                components.union(nodes[0], nodes[1]);
            }
        }
        let mut open_components = HashSet::new();
        for (node, degree) in degrees.into_iter().enumerate() {
            if degree < 2 {
                crate::resource::insert_set(
                    ctx,
                    &mut open_components,
                    components.find(node),
                    "catia_compact_boundary_open_components",
                )?;
            }
        }
        return Ok(
            (0..components.len()).all(|node| open_components.contains(&components.find(node)))
        );
    }
    let mut complete_pairs = Vec::new();
    for pair in selected_pairs {
        let Some(pair) = pair else {
            return Ok(true);
        };
        crate::resource::push(
            ctx,
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
            incidence_cycles(edges, &edge_points).is_some_and(|cycles| cycles.len() == 1)
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            deferred_boundary_closes(ctx, domain, &edge_points)?
        }
    })
}

enum CompactBoundaryAdvanceOutcome {
    Complete(Vec<MeshQuotientGaugeState>),
    Rejected,
    Exhausted,
}

fn advance_compact_boundary_domains<'a>(
    ctx: &DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a MeshFaceBoundaryDomain>,
    choices: &[Vec<[usize; 2]>],
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
    mut states: Vec<MeshQuotientGaugeState>,
    budget: &WorkBudget<'_>,
) -> Result<CompactBoundaryAdvanceOutcome, CodecError> {
    const MAX_QUOTIENT_STATES: usize = 4_096;

    let mut ordered = Vec::<Vec<MeshFaceBoundaryAssignment>>::new();
    for domain in domains {
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
        charge_collection_items(ctx, edge_count, "catia compact boundary edges")?;
        let edges = match domain {
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
        charge_collection_items(ctx, edges.len(), "catia compact boundary selected edges")?;
        let Some(edge_points) = edges
            .iter()
            .map(|edge| {
                selected
                    .filter(|(selected_edge, _)| selected_edge == edge)
                    .map(|(_, pair)| pair)
                    .or(assignment[*edge])
                    .map(|pair| (*edge, pair))
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
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
                charge_collection_items(
                    ctx,
                    assignments.len(),
                    "catia compact boundary alternatives",
                )?;
                assignments.clone()
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                let Some(cycles) = incidence_cycles(edges, &points) else {
                    return Ok(CompactBoundaryAdvanceOutcome::Rejected);
                };
                let [cycle] = cycles.as_slice() else {
                    return Ok(CompactBoundaryAdvanceOutcome::Rejected);
                };
                charge_collection_items(ctx, cycle.len(), "catia compact unordered boundary uses")?;
                vec![MeshFaceBoundaryAssignment {
                    boundaries: vec![cycle
                        .into_iter()
                        .map(|(edge, _)| MeshBoundaryEdgeCandidate {
                            edge: *edge,
                            start: 0,
                            end: 0,
                            reversed: None,
                        })
                        .collect()],
                }]
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                let Some(materialized) = deferred_boundary_assignment(ctx, domain, &points)? else {
                    return Ok(CompactBoundaryAdvanceOutcome::Rejected);
                };
                charge_collection_items(ctx, 1, "catia compact deferred alternative")?;
                vec![materialized]
            }
        };
        charge_collection_items(ctx, 1, "catia compact boundary domain rows")?;
        ordered.push(alternatives);
    }
    if ordered.is_empty() {
        return Ok(CompactBoundaryAdvanceOutcome::Complete(states));
    }
    charge_collection_items(
        ctx,
        assignment.len(),
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
    charge_collection_items(
        ctx,
        candidate_count,
        "catia compact boundary candidate pairs",
    )?;
    let candidates = assignment
        .iter()
        .enumerate()
        .map(|(edge, pair)| {
            selected
                .filter(|(selected_edge, _)| *selected_edge == edge)
                .map(|(_, pair)| vec![pair])
                .or_else(|| pair.map(|pair| vec![pair]))
                .unwrap_or_else(|| choices[edge].clone())
        })
        .collect::<Vec<_>>();
    for alternatives in ordered {
        let mut next = Vec::new();
        let mut signatures = HashSet::new();
        for (state, oriented_edges) in states {
            for face in &alternatives {
                for (_, mut candidate) in state.assignment_options_limited(
                    face,
                    &candidates,
                    &oriented_edges,
                    MAX_QUOTIENT_STATES.saturating_sub(next.len()),
                    Some(budget),
                ) {
                    let mut next_oriented = oriented_edges.clone();
                    next_oriented.extend(face.boundaries.iter().flatten().map(|use_| use_.edge));
                    if !budget.charge_by(
                        candidate
                            .signature_work()
                            .saturating_add(work_units(next_oriented.len())),
                    ) {
                        return Ok(CompactBoundaryAdvanceOutcome::Exhausted);
                    }
                    let mut oriented_signature = next_oriented.iter().copied().collect::<Vec<_>>();
                    oriented_signature.sort_unstable();
                    if signatures.insert((candidate.signature(), oriented_signature)) {
                        next.push((candidate, next_oriented));
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
pub(super) fn compact_boundary_domains_jointly_viable<'a>(
    ctx: &DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a MeshFaceBoundaryDomain>,
    choices: &[Vec<[usize; 2]>],
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
    quotient: &MeshQuotient,
    budget: &WorkBudget<'_>,
) -> Result<bool, CodecError> {
    Ok(matches!(
        advance_compact_boundary_domains(
            ctx,
            domains,
            choices,
            assignment,
            selected,
            vec![(quotient.clone(), HashSet::new())],
            budget,
        )?,
        CompactBoundaryAdvanceOutcome::Complete(_)
    ))
}

fn adjust_incidence_degrees(
    degrees: &mut [BTreeMap<usize, u8>],
    edge_faces: &[[usize; 2]],
    edge: usize,
    pair: [usize; 2],
) -> IncidenceDegreeUndo {
    let mut undo = IncidenceDegreeUndo {
        entries: Vec::new(),
    };
    let faces = edge_faces[edge];
    for (rank, face) in faces.into_iter().enumerate() {
        if rank > 0 && face == faces[0] {
            continue;
        }
        for point in pair {
            let previous = degrees[face].get(&point).copied();
            *degrees[face].entry(point).or_default() += 1;
            undo.entries.push((face, point, previous));
        }
    }
    undo
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

impl IncidenceComponentSearch<'_, '_> {
    fn candidate_pairs(
        &self,
        edge: usize,
        required_point: Option<usize>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> IncidenceCandidatePairs {
        if let Some(candidates) = coordinate_domains
            .filter(|_| self.choices[edge].is_empty())
            .and_then(|domains| domains.implicit_edge_candidates(edge, required_point))
        {
            return IncidenceCandidatePairs::Implicit(candidates);
        }
        IncidenceCandidatePairs::Options(
            required_point
                .and_then(|point| self.explicit_point_supports.get(edge)?.get(&point).cloned())
                .unwrap_or_else(|| {
                    self.choices[edge]
                        .iter()
                        .copied()
                        .filter(|pair| required_point.is_none_or(|point| pair.contains(&point)))
                        .collect()
                })
                .into_iter(),
        )
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
    ) {
        let mut witnesses = self.degree_support_witnesses.borrow_mut();
        let entry = witnesses.entry((face, point)).or_default();
        if !entry.contains(&witness) {
            entry.push(witness);
        }
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
    ) -> bool {
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

        faces.iter().copied().all(|face| {
            let start = self
                .constraints
                .partition_point(|&(constraint_face, _)| constraint_face < face);
            let end = self.constraints[start..]
                .partition_point(|&(constraint_face, _)| constraint_face == face)
                + start;
            let constrained_points = &self.constraints[start..end];
            let support_exists = |point| {
                let witnesses = {
                    self.degree_support_witnesses
                        .borrow()
                        .get(&(face, point))
                        .cloned()
                        .unwrap_or_default()
                };
                for (supporting_edge, supporting_pair) in witnesses.into_iter().rev() {
                    let candidate_still_available = self.choices[supporting_edge]
                        .contains(&supporting_pair)
                        || coordinate_domains
                            .filter(|_| self.choices[supporting_edge].is_empty())
                            .is_some_and(|domains| {
                                domains.supports_edge_candidate(supporting_edge, supporting_pair)
                            });
                    if !self.degree_support_budget.charge() {
                        return true;
                    }
                    if selected.is_none_or(|(edge, _)| supporting_edge != edge)
                        && self.active[supporting_edge]
                        && self.assignment[supporting_edge].is_none()
                        && candidate_still_available
                        && supporting_point_fits(supporting_edge, point)
                        && supporting_pair_fits(supporting_edge, supporting_pair)
                    {
                        return true;
                    }
                }
                let indexed_edges = self
                    .point_support_edges
                    .get(face)
                    .and_then(|by_point| by_point.get(&point));
                let supporting_edges =
                    indexed_edges.map_or(self.face_edges[face].as_slice(), Vec::as_slice);
                for &supporting_edge in supporting_edges {
                    if !self.degree_support_budget.charge() {
                        return true;
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
                            );
                            return true;
                        }
                        if self.degree_support_budget.exhausted() {
                            return true;
                        }
                        continue;
                    }
                    for supporting_pair in self.candidate_pairs(supporting_edge, Some(point), None)
                    {
                        if !self.degree_support_budget.charge() {
                            return true;
                        }
                        if supporting_pair.contains(&point) && fits(supporting_pair) {
                            self.remember_degree_support_witness(
                                face,
                                point,
                                (supporting_edge, supporting_pair),
                            );
                            return true;
                        }
                    }
                }
                false
            };
            constrained_points.iter().all(|&(_, point)| {
                degree_after_selection(face, point) != 1 || support_exists(point)
            })
        })
    }

    fn degree_support_preserved(
        &self,
        edge: usize,
        pair: [usize; 2],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> bool {
        let mut faces = self.edge_faces[edge].to_vec();
        faces.sort_unstable();
        faces.dedup();
        let preserved =
            self.degree_frontiers_supported(&faces, Some((edge, pair)), coordinate_domains);
        #[cfg(test)]
        if !self.degree_support_budget.exhausted() {
            assert_eq!(
                preserved,
                self.degree_support_preserved_by_constraint_scan(edge, pair, coordinate_domains)
            );
        }
        preserved
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
            let mut faces = self.edge_faces[edge].to_vec();
            faces.sort_unstable();
            faces.dedup();
            for face in faces {
                let Some(domain) = mesh_assignments.get(face) else {
                    return Ok(false);
                };
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
                                .unwrap_or(true)
                            })
                        }),
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
        if !self.degree_candidate_fits(edge, pair) {
            return Ok(false);
        }
        if !self.degree_support_preserved(edge, pair, coordinate_domains) {
            return Ok(false);
        }
        Ok(true)
    }

    fn constraint_options(
        &self,
        face: usize,
        point: usize,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        limit: usize,
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
                    viability.insert((edge, pair), viable);
                    viable
                };
                if !viable {
                    continue;
                }
                any_viable = true;
                if !self.branch_edge_ready(edge) {
                    continue;
                }
                options.insert((edge, pair));
                if options.len() == limit {
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
        let mut options = options.into_iter().collect::<Vec<_>>();
        options.sort_unstable();
        Ok(IncidenceConstraintOptions::Exact(options))
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
        let mut edges = edges.into_iter().collect::<Vec<_>>();
        edges.sort_by_key(|edge| {
            coordinate_domains
                .filter(|_| self.choices[*edge].is_empty())
                .and_then(|domains| domains.implicit_edge_candidates(*edge, None))
                .map_or(self.choices[*edge].len(), |candidates| {
                    candidates.width_upper_bound()
                })
        });
        'edges: for edge in edges {
            if let Some(candidates) = coordinate_domains
                .filter(|_| self.choices[edge].is_empty())
                .and_then(|domains| domains.implicit_edge_candidates(edge, None))
            {
                let width = candidates.width_upper_bound();
                if best.as_ref().is_none_or(|(_, best, _)| width < *best) {
                    best = Some((edge, width, None));
                    if width == 0 {
                        break;
                    }
                }
                continue;
            }
            let limit = best.as_ref().map_or(usize::MAX, |(_, width, _)| *width);
            let mut options = Vec::new();
            for pair in self.choices[edge].iter().copied() {
                if viable(edge, pair)? {
                    options.push((edge, pair));
                    if options.len() == limit {
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
            let limit = constrained.as_ref().map_or(usize::MAX, Vec::len);
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
            let edges = self
                .edges
                .iter()
                .copied()
                .filter(|&edge| {
                    constraint.active_edges.get(edge) == Some(&true)
                        && self.assignment[edge].is_none()
                        && self.branch_edge_ready(edge)
                })
                .collect::<Vec<_>>();
            if !edges.is_empty() {
                return Ok(Some(self.narrowest_edge_branch(edges, coordinate_domains)?));
            }
        }
        if !self
            .constraints
            .iter()
            .any(|&(face, point)| self.degree(face, point) == 1)
        {
            let complete = self.edges.iter().copied().try_fold(
                Vec::with_capacity(self.edges.len()),
                |mut complete, edge| {
                    complete.push((edge, self.assignment[edge]?));
                    Some(complete)
                },
            );
            if let Some(complete) = complete {
                return Ok(Some(IncidenceBranch::Complete(complete)));
            }
        }
        let edges = self
            .edges
            .iter()
            .copied()
            .filter(|&edge| self.assignment[edge].is_none() && self.branch_edge_ready(edge))
            .collect::<Vec<_>>();
        Ok(Some(self.narrowest_edge_branch(edges, coordinate_domains)?))
    }

    fn adjust(&mut self, edge: usize, pair: [usize; 2]) -> IncidenceDegreeUndo {
        adjust_incidence_degrees(&mut self.degrees, self.edge_faces, edge, pair)
    }

    fn restore_adjustment(&mut self, undo: IncidenceDegreeUndo) {
        restore_incidence_degrees(&mut self.degrees, undo);
    }

    fn advance_ordered_faces(
        &mut self,
        faces: impl IntoIterator<Item = usize>,
        quotient_states: Vec<MeshQuotientGaugeState>,
    ) -> Result<Option<Vec<MeshQuotientGaugeState>>, CodecError> {
        let Some(mesh_assignments) = self.mesh_assignments else {
            return Ok(Some(quotient_states));
        };
        let mut faces = faces.into_iter().collect::<Vec<_>>();
        faces.sort_unstable();
        faces.dedup();
        for &face in &faces {
            let Some(domain) = mesh_assignments.get(face) else {
                return Ok(None);
            };
            let viable = match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => self
                    .face_configuration_domains
                    .as_ref()
                    .and_then(|factors| factors.face_has_active_configuration(face))
                    .unwrap_or_else(|| {
                        assignments.iter().any(|assignment| {
                            mesh_assignment_endpoint_cycles_viable_where(
                                assignment,
                                self.choices,
                                Some(self.boundary_propagation_budget),
                                |edge, pair| {
                                    self.assignment[edge]
                                        .is_none_or(|selected| same_unordered_pair(selected, pair))
                                },
                            )
                            .unwrap_or(true)
                        })
                    }),
                _ => compact_boundary_domain_viable(self.ctx, domain, &self.assignment, None)?,
            };
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

    fn component_faces(&self) -> Vec<usize> {
        let mut faces = self
            .edges
            .iter()
            .flat_map(|edge| self.edge_faces[*edge])
            .collect::<Vec<_>>();
        faces.sort_unstable();
        faces.dedup();
        faces
    }

    #[cfg(test)]
    fn face_configuration_options(
        &self,
    ) -> Result<Option<MeshFaceEndpointConfigurations>, CodecError> {
        self.face_configuration_options_for(&self.component_faces())
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
        charge_collection_items(
            self.ctx,
            component_faces.len(),
            "catia face option candidates",
        )?;
        let mut faces = component_faces
            .iter()
            .copied()
            .filter_map(|face| {
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
                    .fold(assignments.len().max(1), |width, edge| {
                        has_unresolved = true;
                        width.saturating_mul(self.choices[edge].len())
                    });
                if !has_unresolved {
                    return None;
                }
                Some((width, face, assignments))
            })
            .collect::<Vec<_>>();
        faces.sort_by_key(|(width, face, _)| (*width, *face));
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
                persistent
                    .iter()
                    .enumerate()
                    .filter(|(configuration, _)| {
                        factor_mask
                            .is_none_or(|mask| configuration_mask_contains(mask, *configuration))
                    })
                    .map(|(_, configuration)| configuration)
                    .filter(|configuration| {
                        configuration.iter().all(|(edge, pair)| {
                            self.assignment[*edge]
                                .is_none_or(|selected| same_unordered_pair(selected, *pair))
                        })
                    })
                    .cloned()
                    .collect()
            } else {
                let Some(configurations) = mesh_face_endpoint_configurations(
                    assignments,
                    self.choices,
                    &self.assignment,
                    self.boundary_propagation_budget,
                ) else {
                    if self.boundary_propagation_budget.exhausted() {
                        break;
                    }
                    continue;
                };
                configurations
            };
            let mut projected = configurations
                .into_iter()
                .map(|configuration| {
                    configuration
                        .into_iter()
                        .filter(|(edge, _)| self.active[*edge] && self.assignment[*edge].is_none())
                        .collect::<Vec<_>>()
                })
                .collect::<HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            projected.sort_unstable();
            if projected.is_empty() {
                return Ok(Some(Vec::new()));
            }
            if projected.iter().all(Vec::is_empty) {
                continue;
            }
            let forced = projected.len() == 1;
            charge_collection_items(self.ctx, 1, "catia face option domains")?;
            domains.push(FaceConfigurationDomain {
                width,
                face,
                configurations: projected,
            });
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
            let mut configuration_domains = domains
                .iter_mut()
                .map(|domain| std::mem::take(&mut domain.configurations))
                .collect::<Vec<_>>();
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
        quotient_states: &[MeshQuotientGaugeState],
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
                if let Some(next_states) =
                    self.advance_ordered_faces(applied.affected_faces, quotient_states.to_vec())?
                {
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
            let undo = self.adjust(edge, pair);
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
        affected_faces.sort_unstable();
        affected_faces.dedup();
        if !self.degree_frontiers_supported(
            &affected_faces,
            None,
            next_coordinate_domains.as_deref(),
        ) {
            if self.budget.exhausted() {
                self.state = IncidenceSearchState::Exhausted;
            }
            self.rollback_face_configuration(assigned);
            return Ok(None);
        }
        let assigned_pairs = assigned
            .iter()
            .map(|(edge, pair, _)| (*edge, *pair))
            .collect::<Vec<_>>();
        let factor_checkpoint = match &mut self.face_configuration_domains {
            Some(factors) => match factors.refine_edges(&assigned_pairs) {
                Ok(checkpoint) => checkpoint,
                Err(()) => {
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
        quotient_states: &[MeshQuotientGaugeState],
        coordinate_domains: Option<&Arc<MeshCoordinateRootDomains>>,
        component_faces: &[usize],
    ) -> Result<(), CodecError> {
        let mut assigned = Vec::new();
        let mut factor_checkpoint = None;
        let mut states = quotient_states.to_vec();
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
        let component_faces = self.component_faces();
        let coordinate_domains = self
            .coordinate_domains
            .map(|domains| Arc::new(domains.clone()));
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
        quotient_states: &[MeshQuotientGaugeState],
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
        let state = self
            .edges
            .iter()
            .map(|&edge| self.assignment[edge])
            .collect::<Vec<_>>();
        if self.dead_states.contains(&state) {
            return Ok(());
        }
        let solutions_before = self.solutions.len();
        self.search_state(quotient_states, coordinate_domains, component_faces)?;
        if self.state == IncidenceSearchState::Open && self.solutions.len() == solutions_before {
            self.dead_states.insert(state);
        }
        Ok(())
    }

    fn search_state(
        &mut self,
        quotient_states: &[MeshQuotientGaugeState],
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
        quotient_states: &[MeshQuotientGaugeState],
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
                    self.solutions.push(solution);
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
            let undo = self.adjust(edge, pair);
            self.assignment[edge] = Some(pair);
            let factor_checkpoint = match &mut self.face_configuration_domains {
                Some(factors) => match factors.refine_edges(&[(edge, pair)]) {
                    Ok(checkpoint) => checkpoint,
                    Err(()) => {
                        self.assignment[edge] = None;
                        self.restore_adjustment(undo);
                        continue;
                    }
                },
                None => None,
            };
            let mut faces = self.edge_faces[edge].to_vec();
            faces.sort_unstable();
            faces.dedup();
            if self
                .partial_solution_filter
                .is_none_or(|constraint| (constraint.valid)(&self.assignment))
            {
                if let Some(next_states) =
                    self.advance_ordered_faces(faces, quotient_states.to_vec())?
                {
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
    mesh: &MeshDeferredBoundaryCycle,
    incidence: &[(usize, bool)],
    missing: &HashSet<usize>,
) -> Option<Vec<MeshBoundaryEdgeCandidate>> {
    if mesh.exact_uses.is_empty() {
        return (incidence.len() <= mesh.length
            && incidence.iter().all(|(edge, _)| missing.contains(edge)))
        .then(|| {
            let slack = mesh.length - incidence.len();
            let mut start = 0usize;
            incidence
                .iter()
                .enumerate()
                .map(|(index, (edge, _))| {
                    let span = 1 + usize::from(index == 0) * slack;
                    let use_ = MeshBoundaryEdgeCandidate {
                        edge: *edge,
                        start,
                        end: (start + span) % mesh.length,
                        reversed: None,
                    };
                    start = (start + span) % mesh.length;
                    use_
                })
                .collect()
        });
    }
    let expected = mesh
        .exact_uses
        .iter()
        .map(|(use_, _)| use_.edge)
        .collect::<Vec<_>>();
    for reversed in [false, true] {
        let mut actual = incidence.iter().map(|(edge, _)| *edge).collect::<Vec<_>>();
        if reversed {
            actual.reverse();
        }
        let Some(anchor) = actual.iter().position(|edge| *edge == expected[0]) else {
            continue;
        };
        actual.rotate_left(anchor);
        let mut positions = Vec::with_capacity(expected.len());
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
            let mut boundary = Vec::with_capacity(actual.len());
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
            return Some(boundary);
        }
    }
    None
}

pub(super) fn deferred_boundary_cycle_matches(
    mesh: &MeshDeferredBoundaryCycle,
    incidence: &[(usize, bool)],
    missing: &HashSet<usize>,
) -> bool {
    deferred_boundary_cycle_assignment(mesh, incidence, missing).is_some()
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
    charge_collection_items(ctx, incident_count, "catia deferred incident edges")?;
    let mut incident = domain.missing_edges.clone();
    incident.extend(
        domain
            .cycles
            .iter()
            .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
    );
    incident.sort_unstable();
    incident.dedup();
    let Some(incidence) = incidence_cycles(&incident, edge_points) else {
        return Ok(None);
    };
    if incidence.len() != domain.cycles.len() {
        return Ok(None);
    }
    charge_collection_items(
        ctx,
        domain.missing_edges.len(),
        "catia deferred missing edges",
    )?;
    let missing = domain.missing_edges.iter().copied().collect::<HashSet<_>>();
    let compatibility_count = domain
        .cycles
        .len()
        .checked_mul(incidence.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("catia deferred compatibility cells", u64::MAX, u64::MAX)
        })?;
    charge_collection_items(
        ctx,
        domain.cycles.len(),
        "catia deferred compatibility rows",
    )?;
    charge_collection_items(
        ctx,
        compatibility_count,
        "catia deferred compatibility cells",
    )?;
    let compatible = domain
        .cycles
        .iter()
        .map(|mesh| {
            incidence
                .iter()
                .map(|candidate| deferred_boundary_cycle_assignment(mesh, candidate, &missing))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    charge_collection_items(ctx, domain.cycles.len(), "catia deferred matching rows")?;
    charge_collection_items(ctx, compatibility_count, "catia deferred matching cells")?;
    let boolean_compatible = compatible
        .iter()
        .map(|cycles| cycles.iter().map(Option::is_some).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut matched_mesh = ctx.alloc_filled(incidence.len(), None, "catia_deferred_match")?;
    for mesh in 0..domain.cycles.len() {
        let mut visited = ctx.alloc_filled(incidence.len(), false, "catia_deferred_visit")?;
        if !augment_cycle_matching(mesh, &boolean_compatible, &mut visited, &mut matched_mesh) {
            return Ok(None);
        }
    }
    let mut boundaries =
        ctx.alloc_filled(domain.cycles.len(), None, "catia_deferred_boundaries")?;
    for (incidence, mesh) in matched_mesh.into_iter().enumerate() {
        let Some(mesh) = mesh else {
            return Ok(None);
        };
        charge_collection_items(
            ctx,
            compatible[mesh][incidence].as_ref().map_or(0, Vec::len),
            "catia deferred copied boundary uses",
        )?;
        boundaries[mesh].clone_from(&compatible[mesh][incidence]);
    }
    Ok(boundaries
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .map(|boundaries| MeshFaceBoundaryAssignment { boundaries }))
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
    charge_collection_items(ctx, incident_count, "catia deferred close incident edges")?;
    let mut incident = domain.missing_edges.clone();
    incident.extend(
        domain
            .cycles
            .iter()
            .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
    );
    incident.sort_unstable();
    incident.dedup();
    let Some(incidence) = incidence_cycles(&incident, edge_points) else {
        return Ok(false);
    };
    if incidence.len() != domain.cycles.len() {
        return Ok(false);
    }
    charge_collection_items(
        ctx,
        domain.missing_edges.len(),
        "catia deferred close missing edges",
    )?;
    let missing = domain.missing_edges.iter().copied().collect::<HashSet<_>>();
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
    charge_collection_items(
        ctx,
        domain.cycles.len(),
        "catia deferred close compatibility rows",
    )?;
    charge_collection_items(ctx, cells, "catia deferred close compatibility cells")?;
    let compatible = domain
        .cycles
        .iter()
        .map(|mesh| {
            incidence
                .iter()
                .map(|candidate| deferred_boundary_cycle_matches(mesh, candidate, &missing))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut matched_mesh = ctx.alloc_filled(incidence.len(), None, "catia_deferred_close_match")?;
    for mesh in 0..domain.cycles.len() {
        let mut visited = ctx.alloc_filled(incidence.len(), false, "catia_deferred_close_visit")?;
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
                incidence_cycles(edges, edge_points).is_some_and(|cycles| cycles.len() == 1)
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
        let mut points = ctx.alloc_filled(
            assignment.len(),
            [0; 2],
            "catia component incidence edge points",
        )?;
        for &edge in &face_edges[face] {
            let Some(pair) = assignment[edge] else {
                return Ok(false);
            };
            points[edge] = pair;
        }
        if incidence_cycles(&face_edges[face], &points).is_none() {
            return Ok(false);
        }
        let Some(domain) = domains.and_then(|domains| domains.get(face)) else {
            continue;
        };
        let viable = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                assignments.iter().any(|boundary_assignment| {
                    mesh_assignment_endpoint_cycles_viable_where(
                        boundary_assignment,
                        choices,
                        None,
                        |edge, pair| {
                            assignment[edge]
                                .is_none_or(|selected| same_unordered_pair(selected, pair))
                        },
                    )
                    .unwrap_or(true)
                })
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                incidence_cycles(edges, &points).is_some_and(|cycles| cycles.len() == 1)
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
    if !edge_faces.iter().any(|faces| faces[0] != faces[1]) {
        return Ok(true);
    }
    let edge_points = assignment
        .iter()
        .map(|pair| pair.unwrap_or_default())
        .collect::<Vec<_>>();
    let mut edge_uses = HashMap::<usize, Vec<(usize, bool)>>::new();
    let mut boundary_count = 0usize;
    for incident in face_edges {
        let selected = incident
            .iter()
            .copied()
            .filter(|edge| assignment[*edge].is_some())
            .collect::<Vec<_>>();
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
                edges_at_point.entry(point).or_default().push(edge);
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
        let mut unseen = selected.iter().copied().collect::<HashSet<_>>();
        for first in selected {
            if !unseen.contains(&first) {
                continue;
            }
            let mut stack = vec![first];
            let mut component = Vec::new();
            let mut points = HashSet::new();
            while let Some(edge) = stack.pop() {
                if !unseen.remove(&edge) {
                    continue;
                }
                component.push(edge);
                for point in edge_points[edge] {
                    points.insert(point);
                    stack.extend(edges_at_point[&point].iter().copied());
                }
            }
            component.sort_unstable();
            let trail = if points.iter().all(|point| degrees[point] == 2) {
                let Some(cycles) = incidence_cycles(&component, &edge_points) else {
                    return Ok(false);
                };
                let [cycle] = cycles.as_slice() else {
                    return Ok(false);
                };
                cycle.iter().copied().collect()
            } else {
                let mut endpoints = points
                    .iter()
                    .copied()
                    .filter(|point| degrees[point] == 1)
                    .collect::<Vec<_>>();
                endpoints.sort_unstable();
                let [start, end] = endpoints.as_slice() else {
                    return Ok(false);
                };
                if points
                    .iter()
                    .any(|point| !endpoints.contains(point) && degrees[point] != 2)
                {
                    return Ok(false);
                }
                let mut remaining = component.iter().copied().collect::<HashSet<_>>();
                let mut point = *start;
                let mut trail = Vec::with_capacity(component.len());
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
                edge_uses
                    .entry(edge)
                    .or_default()
                    .push((boundary, reversed));
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
fn component_incidence_pair_solutions<F>(
    ctx: &DecodeContext<'_>,
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
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
pub(super) fn component_incidence_pair_solution_outcome<F>(
    ctx: &DecodeContext<'_>,
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
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
fn visit_component_incidence_pair_solutions<F, V>(
    ctx: &DecodeContext<'_>,
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
    visitor: &mut V,
) -> Result<IncidenceSolve<usize>, CodecError>
where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    visit_component_incidence_pair_solutions_with_coordinate_root_policy(
        ctx,
        choices,
        edge_faces,
        face_count,
        point_count,
        mesh_assignments,
        mesh_quotient,
        CoordinateRootPolicy::RequireUnique,
        partial_solution_valid,
        solution_valid,
        visitor,
        &budget,
    )
}

#[allow(clippy::too_many_arguments)]
fn visit_component_incidence_pair_solutions_with_coordinate_root_policy<F, V>(
    ctx: &DecodeContext<'_>,
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    coordinate_root_policy: CoordinateRootPolicy,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
    visitor: &mut V,
    session_budget: &WorkBudget<'_>,
) -> Result<IncidenceSolve<usize>, CodecError>
where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    #[allow(clippy::too_many_arguments)]
    fn solve_component_domain(
        ctx: &DecodeContext<'_>,
        component: &[usize],
        choices: &[Vec<[usize; 2]>],
        edge_faces: &[[usize; 2]],
        face_edges: &[Vec<usize>],
        mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
        assignment: &[Option<[usize; 2]>],
        degrees: &[BTreeMap<usize, u8>],
        point_count: usize,
        budget: &WorkBudget<'_>,
        coordinate_propagation_budget: &WorkBudget<'_>,
        boundary_propagation_budget: &WorkBudget<'_>,
        orientation_budget: &WorkBudget<'_>,
        solution_visitor: Option<MeshEndpointSolutionVisitor<'_>>,
    ) -> Result<bool, CodecError> {
        let mut active = ctx.alloc_filled(choices.len(), false, "catia incidence active edges")?;
        let mut constraints = HashSet::<(usize, usize)>::new();
        let mut point_support_edges = ctx.alloc_filled(
            face_edges.len(),
            HashMap::<usize, Vec<usize>>::new(),
            "catia incidence point support edges",
        )?;
        let mut component_faces = HashSet::new();
        for &edge in component {
            active[edge] = true;
            let faces = edge_faces[edge];
            for (rank, face) in faces.into_iter().enumerate() {
                if rank > 0 && face == faces[0] {
                    continue;
                }
                crate::resource::insert_set(
                    ctx,
                    &mut component_faces,
                    face,
                    "catia_incidence_component_faces",
                )?;
                let mut points = coordinate_domains
                    .filter(|_| choices[edge].is_empty())
                    .and_then(|domains| domains.edge_candidate_points(edge))
                    .unwrap_or_else(|| choices[edge].iter().flatten().copied().collect());
                points.sort_unstable();
                points.dedup();
                for point in points {
                    crate::resource::insert_set(
                        ctx,
                        &mut constraints,
                        (face, point),
                        "catia_incidence_point_constraints",
                    )?;
                    if !point_support_edges[face].contains_key(&point) {
                        crate::resource::insert_map(
                            ctx,
                            &mut point_support_edges[face],
                            point,
                            Vec::new(),
                            "catia_incidence_point_support_keys",
                        )?;
                    }
                    if let Some(support) = point_support_edges[face].get_mut(&point) {
                        crate::resource::push(
                            ctx,
                            support,
                            edge,
                            "catia_incidence_point_support_entries",
                        )?;
                    }
                }
            }
        }
        let mut constraints = constraints.into_iter().collect::<Vec<_>>();
        constraints.sort_unstable();
        let explicit_point_supports = choices
            .iter()
            .map(|pairs| {
                let mut supports = HashMap::<usize, Vec<[usize; 2]>>::new();
                for &pair in pairs {
                    supports.entry(pair[0]).or_default().push(pair);
                    if pair[1] != pair[0] {
                        supports.entry(pair[1]).or_default().push(pair);
                    }
                }
                supports
            })
            .collect();
        let face_configuration_domains = prepare_face_configuration_domains(
            ctx,
            mesh_assignments,
            choices,
            assignment,
            &active,
        )?;
        let filter = |solution: &[MeshEndpointPair]| -> Result<bool, CodecError> {
            let mut completed = assignment.to_vec();
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
        let solution_filter =
            Some(&filter as &dyn Fn(&[MeshEndpointPair]) -> Result<bool, CodecError>);
        let degree_support_budget = budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        let mut search = IncidenceComponentSearch {
            ctx,
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
            assignment: assignment.to_vec(),
            degrees: degrees.to_vec(),
            solutions: Vec::new(),
            solution_filter,
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

    #[allow(clippy::too_many_arguments)]
    fn visit_components<F, V>(
        ctx: &DecodeContext<'_>,
        component_index: usize,
        choices: &[Vec<[usize; 2]>],
        edge_faces: &[[usize; 2]],
        face_edges: &[Vec<usize>],
        mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
        mesh_quotient: Option<&MeshQuotient>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        coordinate_root_policy: CoordinateRootPolicy,
        partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
        solution_valid: &F,
        assignment: &mut [Option<[usize; 2]>],
        degrees: &mut [BTreeMap<usize, u8>],
        point_count: usize,
        budget: &WorkBudget<'_>,
        visitor: &mut V,
        visited: &mut usize,
        ambiguous: &mut bool,
        components: &[Vec<usize>],
        component_budget: &WorkBudget<'_>,
        orientation_budget: &WorkBudget<'_>,
        coordinate_propagation_budget: &WorkBudget<'_>,
        boundary_propagation_budget: &WorkBudget<'_>,
        session_budget: &WorkBudget<'_>,
    ) -> Result<ControlFlow<()>, IncidenceVisitError>
    where
        F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
        V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
    {
        let Some(component) = components.get(component_index) else {
            let pairs = assignment
                .iter()
                .copied()
                .collect::<Option<Vec<_>>>()
                .ok_or(IncidenceVisitError::Exhausted)?;
            let boundary_closed = boundary_domains_close(ctx, mesh_assignments, &pairs)?;
            let solution_accepted = boundary_closed && solution_valid(&pairs)?;
            if !solution_accepted {
                return Ok(ControlFlow::Continue(()));
            }
            if let Some(quotient) = mesh_quotient {
                let singleton = pairs
                    .iter()
                    .copied()
                    .map(|pair| vec![pair])
                    .collect::<Vec<_>>();
                let mut quotient = quotient.clone();
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
                    quotient.coordinate_domain_preparation_limit(point_count, &singleton)
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

        let base_assignment = assignment.to_vec();
        let base_degrees = degrees.to_vec();
        let mut downstream_control = Ok(ControlFlow::Continue(()));
        let mut visit_solution =
            |solution: &[MeshEndpointPair]| -> Result<ControlFlow<()>, CodecError> {
                if !budget.charge() {
                    downstream_control = Err(IncidenceVisitError::Exhausted);
                    return Ok(ControlFlow::Break(()));
                }
                let mut degree_undo = Vec::with_capacity(solution.len());
                for &(edge, pair) in solution {
                    assignment[edge] = Some(pair);
                    degree_undo.push((
                        edge,
                        adjust_incidence_degrees(degrees, edge_faces, edge, pair),
                    ));
                }
                let candidates = coordinate_domains.map(|_| {
                    assignment
                        .iter()
                        .enumerate()
                        .map(|(edge, pair)| {
                            pair.map_or_else(|| choices[edge].clone(), |pair| vec![pair])
                        })
                        .collect::<Vec<_>>()
                });
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
                            component_index + 1,
                            choices,
                            edge_faces,
                            face_edges,
                            mesh_assignments,
                            mesh_quotient,
                            refined_domains.as_ref().or(coordinate_domains),
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
            component,
            narrowed_choices,
            edge_faces,
            face_edges,
            mesh_assignments,
            coordinate_domains,
            partial_solution_valid,
            &base_assignment,
            &base_degrees,
            point_count,
            component_budget,
            coordinate_propagation_budget,
            boundary_propagation_budget,
            orientation_budget,
            Some(&mut visit_solution),
        )?;
        match downstream_control {
            Ok(ControlFlow::Continue(())) if component_exhausted => {
                Err(IncidenceVisitError::Exhausted)
            }
            control => control,
        }
    }

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
                let mut quotient = quotient.clone();
                let Some(preparation_limit) =
                    quotient.coordinate_domain_preparation_limit(point_count, choices)
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
        let base_choices = coordinate_domains
            .as_ref()
            .map_or(choices, MeshCoordinateRootDomains::edge_candidates)
            .to_vec();
        let mut narrowed_choices = base_choices.clone();
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
                narrowed_choices.clone_from(&base_choices);
            }
            let implicit_choices = narrowed_choices.clone();
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
                        narrowed_choices = refined.edge_candidates().to_vec();
                        coordinate_domains = Some(refined);
                    }
                    None if refinement_budget.exhausted() => {
                        narrowed_choices.clone_from(&base_choices);
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
            incidence_choice_components(choices, edge_faces, mesh_assignments, mesh_quotient);
        if let Some(constraint) = partial_solution_valid {
            components =
                join_incidence_components_by_coupling(components, constraint.coupled_edges);
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
                    crate::resource::push(
                        ctx,
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
            let Some(pairs) = fixed.into_iter().collect::<Option<Vec<_>>>() else {
                return Ok(None);
            };
            let boundary_closed = boundary_domains_close(ctx, mesh_assignments, &pairs)?;
            let solution_valid = solution_valid(&pairs)?;
            if !boundary_closed || !solution_valid {
                return Ok(None);
            }
            if let Some(quotient) = mesh_quotient {
                let singleton = pairs
                    .iter()
                    .copied()
                    .map(|pair| vec![pair])
                    .collect::<Vec<_>>();
                let mut quotient = quotient.clone();
                let Some(closure_limit) =
                    quotient.coordinate_domain_preparation_limit(point_count, &singleton)
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
                    let mut completed = fixed.clone();
                    for &(edge, pair) in solution {
                        completed[edge] = Some(pair);
                    }
                    let coordinate_feasible = if let Some(domains) = coordinate_domains.as_ref() {
                        let candidates = completed
                            .iter()
                            .enumerate()
                            .map(|(edge, pair)| {
                                pair.map_or_else(|| choices[edge].clone(), |pair| vec![pair])
                            })
                            .collect::<Vec<_>>();
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
                component,
                choices,
                edge_faces,
                &face_edges,
                mesh_assignments,
                coordinate_domains.as_ref(),
                partial_solution_valid,
                &fixed,
                &degrees,
                point_count,
                &component_preflight_budget,
                &coordinate_preflight_budget,
                &boundary_preflight_budget,
                &preflight_orientation_budget,
                Some(&mut accept_first),
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
            0,
            choices,
            edge_faces,
            &face_edges,
            mesh_assignments,
            mesh_quotient,
            coordinate_domains.as_ref(),
            coordinate_root_policy,
            partial_solution_valid,
            solution_valid,
            &mut fixed,
            &mut degrees,
            point_count,
            session_budget,
            visitor,
            &mut visited,
            &mut ambiguous,
            &components,
            session_budget,
            &orientation_budget,
            &coordinate_propagation_budget,
            &boundary_propagation_budget,
            session_budget,
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
) -> Result<Option<StandardTopology>, CodecError> {
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
        edge_rows,
        vertex_points,
        edge_faces,
        edge_candidates,
        face_count,
        None,
        quotient.as_ref(),
        None,
        Some(budget),
        &|_| Ok(true),
        &mut |pairs| -> Result<ControlFlow<()>, CodecError> {
            if assignment_count == MAX_TOPOLOGY_ASSIGNMENTS {
                invalid = true;
                return Ok(ControlFlow::Break(()));
            }
            assignment_count += 1;
            let oriented;
            let pairs = if let Some(ports) = edge_ports {
                let Some(propagated) = propagate_edge_port_points(
                    ctx,
                    ports,
                    &pairs.iter().copied().map(Some).collect::<Vec<_>>(),
                )?
                else {
                    invalid = true;
                    return Ok(ControlFlow::Break(()));
                };
                let Some(pairs) = propagated.into_iter().collect::<Option<Vec<_>>>() else {
                    invalid = true;
                    return Ok(ControlFlow::Break(()));
                };
                oriented = pairs;
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
            solution_pairs = Some(pairs.to_vec());
            Ok(ControlFlow::Continue(()))
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
        edge_rows.to_vec(),
        vertex_points.to_vec(),
        edge_faces,
        &solution_pairs,
        face_count,
    )
}

#[allow(clippy::too_many_arguments)]
fn visit_incidence_endpoint_pair_solutions<F, V>(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    face_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    complete_solution_budget: Option<&WorkBudget<'_>>,
    solution_valid: &F,
    visitor: &mut V,
) -> Result<IncidenceSolve<usize>, CodecError>
where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy(
        ctx,
        edge_rows,
        vertex_points,
        edge_faces,
        edge_candidates,
        face_count,
        mesh_assignments,
        mesh_quotient,
        CoordinateRootPolicy::RequireUnique,
        partial_solution_valid,
        complete_solution_budget,
        solution_valid,
        visitor,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy<F, V>(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    face_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    coordinate_root_policy: CoordinateRootPolicy,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    complete_solution_budget: Option<&WorkBudget<'_>>,
    solution_valid: &F,
    visitor: &mut V,
) -> Result<IncidenceSolve<usize>, CodecError>
where
    F: Fn(&[[usize; 2]]) -> Result<bool, CodecError>,
    V: FnMut(&[[usize; 2]]) -> Result<ControlFlow<()>, CodecError>,
{
    charge_collection_items(ctx, edge_candidates.len(), "catia incidence choice rows")?;
    for candidates in edge_candidates {
        charge_collection_items(ctx, candidates.len(), "catia incidence choice pairs")?;
    }
    let mut choices = edge_candidates.to_vec();
    for candidates in &mut choices {
        for pair in candidates.iter_mut() {
            pair.sort_unstable();
        }
        candidates.sort_unstable();
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
            edge_rows.to_vec(),
            vertex_points.to_vec(),
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
    let outcome = visit_component_incidence_pair_solutions_with_coordinate_root_policy(
        ctx,
        &choices,
        edge_faces,
        face_count,
        vertex_points.len(),
        mesh_assignments,
        mesh_quotient,
        coordinate_root_policy,
        partial_solution_valid,
        &complete_valid,
        &mut budgeted_visitor,
        session_budget,
    )?;
    if complete_solution_budget.is_some_and(WorkBudget::exhausted) {
        Ok(IncidenceSolve::Exhausted)
    } else {
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
