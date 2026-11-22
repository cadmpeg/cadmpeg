// SPDX-License-Identifier: Apache-2.0
//! Fixed, singleton, and standard mesh endpoint resolution.

use super::endpoint_configurations::{
    endpoint_configuration_directions, endpoint_configuration_for_assignment,
    mesh_face_endpoint_configurations, MeshDirectionEnumerationError,
};
use super::endpoint_cycles::mesh_assignment_endpoint_cycles_viable_with;
use super::endpoint_relations::{
    resolve_endpoint_configuration_relation_streaming, MeshEndpointSolutionPredicate,
    ResolveEndpointConfigurationRelationStreamingInputs,
};
use super::quotient::{initial_mesh_quotient, MeshQuotient};
use super::selection_search::{
    canonical_singleton_coordinate_cycles, reconstruct_singleton_coordinate_topology,
    resolve_mesh_selection_from_quotient, resolve_singleton_mesh_selection,
    singleton_mesh_boundary_directions,
};
use super::selection_state::{
    possible_face_choices_within, possible_face_equations, MeshSelectionSearch,
};
use super::{
    MeshCandidateFailure, MeshEndpointResolve, MeshSolve, SearchOutcome,
    MAX_FACE_ENDPOINT_CONFIGURATION_WORK, MAX_MESH_CONSTRAINT_OPERATIONS,
};
#[cfg(test)]
use crate::families::standard::topology::EdgeBoundaryLayout;
use crate::families::standard::topology::{
    reconstruct_mesh_selection, EdgeRow, StandardTopologyDraft,
};
use crate::solve::mesh_gauge::{mesh_candidates_equivalent, MeshCandidateGauge};
#[cfg(test)]
use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;
use crate::solve::missing_edge::MeshFaceBoundaryAssignment;
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use std::cell::RefCell;
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(super) struct MeshEndpointGeometry<'a> {
    pub(super) edge_rows: &'a [EdgeRow],
    pub(super) vertex_points: &'a [[f64; 3]],
}

pub(super) fn resolve_fixed_mesh_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    geometry: MeshEndpointGeometry<'_>,
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[MeshFaceBoundaryAssignment],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<MeshEndpointResolve, CodecError> {
    let MeshEndpointGeometry {
        edge_rows,
        vertex_points,
    } = geometry;
    if selected.is_empty()
        || edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }
    let mut assignment_domains = Vec::new();

    for assignment in selected {
        let domain = vec![assignment.clone()];
        assignment_domains.push(domain);
    }
    if edge_candidates
        .iter()
        .any(|candidates| candidates.len() != 1)
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }
    if let Some(resolved) = resolve_singleton_mesh_endpoint_candidates(
        ctx,
        ResolveSingletonMeshEndpointCandidatesInputs {
            edge_rows,
            vertex_points,
            edge_candidates,
            assignments: &assignment_domains,
            port_identities,
            edge_direction_evidence: None,
            budget,
            candidate_gauge,
        },
    )? {
        if !matches!(
            &resolved,
            MeshSolve::Failed(MeshCandidateFailure::Rejected(()))
        ) {
            return Ok(resolved);
        }
    }
    if edge_candidates.iter().any(Vec::is_empty) {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }
    let mut edge_pairs = Vec::new();

    for candidates in edge_candidates {
        if let Some(&pair) = candidates.first() {
            edge_pairs.push(pair);
        }
    }
    let mut fixed_face_directions = Vec::new();

    let mut direction_overflow = false;
    for assignment in selected {
        let Some(configuration) = endpoint_configuration_for_assignment(assignment, &edge_pairs)
        else {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
        };
        let directions = match endpoint_configuration_directions(assignment, &configuration) {
            Ok(directions) => directions,
            Err(MeshDirectionEnumerationError::Overflow) => {
                direction_overflow = true;
                break;
            }
            Err(MeshDirectionEnumerationError::Invalid) => {
                return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())))
            }
        };
        if directions.is_empty() {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
        }
        fixed_face_directions.push(Some(directions));
    }
    let use_fixed_direction_search = !direction_overflow;
    if direction_overflow {
        fixed_face_directions = vec![None; assignment_domains.len()];
    }
    let mut edge_has_fixed_direction = vec![false; edge_candidates.len()];
    for use_ in selected
        .iter()
        .flat_map(|assignment| &assignment.boundaries)
        .flatten()
    {
        if use_.reversed.is_some() {
            let Some(fixed) = edge_has_fixed_direction.get_mut(use_.edge) else {
                return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
            };
            *fixed = true;
        }
    }
    let Some(quotient) =
        initial_mesh_quotient(edge_candidates, vertex_points.len(), port_identities)?
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    };
    let mut direct_quotient = quotient.clone();
    let mut direct_orientations = edge_has_fixed_direction
        .iter()
        .map(|fixed| (!fixed).then_some(false))
        .collect::<Vec<_>>();
    let mut direct_directions = Vec::new();

    let mut direct_possible = true;
    for (assignment, direction_options) in selected.iter().zip(&fixed_face_directions) {
        let Some(direction_options) = direction_options.as_ref() else {
            direct_possible = false;
            break;
        };
        let Some(label_directions) = direction_options.first() else {
            direct_possible = false;
            break;
        };
        let mut next_orientations = direct_orientations.clone();
        let constrained = assignment
            .boundaries
            .iter()
            .zip(label_directions)
            .flat_map(|(boundary, directions)| boundary.iter().zip(directions))
            .all(|(use_, &label_direction)| {
                let Some(required) = use_.reversed else {
                    return true;
                };
                let Some(orientation) = next_orientations.get_mut(use_.edge) else {
                    return false;
                };
                let required_orientation = required ^ label_direction;
                match *orientation {
                    Some(existing) => existing == required_orientation,
                    None => {
                        *orientation = Some(required_orientation);
                        true
                    }
                }
            });
        if !constrained {
            direct_possible = false;
            break;
        }
        let Some(directions) = direct_quotient.merge_label_directions_in_place(
            assignment,
            label_directions,
            &next_orientations,
            Some(budget),
        )?
        else {
            direct_possible = false;
            break;
        };
        direct_orientations = next_orientations;
        direct_directions.push(directions);
    }
    if direct_possible {
        let outcome =
            reconstruct_mesh_selection(edge_rows, vertex_points, selected, &direct_directions)?
                .map(|topology| {
                    resolve_mesh_selection_from_quotient(
                        ctx,
                        topology,
                        direct_quotient,
                        vertex_points,
                        edge_candidates,
                        port_identities,
                        budget,
                    )
                })
                .transpose()?
                .flatten();
        if let Some(outcome) = outcome {
            match outcome {
                MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
                resolved => return Ok(resolved),
            }
        }
        if budget.exhausted() {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(())));
        }
    }
    let face_work = vec![Some(1usize); assignment_domains.len()];
    let fixed_edge_orientations = if use_fixed_direction_search {
        edge_has_fixed_direction
            .iter()
            .map(|fixed| (!fixed).then_some(false))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut search = MeshSelectionSearch {
        ctx,
        assignments: &assignment_domains,
        #[cfg(test)]
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work,
        edge_candidates,
        edge_rows,
        vertex_points,
        candidate_gauge,
        port_identities: Some(port_identities),
        fixed_face_directions,
        fixed_edge_orientations,
        edge_has_fixed_direction: if use_fixed_direction_search {
            edge_has_fixed_direction
        } else {
            Vec::new()
        },
        selected: vec![None; assignment_domains.len()],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    if use_fixed_direction_search {
        search.search_fixed_direction_with_budget(&quotient, budget)?;
    } else {
        search.search_with_budget(&quotient, budget, budget)?;
    }
    Ok(search.outcome.into())
}

/// Materialize boundary-assignment alternatives after the relation phase has
/// fixed every edge endpoint pair. Re-entering the general endpoint resolver
/// here rebuilds the same relation and recurses indefinitely when multiple
/// boundary assignments share one endpoint configuration. Enumerate only the
/// remaining assignment choices, then apply the exact fixed-pair materializer.
pub(super) fn resolve_fixed_mesh_endpoint_assignment_domains(
    ctx: &DecodeContext<'_>,
    geometry: MeshEndpointGeometry<'_>,
    edge_candidates: &[Vec<[usize; 2]>],
    assignment_domains: &[Vec<MeshFaceBoundaryAssignment>],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<MeshEndpointResolve, CodecError> {
    // Keep recursive search state explicit so budget, solution, and ambiguity
    // ownership remain visible at every branch.
    struct FixedEndpointAssignmentSearch<
        'input0,
        'input1,
        'input2,
        'input3,
        'input4,
        'input5,
        'input7,
        'input8,
        'input9,
    > {
        face: usize,
        assignment_domains: &'input0 [Vec<MeshFaceBoundaryAssignment>],
        selected: &'input1 mut Vec<MeshFaceBoundaryAssignment>,
        edge_rows: &'input2 [EdgeRow],
        vertex_points: &'input3 [[f64; 3]],
        edge_candidates: &'input4 [Vec<[usize; 2]>],
        port_identities: &'input5 [[u32; 2]],
        budget: &'input7 WorkBudget,
        candidate_gauge: Option<MeshCandidateGauge<'input8>>,
        outcome: &'input9 mut SearchOutcome<(StandardTopologyDraft, Vec<usize>)>,
    }
    fn visit(
        ctx: &DecodeContext<'_>,
        inputs: FixedEndpointAssignmentSearch<'_, '_, '_, '_, '_, '_, '_, '_, '_>,
    ) -> Result<(), CodecError> {
        let FixedEndpointAssignmentSearch {
            face,
            assignment_domains,
            selected,
            edge_rows,
            vertex_points,
            edge_candidates,
            port_identities,
            budget,
            candidate_gauge,
            outcome,
        } = inputs;

        if outcome.is_closed() || budget.exhausted() {
            return Ok(());
        }
        if !budget.charge() {
            outcome.exhaust();
            return Ok(());
        }
        if face == assignment_domains.len() {
            let resolved = resolve_fixed_mesh_endpoint_pairs(
                ctx,
                MeshEndpointGeometry {
                    edge_rows,
                    vertex_points,
                },
                edge_candidates,
                selected,
                port_identities,
                budget,
                candidate_gauge,
            )?;
            match resolved {
                MeshSolve::Solved((topology, assignment)) => {
                    let candidate = (topology, assignment);
                    let equivalent = if let SearchOutcome::Solved(previous) = &*outcome {
                        mesh_candidates_equivalent(ctx, previous, &candidate, candidate_gauge)?
                    } else {
                        false
                    };
                    outcome.record_solved(candidate, |_, _| equivalent);
                }
                MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
                MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => outcome.mark_ambiguous(),
                MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => outcome.exhaust(),
            }
            return Ok(());
        }
        for assignment in &assignment_domains[face] {
            selected.push(assignment.clone());
            visit(
                ctx,
                FixedEndpointAssignmentSearch {
                    face: face + 1,
                    assignment_domains,
                    selected,
                    edge_rows,
                    vertex_points,
                    edge_candidates,
                    port_identities,
                    budget,
                    candidate_gauge,
                    outcome,
                },
            )?;
            selected.pop();
            if outcome.is_closed() {
                return Ok(());
            }
        }
        Ok(())
    }

    let MeshEndpointGeometry {
        edge_rows,
        vertex_points,
    } = geometry;
    if assignment_domains.is_empty()
        || assignment_domains.iter().any(Vec::is_empty)
        || edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
        || edge_candidates
            .iter()
            .any(|candidates| candidates.len() != 1)
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }

    let mut outcome = SearchOutcome::Open;
    visit(
        ctx,
        FixedEndpointAssignmentSearch {
            face: 0,
            assignment_domains,
            selected: &mut Vec::new(),
            edge_rows,
            vertex_points,
            edge_candidates,
            port_identities,
            budget,
            candidate_gauge,
            outcome: &mut outcome,
        },
    )?;
    if budget.exhausted() {
        outcome.exhaust();
    }
    Ok(outcome.into())
}

pub(in crate::solve) fn prune_mesh_endpoint_pair_support(
    assignments: &mut [Vec<MeshFaceBoundaryAssignment>],
    edge_candidates: &mut [Vec<[usize; 2]>],
    budget: &WorkBudget,
) -> bool {
    'fixpoint: loop {
        let mut changed = false;
        for face in assignments.iter_mut() {
            let before = face.len();
            face.retain(|assignment| {
                mesh_assignment_endpoint_cycles_viable_with(
                    assignment,
                    edge_candidates,
                    None,
                    Some(budget),
                )
                .unwrap_or(true)
            });
            if budget.exhausted() {
                // Pair-support pruning is optional. Every removal made before
                // exhaustion was proved locally; the independently bounded
                // quotient search can continue from that sound partial result.
                return true;
            }
            if face.is_empty() {
                return false;
            }
            changed |= face.len() != before;
        }
        let mut incident_faces = vec![Vec::new(); edge_candidates.len()];
        for (face, choices) in assignments.iter().enumerate() {
            for use_ in choices
                .iter()
                .flat_map(|assignment| assignment.boundaries.iter().flatten())
            {
                let Some(faces) = incident_faces.get_mut(use_.edge) else {
                    continue;
                };
                if faces.last() != Some(&face) {
                    faces.push(face);
                }
            }
        }
        for edge in 0..edge_candidates.len() {
            if edge_candidates[edge].is_empty() {
                continue;
            }
            let incident_faces = &incident_faces[edge];
            let row = &edge_candidates[edge];
            let before = row.len();
            let mut decisions = vec![false; row.len()];

            // Read the unchanged candidate table, then apply this row's decisions.
            // A complete table copy per row repeats all unrelated storage.
            for (pair, decision) in row.iter().zip(&mut decisions) {
                *decision = incident_faces.iter().all(|face| {
                    assignments[*face].iter().any(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .any(|use_| use_.edge == edge)
                            && mesh_assignment_endpoint_cycles_viable_with(
                                assignment,
                                edge_candidates,
                                Some((edge, *pair)),
                                Some(budget),
                            )
                            .unwrap_or(true)
                    })
                });
            }
            let mut position = 0;
            edge_candidates[edge].retain(|_| {
                let supported = decisions[position];
                position += 1;
                supported
            });
            if budget.exhausted() {
                // Do not turn incomplete propagation into a contradiction.
                return true;
            }
            if edge_candidates[edge].is_empty() {
                return false;
            }
            if edge_candidates[edge].len() != before {
                continue 'fixpoint;
            }
        }
        if !changed {
            return true;
        }
    }
}

// The arguments are independent serialized evidence, solver state, and budget
// inputs; grouping them would hide their ownership without reducing coupling.
#[derive(Clone, Copy)]
pub(super) struct ResolveSingletonMeshEndpointCandidatesInputs<
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
    'input7,
    'input8,
> {
    pub(super) edge_rows: &'input0 [EdgeRow],
    pub(super) vertex_points: &'input1 [[f64; 3]],
    pub(super) edge_candidates: &'input2 [Vec<[usize; 2]>],
    pub(super) assignments: &'input3 [Vec<MeshFaceBoundaryAssignment>],
    pub(super) port_identities: &'input4 [[u32; 2]],
    pub(super) edge_direction_evidence: Option<&'input5 [bool]>,
    pub(super) budget: &'input7 WorkBudget,
    pub(super) candidate_gauge: Option<MeshCandidateGauge<'input8>>,
}

pub(super) fn resolve_singleton_mesh_endpoint_candidates(
    ctx: &DecodeContext<'_>,
    inputs: ResolveSingletonMeshEndpointCandidatesInputs<'_, '_, '_, '_, '_, '_, '_, '_>,
) -> Result<Option<MeshEndpointResolve>, CodecError> {
    let ResolveSingletonMeshEndpointCandidatesInputs {
        edge_rows,
        vertex_points,
        edge_candidates,
        assignments,
        port_identities,
        edge_direction_evidence,
        budget,
        candidate_gauge,
    } = inputs;

    if edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
        || edge_direction_evidence.is_some_and(|evidence| evidence.len() != edge_rows.len())
        || edge_candidates
            .iter()
            .any(|candidates| candidates.len() != 1)
    {
        return Ok(None);
    }

    let mut selected = Vec::new();
    let mut endpoint_labelled_directions = Vec::new();

    for face in assignments {
        let mut seen = HashSet::new();
        let mut first = None;
        for assignment in face {
            let mut directions = Vec::new();

            let mut valid = true;
            for boundary in &assignment.boundaries {
                let Some(row) = singleton_mesh_boundary_directions(
                    boundary,
                    edge_candidates,
                    edge_direction_evidence,
                ) else {
                    valid = false;
                    break;
                };
                directions.push(row);
            }
            if !valid {
                continue;
            }
            let Some(signature) =
                canonical_singleton_coordinate_cycles(assignment, &directions, edge_candidates)
            else {
                continue;
            };
            if seen.contains(&signature) {
                continue;
            }

            seen.insert(signature);
            if first.is_some() {
                return Ok(None);
            }
            first = Some((assignment.clone(), directions));
        }
        let Some((assignment, directions)) = first else {
            return Ok(None);
        };
        selected.push(assignment);
        endpoint_labelled_directions.push(directions);
    }
    if let Some(topology) = reconstruct_singleton_coordinate_topology(
        ctx,
        edge_rows,
        vertex_points,
        edge_candidates,
        &selected,
        &endpoint_labelled_directions,
    )? {
        let mut point_assignment = Vec::new();

        point_assignment.extend(0..vertex_points.len());
        return Ok(Some(MeshSolve::Solved((topology, point_assignment))));
    }
    // An unresolved coedge direction does not select a point endpoint. It is
    // a row-orientation gauge. Let the exact coordinate binding prove the
    // resulting cycle, then try the endpoint-labelled gauge only when the
    // fixed false direction cannot bind.
    let mut fixed_directions = Vec::new();

    for assignment in &selected {
        let mut face_directions = Vec::new();

        for boundary in &assignment.boundaries {
            if boundary.is_empty() {
                return Ok(None);
            }
            let mut directions = Vec::new();

            directions.extend(boundary.iter().map(|use_| use_.reversed.unwrap_or(false)));
            face_directions.push(directions);
        }
        fixed_directions.push(face_directions);
    }
    if let Some(resolved) = resolve_singleton_mesh_selection(
        ctx,
        super::selection_search::ResolveSingletonMeshSelectionInputs {
            edge_rows,
            vertex_points,
            edge_candidates,
            selected: &selected,
            directions: &fixed_directions,
            port_identities,
            budget,
            candidate_gauge,
        },
    )? {
        match resolved {
            MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
            resolved => return Ok(Some(resolved)),
        }
    }

    resolve_singleton_mesh_selection(
        ctx,
        super::selection_search::ResolveSingletonMeshSelectionInputs {
            edge_rows,
            vertex_points,
            edge_candidates,
            selected: &selected,
            directions: &endpoint_labelled_directions,
            port_identities,
            budget,
            candidate_gauge,
        },
    )
}

// Endpoint materialization receives independent evidence, budgets, predicates,
// and gauge state so each fallback remains separately bounded and auditable.
pub(super) struct ResolveStandardMeshEndpointCandidatesInputs<
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
> {
    pub(super) edge_rows: &'input0 [EdgeRow],
    pub(super) vertex_points: &'input1 [[f64; 3]],
    pub(super) edge_candidates: &'input2 [Vec<[usize; 2]>],
    pub(super) assignments: Vec<Vec<MeshFaceBoundaryAssignment>>,
    pub(super) port_identities: &'input3 [[u32; 2]],
    pub(super) prepared_quotient: Option<&'input4 MeshQuotient>,
    pub(super) edge_direction_evidence: Option<&'input5 [bool]>,
    pub(super) budget: &'input7 WorkBudget,
    pub(super) partial_solution_valid: Option<&'input9 MeshEndpointSolutionPredicate<'input8>>,
    pub(super) complete_solution_valid: Option<&'input11 MeshEndpointSolutionPredicate<'input10>>,
    pub(super) candidate_gauge: Option<MeshCandidateGauge<'input12>>,
    pub(super) priority_edges: Option<&'input13 [bool]>,
}

pub(super) fn resolve_standard_mesh_endpoint_candidates(
    ctx: &DecodeContext<'_>,
    inputs: ResolveStandardMeshEndpointCandidatesInputs<
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
) -> Result<MeshEndpointResolve, CodecError> {
    const MAX_SELECTION_WORK: usize = 100_000;

    let ResolveStandardMeshEndpointCandidatesInputs {
        edge_rows,
        vertex_points,
        edge_candidates,
        mut assignments,
        port_identities,
        prepared_quotient,
        edge_direction_evidence,
        budget,
        partial_solution_valid,
        complete_solution_valid,
        candidate_gauge,
        priority_edges,
    } = inputs;

    let mut edge_candidates = edge_candidates.to_vec();
    if !prune_mesh_endpoint_pair_support(
        &mut assignments,
        &mut edge_candidates,
        &budget.charging_slice(MAX_MESH_CONSTRAINT_OPERATIONS),
    ) {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    }
    let quotient = if let Some(prepared) = prepared_quotient {
        Some(prepared.clone())
    } else {
        initial_mesh_quotient(&edge_candidates, vertex_points.len(), port_identities)?
    };
    let Some(quotient) = quotient else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
    };
    for face in &mut assignments {
        let mut retained = Vec::new();
        for assignment in std::mem::take(face) {
            if quotient.assignment_has_option(&assignment, &edge_candidates, Some(budget))? {
                retained.push(assignment);
            }
        }
        *face = retained;
        if budget.exhausted() {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(())));
        }
        if face.is_empty() {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
        }
    }
    if let Some(resolved) = resolve_singleton_mesh_endpoint_candidates(
        ctx,
        ResolveSingletonMeshEndpointCandidatesInputs {
            edge_rows,
            vertex_points,
            edge_candidates: &edge_candidates,
            assignments: &assignments,
            port_identities,
            edge_direction_evidence,
            budget,
            candidate_gauge,
        },
    )? {
        return Ok(resolved);
    }
    let coordinate_domains = if let Some(preparation_limit) = quotient
        .clone()
        .coordinate_domain_preparation_limit(vertex_points.len(), &edge_candidates)?
    {
        let preparation_budget = budget.charging_slice(preparation_limit);
        let mut coordinate_quotient = quotient.clone();
        coordinate_quotient.prepare_coordinate_root_domains(
            ctx,
            vertex_points.len(),
            &edge_candidates,
            Some(&preparation_budget),
        )?
    } else {
        None
    };
    let mut face_work = Vec::new();

    let mut total_work = 0usize;
    for face in &assignments {
        let Some(next) = total_work.checked_add(face.len()) else {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(())));
        };
        total_work = next;
        face_work.push(Some(face.len()));
    }
    if total_work > MAX_SELECTION_WORK {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(())));
    }
    let face_equations = possible_face_equations(&assignments);
    let mut face_choices = Vec::new();
    if !possible_face_choices_within(
        &assignments,
        &face_equations,
        &budget.charging_slice(MAX_MESH_CONSTRAINT_OPERATIONS),
        &mut face_choices,
    ) {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(())));
    }
    let unselected = vec![None; edge_candidates.len()];
    let configuration_budget = budget.charging_slice(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut endpoint_configurations = Vec::new();
    for face in &assignments {
        let mut face_configurations = Vec::new();
        for assignment in face {
            let configurations = if configuration_budget.exhausted() {
                None
            } else {
                let local_budget =
                    configuration_budget.child_slice(MAX_FACE_ENDPOINT_CONFIGURATION_WORK);
                let configurations = mesh_face_endpoint_configurations(
                    std::slice::from_ref(assignment),
                    &edge_candidates,
                    &unselected,
                    &local_budget,
                );
                if configuration_budget.consume_child(&local_budget).is_err()
                    || local_budget.exhausted()
                {
                    None
                } else {
                    configurations
                }
            };
            face_configurations.push(configurations);
        }
        endpoint_configurations.push(face_configurations);
    }
    let relation = resolve_endpoint_configuration_relation_streaming(
        ctx,
        ResolveEndpointConfigurationRelationStreamingInputs {
            assignments: &assignments,
            endpoint_configurations: &endpoint_configurations,
            edge_candidates: &edge_candidates,
            edge_rows,
            vertex_points,
            port_identities,
            budget,
            partial_solution_valid,
            complete_solution_valid,
            candidate_gauge,
            priority_edges,
            coordinate_domains: coordinate_domains.as_ref(),
        },
    )?;
    if let Some(resolved) = relation {
        return Ok(resolved);
    }
    let mut search = MeshSelectionSearch {
        ctx,
        assignments: &assignments,
        #[cfg(test)]
        possible_face_equations: face_equations,
        possible_face_choices: face_choices,
        face_work,
        edge_candidates: &edge_candidates,
        edge_rows,
        vertex_points,
        candidate_gauge,
        port_identities: Some(port_identities),
        fixed_face_directions: vec![None; assignments.len()],
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None; assignments.len()],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    search.search_with_budget(&quotient, budget, budget)?;
    Ok(search.outcome.into())
}

#[test]
fn singleton_mesh_path_handles_many_independent_face_cycles() {
    const FACE_COUNT: usize = 128;
    catia_test_context!(ctx);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut edge_rows = Vec::with_capacity(FACE_COUNT * 4);
    let mut edge_candidates = Vec::with_capacity(FACE_COUNT * 4);
    let mut port_identities = Vec::with_capacity(FACE_COUNT * 4);
    let mut assignments = Vec::with_capacity(FACE_COUNT);
    let mut vertex_points = Vec::with_capacity(FACE_COUNT * 4);

    for face in 0..FACE_COUNT {
        let edge = face * 4;
        let point = face * 4;
        vertex_points.extend([
            [
                cadmpeg_core::convert::f64_from_index(point)
                    .expect("fixture index is exactly representable"),
                0.0,
                0.0,
            ],
            [
                cadmpeg_core::convert::f64_from_index(point + 1)
                    .expect("fixture index is exactly representable"),
                0.0,
                0.0,
            ],
            [
                cadmpeg_core::convert::f64_from_index(point + 2)
                    .expect("fixture index is exactly representable"),
                0.0,
                0.0,
            ],
            [
                cadmpeg_core::convert::f64_from_index(point + 3)
                    .expect("fixture index is exactly representable"),
                0.0,
                0.0,
            ],
        ]);
        edge_rows.extend((0..4).map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        }));
        edge_candidates.extend([
            vec![[point, point + 1]],
            vec![[point + 1, point + 2]],
            vec![[point + 2, point + 3]],
            vec![[point, point + 3]],
        ]);
        let identity = u32::try_from(edge * 2).expect("fixture value fits u32");
        port_identities.extend([
            [identity, identity + 1],
            [identity + 2, identity + 3],
            [identity + 4, identity + 5],
            [identity + 6, identity + 7],
        ]);
        assignments.push(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![(0..4)
                .map(|offset| MeshBoundaryEdgeCandidate {
                    edge: edge + offset,
                    start: offset,
                    end: offset + 1,
                    reversed: None,
                })
                .collect()],
        }]);
    }

    let MeshSolve::Solved((topology, point_assignment)) =
        resolve_singleton_mesh_endpoint_candidates(
            &ctx,
            ResolveSingletonMeshEndpointCandidatesInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates: &edge_candidates,
                assignments: &assignments,
                port_identities: &port_identities,
                edge_direction_evidence: None,
                budget: &budget,
                candidate_gauge: None,
            },
        )
        .expect("resolve singleton mesh endpoint candidates")
        .expect("singleton path applies")
    else {
        panic!("singleton path did not solve");
    };
    assert_eq!(topology.faces.len(), FACE_COUNT);
    assert_eq!(point_assignment.len(), FACE_COUNT * 4);
}

#[test]
fn singleton_mesh_path_filters_endpoint_incompatible_face_assignments() {
    catia_test_context!(ctx);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let edge_rows = (0..3)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let vertex_points = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let edge_candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 0]]];
    let port_identities = vec![[0, 1], [2, 3], [4, 5]];
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let assignments = vec![vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1), use_(0)]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1), use_(2)]],
        },
    ]];

    let MeshSolve::Solved((topology, point_assignment)) =
        resolve_singleton_mesh_endpoint_candidates(
            &ctx,
            ResolveSingletonMeshEndpointCandidatesInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates: &edge_candidates,
                assignments: &assignments,
                port_identities: &port_identities,
                edge_direction_evidence: None,
                budget: &budget,
                candidate_gauge: None,
            },
        )
        .expect("resolve singleton mesh endpoint candidates")
        .expect("endpoint filtering should leave one face assignment")
    else {
        panic!("endpoint filtering did not solve");
    };
    assert_eq!(topology.faces.len(), 1);
    assert_eq!(point_assignment, vec![0, 1, 2]);
}

#[test]
fn singleton_mesh_path_handles_closed_endpoint_pairs() {
    catia_test_context!(ctx);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let edge_rows = vec![{
        assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
        EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row")
    }];
    let vertex_points = vec![[0.0, 0.0, 0.0]];
    let edge_candidates = vec![vec![[0, 0]]];
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    }]];
    let port_identities = vec![[0, 0]];

    let MeshSolve::Solved((topology, point_assignment)) =
        resolve_singleton_mesh_endpoint_candidates(
            &ctx,
            ResolveSingletonMeshEndpointCandidatesInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates: &edge_candidates,
                assignments: &assignments,
                port_identities: &port_identities,
                edge_direction_evidence: None,
                budget: &budget,
                candidate_gauge: None,
            },
        )
        .expect("resolve singleton mesh endpoint candidates")
        .expect("closed endpoint pair should use a direction gauge")
    else {
        panic!("closed endpoint pair did not solve");
    };
    assert_eq!(topology.logical_vertex_count, 1);
    assert_eq!(point_assignment, vec![0]);
}

#[cfg(test)]
mod pair_support_tests {
    use super::{prune_mesh_endpoint_pair_support, MAX_MESH_CONSTRAINT_OPERATIONS};
    use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
    use cadmpeg_core::decode::WorkBudget;

    #[test]
    fn endpoint_pair_pruning_keeps_independent_one_edge_faces() {
        const EDGES: usize = 512;
        let mut assignments = (0..EDGES)
            .map(|edge| {
                vec![MeshFaceBoundaryAssignment {
                    boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: None,
                    }]],
                }]
            })
            .collect::<Vec<_>>();
        let mut candidates = vec![vec![[0, 0]]; EDGES];
        assert!(prune_mesh_endpoint_pair_support(
            &mut assignments,
            &mut candidates,
            &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
        ));
        assert!(assignments.iter().all(|choices| choices.len() == 1));
        assert!(candidates.iter().all(|pairs| pairs == &[[0, 0]]));
    }
}

#[test]
fn endpoint_candidate_search_accepts_self_loop_edges() {
    let edge_rows = (0..2)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let vertex_points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let candidates = [vec![[0, 0], [1, 1]], vec![[0, 0], [1, 1]]];
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    }]];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        resolve_standard_mesh_endpoint_candidates(
            ctx,
            ResolveStandardMeshEndpointCandidatesInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates: &candidates,
                assignments: assignments.clone(),
                port_identities: &[[0, 0], [1, 1]],
                prepared_quotient: None,
                edge_direction_evidence: None,
                budget: &budget,
                partial_solution_valid: None,
                complete_solution_valid: None,
                candidate_gauge: None,
                priority_edges: None,
            },
        )
    };
    crate::test_support::with_service_context(run)
        .expect("resolve standard mesh endpoint candidates");
}

#[test]
fn fixed_endpoint_pairs_accept_a_self_loop_face() {
    let edge_rows = (0..2)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let vertex_points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let candidates = [vec![[0, 0]], vec![[1, 1]]];
    let selected = [MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    }];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        resolve_fixed_mesh_endpoint_pairs(
            ctx,
            MeshEndpointGeometry {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
            },
            &candidates,
            &selected,
            &[[0, 0], [1, 1]],
            &budget,
            None,
        )
    };
    crate::test_support::with_service_context(run).expect("resolve fixed mesh endpoint pairs");
}

#[test]
fn fixed_endpoint_pairs_survive_direction_enumeration_overflow() {
    const BOUNDARY_COUNT: usize = 13;
    let edge_count = BOUNDARY_COUNT * 2 + 1;
    let edge_rows = (0..edge_count)
        .map(|_| {
            assert!(EdgeRow::new(1, Vec::new(), EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let candidates = vec![vec![[0, 1]]; edge_count];
    let identities = (0..edge_count)
        .map(|edge| {
            [
                u32::try_from(edge * 2).expect("fixture value fits u32"),
                u32::try_from(edge * 2 + 1).expect("fixture value fits u32"),
            ]
        })
        .collect::<Vec<_>>();
    let selected = [MeshFaceBoundaryAssignment {
        boundaries: (0..BOUNDARY_COUNT)
            .map(|boundary| {
                vec![
                    MeshBoundaryEdgeCandidate {
                        edge: boundary * 2,
                        start: 0,
                        end: 1,
                        reversed: Some(false),
                    },
                    MeshBoundaryEdgeCandidate {
                        edge: boundary * 2 + 1,
                        start: 1,
                        end: 0,
                        reversed: None,
                    },
                ]
            })
            .collect(),
    }];
    let vertex_points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(1);
        resolve_fixed_mesh_endpoint_pairs(
            ctx,
            MeshEndpointGeometry {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
            },
            &candidates,
            &selected,
            &identities,
            &budget,
            None,
        )
    };
    crate::test_support::with_service_context(run).expect("resolve fixed mesh endpoint pairs");
}
