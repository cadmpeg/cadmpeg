// SPDX-License-Identifier: Apache-2.0
//! Endpoint-relation constraints, propagation, and streaming resolution.

use super::endpoint_configurations::{
    endpoint_configuration_cycles_viable, endpoint_configuration_for_assignment,
};
use super::endpoint_resolution::{
    resolve_fixed_mesh_endpoint_assignment_domains, resolve_fixed_mesh_endpoint_pairs,
    MeshEndpointGeometry,
};
use super::face_domain_support::mesh_candidate_point_pairs;
use super::quotient::MeshQuotient;
use super::root_domains::MeshCoordinateRootDomains;
use super::{
    bitset_words, MeshCandidateFailure, MeshEndpointResolve, MeshFaceEndpointConfiguration,
    MeshFaceEndpointConfigurations, MeshSolve, MAX_MESH_CONSTRAINT_OPERATIONS,
    MAX_SELECTION_STATE_MEMO_ENTRIES,
};
#[cfg(test)]
use crate::families::standard::topology::EdgeBoundaryLayout;
use crate::families::standard::topology::EdgeRow;
use crate::solve::mesh_gauge::{
    canonicalize_complete_endpoint_pairs, canonicalize_endpoint_relation_state,
    mesh_candidates_equivalent, MeshCandidateGauge,
};
#[cfg(test)]
use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;
use crate::solve::missing_edge::{same_unordered_pair, MeshFaceBoundaryAssignment};
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

#[derive(Clone)]
pub(in crate::solve) struct MeshEndpointRelationChoice {
    pub(in crate::solve) id: usize,
    pub(in crate::solve) selection: MeshEndpointRelationSelection,
}

/// Enumerated endpoint relation or deferred enumeration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::solve) enum MeshEndpointRelationSelection {
    Enumerated {
        assignments: Vec<usize>,
        edge_pairs: MeshFaceEndpointConfiguration,
    },
    Deferred,
}

impl MeshEndpointRelationSelection {
    /// Explicit edge constraints of this selection.
    pub(in crate::solve) fn edge_pairs(&self) -> &[(usize, [usize; 2])] {
        match self {
            Self::Enumerated { edge_pairs, .. } => edge_pairs,
            Self::Deferred => &[],
        }
    }

    pub(super) fn is_unconstrained(&self) -> bool {
        match self {
            Self::Enumerated { edge_pairs, .. } => edge_pairs.is_empty(),
            Self::Deferred => true,
        }
    }

    /// Selection with canonical assignment and endpoint ordering.
    pub(in crate::solve) fn normalized(&self) -> Self {
        match self {
            Self::Enumerated {
                assignments,
                edge_pairs,
            } => {
                let mut assignments = assignments.clone();
                assignments.sort_unstable_by(Ord::cmp);
                assignments.dedup();
                let mut edge_pairs = edge_pairs.clone();
                for (_, pair) in &mut edge_pairs {
                    pair.sort_unstable_by(Ord::cmp);
                }
                edge_pairs.sort_unstable_by(Ord::cmp);
                Self::Enumerated {
                    assignments,
                    edge_pairs,
                }
            }
            Self::Deferred => Self::Deferred,
        }
    }
}

pub(super) type MeshEndpointRelationSelections = Vec<Vec<usize>>;

pub(in crate::solve) type MeshEndpointRelationStateSignature = (
    Vec<Option<[usize; 2]>>,
    Vec<Vec<MeshEndpointRelationSelection>>,
);

pub(super) type MeshEndpointSolutionPredicate<'a> = dyn Fn(&[Option<[usize; 2]>]) -> bool + 'a;

pub(super) type MeshFixedDirectionOption = (Vec<Vec<bool>>, MeshQuotient, Vec<Option<bool>>);

pub(in crate::solve) fn raw_endpoint_relation_state_signature(
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
) -> MeshEndpointRelationStateSignature {
    let mut normalized_assigned = Vec::new();
    for pair in assigned {
        let pair = match *pair {
            Some(mut pair) => {
                pair.sort_unstable_by(Ord::cmp);
                Some(pair)
            }
            None => None,
        };
        normalized_assigned.push(pair);
    }
    let mut normalized_domains = Vec::new();
    for choices in domains {
        let mut row = Vec::new();
        for choice in choices {
            let normalized = choice.selection.normalized();
            row.push(normalized);
        }
        row.sort_unstable_by(Ord::cmp);
        normalized_domains.push(row);
    }
    (normalized_assigned, normalized_domains)
}

pub(super) fn endpoint_relation_state_signature(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<Option<MeshEndpointRelationStateSignature>, CodecError> {
    if let Some(gauge) = candidate_gauge {
        canonicalize_endpoint_relation_state(ctx, domains, assigned, gauge)
    } else {
        Ok(Some(raw_endpoint_relation_state_signature(
            domains, assigned,
        )))
    }
}

/// Return the edge-pair superset admitted by the surviving relation domains.
/// A relation branch can only select pairs from this set, so coordinate
/// infeasibility of the superset is a sound branch rejection.
pub(super) fn relation_coordinate_candidate_domains(
    domains: &[Vec<MeshEndpointRelationChoice>],
    assigned: &[Option<[usize; 2]>],
    base_candidates: &[Vec<[usize; 2]>],
) -> Option<Vec<Vec<[usize; 2]>>> {
    if assigned.len() != base_candidates.len() {
        return None;
    }
    let has_unconstrained_choice = domains
        .iter()
        .flatten()
        .any(|choice| choice.selection.is_unconstrained());
    let mut candidates = Vec::new();
    let mut possible = Vec::new();
    for base in base_candidates {
        let copy = base.clone();
        candidates.push(copy);
        possible.push(Vec::<[usize; 2]>::new());
    }
    if !has_unconstrained_choice {
        for choice in domains.iter().flatten() {
            for &(edge, pair) in choice.selection.edge_pairs() {
                let row = possible.get_mut(edge)?;
                row.push(pair);
            }
        }
    }
    for (edge, assigned) in assigned.iter().enumerate() {
        if let Some(pair) = assigned {
            candidates[edge].retain(|candidate| same_unordered_pair(*candidate, *pair));
        } else if !has_unconstrained_choice && !possible[edge].is_empty() {
            candidates[edge].retain(|candidate| {
                possible[edge]
                    .iter()
                    .any(|possible| same_unordered_pair(*candidate, *possible))
            });
        }
        if candidates[edge].is_empty() {
            return None;
        }
    }
    Some(candidates)
}

#[derive(Clone)]
pub(super) struct MeshEndpointRelationArc {
    pub(super) neighbor: usize,
    pub(super) supports: Vec<Vec<u64>>,
}

pub(super) struct MeshEndpointRelationConstraints {
    pub(super) arcs: Vec<Vec<MeshEndpointRelationArc>>,
    pub(super) incoming: Vec<Vec<(usize, usize)>>,
    pub(super) choice_counts: Vec<usize>,
}

pub(super) fn canonical_mesh_boundary_directions(directions: &[Vec<bool>]) -> Vec<Vec<bool>> {
    let mut canonical = Vec::new();

    for row in directions {
        let mut complement = row.clone();
        for direction in &mut complement {
            *direction = !*direction;
        }
        canonical.push(if complement < *row {
            complement
        } else {
            row.clone()
        });
    }
    canonical
}

pub(super) type EndpointRelationKey = Vec<Option<[usize; 2]>>;

pub(super) type EndpointRelationKeys<'a> =
    Vec<(&'a MeshEndpointRelationChoice, EndpointRelationKey)>;

pub(super) fn canonical_endpoint_relation_key(
    choice: &MeshEndpointRelationChoice,
    edges: &[usize],
) -> EndpointRelationKey {
    let mut key = Vec::new();
    for &edge in edges {
        let pair = choice
            .selection
            .edge_pairs()
            .iter()
            .find_map(|&(candidate, pair)| (candidate == edge).then_some(pair))
            .map(|mut pair| {
                if pair[1] < pair[0] {
                    pair.swap(0, 1);
                }
                pair
            });
        key.push(pair);
    }
    key
}

pub(super) fn complete_endpoint_relation_keys<'a>(
    choices: &'a [MeshEndpointRelationChoice],
    edges: &[usize],
) -> (bool, EndpointRelationKeys<'a>) {
    let mut keys = Vec::new();
    for choice in choices {
        let key = canonical_endpoint_relation_key(choice, edges);
        keys.push((choice, key));
    }
    let complete = keys.iter().all(|(_, key)| key.iter().all(Option::is_some));
    (complete, keys)
}

pub(super) fn build_endpoint_relation_constraints(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<MeshEndpointRelationChoice>],
    budget: &WorkBudget,
) -> Result<Option<MeshEndpointRelationConstraints>, CodecError> {
    let mut shared_edges = HashMap::<(usize, usize), Vec<usize>>::new();
    let mut edge_faces = HashMap::<usize, HashSet<usize>>::new();
    for (face, choices) in domains.iter().enumerate() {
        for choice in choices {
            for &(edge, _) in choice.selection.edge_pairs() {
                edge_faces.entry(edge).or_default().insert(face);
            }
        }
    }
    for (edge, faces) in edge_faces {
        let mut sorted_faces = Vec::new();
        for face in faces {
            sorted_faces.push(face);
        }
        sorted_faces.sort_unstable_by(Ord::cmp);
        for (left_index, &left) in sorted_faces.iter().enumerate() {
            for &right in &sorted_faces[left_index + 1..] {
                for pair in [(left, right), (right, left)] {
                    shared_edges.entry(pair).or_default().push(edge);
                }
            }
        }
    }

    let mut shared_rows = Vec::new();
    for (pair, mut edges) in shared_edges {
        edges.sort_unstable_by(Ord::cmp);
        shared_rows.push((pair, edges));
    }
    shared_rows.sort_unstable_by_key(|(left, _)| *left);
    let mut arcs = vec![Vec::new(); domains.len()];
    let mut incoming = vec![Vec::new(); domains.len()];
    let mut choice_counts = Vec::new();
    for domain in domains {
        choice_counts.push(domain.len());
    }
    for ((face, neighbor), edges) in shared_rows {
        // A pair is in `shared_edges` only because both faces are in
        // `edge_faces` for the shared edge, and a face reaches `edge_faces`
        // only through an edge one of its own choices names. Both domains
        // therefore hold at least one choice and the charge is at least two,
        // so the pair owes no empty-step unit.
        // Plain `+`: both operands are lengths of live allocations, so each is
        // at most `isize::MAX` and their sum is inside `usize`.
        let index_work = domains[face].len() + domains[neighbor].len();
        if !budget.charge_by(index_work) {
            return Ok(None);
        }
        let (left_complete, left_choices) = complete_endpoint_relation_keys(&domains[face], &edges);
        let (right_complete, right_choices) =
            complete_endpoint_relation_keys(&domains[neighbor], &edges);
        let supports = if left_complete && right_complete {
            let mut index = HashMap::<EndpointRelationKey, Vec<usize>>::new();
            for (choice, key) in right_choices {
                index.entry(key).or_default().push(choice.id);
            }
            let mut supports = Vec::new();
            for (_, key) in &left_choices {
                let mut mask = ctx.alloc_filled(
                    bitset_words(domains[neighbor].len()),
                    0u64,
                    "catia_endpoint_relation_support_mask",
                )?;
                for &other in index.get(key).into_iter().flatten() {
                    mask[other / 64] |= 1u64 << (other % 64);
                }
                supports.push(mask);
            }
            supports
        } else {
            let Some(comparisons) = domains[face].len().checked_mul(domains[neighbor].len()) else {
                return Ok(None);
            };
            let comparison_work = work_units(comparisons);
            if !budget.charge_by(comparison_work) {
                return Ok(None);
            }
            let mut supports = Vec::new();
            for (_, left_key) in &left_choices {
                let mut mask = ctx.alloc_filled(
                    bitset_words(domains[neighbor].len()),
                    0u64,
                    "catia_endpoint_relation_support_mask",
                )?;
                for (other, right_key) in &right_choices {
                    let compatible = left_key.iter().zip(right_key).all(|(left, right)| {
                        left.as_ref()
                            .zip(right.as_ref())
                            .is_none_or(|(left, right)| same_unordered_pair(*left, *right))
                    });
                    if compatible {
                        mask[other.id / 64] |= 1u64 << (other.id % 64);
                    }
                }
                supports.push(mask);
            }
            supports
        };
        let arc_index = arcs[face].len();
        arcs[face].push(MeshEndpointRelationArc { neighbor, supports });
        incoming[neighbor].push((face, arc_index));
    }
    Ok(Some(MeshEndpointRelationConstraints {
        arcs,
        incoming,
        choice_counts,
    }))
}

pub(super) fn propagate_endpoint_relation_domains(
    ctx: &DecodeContext<'_>,
    domains: &mut [Vec<MeshEndpointRelationChoice>],
    assigned: &mut [Option<[usize; 2]>],
    constraints: &MeshEndpointRelationConstraints,
    budget: &WorkBudget,
) -> Result<bool, CodecError> {
    let mut dirty_faces = Vec::new();
    for (face, choices) in domains.iter().enumerate() {
        if choices.len() != constraints.choice_counts[face] {
            dirty_faces.push(face);
        }
    }
    let mut first_pass = true;
    loop {
        let mut changed = false;
        for (face, choices) in domains.iter_mut().enumerate() {
            if !budget.charge_by(work_units(choices.len())) {
                return Ok(false);
            }
            let before = choices.len();
            choices.retain(|choice| {
                choice.selection.edge_pairs().iter().all(|&(edge, pair)| {
                    assigned[edge].is_none_or(|selected| same_unordered_pair(selected, pair))
                })
            });
            if choices.is_empty() {
                return Ok(false);
            }
            if choices.len() != before {
                dirty_faces.push(face);
                changed = true;
            }
        }

        let mut active = Vec::new();
        for &choice_count in &constraints.choice_counts {
            let mask = ctx.alloc_filled(
                bitset_words(choice_count),
                0u64,
                "catia_endpoint_relation_active_mask",
            )?;
            active.push(mask);
        }
        for (face, choices) in domains.iter().enumerate() {
            for choice in choices {
                let Some(active_word) = active[face].get_mut(choice.id / 64) else {
                    return Ok(false);
                };
                *active_word |= 1u64 << (choice.id % 64);
            }
        }
        let mut queue = VecDeque::new();
        if first_pass && dirty_faces.is_empty() {
            for (face, arcs) in constraints.arcs.iter().enumerate() {
                for arc in 0..arcs.len() {
                    queue.push_back((face, arc));
                }
            }
        } else {
            for face in dirty_faces.drain(..) {
                for &incoming in &constraints.incoming[face] {
                    queue.push_back(incoming);
                }
            }
        }
        first_pass = false;
        while let Some((face, arc_index)) = queue.pop_front() {
            let arc = &constraints.arcs[face][arc_index];
            let neighbor_active = &active[arc.neighbor];
            let before = domains[face].len();
            domains[face].retain(|choice| {
                let Some(supports) = arc.supports.get(choice.id) else {
                    return false;
                };
                if !budget.charge_by(work_units(supports.len())) {
                    return false;
                }
                supports
                    .iter()
                    .zip(neighbor_active)
                    .any(|(supported, active)| supported & active != 0)
            });
            if budget.exhausted() || domains[face].is_empty() {
                return Ok(false);
            }
            if domains[face].len() == before {
                continue;
            }
            active[face].fill(0);
            for choice in &domains[face] {
                active[face][choice.id / 64] |= 1u64 << (choice.id % 64);
            }
            changed = true;
            for &incoming in constraints.incoming[face]
                .iter()
                .filter(|&&(source, _)| source != arc.neighbor)
            {
                queue.push_back(incoming);
            }
        }
        // A pair present with one value in every surviving choice of one face
        // is a forced edge relation, even when the face still has assignment
        // or boundary-direction alternatives. Record it before branching on
        // those independent alternatives.
        for choices in domains.iter() {
            if !budget.charge_by(work_units(choices.len())) {
                return Ok(false);
            }
            let choice_count = choices.len();
            let mut pairs = HashMap::<usize, HashSet<[usize; 2]>>::new();
            let mut counts = HashMap::<usize, usize>::new();
            for choice in choices {
                for &(edge, pair) in choice.selection.edge_pairs() {
                    pairs.entry(edge).or_default().insert(pair);

                    *counts.entry(edge).or_default() += 1;
                }
            }
            for (edge, values) in pairs {
                if counts.get(&edge) != Some(&choice_count) || values.len() != 1 {
                    continue;
                }
                let Some(&pair) = values.iter().next() else {
                    continue;
                };
                match assigned[edge] {
                    Some(selected) if !same_unordered_pair(selected, pair) => return Ok(false),
                    Some(_) => {}
                    None => {
                        assigned[edge] = Some(pair);
                        changed = true;
                    }
                }
            }
        }
        for choices in domains.iter() {
            if choices.len() != 1 {
                continue;
            }
            for &(edge, pair) in choices[0].selection.edge_pairs() {
                match assigned[edge] {
                    Some(selected) if !same_unordered_pair(selected, pair) => return Ok(false),
                    Some(_) => {}
                    None => {
                        assigned[edge] = Some(pair);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            return Ok(true);
        }
    }
}

// The recursive walk keeps branch-owned domains and shared memo state explicit;
// a context object would hide which values are cloned for each branch.
pub(super) struct WalkEndpointRelationDomainsInputs<
    'input0,
    'input1,
    'input3,
    'input4,
    'input5,
    'input6,
    'input7,
    'input8,
    'input9,
    'input11,
    'input12,
    F,
> where
    F: FnMut(MeshEndpointRelationSelections, Vec<[usize; 2]>) -> Result<bool, CodecError>,
{
    pub(super) domains: Vec<Vec<MeshEndpointRelationChoice>>,
    pub(super) face_assignments: &'input0 [Vec<MeshFaceBoundaryAssignment>],
    pub(super) assigned: Vec<Option<[usize; 2]>>,
    pub(super) constraints: &'input1 MeshEndpointRelationConstraints,
    pub(super) point_count: usize,
    pub(super) budget: &'input3 WorkBudget,
    pub(super) state_memo: &'input4 mut HashSet<MeshEndpointRelationStateSignature>,
    pub(super) candidate_gauge: Option<MeshCandidateGauge<'input5>>,
    pub(super) priority_edges: Option<&'input6 [bool]>,
    pub(super) partial_solution_valid: Option<&'input8 MeshEndpointSolutionPredicate<'input7>>,
    pub(super) coordinate_domains: Option<&'input9 MeshCoordinateRootDomains>,
    pub(super) coordinate_budget: Option<&'input11 WorkBudget>,
    pub(super) evaluate: &'input12 mut F,
}

pub(super) fn walk_endpoint_relation_domains<F>(
    ctx: &DecodeContext<'_>,
    inputs: WalkEndpointRelationDomainsInputs<'_, '_, '_, '_, '_, '_, '_, '_, '_, '_, '_, F>,
) -> Result<bool, CodecError>
where
    F: FnMut(MeshEndpointRelationSelections, Vec<[usize; 2]>) -> Result<bool, CodecError>,
{
    let WalkEndpointRelationDomainsInputs {
        domains,
        face_assignments,
        assigned,
        constraints,
        point_count,
        budget,
        state_memo,
        candidate_gauge,
        priority_edges,
        partial_solution_valid,
        coordinate_domains,
        coordinate_budget,
        evaluate,
    } = inputs;

    let Ok(_depth) = budget.recursion_guard() else {
        return Ok(true);
    };
    if budget.exhausted() || !budget.charge() {
        return Ok(true);
    }
    let mut domains = domains;
    let mut assigned = assigned;
    let propagated = if domains.iter().all(|choices| choices.len() == 1) {
        true
    } else {
        propagate_endpoint_relation_domains(ctx, &mut domains, &mut assigned, constraints, budget)?
    };
    if !propagated {
        return Ok(false);
    }
    // The partial predicate is monotone by contract: it can reject only
    // assignments that no completion can repair. Apply it as soon as
    // propagation fixes endpoint pairs, before branching over domains.
    if let Some(valid) = partial_solution_valid {
        if !valid(&assigned) {
            return Ok(false);
        }
    }
    // The final coordinate binding is surjective: every coordinate row must
    // occur in the selected endpoint pairs. When every surviving relation
    // choice has an explicit edge configuration, their point union is an
    // upper bound for every completion of this branch.
    if domains
        .iter()
        .flatten()
        .all(|choice| !choice.selection.is_unconstrained())
    {
        let mut possible_points = HashSet::new();
        for point in assigned.iter().flatten().flatten().copied() {
            possible_points.insert(point);
        }
        for point in domains
            .iter()
            .flatten()
            .flat_map(|choice| {
                choice
                    .selection
                    .edge_pairs()
                    .iter()
                    .flat_map(|(_, pair)| pair)
            })
            .copied()
        {
            possible_points.insert(point);
        }
        if possible_points.len() < point_count {
            return Ok(false);
        }
    }
    if let (Some(coordinate_domains), Some(coordinate_budget)) =
        (coordinate_domains, coordinate_budget)
    {
        if !coordinate_budget.exhausted() {
            let Some(candidates) = relation_coordinate_candidate_domains(
                &domains,
                &assigned,
                coordinate_domains.edge_candidates(),
            ) else {
                return Ok(false);
            };
            if coordinate_domains
                .refine_candidates(ctx, &candidates, Some(coordinate_budget))?
                .is_none()
                && !coordinate_budget.exhausted()
            {
                return Ok(false);
            }
        }
    }
    if state_memo.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
        // resource-cap: MAX_GAUGE_STATE_ALTERNATIVES = N
        const MAX_GAUGE_STATE_ALTERNATIVES: usize = 512;
        let alternative_count = domains.iter().map(Vec::len).sum::<usize>();
        let signature =
            if candidate_gauge.is_some() && alternative_count <= MAX_GAUGE_STATE_ALTERNATIVES {
                endpoint_relation_state_signature(ctx, &domains, &assigned, candidate_gauge)?
            } else {
                Some(raw_endpoint_relation_state_signature(&domains, &assigned))
            };
        if let Some(signature) = signature {
            if !state_memo.insert(signature) {
                return Ok(false);
            }
        }
    }
    let mut priority_counts = Vec::new();
    if let Some(edges) = priority_edges.filter(|_| domains.iter().any(|choices| choices.len() > 1))
    {
        for choices in &domains {
            let mut priority = HashSet::new();
            for edge in choices
                .iter()
                .flat_map(|choice| choice.selection.edge_pairs().iter().map(|(edge, _)| *edge))
            {
                if edges.get(edge).copied().unwrap_or(false) {
                    priority.insert(edge);
                }
            }
            priority_counts.push(priority.len());
        }
    }
    // Visit a face touching a monotone preference-dependent edge before an
    // unrelated face. Within each tier use minimum remaining values first;
    // relation degree is only a deterministic tie breaker.
    let Some((face, choices)) = domains
        .iter()
        .enumerate()
        .filter(|(_, choices)| choices.len() > 1)
        .min_by_key(|(face, choices)| {
            let priority_count = priority_counts.get(*face).copied().unwrap_or(0);
            (
                priority_count == 0,
                std::cmp::Reverse(priority_count),
                choices.len(),
                std::cmp::Reverse(constraints.arcs.get(*face).map_or(0, Vec::len)),
                *face,
            )
        })
    else {
        let mut edge_pairs = assigned;
        for choice in domains.iter().filter_map(|choices| choices.first()) {
            for &(edge, pair) in choice.selection.edge_pairs() {
                match edge_pairs[edge] {
                    Some(selected) if !same_unordered_pair(selected, pair) => return Ok(false),
                    Some(_) => {}
                    None => edge_pairs[edge] = Some(pair),
                }
            }
        }
        let mut completed_pairs = Vec::new();
        for pair in edge_pairs {
            let Some(pair) = pair else {
                return Ok(false);
            };
            completed_pairs.push(pair);
        }
        let mut selections = Vec::new();
        for (face, choices) in domains.iter().enumerate() {
            let Some(choice) = choices.first() else {
                return Ok(false);
            };
            let viable = if let MeshEndpointRelationSelection::Enumerated { assignments, .. } =
                &choice.selection
            {
                assignments.clone()
            } else {
                let mut viable = Vec::new();
                for (assignment, assignment_value) in face_assignments[face].iter().enumerate() {
                    let Some(configuration) =
                        endpoint_configuration_for_assignment(assignment_value, &completed_pairs)
                    else {
                        continue;
                    };
                    if endpoint_configuration_cycles_viable(assignment_value, &configuration)
                        != Some(true)
                    {
                        continue;
                    }
                    viable.push(assignment);
                }
                if viable.is_empty() {
                    return Ok(false);
                }
                viable
            };
            selections.push(viable);
        }
        return evaluate(selections, completed_pairs);
    };
    let mut assigned_points = HashSet::new();
    for point in assigned.iter().flatten().flatten().copied() {
        assigned_points.insert(point);
    }
    let mut point_support = HashMap::<usize, usize>::new();
    for choices in &domains {
        for choice in choices {
            let mut points = HashSet::new();
            for point in choice
                .selection
                .edge_pairs()
                .iter()
                .flat_map(|(_, pair)| pair)
                .copied()
            {
                points.insert(point);
            }
            for point in points {
                *point_support.entry(point).or_default() += 1;
            }
        }
    }
    let mut branch_order = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        let mut points = HashSet::new();
        for point in choice
            .selection
            .edge_pairs()
            .iter()
            .flat_map(|(_, pair)| pair)
            .copied()
        {
            points.insert(point);
        }
        let Some(score) = points
            .into_iter()
            .filter(|point| !assigned_points.contains(point))
            .try_fold(0usize, |score, point| {
                point_count
                    .checked_sub(point_support.get(&point).copied().unwrap_or(0))?
                    .checked_add(1)
                    .and_then(|contribution| score.checked_add(contribution))
            })
        else {
            return Ok(true);
        };
        branch_order.push((score, index));
    }
    branch_order.sort_unstable_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| choices[*left].id.cmp(&choices[*right].id))
    });
    for (_, index) in branch_order {
        if budget.exhausted() {
            return Ok(true);
        }
        let mut branch = domains.clone();
        branch[face].clear();
        let choice = &choices[index];
        let selection = choice.selection.clone();
        branch[face].push(MeshEndpointRelationChoice {
            id: choice.id,
            selection,
        });
        if walk_endpoint_relation_domains(
            ctx,
            WalkEndpointRelationDomainsInputs {
                domains: branch,
                face_assignments,
                assigned: assigned.clone(),
                constraints,
                point_count,
                budget,
                state_memo,
                candidate_gauge,
                priority_edges,
                partial_solution_valid,
                coordinate_domains,
                coordinate_budget,
                evaluate,
            },
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Collect one face's endpoint relation choices. A missing configuration list
/// is an unknown result from bounded enumeration, not an empty domain.
pub(super) fn collect_endpoint_relation_face_choices(
    face_assignments: &[MeshFaceBoundaryAssignment],
    face_configurations: &[Option<MeshFaceEndpointConfigurations>],
    covered: &mut [bool],
) -> Option<Vec<MeshEndpointRelationChoice>> {
    let mut choices_by_configuration = HashMap::<MeshFaceEndpointConfiguration, Vec<usize>>::new();
    let mut unknown = false;
    for (assignment, configurations) in face_configurations.iter().enumerate() {
        let Some(configurations) = configurations else {
            unknown = true;
            continue;
        };
        for configuration in configurations {
            for &(edge, _) in configuration {
                let covered = covered.get_mut(edge)?;
                *covered = true;
            }
            let face_assignment = face_assignments.get(assignment)?;
            if endpoint_configuration_cycles_viable(face_assignment, configuration) != Some(true) {
                continue;
            }
            let mut relation_configuration = configuration.clone();
            for (_, pair) in &mut relation_configuration {
                pair.sort_unstable_by(Ord::cmp);
            }
            relation_configuration.sort_unstable_by(Ord::cmp);

            (choices_by_configuration
                .entry(relation_configuration)
                .or_default())
            .push(assignment);
        }
    }
    let mut choices = Vec::new();
    for (edge_pairs, mut assignments) in choices_by_configuration {
        assignments.sort_unstable_by(Ord::cmp);
        assignments.dedup();
        choices.push(MeshEndpointRelationChoice {
            id: 0,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments,
                edge_pairs,
            },
        });
    }
    choices.sort_unstable_by(|left, right| {
        (left.selection.edge_pairs(), &left.selection)
            .cmp(&(right.selection.edge_pairs(), &right.selection))
    });
    if unknown {
        // A stopped enumeration is not evidence that the assignment has no
        // configuration. The wildcard lets the relation walker defer that
        // assignment to the complete endpoint search.
        choices.push(MeshEndpointRelationChoice {
            id: 0,
            selection: MeshEndpointRelationSelection::Deferred,
        });
    }
    for (id, choice) in choices.iter_mut().enumerate() {
        choice.id = id;
    }
    Some(choices)
}

/// Solve the unordered endpoint-configuration relation before selecting
/// intrinsic cycle orientations. A configuration records one candidate pair
/// for every edge in a face. Shared edges must agree on that pair, but their
/// row-port order is selected later by the quotient search.
// These independent inputs describe one bounded relation phase; keeping them
// separate preserves the ownership of parsed evidence and branch state.
#[derive(Clone, Copy)]
pub(super) struct ResolveEndpointConfigurationRelationStreamingInputs<
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
    'input7,
    'input8,
    'input9,
    'input10,
    'input11,
    'input12,
    'input13,
    'input14,
> {
    pub(super) assignments: &'input0 [Vec<MeshFaceBoundaryAssignment>],
    pub(super) endpoint_configurations: &'input1 [Vec<Option<MeshFaceEndpointConfigurations>>],
    pub(super) edge_candidates: &'input2 [Vec<[usize; 2]>],
    pub(super) edge_rows: &'input3 [EdgeRow],
    pub(super) vertex_points: &'input4 [[f64; 3]],
    pub(super) port_identities: &'input5 [[u32; 2]],
    pub(super) budget: &'input7 WorkBudget,
    pub(super) partial_solution_valid: Option<&'input9 MeshEndpointSolutionPredicate<'input8>>,
    pub(super) complete_solution_valid: Option<&'input11 MeshEndpointSolutionPredicate<'input10>>,
    pub(super) candidate_gauge: Option<MeshCandidateGauge<'input12>>,
    pub(super) priority_edges: Option<&'input13 [bool]>,
    pub(super) coordinate_domains: Option<&'input14 MeshCoordinateRootDomains>,
}

pub(super) fn resolve_endpoint_configuration_relation_streaming(
    ctx: &DecodeContext<'_>,
    inputs: ResolveEndpointConfigurationRelationStreamingInputs<
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
) -> Result<Option<MeshEndpointResolve>, CodecError> {
    let ResolveEndpointConfigurationRelationStreamingInputs {
        assignments,
        endpoint_configurations,
        edge_candidates,
        edge_rows,
        vertex_points,
        port_identities,
        budget,
        partial_solution_valid,
        complete_solution_valid,
        candidate_gauge,
        priority_edges,
        coordinate_domains,
    } = inputs;

    if assignments.len() != endpoint_configurations.len()
        || edge_candidates.iter().any(Vec::is_empty)
    {
        return Ok(None);
    }
    let mut domains = Vec::new();
    let mut covered = vec![false; edge_candidates.len()];
    for (face_assignments, face_configurations) in assignments.iter().zip(endpoint_configurations) {
        if face_assignments.len() != face_configurations.len() {
            return Ok(None);
        }
        let choices = collect_endpoint_relation_face_choices(
            face_assignments,
            face_configurations,
            &mut covered,
        );
        let Some(choices) = choices else {
            return Ok(None);
        };
        if choices.is_empty() {
            return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Rejected(()))));
        }
        if !budget.charge_by(choices.len()) {
            return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))));
        }
        domains.push(choices);
    }
    if covered.iter().any(|covered| !covered) {
        return Ok(None);
    }
    let Some(constraints) = build_endpoint_relation_constraints(ctx, &domains, budget)? else {
        return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))));
    };
    let mut resolved = None;
    let mut relation_state_memo =
        HashSet::<(MeshEndpointRelationSelections, Vec<[usize; 2]>)>::new();
    let mut relation_walk_state_memo = HashSet::<MeshEndpointRelationStateSignature>::new();
    let coordinate_budget =
        coordinate_domains.map(|_| budget.charging_slice(MAX_MESH_CONSTRAINT_OPERATIONS));
    let mut ambiguous = false;
    let mut exhausted = false;
    let mut evaluate = |selections: MeshEndpointRelationSelections,
                        edge_pairs: Vec<[usize; 2]>|
     -> Result<bool, CodecError> {
        let mut point_set = HashSet::new();
        for &point in edge_pairs.iter().flatten() {
            point_set.insert(point);
        }
        if point_set.len() != vertex_points.len() {
            return Ok(false);
        }
        if let Some(valid) = partial_solution_valid {
            let mut candidate_pairs = Vec::new();
            for &pair in &edge_pairs {
                candidate_pairs.push(Some(pair));
            }
            if !valid(&candidate_pairs) {
                return Ok(false);
            }
        }
        if relation_state_memo.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let canonical_pairs = if let Some(gauge) = candidate_gauge {
                canonicalize_complete_endpoint_pairs(&edge_pairs, gauge)
            } else {
                Some(edge_pairs.clone())
            };
            let Some(canonical_pairs) = canonical_pairs else {
                return Ok(false);
            };
            let selection_copy = selections.clone();
            if !relation_state_memo.insert((selection_copy, canonical_pairs)) {
                return Ok(false);
            }
        }
        let mut assignment_domains = Vec::new();
        for (face, options) in selections.iter().enumerate() {
            let mut row = Vec::new();
            for &assignment in options {
                let Some(source) = assignments[face].get(assignment) else {
                    continue;
                };
                let mut boundaries = Vec::new();
                for boundary in &source.boundaries {
                    let copy = boundary.clone();
                    boundaries.push(copy);
                }
                row.push(MeshFaceBoundaryAssignment { boundaries });
            }
            assignment_domains.push(row);
        }
        if assignment_domains.iter().any(Vec::is_empty) {
            return Ok(false);
        }
        let mut candidates = Vec::new();
        for &pair in &edge_pairs {
            let row = vec![pair];
            candidates.push(row);
        }
        let endpoint_resolution_budget = budget.charging_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
        let outcome = if assignment_domains.iter().all(|domain| domain.len() == 1) {
            let mut selected = Vec::new();
            let mut complete = true;
            for mut domain in assignment_domains {
                if let Some(choice) = domain.pop() {
                    selected.push(choice);
                } else {
                    complete = false;
                    break;
                }
            }
            if complete {
                resolve_fixed_mesh_endpoint_pairs(
                    ctx,
                    MeshEndpointGeometry {
                        edge_rows,
                        vertex_points,
                    },
                    &candidates,
                    &selected,
                    port_identities,
                    &endpoint_resolution_budget,
                    candidate_gauge,
                )?
            } else {
                MeshSolve::Failed(MeshCandidateFailure::Rejected(()))
            }
        } else {
            resolve_fixed_mesh_endpoint_assignment_domains(
                ctx,
                MeshEndpointGeometry {
                    edge_rows,
                    vertex_points,
                },
                &candidates,
                &assignment_domains,
                port_identities,
                &endpoint_resolution_budget,
                candidate_gauge,
            )?
        };
        match outcome {
            MeshSolve::Solved((topology, assignment)) => {
                if let Some(valid) = complete_solution_valid {
                    let Some(candidate_pairs) =
                        mesh_candidate_point_pairs(ctx, &topology, &assignment)?
                    else {
                        return Ok(false);
                    };
                    if !valid(&candidate_pairs) {
                        return Ok(false);
                    }
                }
                let candidate = (topology, assignment);
                if let Some(previous) = &resolved {
                    let equivalent =
                        mesh_candidates_equivalent(ctx, previous, &candidate, candidate_gauge)?;
                    if !equivalent {
                        ambiguous = true;
                        return Ok(true);
                    }
                } else {
                    resolved = Some(candidate);
                }
            }
            MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => {
                ambiguous = true;
                return Ok(true);
            }
            MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => {
                exhausted = true;
                return Ok(true);
            }
            MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
        }
        if budget.exhausted() {
            exhausted = true;
            return Ok(true);
        }
        Ok(false)
    };
    walk_endpoint_relation_domains(
        ctx,
        WalkEndpointRelationDomainsInputs {
            domains,
            face_assignments: assignments,
            assigned: vec![None; edge_candidates.len()],
            constraints: &constraints,
            point_count: vertex_points.len(),
            budget,
            state_memo: &mut relation_walk_state_memo,
            candidate_gauge,
            priority_edges,
            partial_solution_valid,
            coordinate_domains,
            coordinate_budget: coordinate_budget.as_ref(),
            evaluate: &mut evaluate,
        },
    )?;
    Ok(if ambiguous {
        Some(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())))
    } else if exhausted || budget.exhausted() {
        Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(())))
    } else if let Some((topology, assignment)) = resolved {
        Some(MeshSolve::Solved((topology, assignment)))
    } else {
        Some(MeshSolve::Failed(MeshCandidateFailure::Rejected(())))
    })
}

#[test]
fn relation_coordinate_candidates_keep_only_surviving_pair_values() {
    let base_candidates = vec![vec![[0, 1], [0, 2]], vec![[1, 2]]];
    let domains = vec![
        vec![MeshEndpointRelationChoice {
            id: 0,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0],
                edge_pairs: vec![(0, [0, 1]), (1, [1, 2])],
            },
        }],
        vec![MeshEndpointRelationChoice {
            id: 1,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0],
                edge_pairs: vec![(0, [0, 1])],
            },
        }],
    ];
    let assigned = vec![None, None];
    assert_eq!(
        relation_coordinate_candidate_domains(&domains, &assigned, &base_candidates),
        Some(vec![vec![[0, 1]], vec![[1, 2]]]),
    );

    let unknown_domains = vec![vec![MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Deferred,
    }]];
    assert_eq!(
        relation_coordinate_candidate_domains(&unknown_domains, &assigned, &base_candidates),
        Some(base_candidates.clone()),
    );
    assert!(relation_coordinate_candidate_domains(
        &domains,
        &[Some([2, 3]), None],
        &base_candidates
    )
    .is_none());
}

#[test]
fn endpoint_configuration_relation_solves_cycle_orientation_globally() {
    catia_test_context!(ctx);
    let edge_rows = (0..3)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let vertex_points = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let edge_candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 2,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 2,
                start: 2,
                end: 0,
                reversed: None,
            },
        ]],
    }]];
    let endpoint_configurations = vec![vec![Some(vec![vec![
        (0, [0, 1]),
        (1, [1, 2]),
        (2, [0, 2]),
    ]])]];
    let port_identities = vec![[0, 1], [2, 3], [4, 5]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    let Some(MeshSolve::Solved((topology, point_assignment))) =
        resolve_endpoint_configuration_relation_streaming(
            &ctx,
            ResolveEndpointConfigurationRelationStreamingInputs {
                assignments: &assignments,
                endpoint_configurations: &endpoint_configurations,
                edge_candidates: &edge_candidates,
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                port_identities: &port_identities,
                budget: &budget,
                partial_solution_valid: None,
                complete_solution_valid: None,
                candidate_gauge: None,
                priority_edges: None,
                coordinate_domains: None,
            },
        )
        .expect("resolve endpoint configuration relation")
    else {
        panic!("endpoint configuration relation did not solve the cycle");
    };

    assert_eq!(topology.faces.len(), 1);
    assert_eq!(point_assignment, vec![0, 1, 2]);
}

#[test]
fn fixed_endpoint_pairs_materialize_duplicate_boundary_assignments() {
    catia_test_context!(ctx);
    let edge_rows = (0..3)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let vertex_points = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let edge_candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 2,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 2,
                start: 2,
                end: 0,
                reversed: None,
            },
        ]],
    };
    let assignments = vec![vec![assignment.clone(), assignment]];
    let endpoint_configurations = vec![vec![
        Some(vec![vec![(0, [0, 1]), (1, [1, 2]), (2, [0, 2])]]),
        Some(vec![vec![(0, [0, 1]), (1, [1, 2]), (2, [0, 2])]]),
    ]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let result = resolve_endpoint_configuration_relation_streaming(
        &ctx,
        ResolveEndpointConfigurationRelationStreamingInputs {
            assignments: &assignments,
            endpoint_configurations: &endpoint_configurations,
            edge_candidates: &edge_candidates,
            edge_rows: &edge_rows,
            vertex_points: &vertex_points,
            port_identities: &[[0, 1], [2, 3], [4, 5]],
            budget: &budget,
            partial_solution_valid: None,
            complete_solution_valid: None,
            candidate_gauge: None,
            priority_edges: None,
            coordinate_domains: None,
        },
    )
    .expect("resolve endpoint configuration relation");

    let Some(MeshSolve::Solved((topology, point_assignment))) = result else {
        panic!("endpoint relation did not materialize duplicate assignments");
    };
    assert_eq!(topology.faces.len(), 1);
    assert_eq!(point_assignment, vec![0, 1, 2]);
}

#[test]
fn endpoint_relation_keeps_stopped_face_assignments() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let face_assignments = vec![assignment.clone(), assignment];
    let face_configurations = vec![Some(vec![vec![(0, [0, 0])]]), None];
    let mut covered = vec![false];

    let choices = collect_endpoint_relation_face_choices(
        &face_assignments,
        &face_configurations,
        &mut covered,
    )
    .expect("well-formed endpoint relation choices");

    assert!(covered[0]);
    assert!(choices
        .iter()
        .any(|choice| { matches!(choice.selection, MeshEndpointRelationSelection::Deferred) }));
    assert!(choices.iter().any(|choice| matches!(
        &choice.selection,
        MeshEndpointRelationSelection::Enumerated { assignments, edge_pairs }
            if assignments == &[0] && edge_pairs == &[(0, [0, 0])]
    )));
}

#[test]
fn raw_endpoint_relation_state_signature_ignores_local_order() {
    let left = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 7,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![2, 0, 2],
                    edge_pairs: vec![(1, [3, 2]), (0, [1, 0])],
                },
            },
            MeshEndpointRelationChoice {
                id: 3,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![4],
                    edge_pairs: vec![(2, [5, 4])],
                },
            },
        ],
        vec![MeshEndpointRelationChoice {
            id: 9,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![3, 1],
                edge_pairs: vec![(0, [1, 0])],
            },
        }],
    ];
    let right = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 30,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![4],
                    edge_pairs: vec![(2, [4, 5])],
                },
            },
            MeshEndpointRelationChoice {
                id: 70,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![0, 2],
                    edge_pairs: vec![(0, [0, 1]), (1, [2, 3])],
                },
            },
        ],
        vec![MeshEndpointRelationChoice {
            id: 90,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![1, 3],
                edge_pairs: vec![(0, [0, 1])],
            },
        }],
    ];
    let left_assigned = vec![Some([3, 2]), None, Some([5, 4])];
    let right_assigned = vec![Some([2, 3]), None, Some([4, 5])];

    assert_eq!(
        raw_endpoint_relation_state_signature(&left, &left_assigned),
        raw_endpoint_relation_state_signature(&right, &right_assigned),
    );
}

#[test]
fn endpoint_relation_requires_one_joint_support_for_all_shared_edges() {
    catia_test_context!(ctx);
    let mut domains = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![0],
                    edge_pairs: vec![(0, [0, 1]), (1, [2, 3])],
                },
            },
            MeshEndpointRelationChoice {
                id: 1,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![1],
                    edge_pairs: vec![(0, [4, 5]), (1, [6, 7])],
                },
            },
        ],
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![0],
                    edge_pairs: vec![(0, [0, 1]), (1, [6, 7])],
                },
            },
            MeshEndpointRelationChoice {
                id: 1,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![1],
                    edge_pairs: vec![(0, [4, 5]), (1, [6, 7])],
                },
            },
        ],
    ];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let constraints = build_endpoint_relation_constraints(&ctx, &domains, &budget)
        .expect("build endpoint relation constraints")
        .expect("shared-edge relation constraints should build");
    let mut assigned = vec![None; 2];

    assert!(propagate_endpoint_relation_domains(
        &ctx,
        &mut domains,
        &mut assigned,
        &constraints,
        &budget,
    )
    .expect("propagate endpoint relation domains"));
    assert_eq!(
        domains[0]
            .iter()
            .map(|choice| choice.id)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert_eq!(
        domains[1]
            .iter()
            .map(|choice| choice.id)
            .collect::<Vec<_>>(),
        vec![1]
    );
}

#[test]
fn endpoint_relation_treats_optional_shared_edges_as_wildcards() {
    catia_test_context!(ctx);
    let mut domains = vec![
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![0],
                    edge_pairs: vec![(0, [0, 1])],
                },
            },
            MeshEndpointRelationChoice {
                id: 1,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![1],
                    edge_pairs: vec![(0, [2, 3])],
                },
            },
        ],
        vec![
            MeshEndpointRelationChoice {
                id: 0,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![0],
                    edge_pairs: vec![(0, [4, 5])],
                },
            },
            MeshEndpointRelationChoice {
                id: 1,
                selection: MeshEndpointRelationSelection::Enumerated {
                    assignments: vec![1],
                    edge_pairs: Vec::new(),
                },
            },
        ],
    ];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let constraints = build_endpoint_relation_constraints(&ctx, &domains, &budget)
        .expect("build endpoint relation constraints")
        .expect("shared-edge relation constraints should build");
    let mut assigned = vec![None];

    assert!(propagate_endpoint_relation_domains(
        &ctx,
        &mut domains,
        &mut assigned,
        &constraints,
        &budget,
    )
    .expect("propagate endpoint relation domains"));
    assert_eq!(
        domains[0]
            .iter()
            .map(|choice| choice.id)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        domains[1]
            .iter()
            .map(|choice| choice.id)
            .collect::<Vec<_>>(),
        vec![1]
    );
}

#[test]
fn relation_choice_on_an_unknown_edge_yields_no_coordinate_candidates() {
    let domains = [vec![MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Enumerated {
            assignments: vec![0],
            edge_pairs: vec![(1, [0, 1])],
        },
    }]];
    let run = || relation_coordinate_candidate_domains(&domains, &[None], &[vec![[0, 1]]]);
    assert!(run().is_none());
}

#[test]
fn endpoint_configuration_relation_solves_a_triangle_face() {
    let edge_rows = (0..3)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let vertex_points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let edge_candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 2,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 2,
                start: 2,
                end: 0,
                reversed: None,
            },
        ]],
    }]];
    let configurations = vec![vec![Some(vec![vec![
        (0, [0, 1]),
        (1, [1, 2]),
        (2, [0, 2]),
    ]])]];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        resolve_endpoint_configuration_relation_streaming(
            ctx,
            ResolveEndpointConfigurationRelationStreamingInputs {
                assignments: &assignments,
                endpoint_configurations: &configurations,
                edge_candidates: &edge_candidates,
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                port_identities: &[[0, 1], [2, 3], [4, 5]],
                budget: &budget,
                partial_solution_valid: None,
                complete_solution_valid: None,
                candidate_gauge: None,
                priority_edges: None,
                coordinate_domains: None,
            },
        )
    };
    assert!(matches!(
        crate::test_support::with_service_context(run)
            .expect("resolve endpoint configuration relation"),
        Some(MeshSolve::Solved(_))
    ));
}

#[test]
fn face_choices_refuse_configurations_on_an_unknown_edge() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let assignments = [assignment.clone(), assignment];
    let configurations = [Some(vec![vec![(0, [0, 0])]]), Some(vec![vec![(1, [0, 0])]])];
    let run = || {
        let mut covered = [false];
        collect_endpoint_relation_face_choices(&assignments, &configurations, &mut covered)
    };
    assert!(run().is_none());
}

#[test]
fn endpoint_relation_constraints_build_with_and_without_optional_edges() {
    let make_domains = |optional_edge: bool| {
        let choice = |id, edge_pairs| MeshEndpointRelationChoice {
            id,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0],
                edge_pairs,
            },
        };
        let mut right = vec![choice(0, vec![(0, [0, 1]), (1, [2, 3])])];
        if optional_edge {
            right.push(choice(1, vec![(0, [0, 1])]));
        }
        vec![vec![choice(0, vec![(0, [0, 1]), (1, [2, 3])])], right]
    };
    for optional_edge in [false, true] {
        let domains = make_domains(optional_edge);
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        catia_test_context!(service_ctx);
        assert!(
            build_endpoint_relation_constraints(&service_ctx, &domains, &budget)
                .expect("build endpoint relation constraints")
                .is_some()
        );
    }
}

#[test]
fn endpoint_relation_walk_fails_when_the_evaluator_accepts_nothing() {
    let choices = vec![
        MeshEndpointRelationChoice {
            id: 0,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0],
                edge_pairs: vec![(0, [0, 1])],
            },
        },
        MeshEndpointRelationChoice {
            id: 1,
            selection: MeshEndpointRelationSelection::Enumerated {
                assignments: vec![1],
                edge_pairs: vec![(0, [0, 1])],
            },
        },
    ];
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    };
    let assignments = [vec![assignment.clone(), assignment]];
    let constraints = MeshEndpointRelationConstraints {
        arcs: vec![vec![]],
        incoming: vec![vec![]],
        choice_counts: vec![2],
    };
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        let mut memo = HashSet::new();
        walk_endpoint_relation_domains(
            ctx,
            WalkEndpointRelationDomainsInputs {
                domains: vec![choices.clone()],
                face_assignments: &assignments,
                assigned: vec![None],
                constraints: &constraints,
                point_count: 2,
                budget: &budget,
                state_memo: &mut memo,
                candidate_gauge: None,
                priority_edges: Some(&[true]),
                partial_solution_valid: None,
                coordinate_domains: None,
                coordinate_budget: None,
                evaluate: &mut |_, _| Ok(false),
            },
        )
    };
    assert!(
        !crate::test_support::with_service_context(run).expect("walk endpoint relation domains")
    );
}

#[test]
fn endpoint_relation_propagation_keeps_matching_shared_edge_choices() {
    let choice = MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Enumerated {
            assignments: vec![0],
            edge_pairs: vec![(0, [0, 1])],
        },
    };
    let domains = vec![vec![choice.clone()], vec![choice]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    catia_test_context!(service_ctx);
    let constraints = build_endpoint_relation_constraints(&service_ctx, &domains, &budget)
        .expect("build endpoint relation constraints")
        .expect("shared edge creates relation constraints");
    let mut service_domains = domains.clone();
    assert!(propagate_endpoint_relation_domains(
        &service_ctx,
        &mut service_domains,
        &mut [None],
        &constraints,
        &budget,
    )
    .expect("propagate endpoint relation domains"));
}

#[test]
fn endpoint_relation_propagation_rejects_an_empty_face_domain() {
    let choice = MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Enumerated {
            assignments: vec![0],
            edge_pairs: vec![(0, [0, 1])],
        },
    };
    let domains = vec![vec![choice.clone()], vec![choice]];
    catia_test_context!(service_ctx);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let constraints = build_endpoint_relation_constraints(&service_ctx, &domains, &budget)
        .expect("build endpoint relation constraints")
        .expect("shared edge creates relation constraints");
    let mut empty = domains.clone();
    empty[0].clear();
    assert!(!propagate_endpoint_relation_domains(
        &service_ctx,
        &mut empty,
        &mut [None],
        &constraints,
        &budget,
    )
    .expect("propagate endpoint relation domains"));

    let mut empty = domains;
    empty[0].clear();
}
