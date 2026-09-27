// SPDX-License-Identifier: Apache-2.0
//! Coordinate-root assignment and incidence-constrained closure.

use super::{
    compact_boundary_domain_viable, deferred_boundary_cycle_matches,
    distinct_domain_matching_with_budget, enforce_edge_arc_consistency,
    enforce_sparse_endpoint_membership, incidence_cycles, same_unordered_pair, Arc, Cell, HashMap,
    HashSet, MatchingEdgeConstraint, MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment,
    MeshFaceBoundaryDomain, MeshQuotient, PointAssignmentOutcome, UnionFind, WorkBudget,
};

use cadmpeg_core::decode::work_units;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[allow(clippy::too_many_arguments)]
fn partial_compact_assignment_viable(
    ctx: &DecodeContext<'_>,
    domain: &MeshFaceBoundaryDomain,
    local_edge_by_id: &HashMap<usize, usize>,
    edges: &[[usize; 2]],
    global_edge_count: usize,
    assigned: &[Option<usize>],
    candidate: (usize, usize),
    budget: Option<&WorkBudget<'_>>,
) -> Result<bool, CodecError> {
    fn augment(
        ctx: &DecodeContext<'_>,
        component: usize,
        compatible: &[Vec<bool>],
        seen: &mut [bool],
        matched: &mut [Option<usize>],
    ) -> Result<bool, CodecError> {
        let _depth = ctx.enter_nested("catia deferred boundary augmentation")?;
        for cycle in 0..matched.len() {
            ctx.charge_work(1, "catia deferred boundary augmentation")?;
            if !compatible[component][cycle] || seen[cycle] {
                continue;
            }
            seen[cycle] = true;
            let available = match matched[cycle] {
                Some(previous) => augment(ctx, previous, compatible, seen, matched)?,
                None => true,
            };
            if available {
                matched[cycle] = Some(component);
                return Ok(true);
            }
        }
        Ok(false)
    }

    let relevant = match domain {
        MeshFaceBoundaryDomain::Ordered(_) => return Ok(true),
        MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
            crate::resource::copy_slice(ctx, edges, "catia coordinate relevant edges")?
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            let mut edges = crate::resource::copy_slice(
                ctx,
                &domain.missing_edges,
                "catia coordinate relevant edges",
            )?;
            for edge in domain
                .cycles
                .iter()
                .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge))
            {
                crate::resource::push(ctx, &mut edges, edge, "catia coordinate relevant edges")?;
            }
            edges.sort_unstable();
            edges.dedup();
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
    let mut selected = HashMap::new();
    let mut selected_edges = Vec::new();
    let mut adjacency = HashMap::<usize, Vec<usize>>::new();
    for &edge in &relevant {
        let Some(&local) = local_edge_by_id.get(&edge) else {
            return Ok(false);
        };
        let [left, right] = edges[local];
        let [Some(left), Some(right)] = [value(left), value(right)] else {
            continue;
        };
        crate::resource::insert_map(
            ctx,
            &mut selected,
            edge,
            [left, right],
            "catia coordinate selected edges",
        )?;
        crate::resource::push(
            ctx,
            &mut selected_edges,
            edge,
            "catia coordinate selected edge list",
        )?;
        crate::resource::admit_map_entry(
            ctx,
            &mut adjacency,
            &left,
            "catia coordinate adjacency points",
        )?;
        crate::resource::push(
            ctx,
            adjacency.entry(left).or_default(),
            edge,
            "catia coordinate adjacent edges",
        )?;
        crate::resource::admit_map_entry(
            ctx,
            &mut adjacency,
            &right,
            "catia coordinate adjacency points",
        )?;
        crate::resource::push(
            ctx,
            adjacency.entry(right).or_default(),
            edge,
            "catia coordinate adjacent edges",
        )?;
    }
    let mut closed_components = Vec::new();
    let mut seen_edges = HashSet::new();
    for &first in &selected_edges {
        if seen_edges.contains(&first) {
            continue;
        }
        let mut stack = Vec::new();
        crate::resource::push(ctx, &mut stack, first, "catia coordinate component stack")?;
        let mut component = Vec::new();
        let mut vertices = HashSet::new();
        while let Some(edge) = stack.pop() {
            ctx.charge_work(1, "catia coordinate component walk")?;
            if seen_edges.contains(&edge) {
                continue;
            }
            crate::resource::insert_set(ctx, &mut seen_edges, edge, "catia coordinate seen edges")?;
            crate::resource::push(
                ctx,
                &mut component,
                edge,
                "catia coordinate component edges",
            )?;
            for point in selected[&edge] {
                crate::resource::insert_set(
                    ctx,
                    &mut vertices,
                    point,
                    "catia coordinate component points",
                )?;
                crate::resource::reserve_vec(
                    ctx,
                    &mut stack,
                    adjacency[&point].len(),
                    "catia coordinate component stack",
                )?;
                stack.extend(adjacency[&point].iter().copied());
            }
        }
        if vertices.iter().all(|point| adjacency[point].len() == 2) {
            crate::resource::push(
                ctx,
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
            for &edge in &domain.missing_edges {
                crate::resource::insert_set(
                    ctx,
                    &mut missing,
                    edge,
                    "catia deferred missing edges",
                )?;
            }
            let mut edge_points = ctx.alloc_filled(
                global_edge_count,
                [0; 2],
                "catia coordinate assignment edge points",
            )?;
            for (&edge, &points) in &selected {
                edge_points[edge] = points;
            }
            let mut compatible = ctx.alloc_filled(
                closed_components.len(),
                Vec::new(),
                "catia_deferred_compatible_rows",
            )?;
            for (row, component) in compatible.iter_mut().zip(&closed_components) {
                let incidence = incidence_cycles(ctx, component, &edge_points)?;
                let Some([incidence]) = incidence.as_deref() else {
                    return Ok(false);
                };
                *row = ctx.alloc_filled(
                    domain.cycles.len(),
                    false,
                    "catia_deferred_compatible_cycles",
                )?;
                for (slot, cycle) in row.iter_mut().zip(&domain.cycles) {
                    *slot = deferred_boundary_cycle_matches(ctx, cycle, incidence.as_slice(), &missing)?;
                }
            }
            let mut matched =
                ctx.alloc_filled(domain.cycles.len(), None, "catia_deferred_matched")?;
            for component in 0..closed_components.len() {
                let mut visited =
                    ctx.alloc_filled(domain.cycles.len(), false, "catia_deferred_augment_visit")?;
                if !augment(ctx, component, &compatible, &mut visited, &mut matched)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn close_coordinate_roots_with_incidence(
    ctx: &DecodeContext<'_>,
    quotient: &mut MeshQuotient,
    point_count: usize,
    edge_candidates: &[Vec<[usize; 2]>],
    incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
    budget: Option<&WorkBudget<'_>>,
    component_search_budget: Option<usize>,
    ambiguous: &Cell<bool>,
    exhausted: &Cell<bool>,
) -> Result<Option<HashMap<usize, usize>>, CodecError> {
    const MAX_COORDINATE_CLOSURE_STATES: usize = 256;

    struct LocalIncidence<'a> {
        edge_faces: Vec<[usize; 2]>,
        face_edges: Vec<Vec<usize>>,
        closed_faces: Vec<bool>,
        boundary_domains: &'a [MeshFaceBoundaryDomain],
    }

    fn pair_supported(candidates: &[[usize; 2]], left: usize, right: usize) -> bool {
        candidates.is_empty()
            || candidates
                .iter()
                .any(|pair| same_unordered_pair(*pair, [left, right]))
    }

    #[allow(clippy::too_many_arguments)]
    fn partial_ordered_assignment_viable(
        assignment: &MeshFaceBoundaryAssignment,
        local_edge_by_id: &HashMap<usize, usize>,
        edges: &[[usize; 2]],
        domains: &[Vec<usize>],
        assigned: &[Option<usize>],
        candidate: Option<(usize, usize)>,
        budget: Option<&WorkBudget<'_>>,
    ) -> bool {
        let directions = |use_: MeshBoundaryEdgeCandidate| match use_.reversed {
            Some(reversed) => [Some(reversed), None],
            None => [Some(false), Some(true)],
        };
        let port_root = |use_: MeshBoundaryEdgeCandidate, reversed: bool, end: bool| {
            let local = *local_edge_by_id.get(&use_.edge)?;
            Some(edges[local][usize::from(if end { !reversed } else { reversed })])
        };
        let compatible = |left: usize, right: usize, charge: bool| {
            if charge && budget.is_some_and(|budget| !budget.charge()) {
                return false;
            }
            let value = |root| {
                candidate
                    .filter(|(candidate_root, _)| *candidate_root == root)
                    .map(|(_, point)| point)
                    .or_else(|| assigned.get(root).copied().flatten())
            };
            match (value(left), value(right)) {
                (Some(left), Some(right)) => left == right,
                (Some(point), None) => domains[right].contains(&point),
                (None, Some(point)) => domains[left].contains(&point),
                (None, None) => !domains[left]
                    .iter()
                    .all(|point| !domains[right].contains(point)),
            }
        };

        assignment.boundaries.iter().all(|boundary| {
            let Some((first, tail)) = boundary.split_first() else {
                return false;
            };
            let last = tail.last().copied().unwrap_or(*first);
            directions(*first)
                .into_iter()
                .flatten()
                .any(|first_direction| {
                    let mut previous = [Some(first_direction), None];
                    for index in 1..boundary.len() {
                        let mut next = [None; 2];
                        let mut next_count = 0;
                        for direction in directions(boundary[index]).into_iter().flatten() {
                            if previous
                                .iter()
                                .copied()
                                .flatten()
                                .any(|previous_direction| {
                                    let Some(left) =
                                        port_root(boundary[index - 1], previous_direction, true)
                                    else {
                                        return false;
                                    };
                                    let Some(right) = port_root(boundary[index], direction, false)
                                    else {
                                        return false;
                                    };
                                    compatible(left, right, true)
                                })
                            {
                                next[next_count] = Some(direction);
                                next_count += 1;
                            }
                        }
                        if next_count == 0 {
                            return false;
                        }
                        previous = next;
                    }
                    previous.into_iter().flatten().any(|previous_direction| {
                        let Some(left) = port_root(last, previous_direction, true) else {
                            return false;
                        };
                        let Some(right) = port_root(*first, first_direction, false) else {
                            return false;
                        };
                        compatible(left, right, false)
                    })
                })
        })
    }

    fn complete_ordered_assignment_viable(
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
                    let mut ends = [Some(first_points[usize::from(!first_reversed)]), None];
                    for use_ in &boundary[1..] {
                        let Some(points) = edge_points.get(use_.edge).copied().flatten() else {
                            return false;
                        };
                        let mut next = [None; 2];
                        for current in ends.into_iter().flatten() {
                            for reversed in directions(*use_).into_iter().flatten() {
                                if budget.is_some_and(|budget| !budget.charge()) {
                                    return false;
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
                            return false;
                        }
                        ends = next;
                    }
                    ends.contains(&Some(first_start))
                })
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        ctx: &DecodeContext<'_>,
        domains: &[Vec<usize>],
        edges: &[[usize; 2]],
        edge_ids: &[usize],
        local_edge_by_id: &HashMap<usize, usize>,
        root_edges: &[Vec<usize>],
        edge_candidates: &[Vec<[usize; 2]>],
        incidence: Option<&LocalIncidence<'_>>,
        component_points: &HashSet<usize>,
        assigned: &mut [Option<usize>],
        point_uses: &mut [usize],
        solutions: &mut Vec<Vec<usize>>,
        states: &mut usize,
        state_limit: usize,
        exhausted: &mut bool,
        base_degrees: &mut HashMap<(usize, usize), usize>,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<(), CodecError> {
        enum CoordinateBranch {
            Search { root: usize, values: Vec<usize> },
            Complete(Vec<usize>),
        }

        struct DegreeUndo {
            entries: Vec<((usize, usize), Option<usize>)>,
        }

        fn adjust_assignment_degrees(
            ctx: &DecodeContext<'_>,
            root: usize,
            assigned: &[Option<usize>],
            edges: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            incidence: Option<&LocalIncidence<'_>>,
            degrees: &mut HashMap<(usize, usize), usize>,
        ) -> Result<DegreeUndo, CodecError> {
            let mut undo = DegreeUndo {
                entries: Vec::new(),
            };
            let Some(incidence) = incidence else {
                return Ok(undo);
            };
            let edge_faces = &incidence.edge_faces;
            for &edge in &root_edges[root] {
                let [left, right] = edges[edge];
                let [Some(left), Some(right)] = [assigned[left], assigned[right]] else {
                    continue;
                };
                let faces = edge_faces[edge];
                for (rank, face) in faces.into_iter().enumerate() {
                    if rank > 0 && face == faces[0] {
                        continue;
                    }
                    for point in [left, right] {
                        let key = (face, point);
                        let previous = degrees.get(&key).copied();
                        crate::resource::admit_map_entry(
                            ctx,
                            degrees,
                            &key,
                            "catia_coordinate_closure_degree_entries",
                        )?;
                        let degree = degrees.entry(key).or_default();
                        *degree = degree.saturating_add(1);
                        crate::resource::push(
                            ctx,
                            &mut undo.entries,
                            (key, previous),
                            "catia_coordinate_closure_degree_undo",
                        )?;
                    }
                }
            }
            Ok(undo)
        }

        fn restore_assignment_degrees(
            degrees: &mut HashMap<(usize, usize), usize>,
            undo: DegreeUndo,
        ) {
            for (key, previous) in undo.entries.into_iter().rev() {
                match previous {
                    Some(degree) => {
                        degrees.insert(key, degree);
                    }
                    None => {
                        degrees.remove(&key);
                    }
                }
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn assign(
            ctx: &DecodeContext<'_>,
            root: usize,
            point: usize,
            assigned: &mut [Option<usize>],
            edges: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            incidence: Option<&LocalIncidence<'_>>,
            degrees: &mut HashMap<(usize, usize), usize>,
            budget: Option<&WorkBudget<'_>>,
        ) -> Result<Option<DegreeUndo>, CodecError> {
            if budget.is_some_and(|budget| !budget.charge_by(root_edges[root].len())) {
                return Ok(None);
            }
            assigned[root] = Some(point);
            Ok(Some(adjust_assignment_degrees(
                ctx, root, assigned, edges, root_edges, incidence, degrees,
            )?))
        }

        fn unassign(
            root: usize,
            assigned: &mut [Option<usize>],
            degrees: &mut HashMap<(usize, usize), usize>,
            undo: DegreeUndo,
        ) {
            restore_assignment_degrees(degrees, undo);
            assigned[root] = None;
        }

        #[allow(clippy::too_many_arguments)]
        fn rollback(
            assigned: &mut [Option<usize>],
            point_uses: &mut [usize],
            propagated: Vec<(usize, usize, DegreeUndo)>,
            degrees: &mut HashMap<(usize, usize), usize>,
        ) {
            for (root, point, undo) in propagated.into_iter().rev() {
                point_uses[point] -= 1;
                unassign(root, assigned, degrees, undo);
            }
        }

        fn affected_roots(
            ctx: &DecodeContext<'_>,
            root: usize,
            root_edges: &[Vec<usize>],
            edges: &[[usize; 2]],
            incidence: Option<&LocalIncidence<'_>>,
        ) -> Result<HashSet<usize>, CodecError> {
            let mut affected = HashSet::new();
            for &edge in &root_edges[root] {
                for point in edges[edge] {
                    crate::resource::insert_set(
                        ctx,
                        &mut affected,
                        point,
                        "catia_coordinate_closure_affected_roots",
                    )?;
                }
                let Some(incidence) = incidence else {
                    continue;
                };
                let faces = incidence.edge_faces[edge];
                for (rank, face) in faces.into_iter().enumerate() {
                    if rank > 0 && face == faces[0] {
                        continue;
                    }
                    for &face_edge in &incidence.face_edges[face] {
                        for point in edges[face_edge] {
                            crate::resource::insert_set(
                                ctx,
                                &mut affected,
                                point,
                                "catia_coordinate_closure_affected_roots",
                            )?;
                        }
                    }
                }
            }
            affected.remove(&root);
            Ok(affected)
        }

        let _depth = ctx.enter_nested("catia_coordinate_closure_walk")?;
        if solutions.len() > 1 || *exhausted {
            return Ok(());
        }
        if budget.is_some_and(|budget| !budget.charge()) {
            *exhausted = true;
            return Ok(());
        }
        let viable_values = |root: usize,
                             assigned: &[Option<usize>],
                             base_degrees: &HashMap<(usize, usize), usize>,
                             work_budget: Option<&WorkBudget<'_>>|
         -> Result<Vec<usize>, CodecError> {
            domains[root]
                .iter()
                .try_fold(Vec::new(), |mut values, point| {
                    let viable = (|| -> Result<bool, CodecError> {
                        let pair_viable = root_edges[root].iter().all(|edge| {
                            let [left, right] = edges[*edge];
                            let other = if left == root { right } else { left };
                            assigned[other].is_none_or(|other_point| {
                                pair_supported(
                                    &edge_candidates[edge_ids[*edge]],
                                    *point,
                                    other_point,
                                )
                            })
                        });
                        if !pair_viable {
                            return Ok(false);
                        }
                        let Some(incidence) = incidence else {
                            return Ok(true);
                        };
                        let edge_faces = &incidence.edge_faces;
                        if work_budget.is_some_and(|budget| {
                            !budget.charge_by(work_units(root_edges[root].len()))
                        }) {
                            return Ok(false);
                        }
                        let value = |endpoint| {
                            if endpoint == root {
                                Some(*point)
                            } else {
                                assigned[endpoint]
                            }
                        };
                        let mut degrees = HashMap::new();
                        for (&key, &degree) in base_degrees {
                            crate::resource::insert_map(
                                ctx,
                                &mut degrees,
                                key,
                                degree,
                                "catia_coordinate_closure_probe_degrees",
                            )?;
                        }
                        let mut affected_faces = HashSet::new();
                        for &edge in &root_edges[root] {
                            let [left, right] = edges[edge];
                            let (Some(left), Some(right)) = (value(left), value(right)) else {
                                continue;
                            };
                            let faces = edge_faces[edge];
                            for (rank, face) in faces.into_iter().enumerate() {
                                if rank > 0 && face == faces[0] {
                                    continue;
                                }
                                crate::resource::insert_set(
                                    ctx,
                                    &mut affected_faces,
                                    face,
                                    "catia_coordinate_closure_affected_faces",
                                )?;
                                for endpoint in [left, right] {
                                    crate::resource::admit_map_entry(
                                        ctx,
                                        &mut degrees,
                                        &(face, endpoint),
                                        "catia_coordinate_closure_probe_degrees",
                                    )?;
                                    let degree = degrees.entry((face, endpoint)).or_default();
                                    *degree = degree.saturating_add(1);
                                    if *degree > 2 {
                                        return Ok(false);
                                    }
                                }
                            }
                        }
                        for (&(face, point), &degree) in &degrees {
                            if degree != 1 || !affected_faces.contains(&face) {
                                continue;
                            }
                            if work_budget.is_some_and(|budget| {
                                !budget.charge_by(work_units(incidence.face_edges[face].len()))
                            }) {
                                return Ok(false);
                            }
                            let supported =
                                incidence.face_edges[face].iter().copied().any(|edge| {
                                    let [left, right] = edges[edge];
                                    if value(left).is_some() && value(right).is_some() {
                                        return false;
                                    }
                                    let supports = |endpoint| {
                                        value(endpoint).is_some_and(|value| value == point)
                                            || (value(endpoint).is_none()
                                                && domains[endpoint].contains(&point))
                                    };
                                    supports(left) || supports(right)
                                });
                            if !supported {
                                return Ok(false);
                            }
                        }
                        let boundaries_viable = incidence
                            .boundary_domains
                            .iter()
                            .enumerate()
                            .try_fold(true, |viable, (face, domain)| {
                                if !viable {
                                    return Ok(false);
                                }
                                if !incidence.closed_faces[face] || !affected_faces.contains(&face)
                                {
                                    return Ok(true);
                                }
                                match domain {
                                    MeshFaceBoundaryDomain::Ordered(assignments) => {
                                        Ok(assignments.iter().any(|assignment| {
                                            partial_ordered_assignment_viable(
                                                assignment,
                                                local_edge_by_id,
                                                edges,
                                                domains,
                                                assigned,
                                                Some((root, *point)),
                                                work_budget,
                                            )
                                        }))
                                    }
                                    _ => partial_compact_assignment_viable(
                                        ctx,
                                        domain,
                                        local_edge_by_id,
                                        edges,
                                        edge_candidates.len(),
                                        assigned,
                                        (root, *point),
                                        work_budget,
                                    ),
                                }
                            })?;
                        if !boundaries_viable {
                            return Ok(false);
                        }
                        Ok(true)
                    })()?;
                    if viable {
                        crate::resource::push(
                            ctx,
                            &mut values,
                            *point,
                            "catia_coordinate_viable_values",
                        )?;
                    }
                    Ok(values)
                })
        };

        let mut propagated = Vec::new();
        let mut pending_roots = None::<HashSet<usize>>;
        let branch: Option<CoordinateBranch> = loop {
            let mut scanned_roots = Vec::new();
            if let Some(roots) = pending_roots.take() {
                for root in roots {
                    crate::resource::push(
                        ctx,
                        &mut scanned_roots,
                        root,
                        "catia_coordinate_closure_scanned_roots",
                    )?;
                }
            } else {
                for root in 0..domains.len() {
                    crate::resource::push(
                        ctx,
                        &mut scanned_roots,
                        root,
                        "catia_coordinate_closure_scanned_roots",
                    )?;
                }
            }
            scanned_roots.sort_unstable_by_key(|root| (domains[*root].len(), *root));
            let partial_scan = scanned_roots.len() < domains.len();
            let bounded_scan = !partial_scan
                && budget.is_some_and(|budget| {
                    assigned
                        .iter()
                        .enumerate()
                        .filter(|(_, point)| point.is_none())
                        .map(|(root, _)| domains[root].len().saturating_add(1))
                        .fold(0usize, usize::saturating_add)
                        > budget.remaining()
                });
            let remaining = assigned.iter().filter(|point| point.is_none()).count();
            let unused = component_points
                .iter()
                .filter(|point| point_uses[**point] == 0)
                .count();
            if remaining < unused {
                break None;
            }
            let mut viable_domains = Vec::new();
            let mut dead = false;
            let mut progress = false;
            let mut scan_truncated = false;
            let mut scan_deferred = false;
            let mut supported_unused = HashSet::new();
            let mut unused_point_roots = HashMap::<usize, Vec<usize>>::new();
            for root in scanned_roots {
                if assigned[root].is_some() {
                    continue;
                }
                let work_budget = budget.map(|budget| WorkBudget::new(budget.remaining()));
                if work_budget
                    .as_ref()
                    .is_some_and(|budget| !budget.charge_by(domains[root].len().saturating_add(1)))
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
                    let work = budget.remaining() - work_budget.remaining();
                    if !budget.charge_by(work) {
                        *exhausted = true;
                        break;
                    }
                }
                if values.is_empty() {
                    dead = true;
                    break;
                }
                for &point in values.iter().filter(|point| point_uses[**point] == 0) {
                    crate::resource::insert_set(
                        ctx,
                        &mut supported_unused,
                        point,
                        "catia_coordinate_closure_supported_unused",
                    )?;
                }
                for &point in values.iter().filter(|point| point_uses[**point] == 0) {
                    crate::resource::admit_map_entry(
                        ctx,
                        &mut unused_point_roots,
                        &point,
                        "catia_coordinate_closure_unused_point_keys",
                    )?;
                    crate::resource::push(
                        ctx,
                        unused_point_roots.entry(point).or_default(),
                        root,
                        "catia_coordinate_closure_unused_point_roots",
                    )?;
                }
                if let [point] = values.as_slice() {
                    let Some(undo) = assign(
                        ctx,
                        root,
                        *point,
                        assigned,
                        edges,
                        root_edges,
                        incidence,
                        base_degrees,
                        budget,
                    )?
                    else {
                        *exhausted = true;
                        break;
                    };
                    point_uses[*point] += 1;
                    crate::resource::push(
                        ctx,
                        &mut propagated,
                        (root, *point, undo),
                        "catia_coordinate_closure_propagated",
                    )?;
                    progress = true;
                    if incidence.is_some() || bounded_scan {
                        pending_roots =
                            Some(affected_roots(ctx, root, root_edges, edges, incidence)?);
                        break;
                    }
                } else {
                    crate::resource::push(
                        ctx,
                        &mut viable_domains,
                        (root, values),
                        "catia_coordinate_closure_viable_domains",
                    )?;
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
                let best = viable_domains
                    .into_iter()
                    .min_by_key(|(_, values)| values.len());
                if let Some((root, values)) = best {
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
            if component_points
                .iter()
                .any(|point| point_uses[*point] == 0 && !supported_unused.contains(point))
            {
                break None;
            }
            let mut point_supports = Vec::new();
            for support in unused_point_roots {
                crate::resource::push(
                    ctx,
                    &mut point_supports,
                    support,
                    "catia_coordinate_closure_point_supports",
                )?;
            }
            point_supports.sort_unstable_by_key(|(point, _)| *point);
            let mut uniquely_required = Vec::new();
            for (point, roots) in &point_supports {
                if let Ok(&[root]) = <&[usize; 1]>::try_from(roots.as_slice()) {
                    crate::resource::push(
                        ctx,
                        &mut uniquely_required,
                        (*point, root),
                        "catia_coordinate_closure_unique_supports",
                    )?;
                }
            }
            if let Some(&(point, root)) = uniquely_required.first() {
                if uniquely_required
                    .iter()
                    .any(|&(other_point, other_root)| other_root == root && other_point != point)
                {
                    break None;
                }
                let Some(undo) = assign(
                    ctx,
                    root,
                    point,
                    assigned,
                    edges,
                    root_edges,
                    incidence,
                    base_degrees,
                    budget,
                )?
                else {
                    *exhausted = true;
                    break None;
                };
                point_uses[point] += 1;
                crate::resource::push(
                    ctx,
                    &mut propagated,
                    (root, point, undo),
                    "catia_coordinate_closure_propagated",
                )?;
                pending_roots = Some(affected_roots(ctx, root, root_edges, edges, incidence)?);
                continue;
            }
            let matching_budget = budget.map(|budget| WorkBudget::new(budget.remaining()));
            let mut support_domains = Vec::new();
            for (_, roots) in &point_supports {
                crate::resource::push(
                    ctx,
                    &mut support_domains,
                    roots.as_slice(),
                    "catia_coordinate_closure_support_domains",
                )?;
            }
            let coverage_matching = distinct_domain_matching_with_budget(
                ctx,
                support_domains.iter().copied(),
                assigned.len(),
                matching_budget.as_ref(),
                None,
            )?;
            let mut matching_forced = None;
            let mut unsupported_matches = HashSet::new();
            if let Some(matching) = &coverage_matching {
                for (support, &root) in matching.iter().enumerate() {
                    if distinct_domain_matching_with_budget(
                        ctx,
                        support_domains.iter().copied(),
                        assigned.len(),
                        matching_budget.as_ref(),
                        Some(MatchingEdgeConstraint::Exclude(support, root)),
                    )?
                    .is_none()
                    {
                        if matching_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                            break;
                        }
                        matching_forced = Some((point_supports[support].0, root));
                        break;
                    }
                }
                if matching_forced.is_none() {
                    'supports: for (support, (_, roots)) in point_supports.iter().enumerate() {
                        for &root in roots {
                            if matching[support] == root {
                                continue;
                            }
                            if distinct_domain_matching_with_budget(
                                ctx,
                                support_domains.iter().copied(),
                                assigned.len(),
                                matching_budget.as_ref(),
                                Some(MatchingEdgeConstraint::Require(support, root)),
                            )?
                            .is_none()
                            {
                                if matching_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                                    break 'supports;
                                }
                                crate::resource::insert_set(
                                    ctx,
                                    &mut unsupported_matches,
                                    (root, point_supports[support].0),
                                    "catia_coordinate_closure_unsupported_matches",
                                )?;
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
                    let work = budget.remaining() - matching_budget.remaining();
                    if !budget.charge_by(work) {
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
                        root,
                        point,
                        assigned,
                        edges,
                        root_edges,
                        incidence,
                        base_degrees,
                        budget,
                    )?
                    else {
                        *exhausted = true;
                        break None;
                    };
                    point_uses[point] += 1;
                    crate::resource::push(
                        ctx,
                        &mut propagated,
                        (root, point, undo),
                        "catia_coordinate_closure_propagated",
                    )?;
                    pending_roots = Some(affected_roots(ctx, root, root_edges, edges, incidence)?);
                    continue;
                }
                for (root, values) in &mut viable_domains {
                    values.retain(|point| !unsupported_matches.contains(&(*root, *point)));
                    if values.is_empty() {
                        break;
                    }
                }
                if viable_domains.iter().any(|(_, values)| values.is_empty()) {
                    break None;
                }
                if let Some(&(root, ref values)) =
                    viable_domains.iter().find(|(_, values)| values.len() == 1)
                {
                    let point = values[0];
                    let Some(undo) = assign(
                        ctx,
                        root,
                        point,
                        assigned,
                        edges,
                        root_edges,
                        incidence,
                        base_degrees,
                        budget,
                    )?
                    else {
                        *exhausted = true;
                        break None;
                    };
                    point_uses[point] += 1;
                    crate::resource::push(
                        ctx,
                        &mut propagated,
                        (root, point, undo),
                        "catia_coordinate_closure_propagated",
                    )?;
                    pending_roots = Some(affected_roots(ctx, root, root_edges, edges, incidence)?);
                    continue;
                }
            }
            let best = viable_domains
                .into_iter()
                .min_by_key(|(_, values)| values.len());
            if let Some((root, values)) = best {
                break Some(CoordinateBranch::Search { root, values });
            }
            if assigned.iter().any(Option::is_none) {
                break None;
            }
            let mut complete = Vec::new();
            for &point in assigned.iter().flatten() {
                crate::resource::push(
                    ctx,
                    &mut complete,
                    point,
                    "catia_coordinate_closure_complete_branch",
                )?;
            }
            break Some(CoordinateBranch::Complete(complete));
        };
        let Some(branch) = branch else {
            rollback(assigned, point_uses, propagated, base_degrees);
            return Ok(());
        };
        let (root, values) = match branch {
            CoordinateBranch::Complete(solution) => {
                let incidence_closed = (|| -> Result<bool, CodecError> {
                    let Some(incidence) = incidence else {
                        return Ok(true);
                    };
                    if budget.is_some_and(|budget| !budget.charge_by(edges.len())) {
                        return Ok(false);
                    }
                    let mut degrees = HashMap::<(usize, usize), usize>::new();
                    for (edge, [left, right]) in edges.iter().copied().enumerate() {
                        let [left, right] = [solution[left], solution[right]];
                        let faces = incidence.edge_faces[edge];
                        for (rank, face) in faces.into_iter().enumerate() {
                            if rank > 0 && face == faces[0] {
                                continue;
                            }
                            for point in [left, right] {
                                crate::resource::admit_map_entry(
                                    ctx,
                                    &mut degrees,
                                    &(face, point),
                                    "catia_coordinate_closure_completed_degrees",
                                )?;
                                let degree = degrees.entry((face, point)).or_default();
                                *degree = degree.saturating_add(1);
                            }
                        }
                    }
                    Ok(degrees
                        .into_iter()
                        .all(|((face, _), degree)| !incidence.closed_faces[face] || degree == 2))
                })()?;
                let boundaries_close =
                    incidence.map_or(Ok(true), |incidence| -> Result<bool, CodecError> {
                        let closed_face_count = incidence
                            .closed_faces
                            .iter()
                            .filter(|closed| **closed)
                            .count();
                        if budget.is_some_and(|budget| {
                            !budget.charge_by(edge_ids.len().saturating_add(closed_face_count))
                        }) {
                            return Ok(false);
                        }
                        let mut selected = ctx.alloc_filled(
                            edge_candidates.len(),
                            None,
                            "catia coordinate closure selected edges",
                        )?;
                        for (local_edge, &edge) in edge_ids.iter().enumerate() {
                            let [left, right] = edges[local_edge].map(|root| solution[root]);
                            selected[edge] = Some([left, right]);
                        }
                        for (face, _) in incidence
                            .closed_faces
                            .iter()
                            .enumerate()
                            .filter(|(_, closed)| **closed)
                        {
                            let viable = match &incidence.boundary_domains[face] {
                                MeshFaceBoundaryDomain::Ordered(assignments) => {
                                    assignments.iter().any(|assignment| {
                                        complete_ordered_assignment_viable(
                                            assignment, &selected, budget,
                                        )
                                    })
                                }
                                domain => {
                                    compact_boundary_domain_viable(ctx, domain, &selected, None)?
                                }
                            };
                            if !viable {
                                return Ok(false);
                            }
                        }
                        Ok(true)
                    })?;
                if budget.is_some_and(WorkBudget::exhausted) {
                    *exhausted = true;
                }
                if !*exhausted
                    && incidence_closed
                    && boundaries_close
                    && component_points.iter().all(|point| point_uses[*point] > 0)
                {
                    crate::resource::push(
                        ctx,
                        solutions,
                        solution,
                        "catia_coordinate_closure_solutions",
                    )?;
                }
                rollback(assigned, point_uses, propagated, base_degrees);
                return Ok(());
            }
            CoordinateBranch::Search { root, values } => (root, values),
        };
        if *states >= state_limit {
            if let Some(budget) = budget {
                budget.exhaust();
            }
            *exhausted = true;
            rollback(assigned, point_uses, propagated, base_degrees);
            return Ok(());
        }
        *states += 1;
        for point in values {
            let Some(undo) = assign(
                ctx,
                root,
                point,
                assigned,
                edges,
                root_edges,
                incidence,
                base_degrees,
                budget,
            )?
            else {
                *exhausted = true;
                break;
            };
            point_uses[point] += 1;
            walk(
                ctx,
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
            )?;
            point_uses[point] -= 1;
            unassign(root, assigned, base_degrees, undo);
            if solutions.len() > 1 || *exhausted {
                break;
            }
        }
        rollback(assigned, point_uses, propagated, base_degrees);
        Ok(())
    }

    let mut roots = Vec::new();
    for node in 0..quotient.union.len() {
        if quotient.union.find(node) == node {
            crate::resource::push(ctx, &mut roots, node, "catia_coordinate_closure_roots")?;
        }
    }
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
    let mut root_indices = HashMap::new();
    for (index, root) in roots.iter().copied().enumerate() {
        crate::resource::insert_map(
            ctx,
            &mut root_indices,
            root,
            index,
            "catia_coordinate_closure_root_indices",
        )?;
    }
    let mut edges = Vec::new();
    for edge in 0..edge_candidates.len() {
        let Some(&left) = root_indices.get(&quotient.union.find(edge * 2)) else {
            return Ok(None);
        };
        let Some(&right) = root_indices.get(&quotient.union.find(edge * 2 + 1)) else {
            return Ok(None);
        };
        crate::resource::push(
            ctx,
            &mut edges,
            [left, right],
            "catia_coordinate_closure_edges",
        )?;
    }
    let mut domains = Vec::new();
    for root in roots.iter().copied() {
        let mut domain = Vec::new();
        for point in quotient.domains[root]
            .iter()
            .copied()
            .filter(|point| *point < point_count)
        {
            crate::resource::push(
                ctx,
                &mut domain,
                point,
                "catia_coordinate_closure_domain_points",
            )?;
        }
        domain.sort_unstable();
        crate::resource::push(
            ctx,
            &mut domains,
            domain,
            "catia_coordinate_closure_domains",
        )?;
    }
    if domains.iter().any(Vec::is_empty) {
        return Ok(None);
    }
    let mut covered_points = HashSet::new();
    for point in domains.iter().flatten().copied() {
        crate::resource::insert_set(
            ctx,
            &mut covered_points,
            point,
            "catia_coordinate_closure_covered_points",
        )?;
    }
    if covered_points.len() != point_count {
        return Ok(None);
    }
    let mut dependency =
        UnionFind::charged(ctx, roots.len(), "catia_coordinate_closure_dependency")?;
    for [left, right] in &edges {
        dependency.union(*left, *right);
    }
    let mut root_by_point = HashMap::new();
    for (root, domain) in domains.iter().enumerate() {
        for point in domain {
            if let Some(previous) = crate::resource::insert_map(
                ctx,
                &mut root_by_point,
                *point,
                root,
                "catia_coordinate_closure_point_roots",
            )? {
                dependency.union(previous, root);
            }
        }
    }
    let mut components = HashMap::<usize, Vec<usize>>::new();
    for root in 0..roots.len() {
        let component = dependency.find(root);
        if !components.contains_key(&component) {
            crate::resource::insert_map(
                ctx,
                &mut components,
                component,
                Vec::new(),
                "catia_coordinate_closure_component_keys",
            )?;
        }
        if let Some(members) = components.get_mut(&component) {
            crate::resource::push(
                ctx,
                members,
                root,
                "catia_coordinate_closure_component_members",
            )?;
        }
    }
    let mut ordered_components = Vec::new();
    for component in components.into_values() {
        crate::resource::push(
            ctx,
            &mut ordered_components,
            component,
            "catia_coordinate_closure_components",
        )?;
    }
    let mut components = ordered_components;
    components.sort_by_key(|component| component[0]);
    let incidence = if let Some((edge_faces, boundary_domains)) = incidence {
        if budget.is_some_and(|budget| !budget.charge_by(edge_faces.len())) {
            exhausted.set(true);
            return Ok(None);
        }
        let mut counts = ctx.alloc_filled(
            boundary_domains.len(),
            0usize,
            "catia_coordinate_closure_face_counts",
        )?;
        for faces in edge_faces {
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
    let mut assignment = ctx.alloc_filled(roots.len(), None, "catia coordinate root assignment")?;
    let shared_budget = budget;
    for component in components {
        let support_count = component
            .iter()
            .map(|root| domains[*root].len())
            .fold(0usize, usize::saturating_add);
        let mut component_set = HashSet::new();
        let mut local_index = HashMap::new();
        for (local, &global) in component.iter().enumerate() {
            crate::resource::insert_set(
                ctx,
                &mut component_set,
                global,
                "catia_coordinate_closure_component_set",
            )?;
            crate::resource::insert_map(
                ctx,
                &mut local_index,
                global,
                local,
                "catia_coordinate_closure_local_index",
            )?;
        }
        let mut edge_ids = Vec::new();
        for (edge, [left, _]) in edges.iter().enumerate() {
            if component_set.contains(left) {
                crate::resource::push(
                    ctx,
                    &mut edge_ids,
                    edge,
                    "catia_coordinate_closure_edge_ids",
                )?;
            }
        }
        let mut component_points = HashSet::new();
        for point in component.iter().flat_map(|root| domains[*root].iter()) {
            crate::resource::insert_set(
                ctx,
                &mut component_points,
                *point,
                "catia_coordinate_closure_component_points",
            )?;
        }
        let explicit_pair_supports = edge_ids
            .iter()
            .map(|edge| edge_candidates[*edge].len())
            .fold(0usize, usize::saturating_add);
        let traversal_bound = component
            .len()
            .saturating_add(component_points.len())
            .isqrt()
            .saturating_add(9);
        // A component may require one branch state for every explicit
        // root-point support before propagation distinguishes a solution.
        let state_limit = MAX_COORDINATE_CLOSURE_STATES.max(support_count);
        let component_limit = component_search_budget.map(|base| {
            // Reserve the same graph-traversal allowance used by coordinate
            // preparation plus face-incidence scans for every permitted
            // branch state.
            let state_work = support_count
                .saturating_add(explicit_pair_supports)
                .saturating_mul(traversal_bound)
                .saturating_add(if incidence.is_some() {
                    support_count.saturating_mul(edge_ids.len())
                } else {
                    0
                });
            base.saturating_add(state_work.saturating_mul(state_limit))
        });
        let component_budget = component_limit.map(WorkBudget::new);
        let budget = component_budget.as_ref().or(shared_budget);
        let mut local_edges = Vec::new();
        let mut local_edge_by_id = HashMap::new();
        for (local, &edge) in edge_ids.iter().enumerate() {
            let [left, right] = edges[edge];
            let (Some(&left), Some(&right)) = (local_index.get(&left), local_index.get(&right))
            else {
                return Ok(None);
            };
            crate::resource::push(
                ctx,
                &mut local_edges,
                [left, right],
                "catia_coordinate_closure_local_edges",
            )?;
            crate::resource::insert_map(
                ctx,
                &mut local_edge_by_id,
                edge,
                local,
                "catia_coordinate_closure_local_edge_index",
            )?;
        }
        let local_incidence = match incidence.as_ref() {
            Some((edge_faces, boundary_domains, counts)) => {
                if budget.is_some_and(|budget| !budget.charge_by(work_units(edge_ids.len()))) {
                    exhausted.set(true);
                    return Ok(None);
                }
                let mut local_edge_faces = Vec::new();
                for &edge in &edge_ids {
                    crate::resource::push(
                        ctx,
                        &mut local_edge_faces,
                        edge_faces[edge],
                        "catia_coordinate_closure_local_edge_faces",
                    )?;
                }
                let mut face_edges = ctx.alloc_filled(
                    boundary_domains.len(),
                    Vec::new(),
                    "catia_coordinate_closure_face_edges",
                )?;
                for (edge, faces) in local_edge_faces.iter().copied().enumerate() {
                    for (rank, face) in faces.into_iter().enumerate() {
                        if rank == 0 || face != faces[0] {
                            crate::resource::push(
                                ctx,
                                &mut face_edges[face],
                                edge,
                                "catia_coordinate_closure_face_edge_entries",
                            )?;
                        }
                    }
                }
                let mut closed_faces = Vec::new();
                for (local, total) in face_edges.iter().zip(counts.iter()) {
                    crate::resource::push(
                        ctx,
                        &mut closed_faces,
                        local.len() == *total,
                        "catia_coordinate_closure_closed_faces",
                    )?;
                }
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
        for &root in &component {
            let domain = crate::resource::copy_slice(
                ctx,
                &domains[root],
                "catia_coordinate_closure_local_domain_points",
            )?;
            crate::resource::push(
                ctx,
                &mut local_domains,
                domain,
                "catia_coordinate_closure_local_domains",
            )?;
        }
        let mut root_edges = ctx.alloc_filled(
            component.len(),
            Vec::new(),
            "catia_coordinate_closure_root_edges",
        )?;
        for (edge, [left, right]) in local_edges.iter().copied().enumerate() {
            crate::resource::push(
                ctx,
                &mut root_edges[left],
                edge,
                "catia_coordinate_closure_root_edge_entries",
            )?;
            if right != left {
                crate::resource::push(
                    ctx,
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
            return Ok(None);
        }
        let mut arc_domains = Vec::new();
        for domain in &local_domains {
            let copy = crate::resource::copy_slice(
                ctx,
                domain,
                "catia_coordinate_closure_arc_domain_points",
            )?;
            crate::resource::push(
                ctx,
                &mut arc_domains,
                copy,
                "catia_coordinate_closure_arc_domains",
            )?;
        }
        let arc_budget = budget.map(|budget| WorkBudget::new(budget.remaining()));
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
                let work = budget.remaining() - arc_budget.remaining();
                if !budget.charge_by(work) {
                    exhausted.set(true);
                    return Ok(None);
                }
            }
            local_domains = arc_domains;
        } else {
            if arc_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                exhausted.set(true);
            }
            return Ok(None);
        }
        let mut remaining_points = HashSet::new();
        for &point in local_domains.iter().flatten() {
            crate::resource::insert_set(
                ctx,
                &mut remaining_points,
                point,
                "catia_coordinate_closure_remaining_points",
            )?;
        }
        if remaining_points != component_points {
            return Ok(None);
        }
        let mut solutions = Vec::new();
        let mut states = 0;
        let mut exhausted = false;
        let mut base_degrees = HashMap::new();
        let mut local_assignment = ctx.alloc_filled(
            component.len(),
            None,
            "catia coordinate component assignment",
        )?;
        let mut point_degrees =
            ctx.alloc_filled(point_count, 0, "catia coordinate point degrees")?;
        walk(
            ctx,
            &local_domains,
            &local_edges,
            &edge_ids,
            &local_edge_by_id,
            &root_edges,
            edge_candidates,
            local_incidence.as_ref(),
            &component_points,
            &mut local_assignment,
            &mut point_degrees,
            &mut solutions,
            &mut states,
            state_limit,
            &mut exhausted,
            &mut base_degrees,
            budget,
        )?;
        if exhausted {
            if component_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                if let Some(shared_budget) = shared_budget {
                    shared_budget.exhaust();
                }
            }
            return Ok(None);
        }
        let [local_assignment] = solutions.as_slice() else {
            if solutions.len() > 1 {
                ambiguous.set(true);
            }
            return Ok(None);
        };
        for (&root, &point) in component.iter().zip(local_assignment) {
            assignment[root] = Some(point);
        }
    }
    let mut completed_assignment = Vec::new();
    for point in assignment {
        let Some(point) = point else {
            return Ok(None);
        };
        crate::resource::push(
            ctx,
            &mut completed_assignment,
            point,
            "catia_coordinate_closure_completed_assignment",
        )?;
    }
    let assignment = completed_assignment;
    for (&root, &point) in roots.iter().zip(&assignment) {
        let mut fixed_domain = HashSet::new();
        crate::resource::insert_set(
            ctx,
            &mut fixed_domain,
            point,
            "catia_coordinate_closure_fixed_domain_point",
        )?;
        quotient.domains[root] = Arc::new(fixed_domain);
    }
    let mut root_by_point = HashMap::new();
    for (&root, &point) in roots.iter().zip(&assignment) {
        if let Some(previous) = crate::resource::insert_map(
            ctx,
            &mut root_by_point,
            point,
            root,
            "catia_coordinate_closure_assigned_point_roots",
        )? {
            let Some(merged) = quotient.merge_charged(ctx, previous, root)? else {
                return Ok(None);
            };
            root_by_point.insert(point, merged);
        }
    }
    if !quotient.edge_domains_viable(ctx, edge_candidates)? {
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
        assert!(partial_compact_assignment_viable(
            &ctx,
            &domain,
            &edge_by_id,
            &edges,
            2,
            &assigned,
            (0, 0),
            None,
        )
        .expect("service budget"));

        let mut limited = DecodePolicy::service();
        limited.limits.max_collection_items = 29;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &limited)
            .expect("fixture fits the input limit");
        let error = partial_compact_assignment_viable(
            &ctx,
            &domain,
            &edge_by_id,
            &edges,
            2,
            &assigned,
            (0, 0),
            None,
        )
        .expect_err("edge point collection exceeds the limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia coordinate assignment edge points"));
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
            match partial_compact_assignment_viable(
                &ctx,
                &domain,
                &edge_by_id,
                &edges,
                3,
                &assigned,
                (0, 0),
                None,
            ) {
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
