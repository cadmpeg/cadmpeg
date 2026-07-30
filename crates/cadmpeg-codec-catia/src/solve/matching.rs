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

pub(crate) fn repair_distinct_domain_matching_with_budget<'a>(
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
    matching: &[usize],
    budget: Option<&MeshConstraintBudget>,
) -> Option<Vec<usize>> {
    let domains = domains.into_iter().collect::<Vec<_>>();
    if domains.len() != matching.len() || domains.len() > point_count {
        return None;
    }
    let mut matching = matching.to_vec();
    let mut owner = vec![None; point_count];
    let mut unmatched = Vec::new();
    for domain in 0..matching.len() {
        let point = matching[domain];
        if point < point_count && domains[domain].contains(&point) && owner[point].is_none() {
            owner[point] = Some(domain);
        } else {
            matching[domain] = usize::MAX;
            unmatched.push(domain);
        }
    }
    for start in unmatched {
        let mut seen_domains = vec![false; domains.len()];
        let mut seen_points = vec![false; point_count];
        let mut incoming_point = vec![None; domains.len()];
        let mut via_domain = vec![None; point_count];
        let mut queue = VecDeque::from([start]);
        seen_domains[start] = true;
        let mut free_point = None;
        while let Some(domain) = queue.pop_front() {
            for &point in domains[domain] {
                if budget.is_some_and(|budget| !budget.charge()) {
                    return None;
                }
                if point >= point_count || seen_points[point] {
                    continue;
                }
                seen_points[point] = true;
                via_domain[point] = Some(domain);
                let Some(next) = owner[point] else {
                    free_point = Some(point);
                    break;
                };
                if !seen_domains[next] {
                    seen_domains[next] = true;
                    incoming_point[next] = Some(point);
                    queue.push_back(next);
                }
            }
            if free_point.is_some() {
                break;
            }
        }
        let mut point = free_point?;
        loop {
            let domain = via_domain[point]?;
            owner[point] = Some(domain);
            matching[domain] = point;
            if domain == start {
                break;
            }
            point = incoming_point[domain]?;
        }
    }
    Some(matching)
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

#[cfg(test)]
mod tests {
    use super::repair_distinct_domain_matching_with_budget;

    #[test]
    fn repairs_matching_after_a_matched_edge_is_removed() {
        let domains = [vec![1], vec![0, 2], vec![0, 1]];
        let repaired = repair_distinct_domain_matching_with_budget(
            domains.iter().map(Vec::as_slice),
            3,
            &[0, 1, 2],
            None,
        )
        .expect("the remaining augmenting path should repair the matching");

        assert_eq!(repaired, vec![1, 2, 0]);
    }

    #[test]
    fn rejects_domains_when_a_removed_edge_cannot_be_repaired() {
        let domains = [vec![0], vec![0]];

        assert!(repair_distinct_domain_matching_with_budget(
            domains.iter().map(Vec::as_slice),
            2,
            &[0, 1],
            None,
        )
        .is_none());
    }
}
