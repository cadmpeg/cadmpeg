// SPDX-License-Identifier: Apache-2.0
//! Standard mesh candidate solve and outcome parsing.

use super::common_propagation::{
    propagate_common_boundary_components, propagate_common_ordered_face_quotients,
};
use super::endpoint_resolution::{
    resolve_standard_mesh_endpoint_candidates, ResolveStandardMeshEndpointCandidatesInputs,
};
use super::face_domain_support::{
    endpoint_pairs_respect_candidate_domains, materialize_boundary_domains,
    mesh_domains_have_incident_edge_support, restore_unique_endpoint_pair_orientations,
};
use super::quotient::initial_mesh_quotient;
use super::selection_state::{deduplicate_mesh_quotient_assignments, edge_class_search_constraint};
#[cfg(test)]
use super::MAX_MESH_CONSTRAINT_OPERATIONS;
use super::{
    AssignmentOrder, MeshCandidateAmbiguity, MeshCandidateExhaustion, MeshCandidateFailure,
    MeshCandidateRejection, MeshCandidateSolve, MeshEndpointIncidenceRejection,
    MeshEndpointResolve, MeshFaceAssignmentCandidates, MeshFaceDomainCandidateSolve,
    MeshPartialEndpointConstraint, MeshSolve, MAX_ENDPOINT_RESOLUTION_MEMO_ENTRIES,
    MAX_FACE_DOMAIN_ASSIGNMENTS, MAX_MESH_TOPOLOGY_OPERATIONS,
};
use crate::families::standard::fbb::{largest_fbb_run, parse_edge_tables, parse_vertex_table};
use crate::families::standard::topology::StandardTopologyDraft;
use crate::solve::incidence::{
    visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy, CoordinateRootPolicy,
    IncidenceSolve, VisitIncidenceEndpointPairSolutionsWithCoordinateRootPolicyInputs,
};
use crate::solve::mesh_gauge::{
    build_mesh_coordinate_gauge, canonicalize_mesh_candidate_for_output,
    mesh_candidates_equivalent, MeshCandidateGauge, MeshEdgeGeometry,
};
use crate::solve::missing_edge::{
    standard_mesh_boundary_domains_from_context, visit_duplicate_face_assignments,
    DuplicateFaceAssignmentVisit, MeshFaceBoundaryDomain, StandardMeshBoundaryContext,
};
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use std::cell::Cell;
use std::collections::HashMap;
use std::ops::ControlFlow;

/// Resolve geometric endpoint alternatives through face incidence before
/// applying the exact trim-mesh endpoint quotient. Endpoint graphs must close every
/// face and produce one topology modulo vertex labels, edge direction, and cycle start.
/// `partial_solution_valid` receives partial assignments during search. It must
/// be monotone: once it rejects a selected subset, assigning more edges cannot
/// make that subset valid. `partial_constraint_edges` identifies every edge
/// whose assignment can affect that predicate and must be kept in the same
/// incidence component. `preferred_assignment_edges` identifies additional
/// variables that should be selected before unrelated incidence variables;
/// those variables do not require a single incidence component.
/// `complete_solution_valid` is evaluated only after every endpoint pair has
/// been assigned. Use it for global preferences whose result cannot be known
/// from a partial assignment.
pub(crate) struct ParseStandardMeshCandidateOutcomeInputs<
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
    FP,
    FC,
> where
    FP: Fn(&[Option<[usize; 2]>]) -> bool,
    FC: Fn(&[Option<[usize; 2]>]) -> bool,
{
    pub(crate) bytes: &'input0 [u8],
    pub(crate) edge_faces: &'input1 [[usize; 2]],
    pub(crate) edge_candidates: &'input2 [Vec<[usize; 2]>],
    pub(crate) edge_classes: &'input3 [usize],
    pub(crate) edge_geometry: &'input4 [MeshEdgeGeometry],
    pub(crate) edge_identity_evidence: &'input5 [bool],
    pub(crate) edge_direction_evidence: &'input6 [bool],
    pub(crate) global_handle_ports: bool,
    pub(crate) partial_constraint_edges: &'input7 [bool],
    pub(crate) preferred_assignment_edges: &'input8 [bool],
    pub(crate) priority_edges: Option<&'input9 [bool]>,
    pub(crate) assignment_dependencies: Option<&'input10 [Vec<usize>]>,
    pub(crate) mesh_cache: Option<&'input11 crate::solve::missing_edge::StandardMeshCache>,
    pub(crate) budget: &'input12 WorkBudget,
    pub(crate) partial_solution_valid: FP,
    pub(crate) complete_solution_valid: FC,
}

/// Runs `run`; test builds assert that `budget` paid for every work unit
/// charged during it.
#[cfg(test)]
pub(super) fn charged_to<T>(budget: &WorkBudget, run: impl FnOnce() -> T) -> T {
    let before = budget.consumed();
    let (result, charged) = cadmpeg_core::decode::test_support::work_charged(run);
    let paid = u64::try_from(budget.consumed() - before).unwrap_or(u64::MAX);
    assert!(
        charged <= paid,
        "work outside the candidate budget: charged {charged}, budget paid {paid}"
    );
    result
}

#[cfg(not(test))]
pub(super) fn charged_to<T>(_budget: &WorkBudget, run: impl FnOnce() -> T) -> T {
    run()
}

pub(crate) fn parse_standard_mesh_candidate_outcome<FP, FC>(
    ctx: &DecodeContext<'_>,
    inputs: ParseStandardMeshCandidateOutcomeInputs<
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
        FP,
        FC,
    >,
) -> Result<MeshCandidateSolve, CodecError>
where
    FP: Fn(&[Option<[usize; 2]>]) -> bool,
    FC: Fn(&[Option<[usize; 2]>]) -> bool,
{
    let budget = inputs.budget;
    charged_to(budget, || solve_standard_mesh_candidate(ctx, inputs))
}

pub(super) fn solve_standard_mesh_candidate<FP, FC>(
    ctx: &DecodeContext<'_>,
    inputs: ParseStandardMeshCandidateOutcomeInputs<
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
        FP,
        FC,
    >,
) -> Result<MeshCandidateSolve, CodecError>
where
    FP: Fn(&[Option<[usize; 2]>]) -> bool,
    FC: Fn(&[Option<[usize; 2]>]) -> bool,
{
    let ParseStandardMeshCandidateOutcomeInputs {
        bytes,
        edge_faces,
        edge_candidates,
        edge_classes,
        edge_geometry,
        edge_identity_evidence,
        edge_direction_evidence,
        global_handle_ports,
        partial_constraint_edges,
        preferred_assignment_edges,
        priority_edges,
        assignment_dependencies,
        mesh_cache,
        budget,
        partial_solution_valid,
        complete_solution_valid,
    } = inputs;

    let endpoint_budget = budget.charging_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
    let Some((face_count, edge_rows, vertex_points, mut mesh_domains, port_identities)) =
        (|| -> Result<Option<_>, CodecError> {
            let Some(face_run) = largest_fbb_run(bytes) else {
                return Ok(None);
            };
            let face_count = face_run.face_count();
            let after_faces = face_run.after_faces();
            let Some((edge_rows, vertex_header)) = parse_edge_tables(bytes, after_faces) else {
                return Ok(None);
            };
            let Some(vertex_points) = parse_vertex_table(bytes, vertex_header) else {
                return Ok(None);
            };
            let boundary_context = StandardMeshBoundaryContext::parse_ports_with_cache(
                ctx,
                bytes,
                edge_faces,
                global_handle_ports,
                mesh_cache,
            )?;
            let Some(boundary_context) = boundary_context else {
                return Ok(None);
            };
            let mesh_domains = standard_mesh_boundary_domains_from_context(
                ctx,
                &boundary_context,
                Some(edge_candidates),
                true,
            )?;
            let Some(mesh_domains) = mesh_domains else {
                return Ok(None);
            };
            // Standard-row endpoints are oriented by the complete face quotient.
            let Some(port_identities) =
                crate::solve::missing_edge::solver_ports(ctx, bytes, global_handle_ports)?
            else {
                return Ok(None);
            };
            Ok(Some((
                face_count,
                edge_rows,
                vertex_points,
                mesh_domains,
                port_identities,
            )))
        })()?
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::InputStructure,
        )));
    };
    let coordinate_gauge = build_mesh_coordinate_gauge(
        ctx,
        vertex_points.len(),
        &edge_rows,
        edge_faces,
        edge_geometry,
        edge_candidates,
        edge_identity_evidence,
    )?;
    let candidate_gauge = Some(MeshCandidateGauge {
        edge_rows: &edge_rows,
        edge_faces,
        edge_geometry,
        edge_candidates,
        edge_identity_evidence,
        coordinate_gauge: Some(&coordinate_gauge),
    });
    if edge_rows.len() != edge_faces.len()
        || edge_rows.len() != edge_candidates.len()
        || edge_rows.len() != edge_classes.len()
        || edge_rows.len() != edge_geometry.len()
        || edge_rows.len() != edge_direction_evidence.len()
        || edge_rows.len() != partial_constraint_edges.len()
        || edge_rows.len() != preferred_assignment_edges.len()
        || priority_edges.is_some_and(|edges| edges.len() != edge_rows.len())
        || assignment_dependencies.is_some_and(|dependencies| {
            dependencies.len() != edge_rows.len()
                || dependencies
                    .iter()
                    .flatten()
                    .any(|edge| *edge >= edge_rows.len())
        })
        || edge_candidates
            .iter()
            .flatten()
            .flatten()
            .any(|point| *point >= vertex_points.len())
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::InputCardinality,
        )));
    }
    if mesh_domains.len() != face_count {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::FaceBoundaryCardinality,
        )));
    }
    for domain in &mut mesh_domains {
        if let MeshFaceBoundaryDomain::Ordered(assignments) = domain {
            deduplicate_mesh_quotient_assignments(std::slice::from_mut(assignments));
        }
    }
    if !mesh_domains_have_incident_edge_support(edge_faces, &mesh_domains) {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::QuotientPreparation,
        )));
    }
    if port_identities.len() != edge_rows.len() {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::PortCardinality,
        )));
    }
    let Some(mut mesh_quotient) =
        initial_mesh_quotient(edge_candidates, vertex_points.len(), &port_identities)?
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::QuotientPreparation,
        )));
    };
    let mut propagated_quotient = mesh_quotient.clone();
    match propagate_common_ordered_face_quotients(
        ctx,
        &mesh_domains,
        edge_candidates,
        &mut propagated_quotient,
        budget,
    )? {
        Some(()) => mesh_quotient = propagated_quotient,
        None if budget.exhausted() => {}
        None => {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
                MeshCandidateRejection::QuotientPreparation,
            )))
        }
    }
    if edge_candidates.iter().any(Vec::is_empty)
        && propagate_common_boundary_components(
            ctx,
            &mesh_domains,
            edge_candidates,
            &mut mesh_quotient,
            budget,
        )?
        .is_none()
    {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::QuotientPreparation,
        )));
    }
    let completed_edge_candidates = edge_candidates.to_vec();
    if !mesh_quotient.edge_domains_viable(&completed_edge_candidates)? {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::QuotientPreparation,
        )));
    }
    let Some(class_constraint) =
        edge_class_search_constraint(edge_classes, &completed_edge_candidates)
    else {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::EdgeClassConstraint,
        )));
    };
    let mut constraint_edges = Vec::new();

    constraint_edges.extend(
        partial_constraint_edges
            .iter()
            .zip(preferred_assignment_edges)
            .zip(&class_constraint.active)
            .map(|((partial, preferred), class)| *partial || *preferred || *class),
    );
    let mut assignment_predecessors = vec![None; completed_edge_candidates.len()];
    for &(left, right) in &class_constraint.ordered {
        assignment_predecessors[right] = Some(
            assignment_predecessors[right].map_or(left, |predecessor: usize| predecessor.max(left)),
        );
    }
    let constrained_partial_solution_valid = |pairs: &[Option<[usize; 2]>]| {
        endpoint_pairs_respect_candidate_domains(pairs, &completed_edge_candidates)
            && partial_solution_valid(pairs)
    };
    let complete_preference_rejected = Cell::new(false);
    let constrained_complete_solution_valid = |pairs: &[Option<[usize; 2]>]| {
        let valid = endpoint_pairs_respect_candidate_domains(pairs, &completed_edge_candidates)
            && complete_solution_valid(pairs);
        if !valid {
            complete_preference_rejected.set(true);
        }
        valid
    };
    let mut incidence_solution = None;
    let mut incidence_ambiguity = None;
    let mut incidence_exhausted = false;
    let mut endpoint_resolution_memo = HashMap::<Vec<[usize; 2]>, MeshEndpointResolve>::new();
    let pair_solutions = visit_incidence_endpoint_pair_solutions_with_coordinate_root_policy(
        ctx,
        VisitIncidenceEndpointPairSolutionsWithCoordinateRootPolicyInputs {
            edge_rows: &edge_rows,
            vertex_points: &vertex_points,
            edge_faces,
            edge_candidates: &completed_edge_candidates,
            face_count,
            mesh_assignments: Some(&mesh_domains),
            mesh_quotient: Some(&mesh_quotient),
            coordinate_root_policy: CoordinateRootPolicy::DeferToVisitor,
            partial_solution_valid: Some(MeshPartialEndpointConstraint {
                active_edges: &constraint_edges,
                coupled_edges: partial_constraint_edges,
                assignment_order: AssignmentOrder::new(
                    Some(&assignment_predecessors),
                    assignment_dependencies,
                ),
                valid: &constrained_partial_solution_valid,
            }),
            complete_solution_budget: Some(&endpoint_budget),
            solution_valid: &|pairs| {
                let mut completed = Vec::new();

                completed.extend(pairs.iter().copied().map(Some));
                Ok(constrained_complete_solution_valid(&completed))
            },
            visitor: &mut |pairs| -> Result<ControlFlow<()>, CodecError> {
                let endpoint_resolution = if let Some(cached) = endpoint_resolution_memo.get(pairs)
                {
                    cached.clone()
                } else {
                    let Some(oriented_pairs) = restore_unique_endpoint_pair_orientations(
                        pairs,
                        &completed_edge_candidates,
                    ) else {
                        return Ok(ControlFlow::Continue(()));
                    };
                    let mut singleton = Vec::new();

                    for &pair in &oriented_pairs {
                        singleton.push(vec![pair]);
                    }
                    let Some(mut mesh_assignments) =
                        materialize_boundary_domains(&mesh_domains, &oriented_pairs)?
                    else {
                        return Ok(ControlFlow::Continue(()));
                    };
                    deduplicate_mesh_quotient_assignments(&mut mesh_assignments);
                    let endpoint_resolution_budget =
                        endpoint_budget.charging_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
                    let resolution = resolve_standard_mesh_endpoint_candidates(
                        ctx,
                        ResolveStandardMeshEndpointCandidatesInputs {
                            edge_rows: &edge_rows,
                            vertex_points: &vertex_points,
                            edge_candidates: &singleton,
                            assignments: mesh_assignments,
                            port_identities: &port_identities,
                            prepared_quotient: None,
                            edge_direction_evidence: Some(edge_direction_evidence),
                            budget: &endpoint_resolution_budget,
                            partial_solution_valid: Some(&constrained_partial_solution_valid),
                            complete_solution_valid: Some(&constrained_complete_solution_valid),
                            candidate_gauge,
                            priority_edges,
                        },
                    )?;
                    // Exhaustion is deterministic for this input and child budget.
                    // The parent budget only decreases, so retrying the same key
                    // cannot turn an exhausted materialization into a solution.
                    if endpoint_resolution_memo.len() < MAX_ENDPOINT_RESOLUTION_MEMO_ENTRIES {
                        let endpoint_key = pairs.to_vec();
                        let retained = resolution.clone();
                        endpoint_resolution_memo.insert(endpoint_key, retained);
                    }
                    resolution
                };
                let candidate = match endpoint_resolution {
                    MeshSolve::Solved((topology, assignment)) => (topology, assignment),
                    MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {
                        return Ok(ControlFlow::Continue(()))
                    }
                    MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => {
                        incidence_ambiguity = Some(MeshCandidateAmbiguity::EndpointResolution);
                        return Ok(ControlFlow::Break(()));
                    }
                    MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => {
                        incidence_exhausted = true;
                        return Ok(ControlFlow::Break(()));
                    }
                };
                match &incidence_solution {
                    Some(stored) => {
                        if !mesh_candidates_equivalent(ctx, stored, &candidate, candidate_gauge)? {
                            incidence_ambiguity =
                                Some(MeshCandidateAmbiguity::DistinctTopologySolutions);
                            return Ok(ControlFlow::Break(()));
                        }
                        Ok(ControlFlow::Continue(()))
                    }
                    None => {
                        incidence_solution = Some(candidate);
                        Ok(ControlFlow::Continue(()))
                    }
                }
            },
        },
    )?;
    if let Some(ambiguity) = incidence_ambiguity {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(
            ambiguity,
        )));
    }
    let exhaustion = |from_endpoint_resolution: bool| {
        if complete_preference_rejected.get() {
            MeshCandidateExhaustion::PreferredSolutionSearch
        } else if from_endpoint_resolution {
            MeshCandidateExhaustion::EndpointResolution
        } else {
            MeshCandidateExhaustion::IncidenceEnumeration
        }
    };
    let incidence_rejection = match pair_solutions {
        IncidenceSolve::Ambiguous => {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(
                MeshCandidateAmbiguity::CoordinateRootClosure,
            )));
        }
        IncidenceSolve::Exhausted => {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(
                exhaustion(incidence_exhausted),
            )));
        }
        _ if incidence_exhausted => {
            return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(
                exhaustion(true),
            )));
        }
        IncidenceSolve::Rejected(rejection) => {
            MeshEndpointIncidenceRejection::NoAssignment(rejection)
        }
        IncidenceSolve::Solved(_) => MeshEndpointIncidenceRejection::BoundaryReconstruction,
    };
    if let Some((topology, assignment)) = incidence_solution {
        // Canonicalization is a representation step; retain the validated raw candidate if unavailable.
        let (topology, assignment) =
            canonicalize_mesh_candidate_for_output(ctx, &topology, &assignment, candidate_gauge)?
                .unwrap_or((topology, assignment));
        return Ok(MeshSolve::Solved((topology, assignment)));
    }
    let fallback = (|| -> Result<Option<MeshEndpointResolve>, CodecError> {
        let mut assignments = Vec::new();

        for domain in mesh_domains {
            match domain {
                MeshFaceBoundaryDomain::Ordered(face) => assignments.push(face),
                MeshFaceBoundaryDomain::UnorderedFullCycle(_)
                | MeshFaceBoundaryDomain::DeferredValidation(_) => return Ok(None),
            }
        }
        let resolution = resolve_standard_mesh_endpoint_candidates(
            ctx,
            ResolveStandardMeshEndpointCandidatesInputs {
                edge_rows: &edge_rows,
                vertex_points: &vertex_points,
                edge_candidates,
                assignments,
                port_identities: &port_identities,
                prepared_quotient: Some(&mesh_quotient),
                edge_direction_evidence: Some(edge_direction_evidence),
                budget: &endpoint_budget,
                partial_solution_valid: Some(&constrained_partial_solution_valid),
                complete_solution_valid: Some(&constrained_complete_solution_valid),
                candidate_gauge,
                priority_edges,
            },
        )?;
        Ok(Some(resolution))
    })()?;
    Ok(match fallback {
        Some(MeshSolve::Solved((topology, assignment))) => {
            // Canonicalization is a representation step; retain the validated raw candidate if unavailable.
            let (topology, assignment) = canonicalize_mesh_candidate_for_output(
                ctx,
                &topology,
                &assignment,
                candidate_gauge,
            )?
            .unwrap_or((topology, assignment));
            MeshSolve::Solved((topology, assignment))
        }
        Some(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(()))) => MeshSolve::Failed(
            MeshCandidateFailure::Ambiguous(MeshCandidateAmbiguity::EndpointResolution),
        ),
        Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))) => MeshSolve::Failed(
            MeshCandidateFailure::Exhausted(if complete_preference_rejected.get() {
                MeshCandidateExhaustion::PreferredSolutionSearch
            } else {
                MeshCandidateExhaustion::EndpointResolution
            }),
        ),
        Some(MeshSolve::Failed(MeshCandidateFailure::Rejected(()))) | None => {
            MeshSolve::Failed(MeshCandidateFailure::Rejected(
                MeshCandidateRejection::EndpointIncidence(incidence_rejection),
            ))
        }
    })
}

/// Solve a standard mesh whose repeated edge-face rows still carry alternate
/// second-face domains. Each concrete face assignment is handed to the
/// existing solver, so optional incidences never become required trim uses.
/// A second solved assignment is semantic ambiguity: no topology gauge may
/// erase a different edge-to-face incidence graph.
pub(crate) fn parse_standard_mesh_candidate_outcome_with_face_assignments<F>(
    candidates: MeshFaceAssignmentCandidates<'_>,
    budget: &WorkBudget,
    mut solve: F,
) -> Result<MeshFaceDomainCandidateSolve, CodecError>
where
    F: FnMut(&[[usize; 2]], &WorkBudget) -> Result<MeshCandidateSolve, CodecError>,
{
    let mut solution: Option<(Vec<[usize; 2]>, StandardTopologyDraft, Vec<usize>)> = None;
    let mut rejection = None;
    let mut ambiguity = None;
    let mut exhaustion = None;
    let mut evaluate = |assignment: &[[usize; 2]]| -> Result<bool, CodecError> {
        if !budget.charge() {
            exhaustion = Some(MeshCandidateExhaustion::FaceDomainEnumeration);
            return Ok(false);
        }
        let branch_budget = budget.child_slice(MAX_MESH_TOPOLOGY_OPERATIONS);
        let outcome = solve(assignment, &branch_budget)?;
        if budget.consume_child(&branch_budget).is_err() {
            exhaustion = Some(MeshCandidateExhaustion::FaceDomainEnumeration);
            return Ok(false);
        }
        match outcome {
            MeshSolve::Solved((topology, point_assignment)) => {
                if let Some((_, stored_topology, stored_assignment)) = &solution {
                    if stored_topology != &topology || stored_assignment != &point_assignment {
                        ambiguity = Some(MeshCandidateAmbiguity::DistinctTopologySolutions);
                        return Ok(false);
                    }
                    return Ok(true);
                }
                solution = Some((assignment.to_vec(), topology, point_assignment));
                Ok(true)
            }
            MeshSolve::Failed(MeshCandidateFailure::Rejected(reason)) => {
                rejection.get_or_insert(reason);
                Ok(true)
            }
            MeshSolve::Failed(MeshCandidateFailure::Ambiguous(reason)) => {
                ambiguity = Some(reason);
                Ok(false)
            }
            MeshSolve::Failed(MeshCandidateFailure::Exhausted(reason)) => {
                exhaustion = Some(reason);
                Ok(false)
            }
        }
    };
    let visit = match candidates {
        MeshFaceAssignmentCandidates::Domains {
            edge_faces,
            allowed_faces,
            face_count,
        } => visit_duplicate_face_assignments(
            edge_faces,
            allowed_faces,
            face_count,
            MAX_FACE_DOMAIN_ASSIGNMENTS,
            &mut evaluate,
        )?,
        MeshFaceAssignmentCandidates::Concrete {
            assignments,
            face_count,
        } => {
            let edge_count = assignments.first().map_or(0, Vec::len);
            if assignments.len() > MAX_FACE_DOMAIN_ASSIGNMENTS
                || assignments.iter().any(|assignment| {
                    assignment.len() != edge_count
                        || assignment.iter().flatten().any(|face| *face >= face_count)
                })
            {
                None
            } else {
                let mut outcome = DuplicateFaceAssignmentVisit::Complete;
                for assignment in assignments {
                    if !evaluate(assignment)? {
                        outcome = DuplicateFaceAssignmentVisit::Stopped;
                        break;
                    }
                }
                Some(outcome)
            }
        }
    };
    if visit.is_none() {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::InputStructure,
        )));
    }
    if let Some(ambiguity) = ambiguity {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(
            ambiguity,
        )));
    }
    if let Some(exhaustion) = exhaustion {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(
            exhaustion,
        )));
    }
    if matches!(visit, Some(DuplicateFaceAssignmentVisit::Exhausted)) {
        return Ok(MeshSolve::Failed(MeshCandidateFailure::Exhausted(
            MeshCandidateExhaustion::FaceDomainEnumeration,
        )));
    }
    Ok(
        if let Some((faces, topology, point_assignment)) = solution {
            MeshSolve::Solved((faces, topology, point_assignment))
        } else {
            MeshSolve::Failed(MeshCandidateFailure::Rejected(
                rejection.unwrap_or(MeshCandidateRejection::InputStructure),
            ))
        },
    )
}

#[test]
fn mesh_candidate_rejection_retains_the_failed_solver_stage() {
    catia_test_context!(ctx);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(matches!(
        parse_standard_mesh_candidate_outcome(
            &ctx,
            ParseStandardMeshCandidateOutcomeInputs {
                bytes: &[],
                edge_faces: &[],
                edge_candidates: &[],
                edge_classes: &[],
                edge_geometry: &[],
                edge_identity_evidence: &[],
                edge_direction_evidence: &[],
                global_handle_ports: false,
                partial_constraint_edges: &[],
                preferred_assignment_edges: &[],
                priority_edges: None,
                assignment_dependencies: None,
                mesh_cache: None,
                budget: &budget,
                partial_solution_valid: |_| true,
                complete_solution_valid: |_| true,
            }
        )
        .expect("parse standard mesh candidate outcome"),
        MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::InputStructure
        ))
    ));
}

#[test]
fn face_domain_solver_returns_the_unique_concrete_assignment() {
    let edge_faces = [[0, 0]];
    let allowed_faces = [vec![2, 1]];
    let budget = WorkBudget::new(MAX_MESH_TOPOLOGY_OPERATIONS);
    let mut visited = Vec::new();
    let result = parse_standard_mesh_candidate_outcome_with_face_assignments(
        MeshFaceAssignmentCandidates::Domains {
            edge_faces: &edge_faces,
            allowed_faces: &allowed_faces,
            face_count: 3,
        },
        &budget,
        |faces, _| {
            visited.push(faces.to_vec());
            Ok(if faces[0][1] == 2 {
                MeshSolve::Solved((
                    StandardTopologyDraft {
                        faces: Vec::new(),
                        edge_rows: Vec::new(),
                        vertex_points: Vec::new(),
                        logical_vertex_count: 0,
                    },
                    Vec::new(),
                ))
            } else {
                MeshSolve::Failed(MeshCandidateFailure::Rejected(
                    MeshCandidateRejection::InputStructure,
                ))
            })
        },
    )
    .expect("parse standard mesh candidate outcome with face assignments");

    let MeshSolve::Solved((faces, _, _)) = result else {
        panic!("face-domain solver did not retain the unique branch");
    };
    assert_eq!(visited, vec![vec![[0, 0]], vec![[0, 1]], vec![[0, 2]]]);
    assert_eq!(faces, vec![[0, 2]]);
}

#[test]
fn face_domain_solver_evaluates_only_endpoint_closed_assignments() {
    let assignments = [vec![[0, 0]], vec![[0, 2]]];
    let budget = WorkBudget::new(MAX_MESH_TOPOLOGY_OPERATIONS);
    let mut visited = Vec::new();
    let result = parse_standard_mesh_candidate_outcome_with_face_assignments(
        MeshFaceAssignmentCandidates::Concrete {
            assignments: &assignments,
            face_count: 3,
        },
        &budget,
        |faces, _| {
            visited.push(faces.to_vec());
            Ok(if faces[0][1] == 2 {
                MeshSolve::Solved((
                    StandardTopologyDraft {
                        faces: Vec::new(),
                        edge_rows: Vec::new(),
                        vertex_points: Vec::new(),
                        logical_vertex_count: 0,
                    },
                    Vec::new(),
                ))
            } else {
                MeshSolve::Failed(MeshCandidateFailure::Rejected(
                    MeshCandidateRejection::InputStructure,
                ))
            })
        },
    )
    .expect("parse standard mesh candidate outcome with face assignments");

    let MeshSolve::Solved((faces, _, _)) = result else {
        panic!("face-domain solver did not retain the closed assignment");
    };
    assert_eq!(visited, assignments);
    assert_eq!(faces, vec![[0, 2]]);
}

#[test]
fn face_domain_solver_reports_distinct_assignments_as_ambiguity() {
    let edge_faces = [[0, 0]];
    let allowed_faces = [vec![1, 2]];
    let budget = WorkBudget::new(MAX_MESH_TOPOLOGY_OPERATIONS);
    let result = parse_standard_mesh_candidate_outcome_with_face_assignments(
        MeshFaceAssignmentCandidates::Domains {
            edge_faces: &edge_faces,
            allowed_faces: &allowed_faces,
            face_count: 3,
        },
        &budget,
        |assignment, _| {
            Ok(MeshSolve::Solved((
                StandardTopologyDraft {
                    faces: Vec::new(),
                    edge_rows: Vec::new(),
                    vertex_points: Vec::new(),
                    logical_vertex_count: 0,
                },
                vec![assignment[0][1]],
            )))
        },
    )
    .expect("parse standard mesh candidate outcome with face assignments");

    assert!(matches!(
        result,
        MeshSolve::Failed(MeshCandidateFailure::Ambiguous(
            MeshCandidateAmbiguity::DistinctTopologySolutions
        ))
    ));
}

#[cfg(test)]
#[test]
fn face_domain_local_work_exhaustion_reports_an_exhausted_search() {
    crate::test_support::with_service_context(|ctx| {
        let budget = cadmpeg_core::decode::WorkBudget::new(0);
        let assignments = [vec![[0, 0]]];
        let outcome = parse_standard_mesh_candidate_outcome_with_face_assignments(
            MeshFaceAssignmentCandidates::Concrete {
                assignments: &assignments,
                face_count: 1,
            },
            &budget,
            |_, _| panic!("an exhausted slice must not run the branch"),
        )
        .expect("local slice exhaustion is not a session refusal");
        assert_eq!(
            outcome,
            MeshSolve::Failed(MeshCandidateFailure::Exhausted(
                MeshCandidateExhaustion::FaceDomainEnumeration
            ))
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn concrete_face_assignment_returns_the_solved_topology() {
    let assignments = [vec![[0, 0]]];
    let result = {
        let budget = WorkBudget::new(MAX_MESH_TOPOLOGY_OPERATIONS);
        parse_standard_mesh_candidate_outcome_with_face_assignments(
            MeshFaceAssignmentCandidates::Concrete {
                assignments: &assignments,
                face_count: 1,
            },
            &budget,
            |_, _| {
                Ok(MeshSolve::Solved((
                    StandardTopologyDraft {
                        faces: Vec::new(),
                        edge_rows: Vec::new(),
                        vertex_points: Vec::new(),
                        logical_vertex_count: 0,
                    },
                    Vec::new(),
                )))
            },
        )
    };
    assert!(matches!(result, Ok(MeshSolve::Solved(_))));
}
