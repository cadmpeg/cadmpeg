//! Bipartite distinct-domain matching and coordinate bijection solvers.
//!
//! Pure combinatorics over caller-supplied domains; no byte knowledge.

use crate::solve::mesh_quotient::MeshConstraintBudget;
use std::collections::{HashSet, VecDeque};

pub(crate) fn domains_have_distinct_matching<'a>(
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
) -> bool {
    distinct_domain_matching_with_budget(domains, point_count, None, None).is_some()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchingEdgeConstraint {
    Exclude(usize, usize),
    Require(usize, usize),
}

pub(crate) fn distinct_domain_matching_with_budget<'a>(
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
    budget: Option<&MeshConstraintBudget>,
    edge_constraint: Option<MatchingEdgeConstraint>,
) -> Option<Vec<usize>> {
    let domains = domains.into_iter().collect::<Vec<_>>();
    if domains.len() > point_count {
        return None;
    }
    let mut owner = vec![None; point_count];
    let mut matched = vec![false; domains.len()];
    let mut matched_count = 0usize;
    let mut required_domain = None;
    if let Some(MatchingEdgeConstraint::Require(domain, point)) = edge_constraint {
        if domain >= domains.len() || point >= point_count || !domains[domain].contains(&point) {
            return None;
        }
        owner[point] = Some(domain);
        matched[domain] = true;
        matched_count = 1;
        required_domain = Some(domain);
    }
    while matched_count < domains.len() {
        let mut distance = vec![usize::MAX; domains.len()];
        let mut queue = VecDeque::new();
        for root in 0..domains.len() {
            if !matched[root] {
                distance[root] = 0;
                queue.push_back(root);
            }
        }
        let mut shortest = usize::MAX;
        while let Some(root) = queue.pop_front() {
            if distance[root] >= shortest {
                continue;
            }
            for &point in domains[root] {
                if budget.is_some_and(|budget| !budget.charge()) {
                    return None;
                }
                if edge_constraint == Some(MatchingEdgeConstraint::Exclude(root, point)) {
                    continue;
                }
                if point >= point_count {
                    continue;
                }
                if let Some(next) = owner[point] {
                    if Some(next) != required_domain && distance[next] == usize::MAX {
                        distance[next] = distance[root] + 1;
                        queue.push_back(next);
                    }
                } else {
                    shortest = distance[root];
                }
            }
        }
        if shortest == usize::MAX {
            return None;
        }
        let mut cursor = vec![0usize; domains.len()];
        let mut incoming = vec![None; domains.len()];
        let mut augmented = 0usize;
        for start in 0..domains.len() {
            if matched[start] || distance[start] != 0 {
                continue;
            }
            let mut roots = vec![start];
            let mut free_point = None;
            while let Some(&root) = roots.last() {
                let mut advanced = false;
                while cursor[root] < domains[root].len() {
                    let point = domains[root][cursor[root]];
                    cursor[root] += 1;
                    if budget.is_some_and(|budget| !budget.charge()) {
                        return None;
                    }
                    if edge_constraint == Some(MatchingEdgeConstraint::Exclude(root, point)) {
                        continue;
                    }
                    if point >= point_count {
                        continue;
                    }
                    match owner[point] {
                        None if distance[root] == shortest => {
                            free_point = Some(point);
                            advanced = true;
                            break;
                        }
                        Some(next)
                            if Some(next) != required_domain
                                && distance[next] == distance[root] + 1 =>
                        {
                            incoming[next] = Some(point);
                            roots.push(next);
                            advanced = true;
                            break;
                        }
                        _ => {}
                    }
                }
                if free_point.is_some() {
                    break;
                }
                if !advanced {
                    distance[root] = usize::MAX;
                    roots.pop();
                }
            }
            let Some(mut point) = free_point else {
                continue;
            };
            for (index, &root) in roots.iter().enumerate().rev() {
                owner[point] = Some(root);
                if index != 0 {
                    let previous = incoming[root]?;
                    point = previous;
                }
            }
            matched[start] = true;
            matched_count += 1;
            augmented += 1;
        }
        if augmented == 0 {
            return None;
        }
    }
    let mut assignment = vec![None; domains.len()];
    for (point, domain) in owner.into_iter().enumerate() {
        if let Some(domain) = domain {
            assignment[domain] = Some(point);
        }
    }
    assignment.into_iter().collect()
}

pub(crate) fn unique_coordinate_bijection(
    domains: &[HashSet<usize>],
    points: &[[f64; 3]],
) -> Option<Vec<usize>> {
    fn matching(
        domains: &[Vec<usize>],
        slots_by_class: &[Vec<usize>],
        slot_classes: &[usize],
        forced: Option<(usize, usize)>,
    ) -> Option<Vec<usize>> {
        let mut owner = vec![None; slot_classes.len()];
        let mut order = (0..domains.len()).collect::<Vec<_>>();
        order.sort_unstable_by_key(|vertex| {
            let count = forced
                .filter(|(forced_vertex, _)| forced_vertex == vertex)
                .map_or_else(
                    || {
                        domains[*vertex]
                            .iter()
                            .map(|class| slots_by_class[*class].len())
                            .sum()
                    },
                    |(_, class)| slots_by_class[class].len(),
                );
            (count, *vertex)
        });
        let mut seen_vertices = vec![0usize; domains.len()];
        let mut seen_slots = vec![0usize; slot_classes.len()];
        let mut incoming_slot = vec![None; domains.len()];
        let mut via_vertex = vec![None; slot_classes.len()];
        for (generation, start) in order.into_iter().enumerate() {
            let generation = generation + 1;
            let mut queue = VecDeque::from([start]);
            seen_vertices[start] = generation;
            incoming_slot[start] = None;
            let mut free_slot = None;
            while let Some(vertex) = queue.pop_front() {
                let slots = match forced.filter(|(forced_vertex, _)| *forced_vertex == vertex) {
                    Some((_, class)) => slots_by_class[class].clone(),
                    None => domains[vertex]
                        .iter()
                        .flat_map(|class| slots_by_class[*class].iter().copied())
                        .collect(),
                };
                for slot in slots {
                    if seen_slots[slot] == generation {
                        continue;
                    }
                    seen_slots[slot] = generation;
                    via_vertex[slot] = Some(vertex);
                    let Some(next) = owner[slot] else {
                        free_slot = Some(slot);
                        break;
                    };
                    if seen_vertices[next] != generation {
                        seen_vertices[next] = generation;
                        incoming_slot[next] = Some(slot);
                        queue.push_back(next);
                    }
                }
                if free_slot.is_some() {
                    break;
                }
            }
            let mut slot = free_slot?;
            loop {
                let vertex = via_vertex[slot]?;
                owner[slot] = Some(vertex);
                let Some(previous) = incoming_slot[vertex] else {
                    break;
                };
                slot = previous;
            }
        }
        let mut assignment = vec![None; domains.len()];
        for (slot, vertex) in owner.into_iter().enumerate() {
            assignment[vertex?] = Some(slot_classes[slot]);
        }
        assignment.into_iter().collect()
    }

    if domains.len() != points.len()
        || domains
            .iter()
            .any(|domain| domain.is_empty() || domain.iter().any(|point| *point >= points.len()))
    {
        return None;
    }
    let mut representatives = Vec::<usize>::new();
    let mut point_classes = Vec::with_capacity(points.len());
    for (point, position) in points.iter().enumerate() {
        let class = representatives
            .iter()
            .position(|representative| points[*representative] == *position)
            .unwrap_or_else(|| {
                representatives.push(point);
                representatives.len() - 1
            });
        point_classes.push(class);
    }
    let class_domains = domains
        .iter()
        .map(|domain| {
            let mut classes = domain
                .iter()
                .map(|point| point_classes[*point])
                .collect::<Vec<_>>();
            classes.sort_unstable();
            classes.dedup();
            classes
        })
        .collect::<Vec<_>>();
    let mut capacities = vec![0usize; representatives.len()];
    for class in &point_classes {
        capacities[*class] += 1;
    }
    let mut slot_classes = Vec::with_capacity(points.len());
    let mut slots_by_class = vec![Vec::new(); capacities.len()];
    for (class, capacity) in capacities.into_iter().enumerate() {
        for _ in 0..capacity {
            let slot = slot_classes.len();
            slot_classes.push(class);
            slots_by_class[class].push(slot);
        }
    }
    let classes = matching(&class_domains, &slots_by_class, &slot_classes, None)?;
    for (vertex, domain) in class_domains.iter().enumerate() {
        for &class in domain {
            if class != classes[vertex]
                && matching(
                    &class_domains,
                    &slots_by_class,
                    &slot_classes,
                    Some((vertex, class)),
                )
                .is_some()
            {
                return None;
            }
        }
    }
    let mut available = vec![Vec::new(); representatives.len()];
    for (point, class) in point_classes.into_iter().enumerate() {
        available[class].push(point);
    }
    let mut used = vec![0usize; available.len()];
    Some(
        classes
            .iter()
            .map(|class| {
                let point = available[*class][used[*class]];
                used[*class] += 1;
                point
            })
            .collect(),
    )
}
