// SPDX-License-Identifier: Apache-2.0
//! Coordinate-root assignment and incidence-constrained closure.

use super::{
    alloc_filled, compact_boundary_domain_viable, deferred_boundary_cycle_matches,
    distinct_domain_matching_with_budget, enforce_edge_arc_consistency,
    enforce_sparse_endpoint_membership, incidence_cycles, same_unordered_pair, Arc, Cell, HashMap,
    HashSet, MatchingEdgeConstraint, MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment,
    MeshFaceBoundaryDomain, MeshQuotient, PointAssignmentOutcome, UnionFind, WorkBudget,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn close_coordinate_roots_with_incidence(
    quotient: &mut MeshQuotient,
    point_count: usize,
    edge_candidates: &[Vec<[usize; 2]>],
    incidence: Option<(&[[usize; 2]], &[MeshFaceBoundaryDomain])>,
    budget: Option<&WorkBudget<'_>>,
    component_search_budget: Option<usize>,
    ambiguous: &Cell<bool>,
    exhausted: &Cell<bool>,
) -> Option<HashMap<usize, usize>> {
    const MAX_COORDINATE_CLOSURE_STATES: usize = 256;

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
            let Some(first) = boundary.first().copied() else {
                return false;
            };
            directions(first)
                .into_iter()
                .flatten()
                .any(|first_direction| {
                    let mut previous = vec![first_direction];
                    for index in 1..boundary.len() {
                        let mut next = Vec::new();
                        for direction in directions(boundary[index]).into_iter().flatten() {
                            if previous.iter().copied().any(|previous_direction| {
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
                            }) {
                                next.push(direction);
                            }
                        }
                        if next.is_empty() {
                            return false;
                        }
                        previous = next;
                    }
                    previous.into_iter().any(|previous_direction| {
                        let Some(left) = port_root(
                            *boundary.last().expect("nonempty boundary"),
                            previous_direction,
                            true,
                        ) else {
                            return false;
                        };
                        let Some(right) = port_root(first, first_direction, false) else {
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

    #[allow(clippy::too_many_arguments)]
    fn partial_compact_assignment_viable(
        domain: &MeshFaceBoundaryDomain,
        local_edge_by_id: &HashMap<usize, usize>,
        edges: &[[usize; 2]],
        global_edge_count: usize,
        assigned: &[Option<usize>],
        candidate: (usize, usize),
        budget: Option<&WorkBudget<'_>>,
    ) -> bool {
        fn augment(
            component: usize,
            compatible: &[Vec<bool>],
            seen: &mut [bool],
            matched: &mut [Option<usize>],
        ) -> bool {
            for cycle in 0..matched.len() {
                if !compatible[component][cycle] || seen[cycle] {
                    continue;
                }
                seen[cycle] = true;
                if matched[cycle]
                    .is_none_or(|previous| augment(previous, compatible, seen, matched))
                {
                    matched[cycle] = Some(component);
                    return true;
                }
            }
            false
        }

        let relevant = match domain {
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
                edges.sort_unstable();
                edges.dedup();
                edges
            }
        };
        if budget.is_some_and(|budget| !budget.charge_by(relevant.len().max(1))) {
            return false;
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
                return false;
            };
            let [left, right] = edges[local];
            let [Some(left), Some(right)] = [value(left), value(right)] else {
                continue;
            };
            selected.insert(edge, [left, right]);
            selected_edges.push(edge);
            adjacency.entry(left).or_default().push(edge);
            adjacency.entry(right).or_default().push(edge);
        }
        let mut closed_components = Vec::new();
        let mut seen_edges = HashSet::new();
        for &first in &selected_edges {
            if seen_edges.contains(&first) {
                continue;
            }
            let mut stack = vec![first];
            let mut component = Vec::new();
            let mut vertices = HashSet::new();
            while let Some(edge) = stack.pop() {
                if !seen_edges.insert(edge) {
                    continue;
                }
                component.push(edge);
                for point in selected[&edge] {
                    vertices.insert(point);
                    stack.extend(adjacency[&point].iter().copied());
                }
            }
            if vertices.iter().all(|point| adjacency[point].len() == 2) {
                closed_components.push(component);
            }
        }
        match domain {
            MeshFaceBoundaryDomain::Ordered(_) => true,
            MeshFaceBoundaryDomain::UnorderedFullCycle(_) => {
                closed_components.is_empty()
                    || (selected_edges.len() == relevant.len() && closed_components.len() == 1)
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                if closed_components.len() > domain.cycles.len() {
                    return false;
                }
                let missing = domain.missing_edges.iter().copied().collect::<HashSet<_>>();
                let Ok(mut edge_points) = alloc_filled(
                    global_edge_count,
                    [0; 2],
                    "catia coordinate assignment edge points",
                ) else {
                    return false;
                };
                for (&edge, &points) in &selected {
                    edge_points[edge] = points;
                }
                let compatible = closed_components
                    .iter()
                    .map(|component| {
                        let incidence = incidence_cycles(component, &edge_points);
                        let Some([incidence]) = incidence.as_deref() else {
                            return alloc_filled(
                                domain.cycles.len(),
                                false,
                                "catia_deferred_incompatible",
                            )
                            .ok();
                        };
                        Some(
                            domain
                                .cycles
                                .iter()
                                .map(|cycle| {
                                    deferred_boundary_cycle_matches(cycle, incidence, &missing)
                                })
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Option<Vec<_>>>();
                let Some(compatible) = compatible else {
                    return false;
                };
                let Ok(mut matched) =
                    alloc_filled(domain.cycles.len(), None, "catia_deferred_matched")
                else {
                    return false;
                };
                (0..closed_components.len()).all(|component| {
                    let Ok(mut visited) =
                        alloc_filled(domain.cycles.len(), false, "catia_deferred_augment_visit")
                    else {
                        return false;
                    };
                    augment(component, &compatible, &mut visited, &mut matched)
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        domains: &[Vec<usize>],
        edges: &[[usize; 2]],
        edge_ids: &[usize],
        local_edge_by_id: &HashMap<usize, usize>,
        root_edges: &[Vec<usize>],
        edge_candidates: &[Vec<[usize; 2]>],
        edge_faces: Option<&[[usize; 2]]>,
        face_edges: Option<&[Vec<usize>]>,
        closed_faces: Option<&[bool]>,
        boundary_domains: Option<&[MeshFaceBoundaryDomain]>,
        component_points: &HashSet<usize>,
        assigned: &mut [Option<usize>],
        point_uses: &mut [usize],
        solutions: &mut Vec<Vec<usize>>,
        states: &mut usize,
        state_limit: usize,
        exhausted: &mut bool,
        base_degrees: &mut HashMap<(usize, usize), u8>,
        budget: Option<&WorkBudget<'_>>,
    ) {
        fn adjust_assignment_degrees(
            root: usize,
            increase: bool,
            assigned: &[Option<usize>],
            edges: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            edge_faces: Option<&[[usize; 2]]>,
            degrees: &mut HashMap<(usize, usize), u8>,
        ) {
            let Some(edge_faces) = edge_faces else {
                return;
            };
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
                        if increase {
                            *degrees.entry((face, point)).or_default() += 1;
                        } else {
                            let degree = degrees
                                .get_mut(&(face, point))
                                .expect("assigned edge contributes its face degree");
                            *degree -= 1;
                            if *degree == 0 {
                                degrees.remove(&(face, point));
                            }
                        }
                    }
                }
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn assign(
            root: usize,
            point: usize,
            assigned: &mut [Option<usize>],
            edges: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            edge_faces: Option<&[[usize; 2]]>,
            degrees: &mut HashMap<(usize, usize), u8>,
            budget: Option<&WorkBudget<'_>>,
        ) -> bool {
            if budget.is_some_and(|budget| !budget.charge_by(root_edges[root].len())) {
                return false;
            }
            assigned[root] = Some(point);
            adjust_assignment_degrees(root, true, assigned, edges, root_edges, edge_faces, degrees);
            true
        }

        fn unassign(
            root: usize,
            assigned: &mut [Option<usize>],
            edges: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            edge_faces: Option<&[[usize; 2]]>,
            degrees: &mut HashMap<(usize, usize), u8>,
        ) {
            adjust_assignment_degrees(
                root, false, assigned, edges, root_edges, edge_faces, degrees,
            );
            assigned[root] = None;
        }

        #[allow(clippy::too_many_arguments)]
        fn rollback(
            assigned: &mut [Option<usize>],
            point_uses: &mut [usize],
            propagated: Vec<(usize, usize)>,
            edges: &[[usize; 2]],
            root_edges: &[Vec<usize>],
            edge_faces: Option<&[[usize; 2]]>,
            degrees: &mut HashMap<(usize, usize), u8>,
        ) {
            for (root, point) in propagated.into_iter().rev() {
                point_uses[point] -= 1;
                unassign(root, assigned, edges, root_edges, edge_faces, degrees);
            }
        }

        fn affected_roots(
            root: usize,
            root_edges: &[Vec<usize>],
            edges: &[[usize; 2]],
            edge_faces: Option<&[[usize; 2]]>,
            face_edges: Option<&[Vec<usize>]>,
        ) -> HashSet<usize> {
            let mut affected = HashSet::new();
            for &edge in &root_edges[root] {
                affected.extend(edges[edge]);
                let (Some(edge_faces), Some(face_edges)) = (edge_faces, face_edges) else {
                    continue;
                };
                let faces = edge_faces[edge];
                for (rank, face) in faces.into_iter().enumerate() {
                    if rank > 0 && face == faces[0] {
                        continue;
                    }
                    for &face_edge in &face_edges[face] {
                        affected.extend(edges[face_edge]);
                    }
                }
            }
            affected.remove(&root);
            affected
        }

        if solutions.len() > 1 || *exhausted {
            return;
        }
        if budget.is_some_and(|budget| !budget.charge()) {
            *exhausted = true;
            return;
        }
        let viable_values = |root: usize,
                             assigned: &[Option<usize>],
                             base_degrees: &HashMap<(usize, usize), u8>,
                             work_budget: Option<&WorkBudget<'_>>| {
            domains[root]
                .iter()
                .copied()
                .filter(|point| {
                    let pair_viable = root_edges[root].iter().all(|edge| {
                        let [left, right] = edges[*edge];
                        let other = if left == root { right } else { left };
                        assigned[other].is_none_or(|other_point| {
                            pair_supported(&edge_candidates[edge_ids[*edge]], *point, other_point)
                        })
                    });
                    if !pair_viable {
                        return false;
                    }
                    let Some(edge_faces) = edge_faces else {
                        return true;
                    };
                    if work_budget
                        .is_some_and(|budget| !budget.charge_by(root_edges[root].len().max(1)))
                    {
                        return false;
                    }
                    let value = |endpoint| {
                        if endpoint == root {
                            Some(*point)
                        } else {
                            assigned[endpoint]
                        }
                    };
                    let mut degrees = base_degrees.clone();
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
                            affected_faces.insert(face);
                            for endpoint in [left, right] {
                                let degree = degrees.entry((face, endpoint)).or_default();
                                *degree = degree.saturating_add(1);
                                if *degree > 2 {
                                    return false;
                                }
                            }
                        }
                    }
                    for (&(face, point), &degree) in &degrees {
                        if degree != 1 || !affected_faces.contains(&face) {
                            continue;
                        }
                        let Some(face_edges) = face_edges else {
                            return false;
                        };
                        if work_budget
                            .is_some_and(|budget| !budget.charge_by(face_edges[face].len().max(1)))
                        {
                            return false;
                        }
                        let supported = face_edges[face].iter().copied().any(|edge| {
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
                            return false;
                        }
                    }
                    if let (Some(boundary_domains), Some(closed_faces)) =
                        (boundary_domains, closed_faces)
                    {
                        let boundaries_viable =
                            boundary_domains.iter().enumerate().all(|(face, domain)| {
                                if !closed_faces[face] || !affected_faces.contains(&face) {
                                    return true;
                                }
                                match domain {
                                    MeshFaceBoundaryDomain::Ordered(assignments) => {
                                        assignments.iter().any(|assignment| {
                                            partial_ordered_assignment_viable(
                                                assignment,
                                                local_edge_by_id,
                                                edges,
                                                domains,
                                                assigned,
                                                Some((root, *point)),
                                                work_budget,
                                            )
                                        })
                                    }
                                    _ => partial_compact_assignment_viable(
                                        domain,
                                        local_edge_by_id,
                                        edges,
                                        edge_candidates.len(),
                                        assigned,
                                        (root, *point),
                                        work_budget,
                                    ),
                                }
                            });
                        if !boundaries_viable {
                            return false;
                        }
                    }
                    true
                })
                .collect::<Vec<_>>()
        };

        let mut propagated = Vec::new();
        let mut pending_roots = None::<HashSet<usize>>;
        let branch = loop {
            let mut scanned_roots = pending_roots.take().map_or_else(
                || (0..domains.len()).collect::<Vec<_>>(),
                |roots| roots.into_iter().collect(),
            );
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
                let values = viable_values(root, assigned, base_degrees, work_budget.as_ref());
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
                supported_unused.extend(
                    values
                        .iter()
                        .copied()
                        .filter(|point| point_uses[*point] == 0),
                );
                for &point in values.iter().filter(|point| point_uses[**point] == 0) {
                    unused_point_roots.entry(point).or_default().push(root);
                }
                if let [point] = values.as_slice() {
                    if !assign(
                        root,
                        *point,
                        assigned,
                        edges,
                        root_edges,
                        edge_faces,
                        base_degrees,
                        budget,
                    ) {
                        *exhausted = true;
                        break;
                    }
                    point_uses[*point] += 1;
                    propagated.push((root, *point));
                    progress = true;
                    if edge_faces.is_some() || bounded_scan {
                        pending_roots = Some(affected_roots(
                            root, root_edges, edges, edge_faces, face_edges,
                        ));
                        break;
                    }
                } else {
                    viable_domains.push((root, values));
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
                if best.is_some() {
                    break Some(best);
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
            let mut point_supports = unused_point_roots.into_iter().collect::<Vec<_>>();
            point_supports.sort_unstable_by_key(|(point, _)| *point);
            let uniquely_required = point_supports
                .iter()
                .filter_map(|(point, roots)| {
                    <&[usize; 1]>::try_from(roots.as_slice())
                        .ok()
                        .map(|[root]| (*point, *root))
                })
                .collect::<Vec<_>>();
            if let Some(&(point, root)) = uniquely_required.first() {
                if uniquely_required
                    .iter()
                    .any(|&(other_point, other_root)| other_root == root && other_point != point)
                {
                    break None;
                }
                if !assign(
                    root,
                    point,
                    assigned,
                    edges,
                    root_edges,
                    edge_faces,
                    base_degrees,
                    budget,
                ) {
                    *exhausted = true;
                    break None;
                }
                point_uses[point] += 1;
                propagated.push((root, point));
                pending_roots = Some(affected_roots(
                    root, root_edges, edges, edge_faces, face_edges,
                ));
                continue;
            }
            let matching_budget = budget.map(|budget| WorkBudget::new(budget.remaining()));
            let support_domains = point_supports
                .iter()
                .map(|(_, roots)| roots.as_slice())
                .collect::<Vec<_>>();
            let coverage_matching = distinct_domain_matching_with_budget(
                support_domains.iter().copied(),
                assigned.len(),
                matching_budget.as_ref(),
                None,
            );
            let mut matching_forced = None;
            let mut unsupported_matches = HashSet::new();
            if let Some(matching) = &coverage_matching {
                for (support, &root) in matching.iter().enumerate() {
                    if distinct_domain_matching_with_budget(
                        support_domains.iter().copied(),
                        assigned.len(),
                        matching_budget.as_ref(),
                        Some(MatchingEdgeConstraint::Exclude(support, root)),
                    )
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
                                support_domains.iter().copied(),
                                assigned.len(),
                                matching_budget.as_ref(),
                                Some(MatchingEdgeConstraint::Require(support, root)),
                            )
                            .is_none()
                            {
                                if matching_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                                    break 'supports;
                                }
                                unsupported_matches.insert((root, point_supports[support].0));
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
                    if !assign(
                        root,
                        point,
                        assigned,
                        edges,
                        root_edges,
                        edge_faces,
                        base_degrees,
                        budget,
                    ) {
                        *exhausted = true;
                        break None;
                    }
                    point_uses[point] += 1;
                    propagated.push((root, point));
                    pending_roots = Some(affected_roots(
                        root, root_edges, edges, edge_faces, face_edges,
                    ));
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
                    if !assign(
                        root,
                        point,
                        assigned,
                        edges,
                        root_edges,
                        edge_faces,
                        base_degrees,
                        budget,
                    ) {
                        *exhausted = true;
                        break None;
                    }
                    point_uses[point] += 1;
                    propagated.push((root, point));
                    pending_roots = Some(affected_roots(
                        root, root_edges, edges, edge_faces, face_edges,
                    ));
                    continue;
                }
            }
            let best = viable_domains
                .into_iter()
                .min_by_key(|(_, values)| values.len());
            break Some(best);
        };
        let Some(branch) = branch else {
            rollback(
                assigned,
                point_uses,
                propagated,
                edges,
                root_edges,
                edge_faces,
                base_degrees,
            );
            return;
        };
        let Some((root, values)) = branch else {
            let incidence_closed =
                edge_faces
                    .zip(closed_faces)
                    .is_none_or(|(edge_faces, closed_faces)| {
                        if budget.is_some_and(|budget| !budget.charge_by(edges.len())) {
                            return false;
                        }
                        let mut degrees = HashMap::<(usize, usize), u8>::new();
                        for (edge, [left, right]) in edges.iter().copied().enumerate() {
                            let [Some(left), Some(right)] = [assigned[left], assigned[right]]
                            else {
                                return false;
                            };
                            let faces = edge_faces[edge];
                            for (rank, face) in faces.into_iter().enumerate() {
                                if rank > 0 && face == faces[0] {
                                    continue;
                                }
                                for point in [left, right] {
                                    *degrees.entry((face, point)).or_default() += 1;
                                }
                            }
                        }
                        degrees
                            .into_iter()
                            .all(|((face, _), degree)| !closed_faces[face] || degree == 2)
                    });
            let boundaries_close = boundary_domains.zip(closed_faces).is_none_or(
                |(boundary_domains, closed_faces)| {
                    let closed_face_count = closed_faces.iter().filter(|closed| **closed).count();
                    if budget.is_some_and(|budget| {
                        !budget.charge_by(edge_ids.len().saturating_add(closed_face_count))
                    }) {
                        return false;
                    }
                    let mut selected = vec![None; edge_candidates.len()];
                    for (local_edge, &edge) in edge_ids.iter().enumerate() {
                        let [left, right] = edges[local_edge];
                        let [Some(left), Some(right)] = [assigned[left], assigned[right]] else {
                            return false;
                        };
                        selected[edge] = Some([left, right]);
                    }
                    closed_faces
                        .iter()
                        .enumerate()
                        .filter(|(_, closed)| **closed)
                        .all(|(face, _)| match &boundary_domains[face] {
                            MeshFaceBoundaryDomain::Ordered(assignments) => {
                                assignments.iter().any(|assignment| {
                                    complete_ordered_assignment_viable(
                                        assignment, &selected, budget,
                                    )
                                })
                            }
                            domain => compact_boundary_domain_viable(domain, &selected, None),
                        })
                },
            );
            if budget.is_some_and(WorkBudget::exhausted) {
                *exhausted = true;
            }
            if !*exhausted
                && incidence_closed
                && boundaries_close
                && component_points.iter().all(|point| point_uses[*point] > 0)
            {
                solutions.push(
                    assigned
                        .iter()
                        .copied()
                        .collect::<Option<Vec<_>>>()
                        .expect("complete coordinate assignment"),
                );
            }
            rollback(
                assigned,
                point_uses,
                propagated,
                edges,
                root_edges,
                edge_faces,
                base_degrees,
            );
            return;
        };
        if *states >= state_limit {
            if let Some(budget) = budget {
                budget.exhaust();
            }
            *exhausted = true;
            rollback(
                assigned,
                point_uses,
                propagated,
                edges,
                root_edges,
                edge_faces,
                base_degrees,
            );
            return;
        }
        *states += 1;
        for point in values {
            if !assign(
                root,
                point,
                assigned,
                edges,
                root_edges,
                edge_faces,
                base_degrees,
                budget,
            ) {
                *exhausted = true;
                break;
            }
            point_uses[point] += 1;
            walk(
                domains,
                edges,
                edge_ids,
                local_edge_by_id,
                root_edges,
                edge_candidates,
                edge_faces,
                face_edges,
                closed_faces,
                boundary_domains,
                component_points,
                assigned,
                point_uses,
                solutions,
                states,
                state_limit,
                exhausted,
                base_degrees,
                budget,
            );
            point_uses[point] -= 1;
            unassign(root, assigned, edges, root_edges, edge_faces, base_degrees);
            if solutions.len() > 1 || *exhausted {
                break;
            }
        }
        rollback(
            assigned,
            point_uses,
            propagated,
            edges,
            root_edges,
            edge_faces,
            base_degrees,
        );
    }

    let mut roots = Vec::new();
    for node in 0..quotient.union.len() {
        if quotient.union.find(node) == node {
            roots.push(node);
        }
    }
    if roots.len() < point_count {
        return None;
    }
    if roots.len() == point_count && incidence.is_none() {
        return match quotient.point_assignments_with_budget(point_count, edge_candidates, 2, budget)
        {
            PointAssignmentOutcome::Complete(mut assignments) => match assignments.len() {
                1 => Some(assignments.remove(0)),
                length if length > 1 => {
                    ambiguous.set(true);
                    None
                }
                _ => None,
            },
            PointAssignmentOutcome::Exhausted => None,
        };
    }
    let root_indices = roots
        .iter()
        .enumerate()
        .map(|(index, root)| (*root, index))
        .collect::<HashMap<_, _>>();
    let edges = edge_candidates
        .iter()
        .enumerate()
        .map(|(edge, _)| {
            Some([
                *root_indices.get(&quotient.union.find(edge * 2))?,
                *root_indices.get(&quotient.union.find(edge * 2 + 1))?,
            ])
        })
        .collect::<Option<Vec<_>>>()?;
    let domains = roots
        .iter()
        .map(|root| {
            let mut domain = quotient.domains[*root]
                .iter()
                .copied()
                .filter(|point| *point < point_count)
                .collect::<Vec<_>>();
            domain.sort_unstable();
            domain
        })
        .collect::<Vec<_>>();
    if domains.iter().any(Vec::is_empty) {
        return None;
    }
    if domains
        .iter()
        .flatten()
        .copied()
        .collect::<HashSet<_>>()
        .len()
        != point_count
    {
        return None;
    }
    let mut dependency = UnionFind::new(roots.len());
    for [left, right] in &edges {
        dependency.union(*left, *right);
    }
    let mut root_by_point = HashMap::new();
    for (root, domain) in domains.iter().enumerate() {
        for point in domain {
            if let Some(previous) = root_by_point.insert(*point, root) {
                dependency.union(previous, root);
            }
        }
    }
    let mut components = HashMap::<usize, Vec<usize>>::new();
    for root in 0..roots.len() {
        components
            .entry(dependency.find(root))
            .or_default()
            .push(root);
    }
    let mut components = components.into_values().collect::<Vec<_>>();
    components.sort_by_key(|component| component[0]);
    let face_incidence_counts = incidence.map(|(edge_faces, boundary_domains)| {
        if budget.is_some_and(|budget| !budget.charge_by(edge_faces.len())) {
            return Vec::new();
        }
        let mut counts = vec![0usize; boundary_domains.len()];
        for faces in edge_faces {
            for (rank, face) in faces.iter().copied().enumerate() {
                if rank == 0 || face != faces[0] {
                    counts[face] += 1;
                }
            }
        }
        counts
    });
    if face_incidence_counts.as_ref().is_some_and(Vec::is_empty) {
        return None;
    }
    let mut assignment = vec![None; roots.len()];
    let shared_budget = budget;
    for component in components {
        let support_count = component
            .iter()
            .map(|root| domains[*root].len())
            .fold(0usize, usize::saturating_add);
        let component_set = component.iter().copied().collect::<HashSet<_>>();
        let local_index = component
            .iter()
            .enumerate()
            .map(|(local, global)| (*global, local))
            .collect::<HashMap<_, _>>();
        let edge_ids = edges
            .iter()
            .enumerate()
            .filter_map(|(edge, [left, _])| component_set.contains(left).then_some(edge))
            .collect::<Vec<_>>();
        let component_points = component
            .iter()
            .flat_map(|root| domains[*root].iter())
            .copied()
            .collect::<HashSet<_>>();
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
        let local_edges = edge_ids
            .iter()
            .map(|edge| {
                let [left, right] = edges[*edge];
                Some([*local_index.get(&left)?, *local_index.get(&right)?])
            })
            .collect::<Option<Vec<_>>>()?;
        let local_edge_by_id = edge_ids
            .iter()
            .copied()
            .enumerate()
            .map(|(local, edge)| (edge, local))
            .collect::<HashMap<_, _>>();
        let local_edge_faces = incidence.map(|(edge_faces, _)| {
            edge_ids
                .iter()
                .map(|edge| edge_faces[*edge])
                .collect::<Vec<_>>()
        });
        let face_edges = incidence.and_then(|(_, boundary_domains)| {
            if budget.is_some_and(|budget| !budget.charge_by(edge_ids.len().max(1))) {
                return None;
            }
            let mut face_edges = vec![Vec::new(); boundary_domains.len()];
            for (edge, faces) in local_edge_faces.as_ref()?.iter().copied().enumerate() {
                for (rank, face) in faces.into_iter().enumerate() {
                    if rank == 0 || face != faces[0] {
                        face_edges[face].push(edge);
                    }
                }
            }
            Some(face_edges)
        });
        if incidence.is_some() && face_edges.is_none() {
            if budget.is_some_and(WorkBudget::exhausted) {
                exhausted.set(true);
            }
            return None;
        }
        let closed_faces = face_incidence_counts.as_ref().map(|counts| {
            face_edges
                .as_ref()
                .expect("incidence face edges accompany incidence counts")
                .iter()
                .zip(counts)
                .map(|(local, total)| local.len() == *total)
                .collect::<Vec<_>>()
        });
        let mut local_domains = component
            .iter()
            .map(|root| domains[*root].clone())
            .collect::<Vec<_>>();
        let mut root_edges = vec![Vec::new(); component.len()];
        for (edge, [left, right]) in local_edges.iter().copied().enumerate() {
            root_edges[left].push(edge);
            if right != left {
                root_edges[right].push(edge);
            }
        }
        if !enforce_sparse_endpoint_membership(
            &mut local_domains,
            &local_edges,
            &edge_ids,
            edge_candidates,
            budget,
        ) {
            if budget.is_some_and(WorkBudget::exhausted) {
                exhausted.set(true);
            }
            return None;
        }
        let mut arc_domains = local_domains.clone();
        let arc_budget = budget.map(|budget| WorkBudget::new(budget.remaining()));
        let arc_consistent = enforce_edge_arc_consistency(
            &mut arc_domains,
            &local_edges,
            &edge_ids,
            &root_edges,
            edge_candidates,
            arc_budget.as_ref(),
        );
        if arc_consistent {
            if let (Some(budget), Some(arc_budget)) = (budget, arc_budget.as_ref()) {
                let work = budget.remaining() - arc_budget.remaining();
                if !budget.charge_by(work) {
                    exhausted.set(true);
                    return None;
                }
            }
            local_domains = arc_domains;
        } else {
            if arc_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                exhausted.set(true);
            }
            return None;
        }
        if local_domains
            .iter()
            .flatten()
            .copied()
            .collect::<HashSet<_>>()
            != component_points
        {
            return None;
        }
        let mut solutions = Vec::new();
        let mut states = 0;
        let mut exhausted = false;
        let mut base_degrees = HashMap::new();
        let mut local_assignment = alloc_filled(
            component.len(),
            None,
            "catia coordinate component assignment",
        )
        .ok()?;
        let mut point_degrees =
            alloc_filled(point_count, 0, "catia coordinate point degrees").ok()?;
        walk(
            &local_domains,
            &local_edges,
            &edge_ids,
            &local_edge_by_id,
            &root_edges,
            edge_candidates,
            local_edge_faces.as_deref(),
            face_edges.as_deref(),
            closed_faces.as_deref(),
            incidence.map(|(_, boundary_domains)| boundary_domains),
            &component_points,
            &mut local_assignment,
            &mut point_degrees,
            &mut solutions,
            &mut states,
            state_limit,
            &mut exhausted,
            &mut base_degrees,
            budget,
        );
        if exhausted {
            if component_budget.as_ref().is_some_and(WorkBudget::exhausted) {
                if let Some(shared_budget) = shared_budget {
                    shared_budget.exhaust();
                }
            }
            return None;
        }
        let [local_assignment] = solutions.as_slice() else {
            if solutions.len() > 1 {
                ambiguous.set(true);
            }
            return None;
        };
        for (&root, &point) in component.iter().zip(local_assignment) {
            assignment[root] = Some(point);
        }
    }
    let assignment = assignment.into_iter().collect::<Option<Vec<_>>>()?;
    for (&root, &point) in roots.iter().zip(&assignment) {
        quotient.domains[root] = Arc::new(HashSet::from([point]));
    }
    let mut root_by_point = HashMap::new();
    for (&root, &point) in roots.iter().zip(&assignment) {
        if let Some(previous) = root_by_point.insert(point, root) {
            let merged = quotient.merge(previous, root)?;
            root_by_point.insert(point, merged);
        }
    }
    if !quotient.edge_domains_viable(edge_candidates) {
        return None;
    }
    quotient.point_assignment(point_count, edge_candidates, None)
}
