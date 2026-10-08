// SPDX-License-Identifier: Apache-2.0
//! Coordinate-root assignment and incidence-constrained closure.

use super::{
    compact_boundary_domain_viable, deferred_boundary_cycle_matches,
    distinct_domain_matching_with_budget, domain_contains, domains_disjoint,
    enforce_edge_arc_consistency, enforce_sparse_endpoint_membership, incidence_cycles,
    same_unordered_pair, BTreeMap, BTreeSet, Cell, HashMap, HashSet, MatchingEdgeConstraint,
    MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain, MeshQuotient,
    PointAssignmentOutcome, ScopedValue, UnionFind, WorkBudget,
};

use cadmpeg_core::decode::work_units;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
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
            let mut stack = Vec::new();
            ctx.push_vec(&mut stack, first, "catia coordinate component stack")?;
            let mut component = Vec::new();
            let mut vertices = BTreeSet::new();
            while let Some(edge) = ctx.next_charged(
                &mut std::iter::from_fn(|| stack.pop()),
                "catia coordinate component walk",
            )? {
                if !ctx.insert_hash_set(&mut seen_edges, edge, "catia coordinate seen edges")? {
                    continue;
                }
                ctx.push_vec(&mut component, edge, "catia coordinate component edges")?;
                let Some(&index) =
                    ctx.get_hash_map(&selected_index, &edge, "catia coordinate selected edges")?
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
            if ctx.all_by(
                &vertices,
                |point| Ok(adjacent(*point)?.len() == 2),
                "catia coordinate component points",
            )? {
                ctx.push_vec(
                    &mut closed_components,
                    component,
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
                    let incidence = incidence_cycles(ctx, component, &edge_points)?;
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
                    let mut visited = ctx.alloc_filled(
                        domain.cycles.len(),
                        false,
                        "catia_deferred_augment_visit",
                    )?;
                    if !augment(ctx, component, &compatible, &mut visited, &mut matched)? {
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
                    if {
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
                    } {
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
                            if {
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
                            } {
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
            if let Some(previous) = ctx.insert_hash_map(
                &mut root_by_point,
                point,
                root,
                "catia_coordinate_closure_assigned_point_roots",
            )? {
                let Some(merged) = quotient.merge_charged(ctx, previous, root)? else {
                    return Ok(false);
                };
                if let Some(stored) = ctx.get_mut_hash_map(
                    &mut root_by_point,
                    &point,
                    "catia_coordinate_closure_assigned_point_roots",
                )? {
                    *stored = merged;
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
}
