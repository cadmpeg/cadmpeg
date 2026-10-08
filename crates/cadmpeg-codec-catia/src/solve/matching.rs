//! Bipartite distinct-domain matching and coordinate bijection solvers.
//!
//! Pure combinatorics over caller-supplied domains; no byte knowledge. Search
//! state is solver scratch held in scoped storage for one call; a returned
//! matching is charged to the caller's storage.

use cadmpeg_core::decode::iter_source::IterSource;
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub(super) fn domains_have_distinct_matching<'a>(
    ctx: &DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
) -> Result<bool, CodecError> {
    let (mates, _storage) = ctx.with_scoped_storage("catia_match_scratch", || {
        distinct_domain_mates(ctx, domains, point_count, None, None)
    })?;
    Ok(mates.is_some())
}

/// Charges one domain-point visit to the session and, when present, to the
/// caller's local search budget.
fn charge_matching_work(
    ctx: &DecodeContext<'_>,
    budget: Option<&WorkBudget<'_>>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "catia matching work";
    if budget.is_some_and(|budget| budget.remaining() == 0) {
        return Err(ctx.refuse_codec_limit(OPERATION, 0, 1));
    }
    // The attached unit charges the session once; the caller's budget then
    // takes the same unit locally without charging the session again.
    let visit = ctx.work_budget(1);
    if !visit.charge() {
        return Err(ctx
            .resource_refusal()
            .map_or_else(|| ctx.refuse_codec_limit(OPERATION, 0, 1), Into::into));
    }
    if budget.is_some_and(|budget| budget.consume_child(&visit).is_err()) {
        return Err(ctx.refuse_codec_limit(OPERATION, 0, 1));
    }
    Ok(())
}

/// Copies a complete per-domain assignment into the caller's storage.
fn complete_assignment(
    ctx: &DecodeContext<'_>,
    mates: &[Option<usize>],
    operation: &'static str,
) -> Result<Option<Vec<usize>>, CodecError> {
    let mut completed = ctx.collection_vec(mates.len(), operation)?;
    for mate in ctx.admit_iter(mates, operation)? {
        let Some(point) = *mate else {
            return Ok(None);
        };
        completed.push(point);
    }
    Ok(Some(completed))
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
    let mut scratch = ctx.reserve_scoped(0, "catia_match_scratch")?;
    let Some(mates) = scratch.with_storage(|| {
        distinct_domain_mates(ctx, domains, point_count, budget, edge_constraint)
    })?
    else {
        return Ok(None);
    };
    complete_assignment(ctx, &mates, "catia_match_completed")
}

/// Hopcroft-Karp phases: one layered search from every free domain, then
/// disjoint shortest augmenting paths. Returns the matched point of each domain.
fn distinct_domain_mates<'a>(
    ctx: &DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
    budget: Option<&WorkBudget<'_>>,
    edge_constraint: Option<MatchingEdgeConstraint>,
) -> Result<Option<Vec<Option<usize>>>, CodecError> {
    let domains = ctx.collect_vec(domains, "catia_match_domains")?;
    let count = domains.len();
    if count > point_count {
        return Ok(None);
    }
    let mut owner = ctx.alloc_filled(point_count, None, "catia_match_owners")?;
    let mut mates = ctx.alloc_filled(count, None, "catia_match_mates")?;
    let mut matched_count = 0usize;
    let mut required_domain = None;
    if let Some(MatchingEdgeConstraint::Require(domain, point)) = edge_constraint {
        if domain >= count
            || point >= point_count
            || !ctx.contains(domains[domain], &point, "catia_match_required_edge")?
        {
            return Ok(None);
        }
        owner[point] = Some(domain);
        mates[domain] = Some(point);
        matched_count = 1;
        required_domain = Some(domain);
    }
    // A phase queues each domain at most once, and an augmenting path holds
    // one domain per distance layer, so both fit one slot per domain.
    let mut distance = ctx.alloc_filled(count, None, "catia_match_distance")?;
    let mut queue = ctx.alloc_filled(count, 0usize, "catia_match_queue")?;
    let mut cursor = ctx.alloc_filled(count, 0usize, "catia_match_cursor")?;
    let mut path = ctx.alloc_filled(count, 0usize, "catia_match_augmenting_roots")?;
    while matched_count < count {
        let mut queued = 0usize;
        for root in ctx.admit_iter(&(0..count), "catia_match_phase_roots")? {
            cursor[root] = 0;
            distance[root] = if mates[root].is_none() {
                queue[queued] = root;
                queued += 1;
                Some(0)
            } else {
                None
            };
        }
        let mut head = 0usize;
        let mut shortest = None;
        while head < queued {
            ctx.charge_work(1, "catia_matching_iteration")?;
            let root = queue[head];
            head += 1;
            let Some(root_distance) = distance[root] else {
                continue;
            };
            if shortest.is_some_and(|bound| root_distance >= bound) {
                continue;
            }
            for &point in domains[root] {
                charge_matching_work(ctx, budget)?;
                if edge_constraint == Some(MatchingEdgeConstraint::Exclude(root, point))
                    || point >= point_count
                {
                    continue;
                }
                match owner[point] {
                    Some(next) => {
                        if Some(next) != required_domain && distance[next].is_none() {
                            distance[next] = Some(root_distance + 1);
                            queue[queued] = next;
                            queued += 1;
                        }
                    }
                    None => shortest = Some(root_distance),
                }
            }
        }
        let Some(shortest) = shortest else {
            return Ok(None);
        };
        let mut augmented = false;
        for start in ctx.admit_iter(&(0..count), "catia_match_augmenting_starts")? {
            if mates[start].is_some() || distance[start] != Some(0) {
                continue;
            }
            path[0] = start;
            let mut depth = 1usize;
            let mut free_point = None;
            while let Some(&root) = depth.checked_sub(1).and_then(|top| path.get(top)) {
                ctx.charge_work(1, "catia_matching_iteration")?;
                let mut advanced = false;
                let root_distance = distance[root];
                while cursor[root] < domains[root].len() {
                    charge_matching_work(ctx, budget)?;
                    let point = domains[root][cursor[root]];
                    cursor[root] += 1;
                    if edge_constraint == Some(MatchingEdgeConstraint::Exclude(root, point))
                        || point >= point_count
                    {
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
                            path[depth] = next;
                            depth += 1;
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
                    depth -= 1;
                }
            }
            let Some(mut point) = free_point else {
                continue;
            };
            // Each path domain takes the point the search reached it by and
            // hands its previous point to the domain before it.
            for index in ctx
                .admit_iter(&(0..depth), "catia_match_augmenting_path")?
                .rev()
            {
                let root = path[index];
                owner[point] = Some(root);
                let previous = mates[root].replace(point);
                if index != 0 {
                    let Some(previous) = previous else {
                        return Ok(None);
                    };
                    point = previous;
                }
            }
            matched_count += 1;
            augmented = true;
        }
        if !augmented {
            return Ok(None);
        }
    }
    Ok(Some(mates))
}

pub(super) fn repair_distinct_domain_matching_with_budget<'a>(
    ctx: &DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
    matching: &[usize],
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<Vec<usize>>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_match_repair_scratch")?;
    let Some(mates) = scratch
        .with_storage(|| repaired_domain_mates(ctx, domains, point_count, matching, budget))?
    else {
        return Ok(None);
    };
    complete_assignment(ctx, &mates, "catia_match_repair_completed")
}

/// Keeps every still-valid matched edge and reroutes each other domain along
/// one breadth-first augmenting path.
fn repaired_domain_mates<'a>(
    ctx: &DecodeContext<'_>,
    domains: impl IntoIterator<Item = &'a [usize]>,
    point_count: usize,
    matching: &[usize],
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<Vec<Option<usize>>>, CodecError> {
    let domains = ctx.collect_vec(domains, "catia_match_repair_domains")?;
    let count = domains.len();
    if count != matching.len() || count > point_count {
        return Ok(None);
    }
    let mut owner = ctx.alloc_filled(point_count, None, "catia_match_repair_owners")?;
    let mut mates = ctx.alloc_filled(count, None, "catia_match_repaired")?;
    let mut unmatched = Vec::new();
    for (domain, &point) in ctx
        .admit_iter(matching, "catia_match_repair_seeds")?
        .enumerate()
    {
        if point < point_count
            && owner[point].is_none()
            && ctx.contains(domains[domain], &point, "catia_match_repair_seeds")?
        {
            owner[point] = Some(domain);
            mates[domain] = Some(point);
        } else {
            ctx.push_vec(&mut unmatched, domain, "catia_match_unmatched")?;
        }
    }
    // Each search stamps its visits with its own generation, so one set of
    // marks serves every search; a search queues each domain at most once.
    let mut seen_domains = ctx.alloc_filled(count, 0usize, "catia_match_repair_seen_domains")?;
    let mut seen_points =
        ctx.alloc_filled(point_count, 0usize, "catia_match_repair_seen_points")?;
    let mut via_domain = ctx.alloc_filled(point_count, 0usize, "catia_match_repair_via")?;
    let mut queue = ctx.alloc_filled(count, 0usize, "catia_match_repair_queue")?;
    for (search, &start) in ctx
        .admit_iter(&unmatched, "catia_match_repair_starts")?
        .enumerate()
    {
        let generation = search + 1;
        seen_domains[start] = generation;
        queue[0] = start;
        let mut queued = 1usize;
        let mut head = 0usize;
        let mut free_point = None;
        'search: while head < queued {
            ctx.charge_work(1, "catia_matching_iteration")?;
            let domain = queue[head];
            head += 1;
            for &point in domains[domain] {
                charge_matching_work(ctx, budget)?;
                if point >= point_count || seen_points[point] == generation {
                    continue;
                }
                seen_points[point] = generation;
                via_domain[point] = domain;
                let Some(next) = owner[point] else {
                    free_point = Some(point);
                    break 'search;
                };
                if seen_domains[next] != generation {
                    seen_domains[next] = generation;
                    queue[queued] = next;
                    queued += 1;
                }
            }
        }
        let Some(mut point) = free_point else {
            return Ok(None);
        };
        loop {
            ctx.charge_work(1, "catia_matching_iteration")?;
            let domain = via_domain[point];
            owner[point] = Some(domain);
            let previous = mates[domain].replace(point);
            if domain == start {
                break;
            }
            let Some(previous) = previous else {
                return Ok(None);
            };
            point = previous;
        }
    }
    Ok(Some(mates))
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
        || ctx.any_by(
            matching,
            |point| Ok(*point >= point_count),
            "catia_match_support_validation",
        )?
    {
        return Ok(None);
    }
    let Some(node_count) = domains.len().checked_add(point_count) else {
        return Ok(None);
    };
    let mut scratch = ctx.reserve_scoped(0, "catia_match_support_scratch")?;
    let Some((component, reaches_free)) = scratch.with_storage(|| {
        matching_support_classes(ctx, domains, point_count, node_count, matching, budget)
    })?
    else {
        return Ok(None);
    };
    let domain_count = domains.len();
    let mut changed = false;
    for (domain, values) in ctx
        .admit_iter(&mut *domains, "catia_match_support_retain")?
        .enumerate()
    {
        let before = values.len();
        ctx.retain_vec(
            values,
            |point| {
                Ok(*point == matching[domain]
                    || component[domain] == component[domain_count + *point]
                    || reaches_free[domain_count + *point])
            },
            "catia_match_support_retain",
        )?;
        changed |= values.len() != before;
    }
    Ok(Some(changed))
}

/// Strongly connected components of the alternating graph of a complete
/// matching, and the nodes that reach an unmatched point. An unmatched edge
/// lies in some complete matching exactly when its domain and point share a
/// component or its point reaches an unmatched point.
fn matching_support_classes(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<usize>],
    point_count: usize,
    node_count: usize,
    matching: &[usize],
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<(Vec<Option<usize>>, Vec<bool>)>, CodecError> {
    let mut graph =
        ctx.collect_indexed_vec(node_count, "catia_match_support_graph", |_| Ok(Vec::new()))?;
    let mut reverse =
        ctx.collect_indexed_vec(
            node_count,
            "catia_match_support_reverse",
            |_| Ok(Vec::new()),
        )?;
    let mut matched_points = ctx.alloc_filled(point_count, false, "catia_match_support_points")?;
    for (domain, values) in ctx
        .admit_iter(domains, "catia_match_support_domains")?
        .enumerate()
    {
        let matched = matching[domain];
        if matched_points[matched]
            || !ctx.contains(values, &matched, "catia_match_support_domains")?
        {
            return Ok(None);
        }
        matched_points[matched] = true;
        for &point in values {
            charge_matching_work(ctx, budget)?;
            if point >= point_count {
                return Ok(None);
            }
            let point_node = domains.len() + point;
            let (from, to) = if point == matched {
                (point_node, domain)
            } else {
                (domain, point_node)
            };
            ctx.push_vec(&mut graph[from], to, "catia_match_support_arcs")?;
            ctx.push_vec(&mut reverse[to], from, "catia_match_support_reverse_arcs")?;
        }
    }

    let mut visited = ctx.alloc_filled(node_count, false, "catia_match_support_visit")?;
    let mut finish_order = Vec::new();
    let mut stack = Vec::<(usize, usize)>::new();
    for start in ctx.admit_iter(&(0..node_count), "catia_match_support_starts")? {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        ctx.push_vec(&mut stack, (start, 0usize), "catia_match_support_stack")?;
        while let Some(top) = stack.last_mut() {
            ctx.charge_work(1, "catia_matching_iteration")?;
            let (node, edge_index) = *top;
            if let Some(&next) = graph[node].get(edge_index) {
                top.1 += 1;
                charge_matching_work(ctx, budget)?;
                if !visited[next] {
                    visited[next] = true;
                    ctx.push_vec(&mut stack, (next, 0), "catia_match_support_stack")?;
                }
            } else {
                stack.pop();
                ctx.push_vec(&mut finish_order, node, "catia_match_support_finish_order")?;
            }
        }
    }

    let mut component = ctx.alloc_filled(node_count, None, "catia_match_support_components")?;
    let mut component_count = 0usize;
    let mut members = Vec::new();
    for &start in ctx
        .admit_iter(&finish_order, "catia_match_support_finish_order")?
        .rev()
    {
        if component[start].is_some() {
            continue;
        }
        component[start] = Some(component_count);
        ctx.push_vec(&mut members, start, "catia_match_support_component_stack")?;
        while let Some(node) = members.pop() {
            ctx.charge_work(1, "catia_matching_iteration")?;
            for &next in &reverse[node] {
                charge_matching_work(ctx, budget)?;
                if component[next].is_none() {
                    component[next] = Some(component_count);
                    ctx.push_vec(&mut members, next, "catia_match_support_component_stack")?;
                }
            }
        }
        component_count += 1;
    }

    let mut reaches_free = ctx.alloc_filled(node_count, false, "catia_match_support_free")?;
    let mut pending = Vec::new();
    for (point, &matched) in ctx
        .admit_iter(&matched_points, "catia_match_support_free_points")?
        .enumerate()
    {
        if !matched {
            let node = domains.len() + point;
            reaches_free[node] = true;
            ctx.push_vec(&mut pending, node, "catia_match_support_free_queue")?;
        }
    }
    while let Some(node) = pending.pop() {
        ctx.charge_work(1, "catia_matching_iteration")?;
        for &previous in &reverse[node] {
            charge_matching_work(ctx, budget)?;
            if !reaches_free[previous] {
                reaches_free[previous] = true;
                ctx.push_vec(&mut pending, previous, "catia_match_support_free_queue")?;
            }
        }
    }
    Ok(Some((component, reaches_free)))
}

/// Assigns each vertex one row of `points` drawn from its domain, returning
/// the assignment only when the induced assignment of coordinates is the only
/// one possible. Rows with equal coordinates are interchangeable; within such
/// a class rows are handed out in vertex order. The result does not depend on
/// the order in which a domain yields its rows.
pub(crate) fn unique_coordinate_bijection<D>(
    ctx: &DecodeContext<'_>,
    domains: &[D],
    points: &[[f64; 3]],
) -> Result<Option<Vec<usize>>, CodecError>
where
    for<'d> &'d D: IterSource + IntoIterator<Item = &'d usize>,
{
    if domains.len() != points.len() {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "catia_bijection_scratch")?;
    let Some(classes) = scratch.with_storage(|| unique_coordinate_classes(ctx, domains, points))?
    else {
        return Ok(None);
    };
    let mut used = scratch.with_storage(|| {
        ctx.alloc_filled(classes.class_points.len(), 0usize, "catia_bijection_used")
    })?;
    let mut assigned =
        ctx.collection_vec(classes.vertex_classes.len(), "catia_bijection_points")?;
    for &class in ctx.admit_iter(&classes.vertex_classes, "catia_bijection_points")? {
        assigned.push(classes.class_points[class][used[class]]);
        used[class] += 1;
    }
    Ok(Some(assigned))
}

/// Coordinate classes of a unique bijection: the class of each vertex and the
/// rows of each class in row order.
struct CoordinateClasses {
    vertex_classes: Vec<usize>,
    class_points: Vec<Vec<usize>>,
}

/// Equal coordinates compare equal as `f64` values: signed zeros match and a
/// NaN matches nothing.
fn coordinate_key(point: [f64; 3]) -> Option<[u64; 3]> {
    if point.iter().any(|value| value.is_nan()) {
        return None;
    }
    Some(point.map(|value| if value == 0.0 { 0 } else { value.to_bits() }))
}

/// Assigns vertices to coordinate classes within class capacity, then proves
/// the class assignment unique.
///
/// Another complete assignment differs from this one on a set of vertices
/// whose old and new classes form a balanced, hence cyclic, multigraph of
/// arcs `old class -> new class`. Each such arc joins a vertex's class to
/// another class of its domain. Conversely, a cycle of such arcs moves each
/// of its vertices to the next class and keeps every class load. The
/// assignment is therefore unique exactly when those arcs are acyclic.
fn unique_coordinate_classes<D>(
    ctx: &DecodeContext<'_>,
    domains: &[D],
    points: &[[f64; 3]],
) -> Result<Option<CoordinateClasses>, CodecError>
where
    for<'d> &'d D: IterSource + IntoIterator<Item = &'d usize>,
{
    let count = points.len();
    let mut point_classes = ctx.collection_vec(count, "catia_bijection_point_classes")?;
    let mut class_points = Vec::<Vec<usize>>::new();
    let mut classes_by_key = BTreeMap::new();
    for (point, position) in ctx
        .admit_iter(points, "catia_bijection_point_classes")?
        .enumerate()
    {
        let next = class_points.len();
        let class = match coordinate_key(*position) {
            Some(key) => *ctx
                .entry_btree_map(
                    &mut classes_by_key,
                    key,
                    "catia_bijection_coordinate_classes",
                )?
                .or_insert(next),
            None => next,
        };
        if class == next {
            ctx.push_vec(&mut class_points, Vec::new(), "catia_bijection_classes")?;
        }
        ctx.push_vec(
            &mut class_points[class],
            point,
            "catia_bijection_class_points",
        )?;
        point_classes.push(class);
    }
    let class_count = class_points.len();

    // Class domains in first-reference order; a vertex stamp drops repeats.
    let mut stamps = ctx.alloc_filled(class_count, 0usize, "catia_bijection_class_stamps")?;
    let mut class_domains = ctx.collection_vec(count, "catia_bijection_class_domains")?;
    for (vertex, domain) in ctx
        .admit_iter(domains, "catia_bijection_domains")?
        .enumerate()
    {
        let mut classes = Vec::new();
        let mut values = domain.into_iter();
        while let Some(&point) = ctx.next_charged(&mut values, "catia_bijection_domain_classes")? {
            let Some(&class) = point_classes.get(point) else {
                return Ok(None);
            };
            if stamps[class] != vertex + 1 {
                stamps[class] = vertex + 1;
                ctx.push_vec(&mut classes, class, "catia_bijection_domain_classes")?;
            }
        }
        if classes.is_empty() {
            return Ok(None);
        }
        class_domains.push(classes);
    }

    // Augment one vertex at a time. A search reaches each class once: a class
    // below capacity ends it, a full class queues the vertices on its rows.
    // The rows of a class fill in row order, so its first `load` rows are the
    // occupied ones.
    let mut owner = ctx.alloc_filled(count, 0usize, "catia_bijection_owners")?;
    let mut held_row = ctx.alloc_filled(count, 0usize, "catia_bijection_held_rows")?;
    let mut load = ctx.alloc_filled(class_count, 0usize, "catia_bijection_loads")?;
    let mut seen_classes = ctx.alloc_filled(class_count, 0usize, "catia_bijection_seen_classes")?;
    let mut via_vertex = ctx.alloc_filled(class_count, 0usize, "catia_bijection_via")?;
    let mut seen_vertices = ctx.alloc_filled(count, 0usize, "catia_bijection_seen_vertices")?;
    let mut queue = ctx.alloc_filled(count, 0usize, "catia_bijection_queue")?;
    for start in ctx.admit_iter(&(0..count), "catia_bijection_starts")? {
        let generation = start + 1;
        seen_vertices[start] = generation;
        queue[0] = start;
        let mut queued = 1usize;
        let mut head = 0usize;
        let mut open_class = None;
        'search: while head < queued {
            ctx.charge_work(1, "catia_bijection_match_visit")?;
            let vertex = queue[head];
            head += 1;
            for &class in &class_domains[vertex] {
                ctx.charge_work(1, "catia_bijection_class_visit")?;
                if seen_classes[class] == generation {
                    continue;
                }
                seen_classes[class] = generation;
                via_vertex[class] = vertex;
                let rows = &class_points[class];
                if load[class] < rows.len() {
                    open_class = Some(class);
                    break 'search;
                }
                for &row in rows {
                    ctx.charge_work(1, "catia_bijection_slot_projection")?;
                    let holder = owner[row];
                    if seen_vertices[holder] != generation {
                        seen_vertices[holder] = generation;
                        queue[queued] = holder;
                        queued += 1;
                    }
                }
            }
        }
        let Some(class) = open_class else {
            return Ok(None);
        };
        let mut row = class_points[class][load[class]];
        load[class] += 1;
        let mut vertex = via_vertex[class];
        loop {
            ctx.charge_work(1, "catia_bijection_augmentation")?;
            owner[row] = vertex;
            let previous = std::mem::replace(&mut held_row[vertex], row);
            if vertex == start {
                break;
            }
            row = previous;
            vertex = via_vertex[point_classes[previous]];
        }
    }

    let mut vertex_classes = ctx.collection_vec(count, "catia_bijection_vertex_classes")?;
    for &row in ctx.admit_iter(&held_row, "catia_bijection_vertex_classes")? {
        vertex_classes.push(point_classes[row]);
    }
    // Kahn's order over arcs from each vertex's class to its other classes.
    let mut indegree = ctx.alloc_filled(class_count, 0usize, "catia_bijection_indegree")?;
    for (vertex, classes) in ctx
        .admit_iter(&class_domains, "catia_bijection_alternate_class")?
        .enumerate()
    {
        for &class in ctx.admit_iter(classes, "catia_bijection_alternate_class")? {
            if class != vertex_classes[vertex] {
                indegree[class] += 1;
            }
        }
    }
    let mut ready = Vec::new();
    for (class, &degree) in ctx
        .admit_iter(&indegree, "catia_bijection_acyclic_roots")?
        .enumerate()
    {
        if degree == 0 {
            ctx.push_vec(&mut ready, class, "catia_bijection_acyclic_order")?;
        }
    }
    let mut ordered = 0usize;
    while let Some(class) = ready.pop() {
        ordered += 1;
        for &row in ctx.admit_iter(&class_points[class], "catia_bijection_acyclic_order")? {
            let vertex = owner[row];
            for &next in ctx.admit_iter(&class_domains[vertex], "catia_bijection_acyclic_order")? {
                if next != class {
                    indegree[next] -= 1;
                    if indegree[next] == 0 {
                        ctx.push_vec(&mut ready, next, "catia_bijection_acyclic_order")?;
                    }
                }
            }
        }
    }
    if ordered != class_count {
        return Ok(None);
    }
    Ok(Some(CoordinateClasses {
        vertex_classes,
        class_points,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    #[test]
    fn matching_visit_charges_the_attached_session_once() {
        crate::test_support::with_work_limit(1, |ctx| {
            let budget = ctx.work_budget(1);
            super::charge_matching_work(ctx, Some(&budget)).expect("one visit fits exactly");
            assert_eq!(budget.remaining(), 0);
            assert!(ctx.resource_refusal().is_none());
            assert!(matches!(
                super::charge_matching_work(ctx, Some(&budget)),
                Err(CodecError::ResourceLimit(_))
            ));
        });
    }

    #[test]
    fn matching_detached_slice_still_charges_the_caller() {
        crate::test_support::with_work_limit(0, |ctx| {
            let budget = cadmpeg_core::decode::WorkBudget::new(1);
            let CodecError::ResourceLimit(limit) = super::charge_matching_work(ctx, Some(&budget))
                .expect_err("caller work must be admitted")
            else {
                panic!("resource refusal")
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }

    #[test]
    fn coordinate_bijection_admits_each_domain_projection() {
        let domains = [vec![0_usize], vec![1]];
        let points = [[0.0; 3], [1.0, 0.0, 0.0]];
        let CodecError::ResourceLimit(limit) =
            crate::test_support::with_work_refusal("catia_bijection_domain_classes", |ctx| {
                super::unique_coordinate_bijection(ctx, &domains, &points)
            })
            .expect_err("projection requires work")
        else {
            panic!("resource refusal")
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    }

    #[test]
    fn coordinate_bijection_admits_each_search_and_uniqueness_visit() {
        // The second vertex finds the first vertex's class full.
        let domains = [BTreeSet::from([0_usize]), BTreeSet::from([0, 1])];
        let points = [[0.0; 3], [1.0, 0.0, 0.0]];
        for operation in [
            "catia_bijection_match_visit",
            "catia_bijection_class_visit",
            "catia_bijection_slot_projection",
            "catia_bijection_augmentation",
            "catia_bijection_alternate_class",
            "catia_bijection_acyclic_order",
        ] {
            let CodecError::ResourceLimit(limit) =
                crate::test_support::with_work_refusal(operation, |ctx| {
                    super::unique_coordinate_bijection(ctx, &domains, &points)
                })
                .expect_err("search step requires work")
            else {
                panic!("resource refusal")
            };
            assert_eq!(limit.operation, operation);
        }
    }

    #[test]
    fn coordinate_bijection_matches_exhaustive_uniqueness() {
        // Every domain family over four rows, two of which share coordinates.
        let points = [[0.0; 3], [0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let class_of = [0_usize, 0, 1, 2];
        let rows = points.len();
        for masks in 0..16_u32.pow(4) {
            let domains = (0..rows)
                .map(|vertex| {
                    let mask = (masks >> (4 * vertex)) & 0xf;
                    (0..rows)
                        .filter(|row| mask & (1 << row) != 0)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let mut assignments = BTreeSet::new();
            for permutation in permutations(rows) {
                // Rows with equal coordinates are interchangeable, so a
                // domain admits every row of the classes it names.
                if permutation.iter().enumerate().all(|(vertex, row)| {
                    domains[vertex]
                        .iter()
                        .any(|allowed| class_of[*allowed] == class_of[*row])
                }) {
                    assignments.insert(
                        permutation
                            .iter()
                            .map(|row| class_of[*row])
                            .collect::<Vec<_>>(),
                    );
                }
            }
            let result = crate::test_support::with_service_context(|ctx| {
                super::unique_coordinate_bijection(ctx, &domains, &points)
            })
            .expect("service budget");
            if assignments.len() == 1 {
                let classes = assignments.pop_first().expect("one assignment");
                let rows = result.expect("unique coordinate assignment");
                assert_eq!(
                    rows.iter().map(|row| class_of[*row]).collect::<Vec<_>>(),
                    classes,
                    "domains={domains:?}"
                );
                let mut sorted = rows.clone();
                sorted.sort_unstable();
                assert_eq!(sorted, [0, 1, 2, 3], "domains={domains:?}");
                // Interchangeable rows go out in vertex order.
                assert_eq!(
                    rows.iter()
                        .copied()
                        .filter(|row| class_of[*row] == 0)
                        .collect::<Vec<_>>(),
                    [0, 1],
                    "domains={domains:?}"
                );
            } else {
                assert_eq!(result, None, "domains={domains:?}");
            }
        }
    }

    #[test]
    fn coordinate_classes_follow_f64_equality() {
        let domains = [vec![0_usize, 1], vec![0, 1]];
        crate::test_support::with_service_context(|ctx| {
            assert_eq!(
                super::unique_coordinate_bijection(ctx, &domains, &[[0.0; 3], [-0.0, 0.0, 0.0]])
                    .expect("service budget"),
                Some(vec![0, 1])
            );
            assert_eq!(
                super::unique_coordinate_bijection(
                    ctx,
                    &domains,
                    &[[f64::NAN, 0.0, 0.0], [f64::NAN, 0.0, 0.0]]
                )
                .expect("service budget"),
                None
            );
        });
    }

    fn permutations(count: usize) -> Vec<Vec<usize>> {
        if count == 0 {
            return vec![Vec::new()];
        }
        let mut all = Vec::new();
        for shorter in permutations(count - 1) {
            for position in 0..count {
                let mut permutation = shorter.clone();
                permutation.insert(position, count - 1);
                all.push(permutation);
            }
        }
        all
    }

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    use super::{
        distinct_domain_matching_with_budget, repair_distinct_domain_matching_with_budget,
        retain_distinct_matching_supports, unique_coordinate_bijection, MatchingEdgeConstraint,
    };

    fn allocation_refusals(
        final_limit: u64,
        mut run: impl FnMut(&DecodeContext<'_>) -> Result<(), CodecError>,
    ) -> BTreeSet<&'static str> {
        let mut operations = BTreeSet::new();
        let mut completed = false;
        for limit in 0..=final_limit {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("matching fixture fits the input limit");
            match run(&ctx) {
                Err(CodecError::ResourceLimit(error)) => {
                    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                    operations.insert(error.operation);
                }
                Ok(()) => {
                    completed = true;
                    break;
                }
                Err(error) => panic!("unexpected matching refusal: {error}"),
            }
        }
        assert!(completed, "final limit must admit the matching fixture");
        operations
    }

    #[test]
    fn distinct_matching_charges_each_search_array() {
        let domains = [vec![0], vec![1]];
        let operations = allocation_refusals(32, |ctx| {
            assert!(distinct_domain_matching_with_budget(
                ctx,
                domains.iter().map(Vec::as_slice),
                2,
                None,
                None,
            )?
            .is_some());
            Ok(())
        });
        assert_eq!(
            operations,
            BTreeSet::from([
                "catia_match_domains",
                "catia_match_owners",
                "catia_match_mates",
                "catia_match_distance",
                "catia_match_queue",
                "catia_match_cursor",
                "catia_match_augmenting_roots",
                "catia_match_completed",
            ])
        );
    }

    #[test]
    fn matching_repair_charges_each_augmenting_array() {
        let domains = [vec![0], vec![1]];
        let operations = allocation_refusals(32, |ctx| {
            assert!(repair_distinct_domain_matching_with_budget(
                ctx,
                domains.iter().map(Vec::as_slice),
                2,
                &[1, 1],
                None,
            )?
            .is_some());
            Ok(())
        });
        assert_eq!(
            operations,
            BTreeSet::from([
                "catia_match_repair_domains",
                "catia_match_repair_owners",
                "catia_match_repaired",
                "catia_match_unmatched",
                "catia_match_repair_seen_domains",
                "catia_match_repair_seen_points",
                "catia_match_repair_via",
                "catia_match_repair_queue",
                "catia_match_repair_completed",
            ])
        );
    }

    #[test]
    fn matching_support_charges_each_graph_array() {
        let operations = allocation_refusals(80, |ctx| {
            let mut domains = [vec![0], vec![1]];
            assert_eq!(
                retain_distinct_matching_supports(ctx, &mut domains, 2, &[0, 1], None)?,
                Some(false)
            );
            Ok(())
        });
        assert_eq!(
            operations,
            BTreeSet::from([
                "catia_match_support_graph",
                "catia_match_support_reverse",
                "catia_match_support_points",
                "catia_match_support_arcs",
                "catia_match_support_reverse_arcs",
                "catia_match_support_visit",
                "catia_match_support_stack",
                "catia_match_support_finish_order",
                "catia_match_support_components",
                "catia_match_support_component_stack",
                "catia_match_support_free",
            ])
        );
    }

    #[test]
    fn coordinate_bijection_charges_each_class_array() {
        let domains = [vec![0], vec![1]];
        let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        let operations = allocation_refusals(96, |ctx| {
            assert_eq!(
                unique_coordinate_bijection(ctx, &domains, &points)?,
                Some(vec![0, 1])
            );
            Ok(())
        });
        assert_eq!(
            operations,
            BTreeSet::from([
                "catia_bijection_point_classes",
                "catia_bijection_coordinate_classes",
                "catia_bijection_classes",
                "catia_bijection_class_points",
                "catia_bijection_class_stamps",
                "catia_bijection_class_domains",
                "catia_bijection_domain_classes",
                "catia_bijection_owners",
                "catia_bijection_held_rows",
                "catia_bijection_loads",
                "catia_bijection_seen_classes",
                "catia_bijection_via",
                "catia_bijection_seen_vertices",
                "catia_bijection_queue",
                "catia_bijection_vertex_classes",
                "catia_bijection_indegree",
                "catia_bijection_acyclic_order",
                "catia_bijection_used",
                "catia_bijection_points",
            ])
        );
    }

    #[test]
    fn distinct_matching_domain_collection_refuses_before_absent_result() {
        let domains = [vec![0]];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("matching fixture fits the service profile");
        assert!(distinct_domain_matching_with_budget(
            &ctx,
            domains.iter().map(Vec::as_slice),
            0,
            None,
            None,
        )
        .expect("service resource budget")
        .is_none());

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("matching fixture fits the input limit");
        let result = distinct_domain_matching_with_budget(
            &ctx,
            domains.iter().map(Vec::as_slice),
            0,
            None,
            None,
        );
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia_match_domains"));
    }

    #[test]
    fn matching_repair_domain_collection_refuses_before_absent_result() {
        let domains = [vec![0]];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("matching fixture fits the service profile");
        assert!(repair_distinct_domain_matching_with_budget(
            &ctx,
            domains.iter().map(Vec::as_slice),
            1,
            &[],
            None,
        )
        .expect("service resource budget")
        .is_none());

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("matching fixture fits the input limit");
        let result = repair_distinct_domain_matching_with_budget(
            &ctx,
            domains.iter().map(Vec::as_slice),
            1,
            &[],
            None,
        );
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia_match_repair_domains"));
    }

    #[test]
    fn matching_support_inner_arcs_refuse_before_absent_result() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("matching fixture fits the service profile");
        let mut domains = [vec![0, 2]];
        assert!(
            retain_distinct_matching_supports(&ctx, &mut domains, 2, &[0], None)
                .expect("service resource budget")
                .is_none()
        );

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 8;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("matching fixture fits the input limit");
        let mut domains = [vec![0, 2]];
        let result = retain_distinct_matching_supports(&ctx, &mut domains, 2, &[0], None);
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia_match_support_arcs"));
    }

    #[test]
    fn coordinate_bijection_queue_refuses_before_ambiguous_result() {
        let domains = [vec![0, 1], vec![0, 1]];
        let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        assert!(crate::test_support::with_service_context(|ctx| {
            unique_coordinate_bijection(ctx, &domains, &points)
        })
        .expect("service resource budget")
        .is_none());
        let limit = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "catia_bijection_queue",
            |cap| {
                crate::test_support::with_collection_limit(cap, |ctx| {
                    unique_coordinate_bijection(ctx, &domains, &points)
                })
            },
        );
        assert!(matches!(limit, CodecError::ResourceLimit(limit)
            if limit.operation == "catia_bijection_queue"));
    }

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
