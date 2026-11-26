// SPDX-License-Identifier: Apache-2.0
//! Propagation of quotient merges that every face option shares.

use super::quotient::MeshQuotient;
use super::{port, MeshQuotientGaugeState};
use crate::solve::missing_edge::{
    MeshBoundaryEdgeCandidate, MeshDeferredFaceBoundary, MeshFaceBoundaryAssignment,
    MeshFaceBoundaryDomain,
};
use crate::solve::union_find::UnionFind;
use cadmpeg_core::decode::{index_from_u32, work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(super) struct DeferredFaceQuotientOptions {
    pub(super) alternatives: Vec<MeshQuotient>,
    pub(super) base_nodes: Vec<usize>,
}

pub(super) fn materialize_deferred_quotient_option(
    base: &MeshQuotient,
    local: &MeshQuotient,
    base_nodes: &[usize],
    affected_edges: impl IntoIterator<Item = usize>,
    edge_candidates: &[Vec<[usize; 2]>],
    budget: &WorkBudget,
) -> Result<Option<MeshQuotient>, CodecError> {
    let mut materialized = base.clone();
    for local_node in 0..base_nodes.len() {
        let local_root = local.union.root(local_node)?;
        if local_root != local_node
            && materialized
                .merge(base_nodes[local_root], base_nodes[local_node])?
                .is_none()
        {
            return Ok(None);
        }
    }
    Ok(materialized
        .propagate_edge_domains(affected_edges, edge_candidates, Some(budget))?
        .then_some(materialized))
}

pub(super) fn deferred_face_quotient_options_limited(
    ctx: &DecodeContext<'_>,
    domain: &MeshDeferredFaceBoundary,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &MeshQuotient,
    limit: usize,
    budget: &WorkBudget,
) -> Result<Option<DeferredFaceQuotientOptions>, CodecError> {
    #[derive(Clone, Copy)]
    struct Gap {
        left_end: usize,
        right_start: usize,
        capacity: usize,
    }

    struct DeferredGapFill<'input0, 'input1, 'input2, 'input3, 'input4, 'input5, 'input6, 'input8> {
        gaps: &'input0 [Gap],
        gap: usize,
        at: usize,
        target: usize,
        used: u64,
        previous_end: usize,
        missing_edges: &'input1 [usize],
        missing_nodes: &'input2 [[usize; 2]],
        edge_candidates: &'input3 [Vec<[usize; 2]>],
        quotient: MeshQuotient,
        base_quotient: &'input4 MeshQuotient,
        base_nodes: &'input5 [usize],
        output: &'input6 mut Vec<MeshQuotient>,
        limit: usize,
        budget: &'input8 WorkBudget,
    }

    fn fill_gap(
        ctx: &DecodeContext<'_>,
        inputs: DeferredGapFill<'_, '_, '_, '_, '_, '_, '_, '_>,
    ) -> Result<(), CodecError> {
        let DeferredGapFill {
            gaps,
            gap,
            at,
            target,
            used,
            previous_end,
            missing_edges,
            missing_nodes,
            edge_candidates,
            quotient,
            base_quotient,
            base_nodes,
            output,
            limit,
            budget,
        } = inputs;

        let Ok(_depth) = budget.recursion_guard() else {
            return Ok(());
        };
        if output.len() >= limit || budget.exhausted() {
            return Ok(());
        }
        if at == target {
            let mut quotient = quotient;
            if quotient
                .merge(previous_end, gaps[gap].right_start)?
                .is_none()
            {
                return Ok(());
            }
            walk_gaps(
                ctx,
                DeferredGapWalk {
                    gaps,
                    gap: gap + 1,
                    used,
                    missing_edges,
                    missing_nodes,
                    edge_candidates,
                    quotient: &quotient,
                    base_quotient,
                    base_nodes,
                    output,
                    limit,
                    budget,
                },
            )?;
            return Ok(());
        }
        let Some(options) =
            (missing_edges.len() - index_from_u32(used.count_ones())).checked_mul(2)
        else {
            return Ok(());
        };
        if options > 1 && !budget.charge_by(options) {
            return Ok(());
        }
        let mut seen = HashSet::new();
        for (rank, _) in missing_edges.iter().enumerate() {
            if used & (1 << rank) != 0 {
                continue;
            }
            for reversed in [false, true] {
                let start = missing_nodes[rank][usize::from(reversed)];
                let end = missing_nodes[rank][usize::from(!reversed)];
                let mut next = quotient.clone();
                if next.merge(previous_end, start)?.is_none() {
                    continue;
                }
                let end_root = next.union.find(end)?;
                if !seen.insert((rank, end_root, next.signature()?)) {
                    continue;
                }
                fill_gap(
                    ctx,
                    DeferredGapFill {
                        gaps,
                        gap,
                        at: at + 1,
                        target,
                        used: used | (1 << rank),
                        previous_end: end,
                        missing_edges,
                        missing_nodes,
                        edge_candidates,
                        quotient: next,
                        base_quotient,
                        base_nodes,
                        output,
                        limit,
                        budget,
                    },
                )?;
                if output.len() >= limit || budget.exhausted() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    struct DeferredGapWalk<
        'input0,
        'input1,
        'input2,
        'input3,
        'input4,
        'input5,
        'input6,
        'input7,
        'input9,
    > {
        gaps: &'input0 [Gap],
        gap: usize,
        used: u64,
        missing_edges: &'input1 [usize],
        missing_nodes: &'input2 [[usize; 2]],
        edge_candidates: &'input3 [Vec<[usize; 2]>],
        quotient: &'input4 MeshQuotient,
        base_quotient: &'input5 MeshQuotient,
        base_nodes: &'input6 [usize],
        output: &'input7 mut Vec<MeshQuotient>,
        limit: usize,
        budget: &'input9 WorkBudget,
    }

    fn walk_gaps(
        ctx: &DecodeContext<'_>,
        inputs: DeferredGapWalk<'_, '_, '_, '_, '_, '_, '_, '_, '_>,
    ) -> Result<(), CodecError> {
        let DeferredGapWalk {
            gaps,
            gap,
            used,
            missing_edges,
            missing_nodes,
            edge_candidates,
            quotient,
            base_quotient,
            base_nodes,
            output,
            limit,
            budget,
        } = inputs;

        let Ok(_depth) = budget.recursion_guard() else {
            return Ok(());
        };
        if output.len() >= limit || budget.exhausted() {
            return Ok(());
        }
        if gap == gaps.len() {
            if index_from_u32(used.count_ones()) != missing_edges.len() {
                return Ok(());
            }
            let mut affected_edges = HashSet::new();
            for &edge in missing_edges {
                if !edge_candidates[edge].is_empty() {
                    affected_edges.insert(edge);
                }
            }
            if materialize_deferred_quotient_option(
                base_quotient,
                quotient,
                base_nodes,
                affected_edges,
                edge_candidates,
                budget,
            )?
            .is_some()
            {
                output.push(quotient.clone());
            }
            return Ok(());
        }
        let remaining_edges = missing_edges.len() - index_from_u32(used.count_ones());
        let remaining_gaps = gaps.len() - gap - 1;
        let minimum = 1;
        let Some(available_edges) = remaining_edges.checked_sub(remaining_gaps) else {
            return Ok(());
        };
        let maximum = gaps[gap].capacity.min(available_edges);
        if maximum < minimum {
            return Ok(());
        }
        if maximum > minimum && !budget.charge_by(maximum - minimum + 1) {
            return Ok(());
        }
        for target in minimum..=maximum {
            fill_gap(
                ctx,
                DeferredGapFill {
                    gaps,
                    gap,
                    at: 0,
                    target,
                    used,
                    previous_end: gaps[gap].left_end,
                    missing_edges,
                    missing_nodes,
                    edge_candidates,
                    quotient: quotient.clone(),
                    base_quotient,
                    base_nodes,
                    output,
                    limit,
                    budget,
                },
            )?;
            if output.len() >= limit || budget.exhausted() {
                return Ok(());
            }
        }
        Ok(())
    }

    if domain.missing_edges.len() > index_from_u32(u64::BITS) {
        return Ok(None);
    }
    let mut gaps = Vec::new();
    for cycle in &domain.cycles {
        if cycle.exact_uses.is_empty() {
            return Ok(None);
        }
        for index in 0..cycle.exact_uses.len() {
            let (left, left_span) = cycle.exact_uses[index];
            let right = cycle.exact_uses[(index + 1) % cycle.exact_uses.len()].0;
            let left_end_position = (left.start + left_span) % cycle.length;
            let capacity = (right.start + cycle.length - left_end_position) % cycle.length;
            if capacity == 0 {
                continue;
            }
            let (Some(left_reversed), Some(right_reversed)) = (left.reversed, right.reversed)
            else {
                return Ok(None);
            };
            let Some(left_end) = left
                .edge
                .checked_mul(2)
                .and_then(|node| node.checked_add(usize::from(!left_reversed)))
            else {
                return Ok(None);
            };
            let Some(right_start) = right
                .edge
                .checked_mul(2)
                .and_then(|node| node.checked_add(usize::from(right_reversed)))
            else {
                return Ok(None);
            };
            gaps.push(Gap {
                left_end,
                right_start,
                capacity,
            });
        }
    }
    if gaps.is_empty() {
        return Ok(domain
            .missing_edges
            .is_empty()
            .then(|| DeferredFaceQuotientOptions {
                alternatives: Vec::new(),
                base_nodes: Vec::new(),
            }));
    }
    if domain.missing_edges.len() < gaps.len() {
        return Ok(Some(DeferredFaceQuotientOptions {
            alternatives: Vec::new(),
            base_nodes: Vec::new(),
        }));
    }
    let mut base_nodes = Vec::new();
    for gap in &gaps {
        for node in [gap.left_end, gap.right_start] {
            base_nodes.push(quotient.union.root(node)?);
        }
    }
    for &edge in &domain.missing_edges {
        for node in [edge * 2, edge * 2 + 1] {
            base_nodes.push(quotient.union.root(node)?);
        }
    }
    base_nodes.sort_unstable_by(Ord::cmp);
    base_nodes.dedup();
    let mut local_by_base = HashMap::new();
    for (local, &base) in base_nodes.iter().enumerate() {
        local_by_base.insert(base, local);
    }
    for gap in &mut gaps {
        gap.left_end = local_by_base[&quotient.union.root(gap.left_end)?];
        gap.right_start = local_by_base[&quotient.union.root(gap.right_start)?];
    }
    let mut missing_nodes = Vec::new();
    for &edge in &domain.missing_edges {
        missing_nodes.push([
            local_by_base[&quotient.union.root(edge * 2)?],
            local_by_base[&quotient.union.root(edge * 2 + 1)?],
        ]);
    }
    let mut local_domains = Vec::new();
    for &root in &base_nodes {
        local_domains.push(quotient.domains[root].clone());
    }
    let local_quotient = MeshQuotient::new(local_domains);
    let mut ranked_gaps = Vec::new();
    for gap in gaps {
        let single_edge_options = if gap.capacity == 1 {
            let mut count = 0usize;
            for (rank, _) in domain.missing_edges.iter().enumerate() {
                for reversed in [false, true] {
                    let start = missing_nodes[rank][usize::from(reversed)];
                    let end = missing_nodes[rank][usize::from(!reversed)];
                    let mut trial = local_quotient.clone();
                    if trial.merge(gap.left_end, start)?.is_some()
                        && trial.merge(end, gap.right_start)?.is_some()
                    {
                        count += 1;
                    }
                }
            }
            count
        } else {
            usize::MAX
        };
        ranked_gaps.push((gap, (gap.capacity, single_edge_options)));
    }
    ranked_gaps.sort_unstable_by_key(|(_, left)| *left);
    let mut gaps = Vec::new();
    for (gap, _) in ranked_gaps {
        gaps.push(gap);
    }
    let mut output = Vec::new();
    walk_gaps(
        ctx,
        DeferredGapWalk {
            gaps: &gaps,
            gap: 0,
            used: 0,
            missing_edges: &domain.missing_edges,
            missing_nodes: &missing_nodes,
            edge_candidates,
            quotient: &local_quotient,
            base_quotient: quotient,
            base_nodes: &base_nodes,
            output: &mut output,
            limit,
            budget,
        },
    )?;
    Ok(
        (!budget.exhausted()).then_some(DeferredFaceQuotientOptions {
            alternatives: output,
            base_nodes,
        }),
    )
}

pub(super) fn propagate_common_deferred_quotients(
    mut options: DeferredFaceQuotientOptions,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
    budget: &WorkBudget,
) -> Result<Option<()>, CodecError> {
    let node_count = options.base_nodes.len();
    let mut equivalence_classes = HashMap::<Vec<usize>, Vec<usize>>::new();
    for node in 0..node_count {
        let mut signature = Vec::new();
        for alternative in &mut options.alternatives {
            signature.push(alternative.union.find(node)?);
        }
        if let Some(members) = equivalence_classes.get_mut(&signature) {
            members.push(node);
        } else {
            let members = vec![node];
            equivalence_classes.insert(signature, members);
        }
    }
    for nodes in equivalence_classes.into_values() {
        let Some((&representative, rest)) = nodes.split_first() else {
            continue;
        };
        for &node in rest {
            if quotient
                .merge(options.base_nodes[representative], options.base_nodes[node])?
                .is_none()
            {
                return Ok(None);
            }
        }
    }
    for local in 0..node_count {
        let mut allowed = HashSet::new();
        for alternative in &mut options.alternatives {
            let root = alternative.union.find(local)?;
            for &point in alternative.domains[root].iter() {
                allowed.insert(point);
            }
        }
        let root = quotient.union.find(options.base_nodes[local])?;
        let mut narrowed = HashSet::new();
        for &point in quotient.domains[root].intersection(&allowed) {
            narrowed.insert(point);
        }
        if narrowed.is_empty() {
            return Ok(None);
        }
        quotient.domains[root] = Arc::new(narrowed);
    }
    let mut affected_edges = HashSet::new();
    for node in options.base_nodes {
        let root = quotient.union.find(node)?;
        for edge in quotient.members(root).iter().map(|node| node / 2) {
            if !edge_candidates[edge].is_empty() {
                affected_edges.insert(edge);
            }
        }
    }
    Ok(quotient
        .propagate_edge_domains(affected_edges, edge_candidates, Some(budget))?
        .then_some(()))
}

pub(super) fn common_supported_corner_equations(
    quotient: &mut MeshQuotient,
    assignments: &[MeshFaceBoundaryAssignment],
    budget: &WorkBudget,
) -> Result<Option<HashSet<[usize; 2]>>, CodecError> {
    fn compatible(quotient: &MeshQuotient, left: usize, right: usize) -> Result<bool, CodecError> {
        let left = quotient.union.root(left)?;
        let right = quotient.union.root(right)?;
        Ok(left == right || !quotient.domains[left].is_disjoint(&quotient.domains[right]))
    }

    (|| -> Option<Result<HashSet<[usize; 2]>, CodecError>> {
        let mut common = None::<HashSet<[usize; 2]>>;
        'assignments: for assignment in assignments {
            if !budget.charge() {
                return None;
            }
            let mut forced = HashSet::new();
            for boundary in &assignment.boundaries {
                if boundary.is_empty() {
                    return None;
                }
                let directions = boundary
                    .iter()
                    .map(|use_| match use_.reversed {
                        Some(reversed) => vec![reversed],
                        None => vec![false, true],
                    })
                    .collect::<Vec<_>>();
                let mut supported = directions
                    .iter()
                    .enumerate()
                    .map(|(index, states)| {
                        let next = &directions[(index + 1) % directions.len()];
                        vec![vec![false; next.len()]; states.len()]
                    })
                    .collect::<Vec<_>>();
                for first in 0..directions[0].len() {
                    let mut forward = directions
                        .iter()
                        .map(|states| vec![false; states.len()])
                        .collect::<Vec<_>>();
                    forward[0][first] = true;
                    for index in 0..boundary.len() - 1 {
                        for left in 0..directions[index].len() {
                            if !forward[index][left] {
                                continue;
                            }
                            for right in 0..directions[index + 1].len() {
                                let left_node =
                                    port(boundary[index], directions[index][left], true)?;
                                let right_node =
                                    port(boundary[index + 1], directions[index + 1][right], false)?;
                                if match compatible(quotient, left_node, right_node) {
                                    Ok(value) => value,
                                    Err(error) => return Some(Err(error)),
                                } {
                                    forward[index + 1][right] = true;
                                }
                            }
                        }
                    }
                    let last = boundary.len() - 1;
                    let mut backward = directions
                        .iter()
                        .map(|states| vec![false; states.len()])
                        .collect::<Vec<_>>();
                    for state in 0..directions[last].len() {
                        let left_node = port(boundary[last], directions[last][state], true)?;
                        let right_node = port(boundary[0], directions[0][first], false)?;
                        backward[last][state] = forward[last][state]
                            && match compatible(quotient, left_node, right_node) {
                                Ok(value) => value,
                                Err(error) => return Some(Err(error)),
                            };
                    }
                    for index in (0..last).rev() {
                        for left in 0..directions[index].len() {
                            if !forward[index][left] {
                                continue;
                            }
                            for right in 0..directions[index + 1].len() {
                                if !backward[index + 1][right] {
                                    continue;
                                }
                                let left_node =
                                    port(boundary[index], directions[index][left], true)?;
                                let right_node =
                                    port(boundary[index + 1], directions[index + 1][right], false)?;
                                match compatible(quotient, left_node, right_node) {
                                    Ok(true) => {
                                        backward[index][left] = true;
                                        break;
                                    }
                                    Ok(false) => {}
                                    Err(error) => return Some(Err(error)),
                                }
                            }
                        }
                    }
                    if !backward[0][first] {
                        continue;
                    }
                    for index in 0..last {
                        for left in 0..directions[index].len() {
                            if !forward[index][left] {
                                continue;
                            }
                            for right in 0..directions[index + 1].len() {
                                if backward[index + 1][right] {
                                    let left_node =
                                        port(boundary[index], directions[index][left], true)?;
                                    let right_node = port(
                                        boundary[index + 1],
                                        directions[index + 1][right],
                                        false,
                                    )?;
                                    if match compatible(quotient, left_node, right_node) {
                                        Ok(value) => value,
                                        Err(error) => return Some(Err(error)),
                                    } {
                                        supported[index][left][right] = true;
                                    }
                                }
                            }
                        }
                    }
                    for state in 0..directions[last].len() {
                        if backward[last][state] {
                            supported[last][state][first] = true;
                        }
                    }
                }
                if supported
                    .iter()
                    .any(|transitions| transitions.iter().flatten().all(|value| !value))
                {
                    continue 'assignments;
                }
                for index in 0..boundary.len() {
                    let next = (index + 1) % boundary.len();
                    let mut equations = HashSet::new();
                    for left in 0..directions[index].len() {
                        for right in 0..directions[next].len() {
                            if supported[index][left][right] {
                                let left = match quotient.union.find(port(
                                    boundary[index],
                                    directions[index][left],
                                    true,
                                )?) {
                                    Ok(root) => root,
                                    Err(error) => return Some(Err(error)),
                                };
                                let right = match quotient.union.find(port(
                                    boundary[next],
                                    directions[next][right],
                                    false,
                                )?) {
                                    Ok(root) => root,
                                    Err(error) => return Some(Err(error)),
                                };
                                let equation = if left <= right {
                                    [left, right]
                                } else {
                                    [right, left]
                                };
                                equations.insert(equation);
                            }
                        }
                    }
                    if equations.len() == 1 {
                        if let Some(equation) = equations.into_iter().next() {
                            forced.insert(equation);
                        }
                    }
                }
            }
            match &mut common {
                Some(common) => common.retain(|equation| forced.contains(equation)),
                None => common = Some(forced),
            }
        }
        common.map(Ok)
    })()
    .transpose()
}

pub(super) fn propagate_common_full_quotients(
    mut alternatives: Vec<MeshQuotient>,
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
) -> Result<Option<()>, CodecError> {
    let node_count = quotient.union.len();
    let mut equivalence_classes = HashMap::<Vec<usize>, Vec<usize>>::new();
    for node in 0..node_count {
        let mut signature = Vec::new();
        for alternative in &mut alternatives {
            signature.push(alternative.union.find(node)?);
        }
        if let Some(members) = equivalence_classes.get_mut(&signature) {
            members.push(node);
        } else {
            let members = vec![node];
            equivalence_classes.insert(signature, members);
        }
    }
    for nodes in equivalence_classes.into_values() {
        let Some((&representative, rest)) = nodes.split_first() else {
            continue;
        };
        for &node in rest {
            if quotient.merge(representative, node)?.is_none() {
                return Ok(None);
            }
        }
    }

    let mut roots = Vec::new();
    for node in 0..node_count {
        if quotient.union.find(node)? == node {
            roots.push(node);
        }
    }
    for root in roots {
        let representative = quotient.members(root)[0];
        let mut allowed = HashSet::new();
        for alternative in &mut alternatives {
            let alternative_root = alternative.union.find(representative)?;
            for &point in alternative.domains[alternative_root].iter() {
                allowed.insert(point);
            }
        }
        let mut narrowed = HashSet::new();
        for &point in quotient.domains[root].intersection(&allowed) {
            narrowed.insert(point);
        }
        if narrowed.is_empty() {
            return Ok(None);
        }
        quotient.domains[root] = Arc::new(narrowed);
    }
    Ok(quotient.edge_domains_viable(edge_candidates)?.then_some(()))
}

pub(in crate::solve) fn propagate_common_ordered_face_quotients(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
    budget: &WorkBudget,
) -> Result<Option<()>, CodecError> {
    (|| -> Option<Result<(), CodecError>> {
        const MAX_FACE_OPTIONS: usize = 4_096;
        const MAX_ORDERED_FACE_CONSTRAINT_OPERATIONS: usize = 64;
        const MAX_DEFERRED_FACE_CONSTRAINT_OPERATIONS: usize = 512;
        let mut face_order = vec![0usize; domains.len()];
        for (face, slot) in face_order.iter_mut().enumerate() {
            *slot = face;
        }
        let order_key = |face: &usize| match &domains[*face] {
            MeshFaceBoundaryDomain::DeferredValidation(_) => (0, 0),
            MeshFaceBoundaryDomain::Ordered(assignments) => (1, assignments.len()),
            MeshFaceBoundaryDomain::UnorderedFullCycle(_) => (2, 0),
        };
        face_order.sort_unstable_by_key(|left| order_key(left));
        loop {
            let before = match quotient.monotone_measure() {
                Ok(measure) => measure?,
                Err(error) => return Some(Err(error)),
            };
            for &face in &face_order {
                let domain = &domains[face];
                let face_limit = match domain {
                    MeshFaceBoundaryDomain::DeferredValidation(_) => {
                        MAX_DEFERRED_FACE_CONSTRAINT_OPERATIONS
                    }
                    MeshFaceBoundaryDomain::Ordered(_)
                    | MeshFaceBoundaryDomain::UnorderedFullCycle(_) => {
                        MAX_ORDERED_FACE_CONSTRAINT_OPERATIONS
                    }
                };
                let face_budget = budget.charging_slice(face_limit);
                // An exhausted face slice skips this face.
                if let MeshFaceBoundaryDomain::DeferredValidation(domain) = domain {
                    let mut merged_nodes = Vec::new();
                    for cycle in &domain.cycles {
                        for index in 0..cycle.exact_uses.len() {
                            let (left, left_span) = cycle.exact_uses[index];
                            let right = cycle.exact_uses[(index + 1) % cycle.exact_uses.len()].0;
                            let left_end = (left.start + left_span) % cycle.length;
                            let capacity = (right.start + cycle.length - left_end) % cycle.length;
                            if capacity != 0 {
                                continue;
                            }
                            if left.reversed.is_none() || right.reversed.is_none() {
                                continue;
                            }
                            let (left_reversed, right_reversed) = (left.reversed?, right.reversed?);
                            let left_node = left
                                .edge
                                .checked_mul(2)?
                                .checked_add(usize::from(!left_reversed))?;
                            let right_node = right
                                .edge
                                .checked_mul(2)?
                                .checked_add(usize::from(right_reversed))?;
                            let merged = match quotient.merge(left_node, right_node) {
                                Ok(Some(root)) => root,
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            };
                            merged_nodes.push(merged);
                        }
                    }
                    let affected_edges =
                        match quotient.affected_edges_for_nodes(&merged_nodes, edge_candidates) {
                            Ok(edges) => edges,
                            Err(error) => return Some(Err(error)),
                        };
                    match quotient.propagate_edge_domains(
                        affected_edges,
                        edge_candidates,
                        Some(budget),
                    ) {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(error) => return Some(Err(error)),
                    }
                    let options = match deferred_face_quotient_options_limited(
                        ctx,
                        domain,
                        edge_candidates,
                        quotient,
                        MAX_FACE_OPTIONS + 1,
                        &face_budget,
                    ) {
                        Ok(Some(options)) => options,
                        Ok(None) => continue,
                        Err(error) => return Some(Err(error)),
                    };
                    if options.alternatives.len() <= MAX_FACE_OPTIONS
                        && !options.alternatives.is_empty()
                    {
                        match propagate_common_deferred_quotients(
                            options,
                            edge_candidates,
                            quotient,
                            budget,
                        ) {
                            Ok(Some(())) => {}
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        }
                    }
                    continue;
                }
                let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                    continue;
                };
                let equations =
                    match common_supported_corner_equations(quotient, assignments, &face_budget) {
                        Ok(equations) => equations,
                        Err(error) => return Some(Err(error)),
                    };
                if let Some(equations) = equations {
                    let mut merged_nodes = Vec::new();
                    for [left, right] in equations {
                        let merged = match quotient.merge(left, right) {
                            Ok(Some(root)) => root,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                        merged_nodes.push(merged);
                    }
                    let affected_edges =
                        match quotient.affected_edges_for_nodes(&merged_nodes, edge_candidates) {
                            Ok(edges) => edges,
                            Err(error) => return Some(Err(error)),
                        };
                    match quotient.propagate_edge_domains(
                        affected_edges,
                        edge_candidates,
                        Some(budget),
                    ) {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(error) => return Some(Err(error)),
                    }
                }
                // This preparation pass propagates local equations only. The
                // complete incidence and endpoint searches below retain every
                // face domain and validate their correlated assignments. Signing
                // whole-mesh alternatives here repeats that search per face.
            }
            if match quotient.monotone_measure() {
                Ok(measure) => measure?,
                Err(error) => return Some(Err(error)),
            } == before
            {
                return Some(Ok(()));
            }
        }
    })()
    .transpose()
}

pub(super) fn mesh_boundary_domain_edges(domain: &MeshFaceBoundaryDomain) -> Vec<usize> {
    let mut edges = Vec::new();
    match domain {
        MeshFaceBoundaryDomain::Ordered(assignments) => {
            for assignment in assignments {
                for use_ in assignment.boundaries.iter().flatten() {
                    edges.push(use_.edge);
                }
            }
        }
        MeshFaceBoundaryDomain::UnorderedFullCycle(ordered_edges) => {
            edges.clone_from(ordered_edges);
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            edges.clone_from(&domain.missing_edges);
            for cycle in &domain.cycles {
                for (use_, _) in &cycle.exact_uses {
                    edges.push(use_.edge);
                }
            }
        }
    }
    edges.sort_unstable_by(Ord::cmp);
    edges.dedup();
    edges
}

pub(in crate::solve) fn bounded_unordered_cycle_assignments(
    edges: &[usize],
    quotient: &MeshQuotient,
    limit: usize,
    budget: &WorkBudget,
) -> Result<Option<Vec<MeshFaceBoundaryAssignment>>, CodecError> {
    struct Search<'a> {
        edges: &'a [usize],
        compatible: &'a HashSet<(usize, usize)>,
        limit: usize,
        budget: &'a WorkBudget,
        assignments: Vec<MeshFaceBoundaryAssignment>,
    }

    impl Search<'_> {
        fn walk(
            &mut self,
            first_start: usize,
            previous_end: usize,
            used: u64,
            boundary: &mut Vec<MeshBoundaryEdgeCandidate>,
        ) -> Result<bool, CodecError> {
            let Ok(_depth) = self.budget.recursion_guard() else {
                return Ok(false);
            };
            if !self.budget.charge() {
                return Ok(false);
            }
            if boundary.len() == self.edges.len() {
                if self.compatible.contains(&(previous_end, first_start)) {
                    let completed = boundary.clone();
                    let boundaries = vec![completed];
                    self.assignments
                        .push(MeshFaceBoundaryAssignment { boundaries });
                }
                return Ok(self.assignments.len() <= self.limit);
            }
            for rank in 1..self.edges.len() {
                if used & (1 << rank) != 0 {
                    continue;
                }
                let edge = self.edges[rank];
                for reversed in [false, true] {
                    let start = edge * 2 + usize::from(reversed);
                    if !self.compatible.contains(&(previous_end, start)) {
                        continue;
                    }
                    boundary.push(MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: Some(reversed),
                    });
                    if !self.walk(
                        first_start,
                        edge * 2 + usize::from(!reversed),
                        used | (1 << rank),
                        boundary,
                    )? {
                        return Ok(false);
                    }
                    boundary.pop();
                }
            }
            Ok(true)
        }
    }

    if edges.is_empty() || edges.len() > index_from_u32(u64::BITS) {
        return Ok(None);
    }
    let edge_count = edges.len();
    let mut edges = edges.to_vec();
    edges.sort_unstable_by(Ord::cmp);
    edges.dedup();
    if edges.len() != edge_count {
        return Ok(None);
    }
    let mut quotient = quotient.clone();
    let mut nodes = Vec::new();
    for &edge in &edges {
        nodes.push(edge * 2);
        nodes.push(edge * 2 + 1);
    }
    let mut compatible = HashSet::new();
    for &left in &nodes {
        let left_root = quotient.union.find(left)?;
        for &right in &nodes {
            let right_root = quotient.union.find(right)?;
            if left_root == right_root
                || !quotient.domains[left_root].is_disjoint(&quotient.domains[right_root])
            {
                compatible.insert((left, right));
            }
        }
    }
    let first = edges[0];
    let first_start = first * 2;
    let mut boundary = Vec::new();
    boundary.push(MeshBoundaryEdgeCandidate {
        edge: first,
        start: 0,
        end: 0,
        reversed: Some(false),
    });
    let mut search = Search {
        edges: &edges,
        compatible: &compatible,
        limit,
        budget,
        assignments: Vec::new(),
    };
    Ok(search
        .walk(first_start, first * 2 + 1, 1, &mut boundary)?
        .then_some(search.assignments))
}

pub(super) fn advance_boundary_component_states(
    ctx: &DecodeContext<'_>,
    domain: &MeshFaceBoundaryDomain,
    states: &[MeshQuotientGaugeState],
    edge_candidates: &[Vec<[usize; 2]>],
    limit: usize,
    budget: &WorkBudget,
) -> Result<Option<Vec<MeshQuotientGaugeState>>, CodecError> {
    let outcome = (|| -> Result<Option<Vec<MeshQuotientGaugeState>>, CodecError> {
        let mut next = Vec::new();
        let mut signatures = HashSet::new();
        let domain_edges = mesh_boundary_domain_edges(domain);
        for (state, oriented_edges) in states {
            // A full state set ends this pass without a result.
            let Some(remaining) = limit
                .checked_add(1)
                .and_then(|end| end.checked_sub(next.len()))
                .filter(|remaining| *remaining != 0)
            else {
                return Ok(None);
            };
            let candidates = match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    let mut candidates = Vec::new();
                    for assignment in assignments {
                        for (_, quotient) in state.assignment_options_limited(
                            assignment,
                            edge_candidates,
                            oriented_edges,
                            remaining,
                            budget,
                        )? {
                            candidates.push(quotient);
                        }
                    }
                    candidates
                }
                MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                    let Some(options) = deferred_face_quotient_options_limited(
                        ctx,
                        domain,
                        edge_candidates,
                        state,
                        remaining,
                        budget,
                    )?
                    else {
                        return Ok(None);
                    };
                    if options.alternatives.is_empty() && domain.missing_edges.is_empty() {
                        vec![state.clone()]
                    } else {
                        let mut affected_edges = HashSet::new();
                        for &edge in domain_edges
                            .iter()
                            .filter(|edge| !edge_candidates[**edge].is_empty())
                        {
                            affected_edges.insert(edge);
                        }
                        let mut candidates = Vec::new();
                        for local in &options.alternatives {
                            if let Some(candidate) = materialize_deferred_quotient_option(
                                state,
                                local,
                                &options.base_nodes,
                                affected_edges.iter().copied(),
                                edge_candidates,
                                budget,
                            )? {
                                candidates.push(candidate);
                            }
                        }
                        candidates
                    }
                }
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                    let Some(assignments) =
                        bounded_unordered_cycle_assignments(edges, state, remaining, budget)?
                    else {
                        return Ok(None);
                    };
                    let mut candidates = Vec::new();
                    for assignment in &assignments {
                        for (_, quotient) in state.assignment_options_limited(
                            assignment,
                            edge_candidates,
                            oriented_edges,
                            remaining,
                            budget,
                        )? {
                            candidates.push(quotient);
                        }
                    }
                    candidates
                }
            };
            for mut candidate in candidates {
                let mut next_oriented = HashSet::new();
                for &edge in oriented_edges.iter().chain(domain_edges.iter()) {
                    next_oriented.insert(edge);
                }
                let Some(work) = candidate
                    .signature_work()?
                    .and_then(|work| work.checked_add(work_units(next_oriented.len())))
                else {
                    return Ok(None);
                };
                if !budget.charge_by(work) {
                    return Ok(None);
                }
                let mut oriented_signature = Vec::new();
                for &edge in &next_oriented {
                    oriented_signature.push(edge);
                }
                oriented_signature.sort_unstable_by(Ord::cmp);
                if signatures.insert((candidate.signature()?, oriented_signature)) {
                    next.push((candidate, next_oriented));
                }
                if next.len() > limit {
                    return Ok(None);
                }
            }
            if budget.exhausted() {
                return Ok(None);
            }
        }
        Ok((!next.is_empty()).then_some(next))
    })()?;
    // Local exhaustion ends this component pass without a state set.
    if budget.exhausted() {
        return Ok(None);
    }
    Ok(outcome)
}

pub(in crate::solve) fn propagate_common_boundary_components(
    ctx: &DecodeContext<'_>,
    domains: &[MeshFaceBoundaryDomain],
    edge_candidates: &[Vec<[usize; 2]>],
    quotient: &mut MeshQuotient,
    budget: &WorkBudget,
) -> Result<Option<()>, CodecError> {
    const MAX_COMPONENT_STATES: usize = 128;
    const MAX_COMPONENT_OPERATIONS: usize = 8_192;
    const MAX_COMPONENT_ROUNDS: usize = 8;

    let mut domain_edges = Vec::new();
    let mut active_faces = Vec::new();
    for (face, domain) in domains.iter().enumerate() {
        let edges = mesh_boundary_domain_edges(domain);
        if edges.iter().any(|edge| edge_candidates[*edge].is_empty()) {
            active_faces.push(face);
        }
        domain_edges.push(edges);
    }
    let mut active_index = HashMap::new();
    for (index, &face) in active_faces.iter().enumerate() {
        active_index.insert(face, index);
    }
    let mut components = UnionFind::new(active_faces.len());
    let mut edge_owner = HashMap::<usize, usize>::new();
    for &face in &active_faces {
        let index = active_index[&face];
        for &edge in &domain_edges[face] {
            if let Some(previous) = edge_owner.insert(edge, index) {
                components.union(previous, index)?;
            }
        }
    }
    let mut faces_by_component = HashMap::<usize, (usize, Vec<usize>)>::new();
    for face in active_faces {
        let root = components.find(active_index[&face])?;
        // `active_faces` is built by an ascending enumeration, so the face that
        // creates a component's entry is that component's smallest face.
        if let Some((_, faces)) = faces_by_component.get_mut(&root) {
            faces.push(face);
        } else {
            let faces = vec![face];
            faces_by_component.insert(root, (face, faces));
        }
    }
    let mut face_components = Vec::new();
    for group in faces_by_component.into_values() {
        face_components.push(group);
    }
    face_components.sort_by_key(|(left, _)| *left);

    let face_keys = domains
        .iter()
        .enumerate()
        .map(|(face, domain)| match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                let mut direction_work = 0usize;
                for assignment in assignments {
                    for boundary in &assignment.boundaries {
                        let unresolved = boundary
                            .iter()
                            .filter(|use_| use_.reversed.is_none())
                            .count();
                        direction_work += unresolved;
                    }
                }
                (0, assignments.len(), direction_work, face)
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                (1, domain.missing_edges.len(), 0, face)
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => (2, edges.len(), 0, face),
        })
        .collect::<Vec<_>>();
    for (_, mut faces) in face_components {
        let mut ordered_faces = Vec::new();
        let mut selected_edges = HashSet::new();
        while !faces.is_empty() {
            let next = faces
                .iter()
                .enumerate()
                .min_by_key(|(_, face)| {
                    let shared = domain_edges[**face]
                        .iter()
                        .filter(|edge| selected_edges.contains(*edge))
                        .count();
                    let key = face_keys[**face];
                    (key.0, usize::MAX - shared, key)
                })
                .map(|(index, _)| index);
            let Some(next) = next else {
                return Ok(None);
            };
            let face = faces.swap_remove(next);
            for &edge in &domain_edges[face] {
                selected_edges.insert(edge);
            }
            ordered_faces.push(face);
        }
        let budget = budget.charging_slice(MAX_COMPONENT_OPERATIONS);
        for _ in 0..MAX_COMPONENT_ROUNDS {
            let Some(before) = quotient.monotone_measure()? else {
                return Ok(None);
            };
            let mut cursor = 0usize;
            while cursor < ordered_faces.len() {
                let mut states = vec![(quotient.clone(), HashSet::<usize>::new())];
                let mut processed = 0usize;
                while let Some(&face) = ordered_faces.get(cursor + processed) {
                    let Some(next) = advance_boundary_component_states(
                        ctx,
                        &domains[face],
                        &states,
                        edge_candidates,
                        MAX_COMPONENT_STATES,
                        &budget,
                    )?
                    else {
                        break;
                    };
                    states = next;
                    processed += 1;
                }
                if processed == 0 {
                    cursor += 1;
                    continue;
                }
                let mut alternatives = Vec::new();
                for (state, _) in states {
                    alternatives.push(state);
                }
                if propagate_common_full_quotients(alternatives, edge_candidates, quotient)?
                    .is_none()
                {
                    return Ok(None);
                }
                cursor += processed;
            }
            let Some(after) = quotient.monotone_measure()? else {
                return Ok(None);
            };
            if after == before {
                break;
            }
        }
    }
    Ok(Some(()))
}

#[cfg(test)]
#[test]
fn ordered_face_local_ceiling_skips_constraint_propagation() {
    crate::test_support::with_service_context(|ctx| {
        let mut quotient =
            MeshQuotient::new((0..130).map(|_| Arc::new(HashSet::from([0]))).collect());
        let domains = [MeshFaceBoundaryDomain::Ordered(
            (0..65)
                .map(|edge| MeshFaceBoundaryAssignment {
                    boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: Some(false),
                    }]],
                })
                .collect(),
        )];
        let candidates = vec![Vec::new(); 65];
        let budget = cadmpeg_core::decode::WorkBudget::new(1_000_000);
        // 65 edges exceed the 64-unit face slice: the face contributes no
        // equations and the quotient keeps its 130 independent roots.
        propagate_common_ordered_face_quotients(ctx, &domains, &candidates, &mut quotient, &budget)
            .expect("a face over its local ceiling is skipped")
            .expect("skipping a face leaves the quotient viable");
        assert_eq!(quotient.root_count().expect("root count"), 130);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[cfg(test)]
mod preparation_tests {
    use super::{propagate_common_ordered_face_quotients, MeshQuotient};
    use crate::solve::missing_edge::{
        MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
    };
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn directed_face_merges_only_its_corner_roots() {
        crate::test_support::with_service_context(|ctx| {
            let shared = Arc::new(HashSet::from([0, 1]));
            let mut quotient = MeshQuotient::new(vec![shared; 1_000]);
            let domains = [MeshFaceBoundaryDomain::Ordered(vec![
                MeshFaceBoundaryAssignment {
                    boundaries: vec![(0..4)
                        .map(|edge| MeshBoundaryEdgeCandidate {
                            edge,
                            start: edge,
                            end: (edge + 1) % 4,
                            reversed: Some(false),
                        })
                        .collect()],
                },
            ])];
            let candidates = vec![Vec::new(); 500];
            let budget = cadmpeg_core::decode::WorkBudget::new(1_000_000);
            propagate_common_ordered_face_quotients(
                ctx,
                &domains,
                &candidates,
                &mut quotient,
                &budget,
            )
            .expect("directed face work fits")
            .expect("complete corner equations");
            for [left, right] in [[1, 2], [3, 4], [5, 6], [7, 0]] {
                assert_eq!(
                    quotient.root(left).expect("corner root"),
                    quotient.root(right).expect("corner root")
                );
            }
            assert_eq!(quotient.root_count().expect("root count"), 996);
            assert_eq!(quotient.root(8).expect("independent root"), 8);
        });
    }
}

#[cfg(test)]
#[test]
fn boundary_component_exhausted_slice_returns_no_states() {
    crate::test_support::with_service_context(|ctx| {
        let budget = cadmpeg_core::decode::WorkBudget::new(0);
        assert!(!budget.charge());
        let domain = MeshFaceBoundaryDomain::Ordered(Vec::new());
        assert!(
            advance_boundary_component_states(ctx, &domain, &[], &[], 128, &budget)
                .expect("component slice exhaustion is not a session refusal")
                .is_none()
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}
