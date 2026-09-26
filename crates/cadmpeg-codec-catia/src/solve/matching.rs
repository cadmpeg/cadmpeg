//! Bipartite distinct-domain matching and coordinate bijection solvers.
//!
//! Pure combinatorics over caller-supplied domains; no byte knowledge.

use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use std::collections::{HashSet, VecDeque};

pub(super) fn domains_have_distinct_matching<'a>(
    ctx: &DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
) -> Result<bool, CodecError> {
    Ok(distinct_domain_matching_with_budget(ctx, domains, point_count, None, None)?.is_some())
}

fn charge_matching_work(
    ctx: &DecodeContext<'_>,
    budget: Option<&WorkBudget<'_>>,
) -> Result<(), CodecError> {
    if budget.is_some_and(|budget| !budget.charge()) {
        return Err(ctx.refuse_codec_limit("catia matching work", 0, 1));
    }
    ctx.charge_work(1, "catia matching work")
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchingEdgeConstraint {
    Exclude(usize, usize),
    Require(usize, usize),
}

pub(crate) fn distinct_domain_matching_with_budget<'a>(
    ctx: &DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
    budget: Option<&WorkBudget<'_>>,
    edge_constraint: Option<MatchingEdgeConstraint>,
) -> Result<Option<Vec<usize>>, CodecError> {
    let domains = domains.into_iter().collect::<Vec<_>>();
    if domains.len() > point_count {
        return Ok(None);
    }
    let mut owner = ctx.alloc_filled(point_count, None, "catia_match_owners")?;
    let mut matched = ctx.alloc_filled(domains.len(), false, "catia_match_flags")?;
    let mut matched_count = 0usize;
    let mut required_domain = None;
    if let Some(MatchingEdgeConstraint::Require(domain, point)) = edge_constraint {
        if domain >= domains.len() || point >= point_count || !domains[domain].contains(&point) {
            return Ok(None);
        }
        owner[point] = Some(domain);
        matched[domain] = true;
        matched_count = 1;
        required_domain = Some(domain);
    }
    while matched_count < domains.len() {
        let mut distance = ctx.alloc_filled(domains.len(), None, "catia_match_distance")?;
        let mut queue = VecDeque::new();
        for root in 0..domains.len() {
            if !matched[root] {
                distance[root] = Some(0);
                queue.push_back(root);
            }
        }
        let mut shortest = None;
        while let Some(root) = queue.pop_front() {
            let Some(root_distance) = distance[root] else {
                continue;
            };
            if shortest.is_some_and(|bound| root_distance >= bound) {
                continue;
            }
            for &point in domains[root] {
                charge_matching_work(ctx, budget)?;
                if edge_constraint == Some(MatchingEdgeConstraint::Exclude(root, point)) {
                    continue;
                }
                if point >= point_count {
                    continue;
                }
                if let Some(next) = owner[point] {
                    if Some(next) != required_domain && distance[next].is_none() {
                        distance[next] = Some(root_distance + 1);
                        queue.push_back(next);
                    }
                } else {
                    shortest = Some(root_distance);
                }
            }
        }
        let Some(shortest) = shortest else {
            return Ok(None);
        };
        let mut cursor = ctx.alloc_filled(domains.len(), 0usize, "catia_match_cursor")?;
        let mut incoming = ctx.alloc_filled(domains.len(), None, "catia_match_incoming")?;
        let mut augmented = 0usize;
        for start in 0..domains.len() {
            if matched[start] || distance[start] != Some(0) {
                continue;
            }
            let mut roots = vec![start];
            let mut free_point = None;
            while let Some(&root) = roots.last() {
                let mut advanced = false;
                let root_distance = distance[root];
                while cursor[root] < domains[root].len() {
                    let point = domains[root][cursor[root]];
                    cursor[root] += 1;
                    charge_matching_work(ctx, budget)?;
                    if edge_constraint == Some(MatchingEdgeConstraint::Exclude(root, point)) {
                        continue;
                    }
                    if point >= point_count {
                        continue;
                    }
                    match owner[point] {
                        None if root_distance == Some(shortest) => {
                            free_point = Some(point);
                            advanced = true;
                            break;
                        }
                        Some(next)
                            if Some(next) != required_domain
                                && root_distance
                                    .is_some_and(|value| distance[next] == Some(value + 1)) =>
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
                    distance[root] = None;
                    roots.pop();
                }
            }
            let Some(mut point) = free_point else {
                continue;
            };
            for (index, &root) in roots.iter().enumerate().rev() {
                owner[point] = Some(root);
                if index != 0 {
                    let Some(previous) = incoming[root] else {
                        return Ok(None);
                    };
                    point = previous;
                }
            }
            matched[start] = true;
            matched_count += 1;
            augmented += 1;
        }
        if augmented == 0 {
            return Ok(None);
        }
    }
    let mut assignment = ctx.alloc_filled(domains.len(), None, "catia_match_assignment")?;
    for (point, domain) in owner.into_iter().enumerate() {
        if let Some(domain) = domain {
            assignment[domain] = Some(point);
        }
    }
    Ok(assignment.into_iter().collect())
}

pub(super) fn repair_distinct_domain_matching_with_budget<'a>(
    ctx: &DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
    matching: &[usize],
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<Vec<usize>>, CodecError> {
    let domains = domains.into_iter().collect::<Vec<_>>();
    if domains.len() != matching.len() || domains.len() > point_count {
        return Ok(None);
    }
    let mut owner = ctx.alloc_filled(point_count, None, "catia_match_repair_owners")?;
    let mut unmatched = Vec::new();
    let mut repaired = Vec::with_capacity(matching.len());
    for (domain, &point) in matching.iter().enumerate() {
        if point < point_count && domains[domain].contains(&point) && owner[point].is_none() {
            owner[point] = Some(domain);
            repaired.push(Some(point));
        } else {
            repaired.push(None);
            unmatched.push(domain);
        }
    }
    for start in unmatched {
        let mut seen_domains =
            ctx.alloc_filled(domains.len(), false, "catia_match_repair_seen_domains")?;
        let mut seen_points =
            ctx.alloc_filled(point_count, false, "catia_match_repair_seen_points")?;
        let mut incoming_point =
            ctx.alloc_filled(domains.len(), None, "catia_match_repair_incoming")?;
        let mut via_domain = ctx.alloc_filled(point_count, None, "catia_match_repair_via")?;
        let mut queue = VecDeque::from([start]);
        seen_domains[start] = true;
        let mut free_point = None;
        while let Some(domain) = queue.pop_front() {
            for &point in domains[domain] {
                charge_matching_work(ctx, budget)?;
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
        let Some(mut point) = free_point else {
            return Ok(None);
        };
        loop {
            let Some(domain) = via_domain[point] else {
                return Ok(None);
            };
            owner[point] = Some(domain);
            repaired[domain] = Some(point);
            if domain == start {
                break;
            }
            let Some(previous) = incoming_point[domain] else {
                return Ok(None);
            };
            point = previous;
        }
    }
    Ok(repaired.into_iter().collect())
}

pub(crate) fn retain_distinct_matching_supports(
    ctx: &DecodeContext<'_>,
    domains: &mut [Vec<usize>],
    point_count: usize,
    matching: &[usize],
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<bool>, CodecError> {
    if domains.len() != matching.len()
        || domains.len() > point_count
        || matching.iter().any(|point| *point >= point_count)
    {
        return Ok(None);
    }
    let Some(node_count) = domains.len().checked_add(point_count) else {
        return Ok(None);
    };
    let mut graph = ctx.alloc_filled(node_count, Vec::new(), "catia_match_support_graph")?;
    let mut reverse = ctx.alloc_filled(node_count, Vec::new(), "catia_match_support_reverse")?;
    let mut matched_points = ctx.alloc_filled(point_count, false, "catia_match_support_points")?;
    for (domain, values) in domains.iter().enumerate() {
        if !values.contains(&matching[domain]) || matched_points[matching[domain]] {
            return Ok(None);
        }
        matched_points[matching[domain]] = true;
        for &point in values {
            if point >= point_count {
                return Ok(None);
            }
            charge_matching_work(ctx, budget)?;
            let point_node = domains.len() + point;
            let (from, to) = if point == matching[domain] {
                (point_node, domain)
            } else {
                (domain, point_node)
            };
            graph[from].push(to);
            reverse[to].push(from);
        }
    }

    let mut visited = ctx.alloc_filled(node_count, false, "catia_match_support_visit")?;
    let mut finish_order = Vec::with_capacity(node_count);
    for start in 0..node_count {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![(start, 0usize)];
        while let Some((node, edge_index)) = stack.pop() {
            if let Some(&next) = graph[node].get(edge_index) {
                stack.push((node, edge_index + 1));
                charge_matching_work(ctx, budget)?;
                if !visited[next] {
                    visited[next] = true;
                    stack.push((next, 0));
                }
            } else {
                finish_order.push(node);
            }
        }
    }

    let mut component = ctx.alloc_filled(node_count, None, "catia_match_support_components")?;
    let mut component_count = 0usize;
    for &start in finish_order.iter().rev() {
        if component[start].is_some() {
            continue;
        }
        component[start] = Some(component_count);
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for &next in &reverse[node] {
                charge_matching_work(ctx, budget)?;
                if component[next].is_none() {
                    component[next] = Some(component_count);
                    stack.push(next);
                }
            }
        }
        component_count += 1;
    }

    let mut reaches_free = ctx.alloc_filled(node_count, false, "catia_match_support_free")?;
    let mut queue = VecDeque::new();
    for (point, matched) in matched_points.into_iter().enumerate() {
        if !matched {
            let node = domains.len() + point;
            reaches_free[node] = true;
            queue.push_back(node);
        }
    }
    while let Some(node) = queue.pop_front() {
        for &previous in &reverse[node] {
            charge_matching_work(ctx, budget)?;
            if !reaches_free[previous] {
                reaches_free[previous] = true;
                queue.push_back(previous);
            }
        }
    }

    let domain_count = domains.len();
    let mut changed = false;
    for (domain, values) in domains.iter_mut().enumerate() {
        let before = values.len();
        values.retain(|point| {
            *point == matching[domain]
                || component[domain] == component[domain_count + *point]
                || reaches_free[domain_count + *point]
        });
        changed |= values.len() != before;
    }
    Ok(Some(changed))
}

pub(crate) fn unique_coordinate_bijection(
    ctx: &DecodeContext<'_>,
    domains: &[HashSet<usize>],
    points: &[[f64; 3]],
) -> Result<Option<Vec<usize>>, CodecError> {
    fn matching(
        ctx: &DecodeContext<'_>,
        domains: &[Vec<usize>],
        slots_by_class: &[Vec<usize>],
        slot_classes: &[usize],
        forced: Option<(usize, usize)>,
    ) -> Result<Option<Vec<usize>>, CodecError> {
        let mut owner = ctx.alloc_filled(slot_classes.len(), None, "catia_bijection_owners")?;
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
        let mut seen_vertices =
            ctx.alloc_filled(domains.len(), 0usize, "catia_bijection_seen_vertices")?;
        let mut seen_slots =
            ctx.alloc_filled(slot_classes.len(), 0usize, "catia_bijection_seen_slots")?;
        let mut incoming_slot =
            ctx.alloc_filled(domains.len(), None, "catia_bijection_incoming")?;
        let mut via_vertex = ctx.alloc_filled(slot_classes.len(), None, "catia_bijection_via")?;
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
            let Some(mut slot) = free_slot else {
                return Ok(None);
            };
            loop {
                let Some(vertex) = via_vertex[slot] else {
                    return Ok(None);
                };
                owner[slot] = Some(vertex);
                let Some(previous) = incoming_slot[vertex] else {
                    break;
                };
                slot = previous;
            }
        }
        let mut assignment = ctx.alloc_filled(domains.len(), None, "catia_bijection_assignment")?;
        for (slot, vertex) in owner.into_iter().enumerate() {
            let Some(vertex) = vertex else {
                return Ok(None);
            };
            assignment[vertex] = Some(slot_classes[slot]);
        }
        Ok(assignment.into_iter().collect())
    }

    if domains.len() != points.len()
        || domains
            .iter()
            .any(|domain| domain.is_empty() || domain.iter().any(|point| *point >= points.len()))
    {
        return Ok(None);
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
    let mut capacities =
        ctx.alloc_filled(representatives.len(), 0usize, "catia_bijection_capacities")?;
    for class in &point_classes {
        capacities[*class] += 1;
    }
    let mut slot_classes = Vec::with_capacity(points.len());
    let mut slots_by_class =
        ctx.alloc_filled(capacities.len(), Vec::new(), "catia_bijection_slots")?;
    for (class, capacity) in capacities.into_iter().enumerate() {
        for _ in 0..capacity {
            let slot = slot_classes.len();
            slot_classes.push(class);
            slots_by_class[class].push(slot);
        }
    }
    let Some(classes) = matching(ctx, &class_domains, &slots_by_class, &slot_classes, None)? else {
        return Ok(None);
    };
    for (vertex, domain) in class_domains.iter().enumerate() {
        for &class in domain {
            if class != classes[vertex]
                && matching(
                    ctx,
                    &class_domains,
                    &slots_by_class,
                    &slot_classes,
                    Some((vertex, class)),
                )?
                .is_some()
            {
                return Ok(None);
            }
        }
    }
    let mut available = ctx.alloc_filled(
        representatives.len(),
        Vec::new(),
        "catia_bijection_available",
    )?;
    for (point, class) in point_classes.into_iter().enumerate() {
        available[class].push(point);
    }
    let mut used = ctx.alloc_filled(available.len(), 0usize, "catia_bijection_used")?;
    Ok(Some(
        classes
            .iter()
            .map(|class| {
                let point = available[*class][used[*class]];
                used[*class] += 1;
                point
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    use super::{
        distinct_domain_matching_with_budget, repair_distinct_domain_matching_with_budget,
        retain_distinct_matching_supports, MatchingEdgeConstraint,
    };

    #[test]
    fn repairs_matching_after_a_matched_edge_is_removed() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::service())
            .expect("matching fixture fits the service profile");
        let domains = [vec![1], vec![0, 2], vec![0, 1]];
        let repaired = repair_distinct_domain_matching_with_budget(
            &ctx,
            domains.iter().map(Vec::as_slice),
            3,
            &[0, 1, 2],
            None,
        )
        .expect("matching repair fits the service profile")
        .expect("the remaining augmenting path should repair the matching");

        assert_eq!(repaired, vec![1, 2, 0]);
    }

    #[test]
    fn rejects_domains_when_a_removed_edge_cannot_be_repaired() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::service())
            .expect("matching fixture fits the service profile");
        let domains = [vec![0], vec![0]];

        assert!(repair_distinct_domain_matching_with_budget(
            &ctx,
            domains.iter().map(Vec::as_slice),
            2,
            &[0, 1],
            None,
        )
        .expect("matching repair fits the service profile")
        .is_none());
    }

    #[test]
    fn matching_supports_retain_alternating_cycles_and_paths_to_free_points() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::service())
            .expect("matching fixture fits the service profile");
        let mut domains = [vec![0, 1], vec![0, 2]];

        assert_eq!(
            retain_distinct_matching_supports(&ctx, &mut domains, 3, &[1, 0], None)
                .expect("matching supports fit the service profile"),
            Some(false)
        );
        assert_eq!(domains, [vec![0, 1], vec![0, 2]]);
    }

    #[test]
    fn matching_supports_remove_edges_outside_every_complete_matching() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::service())
            .expect("matching fixture fits the service profile");
        let mut domains = [vec![0, 1], vec![0]];

        assert_eq!(
            retain_distinct_matching_supports(&ctx, &mut domains, 2, &[1, 0], None)
                .expect("matching supports fit the service profile"),
            Some(true)
        );
        assert_eq!(domains, [vec![1], vec![0]]);
    }

    #[test]
    fn matching_support_pruning_matches_forced_edge_search() {
        const POINT_COUNT: usize = 4;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::service())
            .expect("matching fixture fits the service profile");
        for first_mask in 1u8..1 << POINT_COUNT {
            for second_mask in 1u8..1 << POINT_COUNT {
                for third_mask in 1u8..1 << POINT_COUNT {
                    let original = [first_mask, second_mask, third_mask].map(|mask| {
                        (0..POINT_COUNT)
                            .filter(|point| mask & (1 << point) != 0)
                            .collect::<Vec<_>>()
                    });
                    let Some(matching) = distinct_domain_matching_with_budget(
                        &ctx,
                        original.iter().map(Vec::as_slice),
                        POINT_COUNT,
                        None,
                        None,
                    )
                    .expect("matching search fits the service profile") else {
                        continue;
                    };
                    let mut pruned = original.clone();
                    retain_distinct_matching_supports(
                        &ctx,
                        &mut pruned,
                        POINT_COUNT,
                        &matching,
                        None,
                    )
                    .expect("matching supports fit the service profile")
                    .expect("valid matching");
                    for (domain, values) in original.iter().enumerate() {
                        for &point in values {
                            let supported = distinct_domain_matching_with_budget(
                                &ctx,
                                original.iter().map(Vec::as_slice),
                                POINT_COUNT,
                                None,
                                Some(MatchingEdgeConstraint::Require(domain, point)),
                            )
                            .expect("matching search fits the service profile")
                            .is_some();
                            assert_eq!(
                                pruned[domain].contains(&point),
                                supported,
                                "domains={original:?}, edge=({domain}, {point})"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn matching_owner_allocation_refuses_at_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let point_count = usize::MAX;
        policy.limits.max_collection_items =
            u64::try_from(point_count).expect("pointer width fits u64") - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("matching fixture fits the input limit");
        let domains = [vec![0]];
        let error = distinct_domain_matching_with_budget(
            &ctx,
            domains.iter().map(Vec::as_slice),
            point_count,
            None,
            None,
        )
        .expect_err("the owner array exceeds the collection limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia_match_owners"));
    }
}
