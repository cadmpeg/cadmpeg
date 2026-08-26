//! Incidence backtracking constraint solver for standard B-rep topology.
//!
//! Reconstructs face/edge incidence from serialized boundary domains.

use cadmpeg_core::decode::{alloc_filled, WorkBudget};

use crate::families::standard::topology::{
    incidence_cycles, reconstruct_incidence, solve_boundary_orientation_constraints, EdgeRow,
    StandardTopology,
};
use crate::solve::mesh_quotient::{
    initial_mesh_quotient, mesh_assignment_endpoint_cycle_support_by,
    mesh_assignment_endpoint_cycles_viable_by, mesh_assignment_endpoint_cycles_viable_where,
    mesh_face_endpoint_configurations, CoordinateRootClosure, MeshCoordinateRootDomains,
    MeshEndpointCandidates, MeshEndpointPair, MeshEndpointSolutionFilter,
    MeshFaceEndpointConfigurations, MeshImplicitEdgeCandidates, MeshPartialEndpointConstraint,
    MeshQuotient, MeshQuotientGaugeState, MAX_FACE_ENDPOINT_CONFIGURATION_WORK,
    MAX_MESH_CONSTRAINT_OPERATIONS,
};
use crate::solve::missing_edge::{
    propagate_edge_port_points, same_unordered_pair, MeshBoundaryEdgeCandidate,
    MeshDeferredBoundaryCycle, MeshDeferredFaceBoundary, MeshFaceBoundaryAssignment,
    MeshFaceBoundaryDomain,
};
use crate::solve::UnionFind;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ops::ControlFlow;

type MeshEndpointSolutionVisitor<'a> = &'a mut dyn FnMut(&[MeshEndpointPair]) -> ControlFlow<()>;
type DegreeSupportWitnesses = RefCell<HashMap<(usize, usize), (usize, [usize; 2])>>;

pub(crate) fn prune_incidence_choices(
    choices: &mut [Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
) -> Option<()> {
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
        supports: &mut [BTreeMap<usize, u32>],
        edge_supports: &mut [HashSet<usize>],
        edge_faces: &[[usize; 2]],
        edge: usize,
        retained: HashSet<usize>,
    ) -> Option<()> {
        let removed = edge_supports[edge]
            .difference(&retained)
            .copied()
            .collect::<Vec<_>>();
        for face in unique_faces(edge_faces[edge]) {
            for &point in &removed {
                let count = supports[face].get_mut(&point)?;
                *count = count.checked_sub(1)?;
                if *count == 0 {
                    supports[face].remove(&point);
                }
            }
        }
        edge_supports[edge] = retained;
        Some(())
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

    fn choice_points(choices: &[[usize; 2]]) -> HashSet<usize> {
        choices.iter().flatten().copied().collect::<HashSet<_>>()
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
        return None;
    }
    let mut face_edges = alloc_filled(face_count, Vec::new(), "catia_incidence_face_edges").ok()?;
    for (edge, faces) in edge_faces.iter().copied().enumerate() {
        for face in unique_faces(faces) {
            face_edges[face].push(edge);
        }
    }
    let mut fixed = alloc_filled(choices.len(), false, "catia_incidence_fixed_edges").ok()?;
    let mut degrees = alloc_filled(
        face_count,
        BTreeMap::<usize, u8>::new(),
        "catia_incidence_degrees",
    )
    .ok()?;
    let mut edge_supports = choices
        .iter()
        .map(|pairs| choice_points(pairs))
        .collect::<Vec<_>>();
    let mut supports = alloc_filled(
        face_count,
        BTreeMap::<usize, u32>::new(),
        "catia_incidence_supports",
    )
    .ok()?;
    for (edge, points) in edge_supports.iter().enumerate() {
        for face in unique_faces(edge_faces[edge]) {
            for &point in points {
                let count = supports[face].entry(point).or_default();
                *count = count.checked_add(1)?;
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
            let retained = choices[edge]
                .iter()
                .copied()
                .filter(|pair| {
                    fits(&degrees, edge_faces, edge, *pair)
                        && preserves_new_degree_support(
                            &supports,
                            &edge_supports,
                            &degrees,
                            edge_faces,
                            edge,
                            *pair,
                        )
                })
                .collect::<Vec<_>>();
            choices[edge] = retained;
            changed |= choices[edge].len() != before;
            remove_edge_support(
                &mut supports,
                &mut edge_supports,
                edge_faces,
                edge,
                choice_points(&choices[edge]),
            )?;
            let [pair] = choices[edge].as_slice() else {
                if choices[edge].is_empty() {
                    return None;
                }
                continue;
            };
            for face in unique_faces(edge_faces[edge]) {
                for point in pair {
                    let degree = degrees[face].entry(*point).or_default();
                    *degree = degree.checked_add(1)?;
                }
            }
            remove_edge_support(
                &mut supports,
                &mut edge_supports,
                edge_faces,
                edge,
                HashSet::new(),
            )?;
            fixed[edge] = true;
            changed = true;
        }
        for face in 0..face_count {
            for point in degree_one_points(&degrees, face) {
                match supports[face].get(&point).copied().unwrap_or(0) {
                    0 => return None,
                    1 => {
                        let edge = sole_supporting_edge(&face_edges, &edge_supports, face, point)?;
                        let before = choices[edge].len();
                        choices[edge].retain(|pair| pair.contains(&point));
                        if choices[edge].is_empty() {
                            return None;
                        }
                        changed |= choices[edge].len() != before;
                        remove_edge_support(
                            &mut supports,
                            &mut edge_supports,
                            edge_faces,
                            edge,
                            choice_points(&choices[edge]),
                        )?;
                    }
                    _ => {}
                }
            }
        }
        if !changed {
            return Some(());
        }
    }
}

pub(crate) fn incidence_choice_components(
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
        if quotient.union.len() == choices.len().saturating_mul(2) {
            let mut owner = HashMap::<usize, usize>::new();
            for &edge in &ambiguous {
                for port in [edge * 2, edge * 2 + 1] {
                    let root = quotient.union.find(port);
                    for point in quotient.domains[root].iter().copied() {
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
pub(crate) fn join_incidence_components_by_coupling(
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

pub(crate) fn order_incidence_components_by_branch_width(
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
pub(crate) fn order_incidence_components_by_constraints(
    components: &mut Vec<Vec<usize>>,
    choices: &[Vec<[usize; 2]>],
    assignment_predecessors: Option<&[Option<usize>]>,
    assignment_dependencies: Option<&[Vec<usize>]>,
) -> Option<()> {
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
        return None;
    }
    if assignment_predecessors.is_none() && assignment_dependencies.is_none() {
        return order_incidence_components_by_branch_width(components, choices);
    }

    let component_by_edge = components
        .iter()
        .enumerate()
        .flat_map(|(component, edges)| edges.iter().copied().map(move |edge| (edge, component)))
        .collect::<HashMap<_, _>>();
    if component_by_edge.len() != components.iter().map(Vec::len).sum::<usize>() {
        return None;
    }
    let mut incoming =
        alloc_filled(components.len(), 0usize, "catia_incidence_component_in").ok()?;
    let mut outgoing = alloc_filled(
        components.len(),
        Vec::<usize>::new(),
        "catia_incidence_component_out",
    )
    .ok()?;
    let mut local_incoming =
        alloc_filled(choices.len(), 0usize, "catia_incidence_local_in").ok()?;
    let mut local_outgoing = alloc_filled(
        choices.len(),
        Vec::<usize>::new(),
        "catia_incidence_local_out",
    )
    .ok()?;
    let mut add_dependency = |target_edge: usize, prerequisite_edge: usize| {
        let (Some(&target_component), Some(&prerequisite_component)) = (
            component_by_edge.get(&target_edge),
            component_by_edge.get(&prerequisite_edge),
        ) else {
            return;
        };
        if target_component == prerequisite_component {
            if !local_outgoing[prerequisite_edge].contains(&target_edge) {
                local_outgoing[prerequisite_edge].push(target_edge);
                local_incoming[target_edge] += 1;
            }
            return;
        }
        if !outgoing[prerequisite_component].contains(&target_component) {
            outgoing[prerequisite_component].push(target_component);
            incoming[target_component] += 1;
        }
    };
    if let Some(predecessors) = assignment_predecessors {
        for (target, prerequisite) in predecessors.iter().enumerate() {
            if let Some(prerequisite) = prerequisite {
                add_dependency(target, *prerequisite);
            }
        }
    }
    if let Some(dependencies) = assignment_dependencies {
        for (target, prerequisites) in dependencies.iter().enumerate() {
            for prerequisite in prerequisites {
                add_dependency(target, *prerequisite);
            }
        }
    }
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
                local_ready.push(dependent);
            }
        }
    }
    if local_ordered != component_by_edge.len() {
        return None;
    }

    let branch_width = |component: &[usize]| {
        component.iter().fold(1usize, |width, edge| {
            width.saturating_mul(choices[*edge].len())
        })
    };
    let mut ready = (0..components.len())
        .filter(|component| incoming[*component] == 0)
        .collect::<Vec<_>>();
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
                ready.push(dependent);
            }
        }
    }
    if ordered.len() != components.len() {
        return None;
    }
    *components = ordered;
    Some(())
}

pub(crate) struct IncidenceComponentSearch<'a, 'v> {
    pub(crate) choices: &'a [Vec<[usize; 2]>],
    pub(crate) explicit_point_supports: Option<Vec<HashMap<usize, Vec<[usize; 2]>>>>,
    pub(crate) point_support_edges: Option<Vec<HashMap<usize, Vec<usize>>>>,
    pub(crate) degree_support_witnesses: DegreeSupportWitnesses,
    pub(crate) edge_faces: &'a [[usize; 2]],
    pub(crate) face_edges: &'a [Vec<usize>],
    pub(crate) mesh_assignments: Option<&'a [MeshFaceBoundaryDomain]>,
    pub(crate) face_configuration_domains: Option<PreparedFaceFactors>,
    pub(crate) mesh_quotient: Option<&'a MeshQuotient>,
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
    pub(crate) coordinate_propagation_budget: &'a WorkBudget<'a>,
    pub(crate) boundary_propagation_budget: &'a WorkBudget<'a>,
    pub(crate) exhausted: bool,
    pub(crate) stopped: bool,
}

enum IncidenceBranch {
    Options(std::vec::IntoIter<(usize, [usize; 2])>),
    Implicit {
        edge: usize,
        candidates: MeshImplicitEdgeCandidates,
    },
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
    assigned: Vec<(usize, [usize; 2])>,
    affected_faces: Vec<usize>,
    coordinate_domains: Option<MeshCoordinateRootDomains>,
    factor_checkpoint: Option<Vec<Vec<u64>>>,
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

pub(crate) struct PreparedFaceFactors {
    domains: Vec<Option<MeshFaceEndpointConfigurations>>,
    factor_faces: Vec<usize>,
    factor_by_face: Vec<Option<usize>>,
    factors_by_edge: Vec<Vec<usize>>,
    graph: Option<FaceFactorGraph>,
    active: Option<Vec<Vec<u64>>>,
}

fn full_configuration_mask(len: usize) -> Option<Vec<u64>> {
    let mut mask = alloc_filled(
        len.div_ceil(u64::BITS as usize),
        u64::MAX,
        "catia_face_config_full_mask",
    )
    .ok()?;
    if let Some(last) = mask.last_mut() {
        let remainder = len % u64::BITS as usize;
        if remainder != 0 {
            *last = (1 << remainder) - 1;
        }
    }
    Some(mask)
}

fn set_mask_bit<K: Eq + std::hash::Hash>(
    map: &mut HashMap<K, Vec<u64>>,
    key: K,
    word: usize,
    bit: u64,
    word_count: usize,
    operation: &'static str,
) -> Option<()> {
    match map.entry(key) {
        std::collections::hash_map::Entry::Occupied(mut occupied) => {
            occupied.get_mut()[word] |= bit;
        }
        std::collections::hash_map::Entry::Vacant(vacant) => {
            let mut mask = alloc_filled(word_count, 0u64, operation).ok()?;
            mask[word] |= bit;
            vacant.insert(mask);
        }
    }
    Some(())
}

fn configuration_mask_contains(mask: &[u64], index: usize) -> bool {
    mask.get(index / u64::BITS as usize)
        .is_some_and(|word| word & (1 << (index % u64::BITS as usize)) != 0)
}

impl FaceFactorGraph {
    fn compile(
        domains: &[MeshFaceEndpointConfigurations],
        budget: &WorkBudget<'_>,
    ) -> Option<Self> {
        let edge_sets = domains
            .iter()
            .map(|domain| {
                domain
                    .iter()
                    .flatten()
                    .map(|(edge, _)| *edge)
                    .collect::<HashSet<_>>()
            })
            .collect::<Vec<_>>();
        let mut right_indexes = Vec::with_capacity(domains.len());
        for domain in domains {
            let word_count = domain.len().div_ceil(u64::BITS as usize);
            let mut present = HashMap::<usize, Vec<u64>>::new();
            let mut matching = HashMap::<(usize, [usize; 2]), Vec<u64>>::new();
            for (configuration, candidate) in domain.iter().enumerate() {
                if !budget.charge_by(candidate.len().max(1)) {
                    return None;
                }
                let word = configuration / u64::BITS as usize;
                let bit = 1 << (configuration % u64::BITS as usize);
                for &(edge, pair) in candidate {
                    set_mask_bit(
                        &mut present,
                        edge,
                        word,
                        bit,
                        word_count,
                        "catia_face_config_present",
                    )?;
                    set_mask_bit(
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
            alloc_filled(domains.len(), Vec::new(), "catia_face_factor_incoming").ok()?;
        for left in 0..domains.len() {
            for right in 0..domains.len() {
                if left == right || edge_sets[left].is_disjoint(&edge_sets[right]) {
                    continue;
                }
                let word_count = domains[right].len().div_ceil(u64::BITS as usize);
                let (present, matching) = &right_indexes[right];
                let mut supports = Vec::with_capacity(domains[left].len());
                for candidate in &domains[left] {
                    if !budget.charge_by(candidate.len().saturating_add(word_count).max(1)) {
                        return None;
                    }
                    let mut compatible = full_configuration_mask(domains[right].len())?;
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
                arcs.push(FaceFactorArc {
                    left,
                    right,
                    supports,
                });
                incoming[right].push(arc);
            }
        }
        Some(Self {
            arcs,
            incoming,
            domain_lengths: domains.iter().map(Vec::len).collect(),
        })
    }

    fn full_state(&self) -> Option<Vec<Vec<u64>>> {
        self.domain_lengths
            .iter()
            .map(|length| full_configuration_mask(*length))
            .collect()
    }

    fn propagate(
        &self,
        active: &mut [Vec<u64>],
        initial: impl IntoIterator<Item = usize>,
        budget: &WorkBudget<'_>,
    ) -> Option<bool> {
        let mut queue = initial.into_iter().collect::<VecDeque<_>>();
        while let Some(arc_index) = queue.pop_front() {
            let arc = &self.arcs[arc_index];
            let mut changed = false;
            for (configuration, supports) in arc.supports.iter().enumerate() {
                if !configuration_mask_contains(&active[arc.left], configuration) {
                    continue;
                }
                if !budget.charge_by(supports.len().max(1)) {
                    return None;
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
                return Some(false);
            }
            queue.extend(self.incoming[arc.left].iter().copied());
        }
        Some(true)
    }

    fn propagate_all(&self, active: &mut [Vec<u64>], budget: &WorkBudget<'_>) -> Option<bool> {
        self.propagate(active, 0..self.arcs.len(), budget)
    }

    fn propagate_from(
        &self,
        domain: usize,
        active: &mut [Vec<u64>],
        budget: &WorkBudget<'_>,
    ) -> Option<bool> {
        self.propagate(active, self.incoming[domain].iter().copied(), budget)
    }
}

impl PreparedFaceFactors {
    #[cfg(test)]
    pub(crate) fn domains(&self) -> &[Option<MeshFaceEndpointConfigurations>] {
        &self.domains
    }

    fn refine_edges(
        &mut self,
        assigned: &[(usize, [usize; 2])],
        budget: &WorkBudget<'_>,
    ) -> Result<Option<Vec<Vec<u64>>>, ()> {
        let (Some(graph), Some(active)) = (&self.graph, &mut self.active) else {
            return Ok(None);
        };
        let checkpoint = active.clone();
        let mut affected = Vec::new();
        for &(edge, pair) in assigned {
            let Some(factors) = self.factors_by_edge.get(edge) else {
                continue;
            };
            for &factor in factors {
                let face = self.factor_faces[factor];
                let Some(configurations) = self.domains.get(face).and_then(Option::as_ref) else {
                    *active = checkpoint;
                    return Err(());
                };
                affected.push(factor);
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
                    *active = checkpoint;
                    return Err(());
                }
            }
        }
        affected.sort_unstable();
        affected.dedup();
        let initial = affected
            .iter()
            .flat_map(|factor| graph.incoming[*factor].iter().copied())
            .collect::<Vec<_>>();
        match graph.propagate(active, initial, budget) {
            Some(true) | None => Ok(Some(checkpoint)),
            Some(false) => {
                *active = checkpoint;
                Err(())
            }
        }
    }

    fn restore(&mut self, checkpoint: Option<Vec<Vec<u64>>>) {
        if let Some(checkpoint) = checkpoint {
            self.active = Some(checkpoint);
        }
    }

    fn assignment_state(
        &self,
        assignment: &[Option<[usize; 2]>],
        budget: &WorkBudget<'_>,
    ) -> Result<Option<Vec<Vec<u64>>>, ()> {
        let (Some(graph), Some(retained)) = (&self.graph, &self.active) else {
            return Ok(None);
        };
        let mut active = retained.clone();
        for (factor, &face) in self.factor_faces.iter().enumerate() {
            let Some(configurations) = self.domains.get(face).and_then(Option::as_ref) else {
                return Err(());
            };
            for (configuration, pairs) in configurations.iter().enumerate() {
                if !configuration_mask_contains(&active[factor], configuration)
                    || pairs.iter().all(|(edge, pair)| {
                        assignment
                            .get(*edge)
                            .copied()
                            .flatten()
                            .is_none_or(|selected| same_unordered_pair(selected, *pair))
                    })
                {
                    continue;
                }
                active[factor][configuration / u64::BITS as usize] &=
                    !(1 << (configuration % u64::BITS as usize));
            }
            if active[factor].iter().all(|word| *word == 0) {
                return Err(());
            }
        }
        match graph.propagate_all(&mut active, budget) {
            Some(true) => Ok(Some(active)),
            Some(false) => Err(()),
            None => Ok(None),
        }
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

pub(crate) fn prune_face_configuration_support(
    domains: &mut [MeshFaceEndpointConfigurations],
    budget: &WorkBudget<'_>,
) -> bool {
    let edge_sets = domains
        .iter()
        .map(|domain| {
            domain
                .iter()
                .flatten()
                .map(|(edge, _)| *edge)
                .collect::<HashSet<_>>()
        })
        .collect::<Vec<_>>();
    let Ok(mut neighbors) = alloc_filled(domains.len(), Vec::new(), "catia_face_config_neighbors")
    else {
        return true;
    };
    let mut queue = VecDeque::new();
    for left in 0..domains.len() {
        for right in 0..domains.len() {
            if left != right && !edge_sets[left].is_disjoint(&edge_sets[right]) {
                neighbors[left].push(right);
                queue.push_back((left, right));
            }
        }
    }
    while let Some((left, right)) = queue.pop_front() {
        let word_count = domains[right].len().div_ceil(u64::BITS as usize);
        let mut present = HashMap::<usize, Vec<u64>>::new();
        let mut matching = HashMap::<(usize, [usize; 2]), Vec<u64>>::new();
        for (configuration, candidate) in domains[right].iter().enumerate() {
            if !budget.charge_by(candidate.len().max(1)) {
                return true;
            }
            let word = configuration / u64::BITS as usize;
            let bit = 1 << (configuration % u64::BITS as usize);
            for &(edge, pair) in candidate {
                if set_mask_bit(
                    &mut present,
                    edge,
                    word,
                    bit,
                    word_count,
                    "catia_face_config_present",
                )
                .is_none()
                    || set_mask_bit(
                        &mut matching,
                        (edge, pair),
                        word,
                        bit,
                        word_count,
                        "catia_face_config_matching",
                    )
                    .is_none()
                {
                    return true;
                }
            }
        }
        let mut keep = Vec::with_capacity(domains[left].len());
        for candidate in &domains[left] {
            if !budget.charge_by(candidate.len().saturating_add(word_count).max(1)) {
                return true;
            }
            let Ok(mut viable) = alloc_filled(word_count, u64::MAX, "catia_face_config_viable")
            else {
                return true;
            };
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
            return false;
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
                    queue.push_back((neighbor, left));
                }
            }
        }
    }
    true
}

pub(crate) fn prune_face_configuration_singleton_support(
    domains: &mut [MeshFaceEndpointConfigurations],
    budget: &WorkBudget<'_>,
) -> bool {
    let Some(graph) = FaceFactorGraph::compile(domains, budget) else {
        return true;
    };
    let Some(mut active) = graph.full_state() else {
        return true;
    };
    let active_clone_work = active.iter().map(Vec::len).sum::<usize>().max(1);
    match graph.propagate_all(&mut active, budget) {
        Some(true) => {}
        Some(false) => return false,
        None => return true,
    }
    loop {
        let mut changed = false;
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
                    return true;
                }
                let mut trial = active.clone();
                trial[domain].fill(0);
                trial[domain][configuration / u64::BITS as usize] =
                    1 << (configuration % u64::BITS as usize);
                match graph.propagate_from(domain, &mut trial, budget) {
                    Some(true) => {}
                    Some(false) => {
                        active[domain][configuration / u64::BITS as usize] &=
                            !(1 << (configuration % u64::BITS as usize));
                        changed = true;
                        domain_changed = true;
                    }
                    None => {
                        retain_configuration_masks(domains, &active);
                        return true;
                    }
                }
            }
            if active[domain].iter().all(|word| *word == 0) {
                return false;
            }
            if domain_changed {
                match graph.propagate_from(domain, &mut active, budget) {
                    Some(true) => {}
                    Some(false) => return false,
                    None => {
                        retain_configuration_masks(domains, &active);
                        return true;
                    }
                }
            }
        }
        if !changed {
            retain_configuration_masks(domains, &active);
            return true;
        }
    }
}

pub(crate) fn prune_ordered_face_endpoint_support(
    domains: &[MeshFaceBoundaryDomain],
    choices: &mut [Vec<[usize; 2]>],
    budget: &WorkBudget<'_>,
) -> bool {
    loop {
        let mut changed = false;
        for domain in domains {
            let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                continue;
            };
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
            let Ok(selected) = alloc_filled(choices.len(), None, "catia_ordered_face_selection")
            else {
                return true;
            };
            let Some(configurations) =
                mesh_face_endpoint_configurations(assignments, choices, &selected, budget)
            else {
                if budget.exhausted() {
                    return true;
                }
                continue;
            };
            if configurations.is_empty() {
                return false;
            }
            let mut supported = HashMap::<usize, HashSet<[usize; 2]>>::new();
            for configuration in configurations {
                for (edge, pair) in configuration {
                    if !budget.charge() {
                        return true;
                    }
                    supported.entry(edge).or_default().insert(pair);
                }
            }
            for edge in edges {
                let Some(edge_supported) = supported.get(&edge) else {
                    continue;
                };
                let mut retained = Vec::with_capacity(choices[edge].len());
                for pair in choices[edge].iter().copied() {
                    if !budget.charge() {
                        return true;
                    }
                    let mut canonical = pair;
                    canonical.sort_unstable();
                    if edge_supported.contains(&canonical) {
                        retained.push(pair);
                    }
                }
                if retained.is_empty() {
                    return false;
                }
                if retained.len() != choices[edge].len() {
                    choices[edge] = retained;
                    changed = true;
                }
            }
        }
        if !changed {
            return true;
        }
    }
}

pub(crate) fn prune_implicit_ordered_face_endpoint_support(
    domains: &[MeshFaceBoundaryDomain],
    choices: &mut [Vec<[usize; 2]>],
    coordinate_domains: &MeshCoordinateRootDomains,
    budget: &WorkBudget<'_>,
) -> bool {
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
                ) else {
                    return true;
                };
                if support.is_empty() {
                    continue;
                }
                assignment_found = true;
                for (edge, pairs) in support {
                    face_support.entry(edge).or_default().extend(pairs);
                }
            }
            if !assignment_found {
                return false;
            }
            for (edge, supported) in face_support {
                let Some(current) = choices.get(edge) else {
                    return false;
                };
                let values = if current.is_empty() {
                    let Some(values) = coordinate_domains.implicit_edge_candidates(edge, None)
                    else {
                        return false;
                    };
                    values.collect::<Vec<_>>()
                } else {
                    current.clone()
                };
                let mut retained = Vec::new();
                for mut pair in values {
                    if !budget.charge() {
                        return true;
                    }
                    pair.sort_unstable();
                    if supported.contains(&pair) {
                        retained.push(pair);
                    }
                }
                retained.sort_unstable();
                retained.dedup();
                if retained.is_empty() {
                    return false;
                }
                if retained != choices[edge] {
                    choices[edge] = retained;
                    changed = true;
                }
            }
        }
        if !changed {
            return true;
        }
    }
}

pub(crate) fn prepare_face_configuration_domains(
    assignments: Option<&[MeshFaceBoundaryDomain]>,
    choices: &[Vec<[usize; 2]>],
    selected: &[Option<[usize; 2]>],
    active: &[bool],
) -> Option<PreparedFaceFactors> {
    let assignments = assignments?;
    let mut domains = alloc_filled(assignments.len(), None, "catia_face_factor_domains").ok()?;
    for (face, domain) in assignments.iter().enumerate() {
        let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
            continue;
        };
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
    let retained_faces = domains
        .iter()
        .enumerate()
        .filter_map(|(face, domain)| domain.as_ref().map(|_| face))
        .collect::<Vec<_>>();
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
    let viable = prune_face_configuration_support(&mut configurations, &arc_budget);
    let singleton_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    if !viable
        || (!arc_budget.exhausted()
            && !prune_face_configuration_singleton_support(&mut configurations, &singleton_budget))
    {
        if let Some(domain) = configurations.first_mut() {
            domain.clear();
        }
    }
    let graph_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let graph = FaceFactorGraph::compile(&configurations, &graph_budget);
    let active = graph.as_ref().and_then(FaceFactorGraph::full_state);
    let mut factor_by_face = alloc_filled(domains.len(), None, "catia_face_factor_by_face").ok()?;
    let mut factors_by_edge =
        alloc_filled(choices.len(), Vec::new(), "catia_face_factors_by_edge").ok()?;
    for (factor, &face) in retained_faces.iter().enumerate() {
        factor_by_face[face] = Some(factor);
        let mut edges = configurations[factor]
            .iter()
            .flatten()
            .map(|(edge, _)| *edge)
            .collect::<Vec<_>>();
        edges.sort_unstable();
        edges.dedup();
        for edge in edges {
            if let Some(factors) = factors_by_edge.get_mut(edge) {
                factors.push(factor);
            }
        }
    }
    for (face, configurations) in retained_faces.iter().copied().zip(configurations) {
        if let Some(domain) = &mut domains[face] {
            *domain = configurations;
        }
    }
    Some(PreparedFaceFactors {
        domains,
        factor_faces: retained_faces,
        factor_by_face,
        factors_by_edge,
        graph,
        active,
    })
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IncidenceSolve<T> {
    Solved(T),
    Rejected(IncidenceRejection),
    Ambiguous,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoordinateRootPolicy {
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

pub(crate) fn compact_boundary_domain_viable(
    domain: &MeshFaceBoundaryDomain,
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
) -> bool {
    let edges = match domain {
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
            edges
        }
    };
    let selected_pairs = edges
        .iter()
        .copied()
        .map(|edge| {
            selected
                .filter(|(selected_edge, _)| *selected_edge == edge)
                .map(|(_, pair)| pair)
                .or(assignment[edge])
                .map(|pair| (edge, pair))
        })
        .collect::<Vec<_>>();
    let complete = selected_pairs.iter().all(Option::is_some);
    if matches!(domain, MeshFaceBoundaryDomain::UnorderedFullCycle(_)) && !complete {
        let mut point_nodes = HashMap::new();
        let mut degrees = Vec::<u8>::new();
        let mut components = UnionFind::new(0);
        for (_, pair) in selected_pairs.iter().flatten().copied() {
            let nodes = pair.map(|point| {
                *point_nodes.entry(point).or_insert_with(|| {
                    degrees.push(0);
                    components.push()
                })
            });
            if nodes[0] == nodes[1] {
                degrees[nodes[0]] += 2;
            } else {
                degrees[nodes[0]] += 1;
                degrees[nodes[1]] += 1;
                components.union(nodes[0], nodes[1]);
            }
        }
        if degrees.iter().any(|degree| *degree > 2) {
            return false;
        }
        let mut open_components = HashSet::new();
        for (node, degree) in degrees.into_iter().enumerate() {
            if degree < 2 {
                open_components.insert(components.find(node));
            }
        }
        return (0..components.len()).all(|node| open_components.contains(&components.find(node)));
    }
    let Some(selected_pairs) = selected_pairs.into_iter().collect::<Option<Vec<_>>>() else {
        return true;
    };
    let Ok(mut edge_points) = alloc_filled(assignment.len(), [0; 2], "catia labeled edge points")
    else {
        return false;
    };
    for (edge, pair) in selected_pairs {
        edge_points[edge] = pair;
    }
    match domain {
        MeshFaceBoundaryDomain::Ordered(_) => true,
        MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
            incidence_cycles(edges, &edge_points).is_some_and(|cycles| cycles.len() == 1)
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            deferred_boundary_closes(domain, &edge_points)
        }
    }
}

pub(crate) fn labeled_assignment_endpoint_cycles_viable(
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
                let Some(first_points) = edge_points.get(first.edge).copied().flatten() else {
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

pub(crate) enum CompactBoundaryAdvanceOutcome {
    Complete(Vec<MeshQuotientGaugeState>),
    Rejected,
    Exhausted,
}

pub(crate) fn advance_compact_boundary_domains<'a>(
    domains: impl IntoIterator<Item = &'a MeshFaceBoundaryDomain>,
    choices: &[Vec<[usize; 2]>],
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
    mut states: Vec<MeshQuotientGaugeState>,
    budget: &WorkBudget<'_>,
) -> CompactBoundaryAdvanceOutcome {
    const MAX_QUOTIENT_STATES: usize = 4_096;

    let mut ordered = Vec::<Vec<MeshFaceBoundaryAssignment>>::new();
    for domain in domains {
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
        let Ok(mut points) = alloc_filled(
            assignment.len(),
            [0; 2],
            "catia compact boundary edge points",
        ) else {
            return CompactBoundaryAdvanceOutcome::Rejected;
        };
        for (edge, pair) in edge_points {
            points[edge] = pair;
        }
        let alternatives = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => assignments.clone(),
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                let Some([cycle]) = incidence_cycles(edges, &points)
                    .and_then(|cycles| <[Vec<(usize, bool)>; 1]>::try_from(cycles).ok())
                else {
                    return CompactBoundaryAdvanceOutcome::Rejected;
                };
                vec![MeshFaceBoundaryAssignment {
                    boundaries: vec![cycle
                        .into_iter()
                        .map(|(edge, _)| MeshBoundaryEdgeCandidate {
                            edge,
                            start: 0,
                            end: 0,
                            reversed: None,
                        })
                        .collect()],
                }]
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                let Some(materialized) = deferred_boundary_assignment(domain, &points) else {
                    return CompactBoundaryAdvanceOutcome::Rejected;
                };
                vec![materialized]
            }
        };
        ordered.push(alternatives);
    }
    if ordered.is_empty() {
        return CompactBoundaryAdvanceOutcome::Complete(states);
    }
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
                            .saturating_add(next_oriented.len().max(1)),
                    ) {
                        return CompactBoundaryAdvanceOutcome::Exhausted;
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
            return CompactBoundaryAdvanceOutcome::Exhausted;
        }
        if next.is_empty() {
            return CompactBoundaryAdvanceOutcome::Rejected;
        }
        states = next;
    }
    CompactBoundaryAdvanceOutcome::Complete(states)
}

#[cfg(test)]
pub(crate) fn compact_boundary_domains_jointly_viable<'a>(
    domains: impl IntoIterator<Item = &'a MeshFaceBoundaryDomain>,
    choices: &[Vec<[usize; 2]>],
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
    quotient: &MeshQuotient,
    budget: &WorkBudget<'_>,
) -> bool {
    matches!(
        advance_compact_boundary_domains(
            domains,
            choices,
            assignment,
            selected,
            vec![(quotient.clone(), HashSet::new())],
            budget,
        ),
        CompactBoundaryAdvanceOutcome::Complete(_)
    )
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
                .and_then(|point| {
                    self.explicit_point_supports
                        .as_ref()?
                        .get(edge)?
                        .get(&point)
                        .cloned()
                })
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
        domains: &MeshCoordinateRootDomains,
        edge: usize,
        pair: [usize; 2],
    ) -> Option<MeshCoordinateRootDomains> {
        if self.coordinate_propagation_budget.exhausted() {
            return Some(domains.clone());
        }
        let refined =
            domains.refine_edge_candidate_arc(edge, pair, Some(self.coordinate_propagation_budget));
        refined.or_else(|| {
            self.coordinate_propagation_budget
                .exhausted()
                .then(|| domains.clone())
        })
    }

    fn degree(&self, face: usize, point: usize) -> u8 {
        self.degrees[face].get(&point).copied().unwrap_or_default()
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
            .and_then(|constraint| constraint.assignment_predecessors)
            .and_then(|predecessors| predecessors.get(edge).copied().flatten())
            .is_none_or(|predecessor| {
                self.assignment
                    .get(predecessor)
                    .is_some_and(Option::is_some)
            });
        let dependencies_ready = self
            .partial_solution_filter
            .and_then(|constraint| constraint.assignment_dependencies)
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
                let witness = {
                    self.degree_support_witnesses
                        .borrow()
                        .get(&(face, point))
                        .copied()
                };
                if let Some((supporting_edge, supporting_pair)) = witness {
                    let candidate_still_available = self.choices[supporting_edge]
                        .contains(&supporting_pair)
                        || coordinate_domains
                            .filter(|_| self.choices[supporting_edge].is_empty())
                            .is_some_and(|domains| {
                                domains.supports_edge_candidate(supporting_edge, supporting_pair)
                            });
                    if !self.budget.charge() {
                        return false;
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
                    self.degree_support_witnesses
                        .borrow_mut()
                        .remove(&(face, point));
                }
                let indexed_edges = self
                    .point_support_edges
                    .as_ref()
                    .and_then(|by_face| by_face.get(face))
                    .and_then(|by_point| by_point.get(&point));
                let supporting_edges =
                    indexed_edges.map_or(self.face_edges[face].as_slice(), Vec::as_slice);
                for &supporting_edge in supporting_edges {
                    if !self.budget.charge() {
                        return false;
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
                        let mut witness = None;
                        if domains
                            .any_implicit_edge_candidate_with_point(
                                supporting_edge,
                                point,
                                Some(self.budget),
                                |pair| {
                                    if fits(pair) {
                                        witness = Some(pair);
                                        true
                                    } else {
                                        false
                                    }
                                },
                            )
                            .unwrap_or(false)
                        {
                            self.degree_support_witnesses.borrow_mut().insert(
                                (face, point),
                                (
                                    supporting_edge,
                                    witness
                                        .expect("successful implicit search retains its witness"),
                                ),
                            );
                            return true;
                        }
                        continue;
                    }
                    for supporting_pair in self.candidate_pairs(supporting_edge, Some(point), None)
                    {
                        if !self.budget.charge() {
                            return false;
                        }
                        if supporting_pair.contains(&point) && fits(supporting_pair) {
                            self.degree_support_witnesses
                                .borrow_mut()
                                .insert((face, point), (supporting_edge, supporting_pair));
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
        if !self.budget.exhausted() {
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
    pub(crate) fn candidate_fits(&self, edge: usize, pair: [usize; 2]) -> bool {
        self.candidate_fits_in(edge, pair, self.coordinate_domains)
    }

    fn candidate_fits_in(
        &self,
        edge: usize,
        pair: [usize; 2],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> bool {
        if !self.degree_candidate_fits(edge, pair)
            || !self.degree_support_preserved(edge, pair, coordinate_domains)
        {
            return false;
        }
        let Some(mesh_assignments) = self.mesh_assignments else {
            return true;
        };
        let mut faces = self.edge_faces[edge].to_vec();
        faces.sort_unstable();
        faces.dedup();
        let viable = faces.into_iter().all(|face| {
            mesh_assignments
                .get(face)
                .is_some_and(|domain| match domain {
                    MeshFaceBoundaryDomain::Ordered(assignments) => {
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
                                        return Some(MeshEndpointCandidates::Selected(selected));
                                    }
                                    self.choices
                                        .get(candidate_edge)
                                        .filter(|candidates| !candidates.is_empty())
                                        .map(|candidates| {
                                            MeshEndpointCandidates::Explicit(candidates.as_slice())
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
                    }
                    _ => {
                        compact_boundary_domain_viable(domain, &self.assignment, Some((edge, pair)))
                    }
                })
        });
        viable
    }

    fn constraint_options(
        &self,
        face: usize,
        point: usize,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        limit: usize,
        viability: &mut HashMap<MeshEndpointPair, bool>,
    ) -> IncidenceConstraintOptions {
        let mut any_viable = false;
        let mut options = HashSet::new();
        for edge in self.face_edges[face]
            .iter()
            .copied()
            .filter(|&edge| self.active[edge] && self.assignment[edge].is_none())
        {
            for pair in self.candidate_pairs(edge, Some(point), coordinate_domains) {
                let viable = viability.get(&(edge, pair)).copied().unwrap_or_else(|| {
                    let viable = self.candidate_fits_in(edge, pair, coordinate_domains)
                        && coordinate_domains
                            .is_none_or(|domains| domains.supports_edge_candidate(edge, pair));
                    viability.insert((edge, pair), viable);
                    viable
                });
                if !viable {
                    continue;
                }
                any_viable = true;
                if !self.branch_edge_ready(edge) {
                    continue;
                }
                options.insert((edge, pair));
                if options.len() == limit {
                    return IncidenceConstraintOptions::AtLeastLimit;
                }
            }
        }
        if !any_viable {
            return IncidenceConstraintOptions::Unsupported;
        }
        if options.is_empty() {
            return IncidenceConstraintOptions::Deferred;
        }
        let mut options = options.into_iter().collect::<Vec<_>>();
        options.sort_unstable();
        IncidenceConstraintOptions::Exact(options)
    }

    fn narrowest_edge_branch(
        &self,
        edges: impl IntoIterator<Item = usize>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> IncidenceBranch {
        let viable = |edge, pair| {
            self.candidate_fits_in(edge, pair, coordinate_domains)
                && coordinate_domains
                    .is_none_or(|domains| domains.supports_edge_candidate(edge, pair))
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
                if viable(edge, pair) {
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
        match best {
            Some((_, _, Some(options))) => IncidenceBranch::Options(options.into_iter()),
            Some((edge, _, None)) => coordinate_domains
                .and_then(|domains| domains.implicit_edge_candidates(edge, None))
                .map_or_else(
                    || IncidenceBranch::Options(Vec::new().into_iter()),
                    |candidates| IncidenceBranch::Implicit { edge, candidates },
                ),
            None => IncidenceBranch::Options(Vec::new().into_iter()),
        }
    }

    fn branch(
        &self,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Option<IncidenceBranch> {
        let mut constrained = None::<Vec<(usize, [usize; 2])>>;
        let mut viability = HashMap::new();
        for &(face, point) in &self.constraints {
            if self.degree(face, point) != 1 {
                continue;
            }
            let limit = constrained.as_ref().map_or(usize::MAX, Vec::len);
            let options =
                self.constraint_options(face, point, coordinate_domains, limit, &mut viability);
            match options {
                IncidenceConstraintOptions::Unsupported => return None,
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
            return constrained
                .map(Vec::into_iter)
                .map(IncidenceBranch::Options);
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
                return Some(self.narrowest_edge_branch(edges, coordinate_domains));
            }
        }
        let edges = self
            .edges
            .iter()
            .copied()
            .filter(|&edge| self.assignment[edge].is_none() && self.branch_edge_ready(edge))
            .collect::<Vec<_>>();
        Some(self.narrowest_edge_branch(edges, coordinate_domains))
    }

    #[cfg(test)]
    pub(crate) fn branch_options(
        &self,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Option<Vec<(usize, [usize; 2])>> {
        Some(self.branch(coordinate_domains)?.collect())
    }

    pub(crate) fn adjust(&mut self, edge: usize, pair: [usize; 2], increase: bool) {
        let faces = self.edge_faces[edge];
        for (rank, face) in faces.into_iter().enumerate() {
            if rank > 0 && face == faces[0] {
                continue;
            }
            for point in pair {
                if increase {
                    *self.degrees[face].entry(point).or_default() += 1;
                } else {
                    let degree = self.degrees[face]
                        .get_mut(&point)
                        .expect("assigned incidence degree");
                    *degree -= 1;
                    if *degree == 0 {
                        self.degrees[face].remove(&point);
                    }
                }
            }
        }
    }

    fn advance_ordered_faces(
        &mut self,
        faces: impl IntoIterator<Item = usize>,
        quotient_states: Vec<MeshQuotientGaugeState>,
    ) -> Option<Vec<MeshQuotientGaugeState>> {
        let Some(mesh_assignments) = self.mesh_assignments else {
            return Some(quotient_states);
        };
        let mut faces = faces.into_iter().collect::<Vec<_>>();
        faces.sort_unstable();
        faces.dedup();
        let viable = faces.iter().copied().all(|face| {
            mesh_assignments
                .get(face)
                .is_some_and(|domain| match domain {
                    MeshFaceBoundaryDomain::Ordered(assignments) => {
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
                    }
                    _ => compact_boundary_domain_viable(domain, &self.assignment, None),
                })
        });
        if !viable {
            return None;
        }
        if quotient_states.is_empty() {
            Some(quotient_states)
        } else {
            match advance_compact_boundary_domains(
                faces.iter().filter_map(|face| mesh_assignments.get(*face)),
                self.choices,
                &self.assignment,
                None,
                quotient_states,
                self.boundary_propagation_budget,
            ) {
                CompactBoundaryAdvanceOutcome::Complete(states) => Some(states),
                CompactBoundaryAdvanceOutcome::Rejected => None,
                CompactBoundaryAdvanceOutcome::Exhausted => {
                    self.exhausted = true;
                    None
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn ordered_faces_feasible(
        &mut self,
        faces: impl IntoIterator<Item = usize>,
    ) -> bool {
        let states = self.mesh_quotient.map_or_else(Vec::new, |quotient| {
            vec![(quotient.clone(), HashSet::new())]
        });
        self.advance_ordered_faces(faces, states).is_some()
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
    pub(crate) fn face_configuration_options(&self) -> Option<MeshFaceEndpointConfigurations> {
        self.face_configuration_options_for(&self.component_faces())
    }

    fn face_configuration_options_for(
        &self,
        component_faces: &[usize],
    ) -> Option<MeshFaceEndpointConfigurations> {
        let mesh_assignments = self.mesh_assignments?;
        let factor_state = match self.face_configuration_domains.as_ref() {
            Some(factors) => {
                match factors.assignment_state(&self.assignment, self.boundary_propagation_budget) {
                    Ok(state) => state,
                    Err(()) => return Some(Vec::new()),
                }
            }
            None => None,
        };
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
                .zip(factor_state.as_ref())
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
                return Some(Vec::new());
            }
            if projected.iter().all(Vec::is_empty) {
                continue;
            }
            let forced = projected.len() == 1;
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
            return None;
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
                &mut configuration_domains,
                self.boundary_propagation_budget,
            );
            for (domain, configurations) in domains.iter_mut().zip(configuration_domains) {
                domain.configurations = configurations;
            }
            if !viable {
                return Some(Vec::new());
            }
        }
        domains
            .into_iter()
            .min_by_key(|domain| (domain.configurations.len(), domain.width, domain.face))
            .map(|domain| domain.configurations)
    }

    fn search_face_configurations(
        &mut self,
        mut options: MeshFaceEndpointConfigurations,
        quotient_states: &[MeshQuotientGaugeState],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        component_faces: &[usize],
    ) {
        if options.len() == 1 {
            let Some(option) = options.pop() else {
                return;
            };
            self.search_forced_face_configurations(
                option,
                quotient_states,
                coordinate_domains,
                component_faces,
            );
            return;
        }
        for option in options {
            if let Some(applied) = self.apply_face_configuration(option, coordinate_domains) {
                if let Some(next_states) =
                    self.advance_ordered_faces(applied.affected_faces, quotient_states.to_vec())
                {
                    self.search_with_quotient(
                        &next_states,
                        applied.coordinate_domains.as_ref(),
                        component_faces,
                    );
                }
                self.rollback_face_configuration(applied.assigned);
                if let Some(factors) = &mut self.face_configuration_domains {
                    factors.restore(applied.factor_checkpoint);
                }
            }
            if self.exhausted || self.stopped {
                return;
            }
        }
    }

    fn apply_face_configuration(
        &mut self,
        option: Vec<(usize, [usize; 2])>,
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
    ) -> Option<AppliedFaceConfiguration> {
        let mut assigned = Vec::new();
        let mut affected_faces = Vec::new();
        let mut next_coordinate_domains = coordinate_domains.cloned();
        for (edge, pair) in option {
            if !self.active[edge] || self.assignment[edge].is_some() {
                continue;
            }
            if !self.degree_candidate_fits(edge, pair) {
                self.rollback_face_configuration(assigned);
                return None;
            }
            if let Some(domains) = next_coordinate_domains.take() {
                let Some(refined) = self.refine_coordinate_domains(&domains, edge, pair) else {
                    self.rollback_face_configuration(assigned);
                    return None;
                };
                next_coordinate_domains = Some(refined);
            }
            self.adjust(edge, pair, true);
            self.assignment[edge] = Some(pair);
            assigned.push((edge, pair));
            affected_faces.extend(self.edge_faces[edge]);
        }
        if assigned.is_empty()
            || self
                .partial_solution_filter
                .is_some_and(|constraint| !(constraint.valid)(&self.assignment))
        {
            self.rollback_face_configuration(assigned);
            return None;
        }
        affected_faces.sort_unstable();
        affected_faces.dedup();
        if !self.degree_frontiers_supported(&affected_faces, None, next_coordinate_domains.as_ref())
        {
            if self.budget.exhausted() {
                self.exhausted = true;
            }
            self.rollback_face_configuration(assigned);
            return None;
        }
        let factor_checkpoint = match &mut self.face_configuration_domains {
            Some(factors) => {
                match factors.refine_edges(&assigned, self.boundary_propagation_budget) {
                    Ok(checkpoint) => checkpoint,
                    Err(()) => {
                        self.rollback_face_configuration(assigned);
                        return None;
                    }
                }
            }
            None => None,
        };
        Some(AppliedFaceConfiguration {
            assigned,
            affected_faces,
            coordinate_domains: next_coordinate_domains,
            factor_checkpoint,
        })
    }

    fn rollback_face_configuration(&mut self, assigned: Vec<(usize, [usize; 2])>) {
        for (edge, pair) in assigned.into_iter().rev() {
            self.assignment[edge] = None;
            self.adjust(edge, pair, false);
        }
    }

    fn search_forced_face_configurations(
        &mut self,
        mut option: Vec<(usize, [usize; 2])>,
        quotient_states: &[MeshQuotientGaugeState],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        component_faces: &[usize],
    ) {
        let mut assigned = Vec::new();
        let mut factor_checkpoint = None;
        let mut states = quotient_states.to_vec();
        let mut domains = coordinate_domains.cloned();
        while let Some(applied) = self.apply_face_configuration(option, domains.as_ref()) {
            let applied_factor_checkpoint = applied.factor_checkpoint;
            let Some(next_states) = self.advance_ordered_faces(applied.affected_faces, states)
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
            let face_options = self.face_configuration_options_for(component_faces);
            if self.budget.exhausted() {
                self.exhausted = true;
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
                    );
                    break;
                }
                None => {
                    self.search_edge_state(&states, domains.as_ref(), component_faces);
                    break;
                }
            }
            if self.exhausted || self.stopped {
                break;
            }
        }
        self.rollback_face_configuration(assigned);
        if let Some(factors) = &mut self.face_configuration_domains {
            factors.restore(factor_checkpoint);
        }
    }

    pub(crate) fn search(&mut self) {
        let quotient_states = self.mesh_quotient.map_or_else(Vec::new, |quotient| {
            vec![(quotient.clone(), HashSet::new())]
        });
        let component_faces = self.component_faces();
        self.search_with_quotient(&quotient_states, self.coordinate_domains, &component_faces);
        self.exhausted |= self.budget.exhausted();
    }

    fn search_with_quotient(
        &mut self,
        quotient_states: &[MeshQuotientGaugeState],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        component_faces: &[usize],
    ) {
        if self.exhausted || self.stopped {
            return;
        }
        if !self.budget.charge() {
            self.exhausted = true;
            return;
        }
        let state = self
            .edges
            .iter()
            .map(|&edge| self.assignment[edge])
            .collect::<Vec<_>>();
        if self.dead_states.contains(&state) {
            return;
        }
        let solutions_before = self.solutions.len();
        self.search_state(quotient_states, coordinate_domains, component_faces);
        if !self.exhausted && self.solutions.len() == solutions_before {
            self.dead_states.insert(state);
        }
    }

    fn search_state(
        &mut self,
        quotient_states: &[MeshQuotientGaugeState],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        component_faces: &[usize],
    ) {
        const MAX_SOLUTIONS: usize = 256;
        if self.exhausted || self.stopped {
            return;
        }
        if self.solution_visitor.is_none() && self.solutions.len() >= MAX_SOLUTIONS {
            self.exhausted = true;
            return;
        }
        let face_options = self.face_configuration_options_for(component_faces);
        if self.budget.exhausted() {
            self.exhausted = true;
            return;
        }
        if let Some(options) = face_options {
            if !options.is_empty() {
                self.search_face_configurations(
                    options,
                    quotient_states,
                    coordinate_domains,
                    component_faces,
                );
            }
            return;
        }
        self.search_edge_state(quotient_states, coordinate_domains, component_faces);
    }

    fn search_edge_state(
        &mut self,
        quotient_states: &[MeshQuotientGaugeState],
        coordinate_domains: Option<&MeshCoordinateRootDomains>,
        component_faces: &[usize],
    ) {
        let branch = self.branch(coordinate_domains);
        if self.budget.exhausted() {
            self.exhausted = true;
            return;
        }
        let Some(mut options) = branch else {
            return;
        };
        let Some(first_option) = options.next() else {
            if self
                .edges
                .iter()
                .any(|&edge| self.assignment[edge].is_none())
                || self
                    .constraints
                    .iter()
                    .any(|&(face, point)| self.degree(face, point) == 1)
            {
                return;
            }
            let solution = self
                .edges
                .iter()
                .map(|&edge| Some((edge, self.assignment[edge]?)))
                .collect::<Option<Vec<_>>>()
                .expect("every component edge is assigned");
            if self
                .solution_filter
                .is_some_and(|filter| !filter(&solution))
            {
                return;
            }
            if let Some(visitor) = self.solution_visitor.as_deref_mut() {
                if (visitor)(&solution).is_break() {
                    self.stopped = true;
                }
            } else {
                self.solutions.push(solution);
            }
            return;
        };
        for (edge, pair) in std::iter::once(first_option).chain(options) {
            if !self.budget.charge() {
                self.exhausted = true;
                return;
            }
            if self.assignment[edge].is_some() {
                continue;
            }
            if !self.candidate_fits_in(edge, pair, coordinate_domains) {
                if self.budget.exhausted() {
                    self.exhausted = true;
                    return;
                }
                continue;
            }
            let next_coordinate_domains = if let Some(domains) = coordinate_domains {
                let Some(refined) = self.refine_coordinate_domains(domains, edge, pair) else {
                    continue;
                };
                Some(refined)
            } else {
                None
            };
            self.adjust(edge, pair, true);
            self.assignment[edge] = Some(pair);
            let factor_checkpoint = match &mut self.face_configuration_domains {
                Some(factors) => {
                    match factors.refine_edges(&[(edge, pair)], self.boundary_propagation_budget) {
                        Ok(checkpoint) => checkpoint,
                        Err(()) => {
                            self.assignment[edge] = None;
                            self.adjust(edge, pair, false);
                            continue;
                        }
                    }
                }
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
                    self.advance_ordered_faces(faces, quotient_states.to_vec())
                {
                    self.search_with_quotient(
                        &next_states,
                        next_coordinate_domains.as_ref(),
                        component_faces,
                    );
                }
            }
            self.assignment[edge] = None;
            self.adjust(edge, pair, false);
            if let Some(factors) = &mut self.face_configuration_domains {
                factors.restore(factor_checkpoint);
            }
            if self.exhausted || self.stopped {
                return;
            }
        }
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
            incidence
                .iter()
                .map(|(edge, _)| MeshBoundaryEdgeCandidate {
                    edge: *edge,
                    start: 0,
                    end: 0,
                    reversed: None,
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
            let exact = mesh
                .exact_uses
                .iter()
                .map(|(use_, _)| (use_.edge, *use_))
                .collect::<HashMap<_, _>>();
            return Some(
                actual
                    .into_iter()
                    .map(|edge| {
                        exact
                            .get(&edge)
                            .copied()
                            .unwrap_or(MeshBoundaryEdgeCandidate {
                                edge,
                                start: 0,
                                end: 0,
                                reversed: None,
                            })
                    })
                    .collect(),
            );
        }
    }
    None
}

pub(crate) fn deferred_boundary_cycle_matches(
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
        let previous = matched_mesh[incidence];
        if previous.is_none()
            || augment_cycle_matching(
                previous.expect("occupied incidence match"),
                compatible,
                seen,
                matched_mesh,
            )
        {
            matched_mesh[incidence] = Some(mesh);
            return true;
        }
    }
    false
}

pub(crate) fn deferred_boundary_assignment(
    domain: &MeshDeferredFaceBoundary,
    edge_points: &[[usize; 2]],
) -> Option<MeshFaceBoundaryAssignment> {
    let mut incident = domain.missing_edges.clone();
    incident.extend(
        domain
            .cycles
            .iter()
            .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
    );
    incident.sort_unstable();
    incident.dedup();
    let incidence = incidence_cycles(&incident, edge_points)?;
    if incidence.len() != domain.cycles.len() {
        return None;
    }
    let missing = domain.missing_edges.iter().copied().collect::<HashSet<_>>();
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
    let boolean_compatible = compatible
        .iter()
        .map(|cycles| cycles.iter().map(Option::is_some).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut matched_mesh = alloc_filled(incidence.len(), None, "catia_deferred_match").ok()?;
    for mesh in 0..domain.cycles.len() {
        let mut visited = alloc_filled(incidence.len(), false, "catia_deferred_visit").ok()?;
        if !augment_cycle_matching(mesh, &boolean_compatible, &mut visited, &mut matched_mesh) {
            return None;
        }
    }
    let mut boundaries =
        alloc_filled(domain.cycles.len(), None, "catia_deferred_boundaries").ok()?;
    for (incidence, mesh) in matched_mesh.into_iter().enumerate() {
        let mesh = mesh?;
        boundaries[mesh].clone_from(&compatible[mesh][incidence]);
    }
    Some(MeshFaceBoundaryAssignment {
        boundaries: boundaries.into_iter().collect::<Option<Vec<_>>>()?,
    })
}

pub(crate) fn deferred_boundary_closes(
    domain: &MeshDeferredFaceBoundary,
    edge_points: &[[usize; 2]],
) -> bool {
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
        return false;
    };
    if incidence.len() != domain.cycles.len() {
        return false;
    }
    let missing = domain.missing_edges.iter().copied().collect::<HashSet<_>>();
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
    let Ok(mut matched_mesh) = alloc_filled(incidence.len(), None, "catia_deferred_close_match")
    else {
        return false;
    };
    (0..domain.cycles.len()).all(|mesh| {
        let Ok(mut visited) = alloc_filled(incidence.len(), false, "catia_deferred_close_visit")
        else {
            return false;
        };
        augment_cycle_matching(mesh, &compatible, &mut visited, &mut matched_mesh)
    })
}

fn boundary_domains_close(
    domains: Option<&[MeshFaceBoundaryDomain]>,
    edge_points: &[[usize; 2]],
) -> bool {
    domains.is_none_or(|domains| {
        domains.iter().all(|domain| match domain {
            MeshFaceBoundaryDomain::Ordered(_) => true,
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                incidence_cycles(edges, edge_points).is_some_and(|cycles| cycles.len() == 1)
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                deferred_boundary_closes(domain, edge_points)
            }
        })
    })
}

fn component_incidence_faces_viable(
    faces: &HashSet<usize>,
    assignment: &[Option<[usize; 2]>],
    choices: &[Vec<[usize; 2]>],
    face_edges: &[Vec<usize>],
    domains: Option<&[MeshFaceBoundaryDomain]>,
    point_count: usize,
) -> bool {
    faces.iter().copied().all(|face| {
        if domains.is_none() {
            let mut degrees = HashMap::<usize, u8>::new();
            for &edge in &face_edges[face] {
                let Some(pair) = assignment[edge] else {
                    continue;
                };
                for point in pair {
                    if point >= point_count {
                        return false;
                    }
                    let degree = degrees.entry(point).or_default();
                    let Some(next) = degree.checked_add(1) else {
                        return false;
                    };
                    *degree = next;
                }
            }
            return degrees.into_iter().all(|(point, degree)| {
                degree <= 2
                    && (degree != 1
                        || face_edges[face].iter().copied().any(|edge| {
                            assignment[edge].is_none()
                                && choices[edge].iter().any(|pair| pair.contains(&point))
                        }))
            });
        }
        let Ok(mut points) = alloc_filled(
            assignment.len(),
            [0; 2],
            "catia component incidence edge points",
        ) else {
            return false;
        };
        for &edge in &face_edges[face] {
            let Some(pair) = assignment[edge] else {
                return false;
            };
            points[edge] = pair;
        }
        if incidence_cycles(&face_edges[face], &points).is_none() {
            return false;
        }
        let Some(domain) = domains.and_then(|domains| domains.get(face)) else {
            return true;
        };
        match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                assignments.iter().any(|boundary_assignment| {
                    labeled_assignment_endpoint_cycles_viable(boundary_assignment, assignment, None)
                })
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                incidence_cycles(edges, &points).is_some_and(|cycles| cycles.len() == 1)
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                deferred_boundary_closes(domain, &points)
            }
        }
    })
}

pub(crate) fn partial_face_orientability_viable(
    assignment: &[Option<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_edges: &[Vec<usize>],
    budget: &WorkBudget<'_>,
) -> bool {
    if !edge_faces.iter().any(|faces| faces[0] != faces[1]) {
        return true;
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
            return false;
        }
        let mut edges_at_point = HashMap::<usize, Vec<usize>>::new();
        let mut degrees = HashMap::<usize, u8>::new();
        for &edge in &selected {
            for point in edge_points[edge] {
                edges_at_point.entry(point).or_default().push(edge);
                let degree = degrees.entry(point).or_default();
                *degree = match degree.checked_add(1) {
                    Some(degree) => degree,
                    None => return false,
                };
            }
        }
        if degrees.values().any(|degree| *degree > 2) {
            return false;
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
                let Some([cycle]) = incidence_cycles(&component, &edge_points)
                    .and_then(|cycles| <[Vec<(usize, bool)>; 1]>::try_from(cycles).ok())
                else {
                    return false;
                };
                cycle
            } else {
                let mut endpoints = points
                    .iter()
                    .copied()
                    .filter(|point| degrees[point] == 1)
                    .collect::<Vec<_>>();
                endpoints.sort_unstable();
                let [start, end] = endpoints.as_slice() else {
                    return false;
                };
                if points
                    .iter()
                    .any(|point| !endpoints.contains(point) && degrees[point] != 2)
                {
                    return false;
                }
                let mut remaining = component.iter().copied().collect::<HashSet<_>>();
                let mut point = *start;
                let mut trail = Vec::with_capacity(component.len());
                while let Some(&edge) = edges_at_point[&point]
                    .iter()
                    .find(|edge| remaining.contains(edge))
                {
                    if !remaining.remove(&edge) {
                        return false;
                    }
                    let pair = edge_points[edge];
                    let reversed = pair[1] == point;
                    if !reversed && pair[0] != point {
                        return false;
                    }
                    point = pair[usize::from(!reversed)];
                    trail.push((edge, reversed));
                }
                if point != *end || !remaining.is_empty() {
                    return false;
                }
                trail
            };
            let boundary = boundary_count;
            boundary_count = match boundary_count.checked_add(1) {
                Some(count) => count,
                None => return false,
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
        return false;
    }
    solve_boundary_orientation_constraints(boundary_count, &edge_uses, false).is_some()
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn component_incidence_pair_solutions<F>(
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
) -> Option<Vec<Vec<[usize; 2]>>>
where
    F: Fn(&[[usize; 2]]) -> bool,
{
    component_incidence_pair_solution_outcome(
        choices,
        edge_faces,
        face_count,
        point_count,
        mesh_assignments,
        mesh_quotient,
        partial_solution_valid,
        solution_valid,
    )
    .into_option()
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn component_incidence_pair_solution_outcome<F>(
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
) -> IncidenceSolve<Vec<Vec<[usize; 2]>>>
where
    F: Fn(&[[usize; 2]]) -> bool,
{
    const MAX_PAIR_SOLUTIONS: usize = 256;
    let mut solutions = Vec::new();
    let mut result_limit_exhausted = false;
    let outcome = visit_component_incidence_pair_solutions(
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
                ControlFlow::Break(())
            } else {
                solutions.push(pairs.to_vec());
                ControlFlow::Continue(())
            }
        },
    );
    match outcome {
        IncidenceSolve::Solved(_) if result_limit_exhausted => IncidenceSolve::Exhausted,
        IncidenceSolve::Solved(_) => IncidenceSolve::Solved(solutions),
        IncidenceSolve::Rejected(rejection) => IncidenceSolve::Rejected(rejection),
        IncidenceSolve::Ambiguous => IncidenceSolve::Ambiguous,
        IncidenceSolve::Exhausted => IncidenceSolve::Exhausted,
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn visit_component_incidence_pair_solutions<F, V>(
    choices: &[Vec<[usize; 2]>],
    edge_faces: &[[usize; 2]],
    face_count: usize,
    point_count: usize,
    mesh_assignments: Option<&[MeshFaceBoundaryDomain]>,
    mesh_quotient: Option<&MeshQuotient>,
    partial_solution_valid: Option<MeshPartialEndpointConstraint<'_>>,
    solution_valid: &F,
    visitor: &mut V,
) -> IncidenceSolve<usize>
where
    F: Fn(&[[usize; 2]]) -> bool,
    V: FnMut(&[[usize; 2]]) -> ControlFlow<()>,
{
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    visit_component_incidence_pair_solutions_with_coordinate_root_policy(
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
) -> IncidenceSolve<usize>
where
    F: Fn(&[[usize; 2]]) -> bool,
    V: FnMut(&[[usize; 2]]) -> ControlFlow<()>,
{
    #[allow(clippy::too_many_arguments)]
    fn solve_component_domain(
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
    ) -> bool {
        let Ok(mut active) = alloc_filled(choices.len(), false, "catia incidence active edges")
        else {
            return false;
        };
        let mut constraints = HashSet::<(usize, usize)>::new();
        let Ok(mut point_support_edges) = alloc_filled(
            face_edges.len(),
            HashMap::<usize, Vec<usize>>::new(),
            "catia incidence point support edges",
        ) else {
            return false;
        };
        let mut component_faces = HashSet::new();
        for &edge in component {
            active[edge] = true;
            let faces = edge_faces[edge];
            for (rank, face) in faces.into_iter().enumerate() {
                if rank > 0 && face == faces[0] {
                    continue;
                }
                component_faces.insert(face);
                let mut points = coordinate_domains
                    .filter(|_| choices[edge].is_empty())
                    .and_then(|domains| domains.edge_candidate_points(edge))
                    .unwrap_or_else(|| choices[edge].iter().flatten().copied().collect());
                points.sort_unstable();
                points.dedup();
                for point in points {
                    constraints.insert((face, point));
                    point_support_edges[face]
                        .entry(point)
                        .or_default()
                        .push(edge);
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
        let face_configuration_domains =
            prepare_face_configuration_domains(mesh_assignments, choices, assignment, &active);
        let filter = |solution: &[MeshEndpointPair]| {
            let mut completed = assignment.to_vec();
            for &(edge, pair) in solution {
                completed[edge] = Some(pair);
            }
            let locally_closed = component_incidence_faces_viable(
                &component_faces,
                &completed,
                choices,
                face_edges,
                mesh_assignments,
                point_count,
            );
            let orientable = locally_closed
                && (orientation_budget.exhausted()
                    || partial_face_orientability_viable(
                        &completed,
                        edge_faces,
                        face_edges,
                        orientation_budget,
                    )
                    || orientation_budget.exhausted());
            orientable
                && partial_solution_valid.is_none_or(|constraint| (constraint.valid)(&completed))
        };
        let solution_filter = Some(&filter as &dyn Fn(&[MeshEndpointPair]) -> bool);
        let mut search = IncidenceComponentSearch {
            choices,
            explicit_point_supports: Some(explicit_point_supports),
            point_support_edges: Some(point_support_edges),
            degree_support_witnesses: RefCell::new(HashMap::new()),
            edge_faces,
            face_edges,
            mesh_assignments,
            face_configuration_domains,
            mesh_quotient: None,
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
            coordinate_propagation_budget,
            boundary_propagation_budget,
            exhausted: false,
            stopped: false,
        };
        search.search();
        search.exhausted
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_components<F, V>(
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
    ) -> Result<ControlFlow<()>, ()>
    where
        F: Fn(&[[usize; 2]]) -> bool,
        V: FnMut(&[[usize; 2]]) -> ControlFlow<()>,
    {
        let Some(component) = components.get(component_index) else {
            let pairs = assignment
                .iter()
                .copied()
                .collect::<Option<Vec<_>>>()
                .ok_or(())?;
            if !boundary_domains_close(mesh_assignments, &pairs) || !solution_valid(&pairs) {
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
                    if !quotient.point_assignment_exists(point_count, &singleton, Some(budget)) {
                        if budget.exhausted() {
                            return Err(());
                        }
                        return Ok(ControlFlow::Continue(()));
                    }
                    *visited = visited.checked_add(1).ok_or(())?;
                    return Ok(visitor(&pairs));
                };
                let Some(closure_limit) =
                    quotient.coordinate_domain_preparation_limit(point_count, &singleton)
                else {
                    return Ok(ControlFlow::Continue(()));
                };
                let closure_budget = session_budget.session_child_slice(closure_limit);
                let outcome = quotient.coordinate_root_closure_outcome_for_incidence(
                    point_count,
                    &singleton,
                    edge_faces,
                    domains.len(),
                    domains,
                    Some(&closure_budget),
                );
                match outcome {
                    CoordinateRootClosure::Solved(_) => {}
                    CoordinateRootClosure::Ambiguous
                        if coordinate_root_policy == CoordinateRootPolicy::DeferToVisitor => {}
                    CoordinateRootClosure::Ambiguous => {
                        *ambiguous = true;
                        return Ok(ControlFlow::Continue(()));
                    }
                    CoordinateRootClosure::Exhausted => return Err(()),
                    CoordinateRootClosure::Rejected => return Ok(ControlFlow::Continue(())),
                }
            }
            *visited = visited.checked_add(1).ok_or(())?;
            return Ok(visitor(&pairs));
        };

        let base_assignment = assignment.to_vec();
        let base_degrees = degrees.to_vec();
        let mut downstream_control = Ok(ControlFlow::Continue(()));
        let mut visit_solution = |solution: &[MeshEndpointPair]| {
            if !budget.charge() {
                downstream_control = Err(());
                return ControlFlow::Break(());
            }
            for &(edge, pair) in solution {
                assignment[edge] = Some(pair);
                for (rank, face) in edge_faces[edge].into_iter().enumerate() {
                    if rank > 0 && face == edge_faces[edge][0] {
                        continue;
                    }
                    for point in pair {
                        *degrees[face].entry(point).or_default() += 1;
                    }
                }
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
            let refined_domains =
                coordinate_domains
                    .zip(candidates.as_ref())
                    .and_then(|(domains, candidates)| {
                        // Refinement only narrows later component searches. A complete
                        // assignment is checked against the quotient before visitation.
                        (!coordinate_propagation_budget.exhausted())
                            .then(|| {
                                domains.refine_candidates(
                                    candidates,
                                    Some(coordinate_propagation_budget),
                                )
                            })
                            .flatten()
                    });
            let propagation_skipped = coordinate_propagation_budget.exhausted();
            let feasible =
                coordinate_domains.is_none() || refined_domains.is_some() || propagation_skipped;
            let control = if feasible {
                visit_components(
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
            };
            for &(edge, pair) in solution.iter().rev() {
                assignment[edge] = None;
                for (rank, face) in edge_faces[edge].into_iter().enumerate() {
                    if rank > 0 && face == edge_faces[edge][0] {
                        continue;
                    }
                    for point in pair {
                        let degree = degrees[face]
                            .get_mut(&point)
                            .expect("assigned incidence degree");
                        *degree -= 1;
                        if *degree == 0 {
                            degrees[face].remove(&point);
                        }
                    }
                }
            }
            match control {
                Ok(ControlFlow::Continue(())) => ControlFlow::Continue(()),
                terminal => {
                    downstream_control = terminal;
                    ControlFlow::Break(())
                }
            }
        };
        let narrowed_choices =
            coordinate_domains.map_or(choices, MeshCoordinateRootDomains::edge_candidates);
        let component_exhausted = solve_component_domain(
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
        );
        match downstream_control {
            Ok(ControlFlow::Continue(())) if component_exhausted => Err(()),
            control => control,
        }
    }

    let mut exhausted = false;
    let mut ambiguous = false;
    let mut visited = 0usize;
    let mut rejection = IncidenceRejection::InputShape;
    let result = (|| {
        if partial_solution_valid.is_some_and(|constraint| {
            constraint.active_edges.len() != choices.len()
                || constraint.coupled_edges.len() != choices.len()
                || constraint
                    .assignment_predecessors
                    .is_some_and(|predecessors| predecessors.len() != choices.len())
        }) {
            return None;
        }
        let mut coordinate_domains = if choices.iter().any(|candidates| candidates.len() != 1) {
            if let Some(quotient) = mesh_quotient {
                let mut quotient = quotient.clone();
                let preparation_limit =
                    quotient.coordinate_domain_preparation_limit(point_count, choices)?;
                let preparation_budget = session_budget.session_child_slice(preparation_limit);
                let Some(domains) = quotient.prepare_coordinate_root_domains(
                    point_count,
                    choices,
                    Some(&preparation_budget),
                ) else {
                    exhausted = preparation_budget.exhausted();
                    rejection = IncidenceRejection::ComponentDomain;
                    return None;
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
            let implicit_support_viable = coordinate_domains.as_ref().is_none_or(|coordinate| {
                prune_implicit_ordered_face_endpoint_support(
                    domains,
                    &mut narrowed_choices,
                    coordinate,
                    &implicit_support_budget,
                )
            });
            if !implicit_support_viable {
                rejection = IncidenceRejection::ChoicePruning;
                return None;
            }
            if implicit_support_budget.exhausted() {
                narrowed_choices.clone_from(&base_choices);
            }
            let implicit_choices = narrowed_choices.clone();
            let face_support_budget =
                session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
            if !prune_ordered_face_endpoint_support(
                domains,
                &mut narrowed_choices,
                &face_support_budget,
            ) {
                rejection = IncidenceRejection::ChoicePruning;
                return None;
            }
            if face_support_budget.exhausted() {
                narrowed_choices = implicit_choices;
            } else if let Some(domains) = coordinate_domains.take() {
                let refinement_budget =
                    session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
                match domains.refine_candidates(&narrowed_choices, Some(&refinement_budget)) {
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
                        return None;
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
        order_incidence_components_by_constraints(
            &mut components,
            choices,
            partial_solution_valid.and_then(|constraint| constraint.assignment_predecessors),
            partial_solution_valid.and_then(|constraint| constraint.assignment_dependencies),
        )?;
        let mut face_edges =
            alloc_filled(face_count, Vec::new(), "catia incidence face edges").ok()?;
        for (edge, faces) in edge_faces.iter().copied().enumerate() {
            for (rank, face) in faces.into_iter().enumerate() {
                if (rank == 0 || face != faces[0]) && !face_edges[face].contains(&edge) {
                    face_edges[face].push(edge);
                }
            }
        }
        let mut fixed = alloc_filled(choices.len(), None, "catia incidence fixed edges").ok()?;
        let mut degrees = alloc_filled(
            face_count,
            BTreeMap::<usize, u8>::new(),
            "catia incidence face degrees",
        )
        .ok()?;
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
                    *degree = degree.checked_add(1)?;
                }
            }
        }
        if components.is_empty() {
            rejection = IncidenceRejection::FixedAssignment;
            let pairs = fixed.into_iter().collect::<Option<Vec<_>>>()?;
            if !boundary_domains_close(mesh_assignments, &pairs) || !solution_valid(&pairs) {
                return None;
            }
            if let Some(quotient) = mesh_quotient {
                let singleton = pairs
                    .iter()
                    .copied()
                    .map(|pair| vec![pair])
                    .collect::<Vec<_>>();
                let mut quotient = quotient.clone();
                let closure_limit =
                    quotient.coordinate_domain_preparation_limit(point_count, &singleton)?;
                let budget = session_budget.session_child_slice(closure_limit);
                let Some(domains) = mesh_assignments else {
                    if !quotient.point_assignment_exists(point_count, &singleton, Some(&budget)) {
                        exhausted = budget.exhausted();
                        return None;
                    }
                    visited = 1;
                    let _ = visitor(&pairs);
                    return Some(());
                };
                let outcome = quotient.coordinate_root_closure_outcome_for_incidence(
                    point_count,
                    &singleton,
                    edge_faces,
                    face_count,
                    domains,
                    Some(&budget),
                );
                match outcome {
                    CoordinateRootClosure::Solved(_) => {}
                    CoordinateRootClosure::Ambiguous
                        if coordinate_root_policy == CoordinateRootPolicy::DeferToVisitor => {}
                    CoordinateRootClosure::Ambiguous => {
                        ambiguous = true;
                        return Some(());
                    }
                    CoordinateRootClosure::Exhausted => {
                        exhausted = true;
                        return None;
                    }
                    CoordinateRootClosure::Rejected => return None,
                }
            }
            visited = 1;
            let _ = visitor(&pairs);
            return Some(());
        }
        let preflight_orientation_budget =
            session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        let coordinate_preflight_budget =
            session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        let boundary_preflight_budget =
            session_budget.session_child_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        for component in &components {
            let mut found = false;
            let mut accept_first = |solution: &[MeshEndpointPair]| {
                let mut completed = fixed.clone();
                for &(edge, pair) in solution {
                    completed[edge] = Some(pair);
                }
                let coordinate_feasible = coordinate_domains.as_ref().is_none_or(|domains| {
                    let candidates = completed
                        .iter()
                        .enumerate()
                        .map(|(edge, pair)| {
                            pair.map_or_else(|| choices[edge].clone(), |pair| vec![pair])
                        })
                        .collect::<Vec<_>>();
                    domains
                        .refine_candidates(&candidates, Some(&coordinate_preflight_budget))
                        .is_some()
                        || coordinate_preflight_budget.exhausted()
                });
                if !coordinate_feasible {
                    return ControlFlow::Continue(());
                }
                found = true;
                ControlFlow::Break(())
            };
            let component_exhausted = solve_component_domain(
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
                session_budget,
                &coordinate_preflight_budget,
                &boundary_preflight_budget,
                &preflight_orientation_budget,
                Some(&mut accept_first),
            );
            if !found {
                if component_exhausted {
                    exhausted = true;
                    return None;
                }
                rejection = IncidenceRejection::ComponentDomain;
                return None;
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
        if visit_components(
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
        )
        .is_err()
        {
            exhausted = true;
            return None;
        }
        Some(())
    })();
    match result {
        Some(()) if visited > 0 => IncidenceSolve::Solved(visited),
        Some(()) if ambiguous => IncidenceSolve::Ambiguous,
        Some(()) => IncidenceSolve::Rejected(rejection),
        None if exhausted => IncidenceSolve::Exhausted,
        None => IncidenceSolve::Rejected(rejection),
    }
}

pub(crate) fn reconstruct_incidence_candidates(
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_ports: Option<&[[u32; 2]]>,
    face_count: usize,
    budget: &WorkBudget<'_>,
) -> Option<StandardTopology> {
    const MAX_TOPOLOGY_ASSIGNMENTS: usize = 256;

    if edge_ports.is_some_and(|ports| ports.len() != edge_candidates.len()) {
        return None;
    }
    let quotient = match edge_ports {
        Some(ports) => Some(initial_mesh_quotient(
            edge_candidates,
            vertex_points.len(),
            ports,
        )?),
        None => None,
    };
    let mut solution_pairs: Option<Vec<[usize; 2]>> = None;
    let mut assignment_count = 0usize;
    let mut invalid = false;
    let outcome = visit_incidence_endpoint_pair_solutions(
        edge_rows,
        vertex_points,
        edge_faces,
        edge_candidates,
        face_count,
        None,
        quotient.as_ref(),
        None,
        Some(budget),
        &|_| true,
        &mut |pairs| {
            if assignment_count == MAX_TOPOLOGY_ASSIGNMENTS {
                invalid = true;
                return ControlFlow::Break(());
            }
            assignment_count += 1;
            let oriented;
            let pairs = if let Some(ports) = edge_ports {
                let Some(propagated) = propagate_edge_port_points(
                    ports,
                    &pairs.iter().copied().map(Some).collect::<Vec<_>>(),
                ) else {
                    invalid = true;
                    return ControlFlow::Break(());
                };
                let Some(pairs) = propagated.into_iter().collect::<Option<Vec<_>>>() else {
                    invalid = true;
                    return ControlFlow::Break(());
                };
                oriented = pairs;
                oriented.as_slice()
            } else {
                pairs
            };
            if let Some(stored) = &solution_pairs {
                if stored.as_slice() != pairs {
                    invalid = true;
                    return ControlFlow::Break(());
                }
                return ControlFlow::Continue(());
            }
            solution_pairs = Some(pairs.to_vec());
            ControlFlow::Continue(())
        },
    );
    if invalid || !matches!(outcome, IncidenceSolve::Solved(_)) {
        return None;
    }
    reconstruct_incidence(
        edge_rows.to_vec(),
        vertex_points.to_vec(),
        edge_faces,
        &solution_pairs?,
        face_count,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn visit_incidence_endpoint_pair_solutions<F, V>(
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
) -> IncidenceSolve<usize>
where
    F: Fn(&[[usize; 2]]) -> bool,
    V: FnMut(&[[usize; 2]]) -> ControlFlow<()>,
{
    visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy(
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
pub(crate) fn visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy<F, V>(
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
) -> IncidenceSolve<usize>
where
    F: Fn(&[[usize; 2]]) -> bool,
    V: FnMut(&[[usize; 2]]) -> ControlFlow<()>,
{
    let Some(choices) = (|| {
        let mut choices = edge_candidates.to_vec();
        for candidates in &mut choices {
            for pair in candidates.iter_mut() {
                pair.sort_unstable();
            }
            candidates.sort_unstable();
            candidates.dedup();
        }
        if choices.iter().any(Vec::is_empty) {
            if mesh_quotient.is_none()
                || choices.len() != edge_faces.len()
                || edge_faces.iter().flatten().any(|face| *face >= face_count)
                || choices
                    .iter()
                    .flatten()
                    .flatten()
                    .any(|point| *point >= vertex_points.len())
            {
                return None;
            }
        } else {
            prune_incidence_choices(&mut choices, edge_faces, face_count, vertex_points.len())?;
        }
        Some(choices)
    })() else {
        return IncidenceSolve::Rejected(IncidenceRejection::ChoicePruning);
    };
    let complete_valid = |points: &[[usize; 2]]| {
        if complete_solution_budget.is_some_and(|budget| !budget.charge_by(edge_rows.len())) {
            return true;
        }
        solution_valid(points)
            && reconstruct_incidence(
                edge_rows.to_vec(),
                vertex_points.to_vec(),
                edge_faces,
                points,
                face_count,
            )
            .is_some()
    };
    let mut budgeted_visitor = |points: &[[usize; 2]]| {
        if complete_solution_budget.is_some_and(WorkBudget::exhausted) {
            ControlFlow::Break(())
        } else {
            visitor(points)
        }
    };
    let fallback_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let session_budget = complete_solution_budget.unwrap_or(&fallback_budget);
    let outcome = visit_component_incidence_pair_solutions_with_coordinate_root_policy(
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
    );
    if complete_solution_budget.is_some_and(WorkBudget::exhausted) {
        IncidenceSolve::Exhausted
    } else {
        outcome
    }
}
