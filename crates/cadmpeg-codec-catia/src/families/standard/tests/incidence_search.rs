use crate::solve::mesh_quotient::SearchOutcome;

use super::*;

#[test]
fn endpoint_incidence_builds_oriented_tetrahedron_cycles() {
    let rows: Vec<_> = (0..6)
        .map(|edge| EdgeRow {
            kind: 1,
            handles: vec![edge * 2, edge * 2 + 1],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        })
        .collect();
    let points = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let edge_faces = [[0, 1], [0, 2], [0, 3], [1, 3], [1, 2], [2, 3]];
    let edge_points = [[0, 1], [1, 2], [2, 0], [0, 3], [3, 1], [2, 3]];
    let topology = reconstruct_incidence(rows, points, &edge_faces, &edge_points, 4)
        .expect("closed oriented incidence");
    assert_eq!(topology.face_count(), 4);
    assert!(topology
        .faces()
        .iter()
        .all(|face| { face.boundaries.len() == 1 && face.boundaries[0].coedges.len() == 3 }));
    let mut uses = vec![Vec::new(); 6];
    for face in topology.faces() {
        for coedge in &face.boundaries[0].coedges {
            uses[coedge.edge_row].push(coedge.reversed);
        }
    }
    assert!(uses
        .iter()
        .all(|senses| senses == &[false, true] || senses == &[true, false]));
}

#[test]
fn endpoint_candidate_search_selects_a_face_closing_assignment() {
    let rows: Vec<_> = (0..6)
        .map(|edge| EdgeRow {
            kind: 1,
            handles: vec![edge * 2, edge * 2 + 1],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        })
        .collect();
    let points = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let edge_faces = [[0, 1], [0, 2], [0, 3], [1, 3], [1, 2], [2, 3]];
    let candidates = vec![
        vec![[0, 2], [0, 1]],
        vec![[1, 2]],
        vec![[0, 2]],
        vec![[0, 3]],
        vec![[1, 3]],
        vec![[2, 3]],
    ];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let topology = reconstruct_incidence_candidates(
        &rows,
        &points,
        &edge_faces,
        &candidates,
        None,
        4,
        &budget,
    )
    .expect("unique face-closing endpoint assignment");
    assert_eq!(topology.edge_vertices().expect("edge vertices")[0], [0, 1]);

    let ports = [[11, 10], [11, 12], [10, 12], [13, 10], [11, 13], [13, 12]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let topology = reconstruct_incidence_candidates(
        &rows,
        &points,
        &edge_faces,
        &candidates,
        Some(&ports),
        4,
        &budget,
    )
    .expect("unique face-closing assignment with deferred port orientation");
    assert_eq!(topology.edge_vertices().expect("edge vertices")[0], [1, 0]);
}

#[test]
fn endpoint_candidate_fallback_honors_caller_budget() {
    let rows: Vec<_> = (0..6)
        .map(|edge| EdgeRow {
            kind: 1,
            handles: vec![edge * 2, edge * 2 + 1],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        })
        .collect();
    let points = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let edge_faces = [[0, 1], [0, 2], [0, 3], [1, 3], [1, 2], [2, 3]];
    let candidates = vec![
        vec![[0, 2], [0, 1]],
        vec![[1, 2]],
        vec![[0, 2]],
        vec![[0, 3]],
        vec![[1, 3]],
        vec![[2, 3]],
    ];
    let budget = WorkBudget::new(0);

    assert!(reconstruct_incidence_candidates(
        &rows,
        &points,
        &edge_faces,
        &candidates,
        None,
        4,
        &budget,
    )
    .is_none());
    assert!(budget.exhausted());
}

#[test]
fn endpoint_candidate_validation_charges_full_incidence_work() {
    use crate::solve::incidence::{visit_incidence_endpoint_pair_solutions, IncidenceSolve};
    use std::ops::ControlFlow;

    let rows = vec![
        EdgeRow {
            kind: 1,
            handles: vec![0, 1],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        };
        3
    ];
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let edge_faces = [[0, 0]; 3];
    let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];
    let budget = WorkBudget::new(2);
    let mut visited = false;
    let outcome = visit_incidence_endpoint_pair_solutions(
        &rows,
        &points,
        &edge_faces,
        &candidates,
        1,
        None,
        None,
        None,
        Some(&budget),
        &|_| true,
        &mut |_| {
            visited = true;
            ControlFlow::Continue(())
        },
    );

    assert_eq!(outcome, IncidenceSolve::Exhausted);
    assert!(!visited);
}

#[test]
fn incidence_propagation_closes_degree_one_vertices_before_search() {
    let mut choices = vec![vec![[0, 1]], vec![[1, 2], [3, 4]], vec![[2, 0]]];
    let edge_faces = [[0, 0], [0, 0], [0, 0]];
    crate::solve::incidence::prune_incidence_choices(&mut choices, &edge_faces, 1, 5)
        .expect("face incidence is satisfiable");
    assert_eq!(choices, vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 0]]]);
}

#[test]
fn incidence_propagation_removes_candidates_with_unsupported_vertices() {
    let mut choices = vec![vec![[0, 1], [2, 3]], vec![[0, 1]]];
    let edge_faces = [[0, 0], [0, 0]];
    crate::solve::incidence::prune_incidence_choices(&mut choices, &edge_faces, 1, 4)
        .expect("face incidence is satisfiable");
    assert_eq!(choices, vec![vec![[0, 1]], vec![[0, 1]]]);
}

#[test]
fn incidence_deferred_support_retains_an_explicit_boundary_gap() {
    let mut choices = vec![vec![[0, 1]]];
    let edge_faces = [[0, 0]];

    prune_incidence_choices_with_deferred_support(&mut choices, &edge_faces, 1, 2)
        .expect("deferred boundary support is not an explicit contradiction");
    assert_eq!(choices, vec![vec![[0, 1]]]);
}

#[test]
fn incidence_propagation_indexes_wide_endpoint_support_domains() {
    let domain = (0..4096).map(|point| [0, point]).collect::<Vec<_>>();
    let mut choices = vec![domain.clone(), domain.clone()];
    let edge_faces = [[0, 0], [0, 0]];

    crate::solve::incidence::prune_incidence_choices(&mut choices, &edge_faces, 1, domain.len())
        .expect("each endpoint candidate has support from the other edge");
    assert_eq!(choices, vec![domain.clone(), domain]);
}

#[test]
fn incidence_propagation_does_not_allocate_the_declared_point_product() {
    let mut choices = vec![vec![[0, 1]], vec![[0, 1]]];
    let edge_faces = [[0, 0], [0, 0]];

    crate::solve::incidence::prune_incidence_choices(&mut choices, &edge_faces, 1, usize::MAX)
        .expect("sparse endpoint incidence is independent of the declared cardinality");
    assert_eq!(choices, vec![vec![[0, 1]], vec![[0, 1]]]);
}

#[test]
fn incidence_component_rejects_a_choice_that_strands_a_degree_one_vertex() {
    let choices = vec![vec![[0, 1], [0, 2]], vec![[0, 2]]];
    let edge_faces = [[0, 0], [0, 0]];
    let face_edges = vec![vec![0, 1]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true; 2],
        edges: &[0, 1],
        constraints: vec![(0, 0), (0, 1), (0, 2)],
        assignment: vec![None; 2],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert!(!search.candidate_fits(0, [0, 1]));
    assert!(search.candidate_fits(0, [0, 2]));
}

#[test]
fn incidence_component_indexes_and_revalidates_frontier_support() {
    const IRRELEVANT_EDGES: usize = 32;
    let mut choices = vec![vec![[0, 1]], vec![[0, 1]]];
    choices.extend((0..IRRELEVANT_EDGES).map(|edge| vec![[edge + 2, edge + 3]]));
    let edge_faces = vec![[0, 0]; choices.len()];
    let face_edges = vec![(0..choices.len()).collect::<Vec<_>>()];
    let mut explicit_point_supports = vec![HashMap::new(); choices.len()];
    for supports in explicit_point_supports.iter_mut().take(2) {
        supports.insert(0, vec![[0, 1]]);
        supports.insert(1, vec![[0, 1]]);
    }
    let point_support_edges = vec![HashMap::from([(0, vec![0, 1]), (1, vec![0, 1])])];
    let budget = WorkBudget::new(16);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports,
        point_support_edges,
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true; choices.len()],
        edges: &[0, 1],
        constraints: vec![(0, 0), (0, 1)],
        assignment: vec![None; choices.len()],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert!(search.candidate_fits(0, [0, 1]));
    assert!(!budget.exhausted());
    search.assignment[1] = Some([0, 1]);
    assert!(!search.candidate_fits(0, [0, 1]));
}

#[test]
fn incidence_component_caches_implicit_frontier_support() {
    let choices = vec![vec![[0, 1]], Vec::new()];
    let edge_faces = [[0, 0], [0, 0]];
    let face_edges = vec![vec![0, 1]];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&choices, 2, &[[0, 1], [0, 1]])
            .expect("initial quotient");
    let coordinate_domains = quotient
        .prepare_coordinate_root_domains(2, &choices, None)
        .expect("implicit coordinate domains");
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: Some(&coordinate_domains),
        active: vec![true; 2],
        edges: &[0, 1],
        constraints: vec![(0, 0), (0, 1)],
        assignment: vec![None; 2],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert!(search.candidate_fits(0, [0, 1]));
    assert_eq!(
        *search.degree_support_witnesses.borrow(),
        HashMap::from([((0, 0), vec![(1, [0, 1])]), ((0, 1), vec![(1, [0, 1])]),])
    );
}

#[test]
fn incidence_degree_support_budget_exhaustion_keeps_candidate_unknown() {
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    let edge_faces = [[0, 0], [0, 0]];
    let face_edges = vec![vec![0, 1]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let degree_budget = WorkBudget::new(1);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true; 2],
        edges: &[0, 1],
        constraints: vec![(0, 0), (0, 1)],
        assignment: vec![None; 2],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &degree_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert!(search.candidate_fits(0, [0, 1]));
    assert!(!budget.exhausted());
    assert!(degree_budget.exhausted());
    search.search();
    assert!(!search.outcome.is_closed());
}

#[test]
fn incidence_component_requires_degree_support_to_fit_every_incident_face() {
    let choices = vec![vec![[0, 1]], vec![[1, 2]]];
    let edge_faces = [[0, 0], [0, 1]];
    let face_edges = vec![vec![0, 1], vec![1]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true; 2],
        edges: &[0, 1],
        constraints: vec![(0, 0), (0, 1), (0, 2), (1, 1), (1, 2)],
        assignment: vec![None; 2],
        degrees: sparse_degrees(&[&[0, 0, 0], &[0, 0, 2]]),
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert!(!search.candidate_fits(0, [0, 1]));
}

#[test]
fn incidence_candidate_checks_ordered_faces_with_implicit_edge_domains() {
    let choices = vec![vec![[0, 0]], Vec::new()];
    let edge_faces = [[0, 0], [0, 0]];
    let face_edges = vec![vec![0, 1]];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&choices, 2, &[[10, 10], [11, 12]])
            .expect("initial quotient");
    let coordinate_domains = quotient
        .prepare_coordinate_root_domains(2, &choices, None)
        .expect("implicit coordinate domains");
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let assignments = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
    ])];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: Some(&coordinate_domains),
        active: vec![true; 2],
        edges: &[0, 1],
        constraints: Vec::new(),
        assignment: vec![None; 2],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert!(!search.candidate_fits(0, [0, 0]));
    assert!(!propagation_budget.exhausted());
}

#[test]
fn incidence_branch_reuses_candidate_viability_across_incident_face_frontiers() {
    let choices = vec![vec![[0, 2]], vec![[2, 4]]];
    let edge_faces = [[0, 1], [0, 1]];
    let face_edges = vec![vec![0, 1], vec![0, 1]];
    let budget = WorkBudget::new(4);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true, true],
        edges: &[0, 1],
        constraints: vec![(0, 0), (1, 0)],
        assignment: vec![None, None],
        degrees: sparse_degrees(&[&[1, 0], &[1, 0]]),
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert_eq!(search.branch_options(None), Some(vec![(0, [0, 2])]));
    assert!(!budget.exhausted());
}

#[test]
fn incidence_branch_stops_ranking_at_a_singleton_domain() {
    let choices = vec![vec![[0, 2]], vec![[2, 4]]];
    let edge_faces = [[0, 0], [0, 0]];
    let face_edges = vec![vec![0, 1], Vec::new()];
    let budget = WorkBudget::new(4);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true, true],
        edges: &[0, 1],
        constraints: vec![(0, 0), (1, 9)],
        assignment: vec![None, None],
        degrees: sparse_degrees(&[&[1, 0], &[1, 9]]),
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert_eq!(search.branch_options(None), Some(vec![(0, [0, 2])]));
    assert!(!budget.exhausted());
}

#[test]
fn incidence_component_uses_operation_budget_for_a_wide_rejected_frontier() {
    const EDGE_COUNT: usize = 9;
    let choices = (0..EDGE_COUNT)
        .map(|edge| vec![[edge * 2, edge * 2], [edge * 2 + 1, edge * 2 + 1]])
        .collect::<Vec<_>>();
    let edge_faces = (0..EDGE_COUNT).map(|face| [face, face]).collect::<Vec<_>>();
    let face_edges = (0..EDGE_COUNT).map(|edge| vec![edge]).collect::<Vec<_>>();
    let edges = (0..EDGE_COUNT).collect::<Vec<_>>();
    let constraints = (0..EDGE_COUNT)
        .flat_map(|face| [(face, face * 2), (face, face * 2 + 1)])
        .collect::<Vec<_>>();
    let solution_filter =
        |solution: &[(usize, [usize; 2])]| solution.iter().all(|(_, pair)| pair[0] % 2 == 0);
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true; EDGE_COUNT],
        edges: &edges,
        constraints,
        assignment: vec![None; EDGE_COUNT],
        degrees: vec![BTreeMap::new(); EDGE_COUNT],
        solutions: Vec::new(),
        solution_filter: Some(&solution_filter),
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    search.search();

    assert!(!search.outcome.is_closed());
    assert_eq!(search.solutions.len(), 1);
}

#[test]
fn incidence_component_schedules_partial_constraint_variables_first() {
    let choices = vec![vec![[0, 1], [0, 2]], vec![[3, 4], [3, 5], [4, 5]]];
    let edge_faces = [[0, 0], [0, 0]];
    let face_edges = vec![vec![0, 1]];
    let edges = [0, 1];
    let active_edges = [true, true];
    let assignment_dependencies = [vec![1], Vec::new()];
    let valid = |_: &[Option<[usize; 2]>]| true;
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true; 2],
        edges: &edges,
        constraints: Vec::new(),
        assignment: vec![None; 2],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: Some(MeshPartialEndpointConstraint {
            active_edges: &active_edges,
            coupled_edges: &active_edges,
            assignment_order: AssignmentOrder::new(None, Some(&assignment_dependencies)),
            valid: &valid,
        }),
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert_eq!(
        search.branch_options(None),
        Some(vec![(1, [3, 4]), (1, [3, 5]), (1, [4, 5])])
    );
}

#[test]
fn incidence_component_assigns_canonical_class_members_in_order() {
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[0, 1], [0, 2]],
        vec![[3, 4], [3, 5]],
    ];
    let edge_faces = [[0, 0]; 3];
    let face_edges = vec![vec![0, 1, 2]];
    let active_edges = [false; 3];
    let assignment_predecessors = [None, Some(0), Some(0)];
    let valid = |_: &[Option<[usize; 2]>]| true;
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true, true, false],
        edges: &[0, 1],
        constraints: Vec::new(),
        assignment: vec![None; 3],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: Some(MeshPartialEndpointConstraint {
            active_edges: &active_edges,
            coupled_edges: &active_edges,
            assignment_order: AssignmentOrder::new(Some(&assignment_predecessors), None),
            valid: &valid,
        }),
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert_eq!(
        search.branch_options(None),
        Some(vec![(0, [0, 1]), (0, [0, 2])])
    );

    let independent = crate::solve::incidence::IncidenceComponentSearch {
        active: vec![false, false, true],
        edges: &[2],
        ..search
    };
    assert_eq!(independent.branch_options(None), Some(Vec::new()));
}

#[test]
fn incidence_component_declines_when_its_work_budget_is_exhausted() {
    let choices = vec![vec![[0, 0]]];
    let edge_faces = [[0, 0]];
    let face_edges = vec![vec![0]];
    let edges = [0];
    let budget = WorkBudget::new(0);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true],
        edges: &edges,
        constraints: vec![(0, 0)],
        assignment: vec![None],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    search.search();

    assert!(matches!(search.outcome, SearchOutcome::Exhausted));
    assert!(search.solutions.is_empty());
}

#[test]
fn incidence_face_configuration_scan_does_not_charge_irrelevant_faces() {
    let choices = vec![vec![[0, 0]]];
    let edge_faces = [[0, 0]];
    let face_edges = vec![vec![0]];
    let assignments = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 0,
                reversed: Some(false),
            }]],
        },
    ])];
    let budget = WorkBudget::new(0);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![false],
        edges: &[],
        constraints: Vec::new(),
        assignment: vec![Some([0, 0])],
        degrees: sparse_degrees(&[&[2]]),
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert_eq!(search.face_configuration_options(), None);
    assert!(!budget.exhausted());
}

#[test]
fn exhausted_boundary_lookahead_does_not_exhaust_exact_incidence_search() {
    let choices = vec![vec![[0, 0]]];
    let edge_faces = [[0, 0]];
    let face_edges = vec![vec![0]];
    let assignments = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 0,
                reversed: Some(false),
            }]],
        },
    ])];
    let search_budget = WorkBudget::new(16);
    let propagation_budget = WorkBudget::new(0);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true],
        edges: &[0],
        constraints: vec![(0, 0)],
        assignment: vec![None],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &search_budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    search.search();

    assert!(!search.outcome.is_closed());
    assert_eq!(search.solutions, vec![vec![(0, [0, 0])]]);
    assert!(propagation_budget.exhausted());
}

#[test]
fn incidence_face_configuration_branches_on_the_narrowest_estimated_face() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let choices = vec![
        vec![[0, 0]],
        (1..=300).map(|point| [point, point]).collect::<Vec<_>>(),
    ];
    let edge_faces = [[0, 0], [1, 1]];
    let face_edges = vec![vec![0], vec![1]];
    let assignments = vec![
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0)]],
        }]),
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1)]],
        }]),
    ];
    let budget = WorkBudget::new(4);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true, true],
        edges: &[0, 1],
        constraints: Vec::new(),
        assignment: vec![None, None],
        degrees: vec![BTreeMap::new(), BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert_eq!(
        search.face_configuration_options(),
        Some(vec![vec![(0, [0, 0])]])
    );
    assert!(!budget.exhausted());
}

#[test]
fn incidence_face_configuration_branches_on_the_narrowest_projected_face() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let choices = vec![
        vec![[0, 0], [1, 1]],
        vec![[10, 11]],
        vec![[10, 11], [12, 13], [14, 15]],
    ];
    let edge_faces = [[0, 0], [1, 1], [1, 1]];
    let face_edges = vec![vec![0], vec![1, 2]];
    let assignments = vec![
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0)]],
        }]),
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1), use_(2)]],
        }]),
    ];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true; 3],
        edges: &[0, 1, 2],
        constraints: Vec::new(),
        assignment: vec![None; 3],
        degrees: vec![BTreeMap::new(), BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert_eq!(
        search.face_configuration_options(),
        Some(vec![vec![(1, [10, 11]), (2, [10, 11])]])
    );
}

#[test]
fn incidence_face_configuration_reuses_persistent_domains_across_assignments() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let choices = vec![vec![[0, 0], [1, 1]], vec![[0, 0], [1, 1]]];
    let edge_faces = [[0, 0]; 2];
    let face_edges = vec![vec![0, 1]];
    let assignments = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
    ])];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = WorkBudget::new(2);
    let prepared =
        prepare_face_configuration_domains(Some(&assignments), &choices, &[None; 2], &[true; 2])
            .expect("compiled face factors");
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: Some(prepared),
        coordinate_domains: None,
        active: vec![true; 2],
        edges: &[0, 1],
        constraints: Vec::new(),
        assignment: vec![None; 2],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert_eq!(
        search.face_configuration_options(),
        Some(vec![
            vec![(0, [0, 0]), (1, [0, 0])],
            vec![(0, [1, 1]), (1, [1, 1])]
        ])
    );
    search.assignment[0] = Some([1, 1]);
    assert_eq!(
        search.face_configuration_options(),
        Some(vec![vec![(1, [1, 1])]])
    );
    assert!(!propagation_budget.exhausted());
}

#[test]
fn incidence_face_factor_masks_roll_back_between_configuration_branches() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let choices = vec![vec![[0, 1], [2, 3]], vec![[0, 1], [2, 3]]];
    let assignments = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
    ])];

    let edge_faces = [[0, 0]; 2];
    let face_edges = vec![vec![0, 1]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let prepared =
        prepare_face_configuration_domains(Some(&assignments), &choices, &[None; 2], &[true; 2])
            .expect("compiled face factors");
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: Some(prepared),
        coordinate_domains: None,
        active: vec![true; 2],
        edges: &[0, 1],
        constraints: Vec::new(),
        assignment: vec![None; 2],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    search.search();

    assert_eq!(
        search.solutions,
        vec![
            vec![(0, [0, 1]), (1, [0, 1])],
            vec![(0, [2, 3]), (1, [2, 3])],
        ]
    );
}

#[test]
fn persistent_face_configuration_preparation_retains_global_contradictions() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let assignments = vec![
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        }]),
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1), use_(2)]],
        }]),
    ];
    let choices = vec![vec![[0, 1]], vec![[0, 1], [0, 2]], vec![[0, 2]]];
    let selected = vec![Some([0, 1]), None, Some([0, 2])];
    let prepared = prepare_face_configuration_domains(
        Some(&assignments),
        &choices,
        &selected,
        &[false, true, false],
    )
    .expect("ordered face factors");

    assert!(prepared.domains().iter().flatten().any(Vec::is_empty));

    let compatible = vec![vec![[0, 1]], vec![[0, 1], [0, 2]], vec![[0, 1]]];
    let selected = vec![Some([0, 1]), None, Some([0, 1])];
    let prepared = prepare_face_configuration_domains(
        Some(&assignments),
        &compatible,
        &selected,
        &[false, true, false],
    )
    .expect("compatible ordered face factors");
    assert!(prepared
        .domains()
        .iter()
        .flatten()
        .all(|domain| domain.len() == 1));
}

#[test]
fn incidence_face_configuration_support_retains_shared_edge_correlations() {
    let correlated = vec![
        vec![(0, [0, 1]), (1, [2, 3])],
        vec![(0, [4, 5]), (1, [6, 7])],
    ];
    let mut domains = vec![correlated.clone(), vec![vec![(0, [0, 1]), (1, [2, 3])]]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(prune_face_configuration_support(&mut domains, &budget));
    assert_eq!(domains[0], vec![correlated[0].clone()]);

    let mut incompatible = vec![correlated, vec![vec![(0, [0, 1]), (1, [6, 7])]]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(!prune_face_configuration_support(
        &mut incompatible,
        &budget
    ));

    let preserved = incompatible.clone();
    let budget = WorkBudget::new(0);
    assert!(prune_face_configuration_support(&mut incompatible, &budget));
    assert_eq!(incompatible, preserved);
    assert!(budget.exhausted());
}

#[test]
fn incidence_face_configuration_support_propagates_across_a_factor_chain() {
    let mut domains = vec![
        vec![vec![(0, [0, 1])], vec![(0, [0, 2])]],
        vec![
            vec![(0, [0, 1]), (1, [1, 2])],
            vec![(0, [0, 2]), (1, [2, 3])],
        ],
        vec![vec![(1, [1, 2])]],
    ];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(prune_face_configuration_support(&mut domains, &budget));
    assert_eq!(
        domains,
        vec![
            vec![vec![(0, [0, 1])]],
            vec![vec![(0, [0, 1]), (1, [1, 2])]],
            vec![vec![(1, [1, 2])]],
        ]
    );

    let mut optional = vec![vec![vec![(0, [0, 1])]], vec![vec![], vec![(0, [0, 2])]]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(prune_face_configuration_support(&mut optional, &budget));
    assert_eq!(optional[0], vec![vec![(0, [0, 1])]]);
}

#[test]
fn incidence_face_singleton_support_rejects_an_inconsistent_factor_cycle() {
    let equal = |left, right| {
        vec![
            vec![(left, [0, 0]), (right, [0, 0])],
            vec![(left, [1, 1]), (right, [1, 1])],
        ]
    };
    let different = vec![
        vec![(0, [0, 0]), (2, [1, 1])],
        vec![(0, [1, 1]), (2, [0, 0])],
    ];
    let mut inconsistent = vec![equal(0, 1), equal(1, 2), different];
    let arc_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(prune_face_configuration_support(
        &mut inconsistent,
        &arc_budget
    ));
    assert!(inconsistent.iter().all(|domain| domain.len() == 2));

    let singleton_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(!prune_face_configuration_singleton_support(
        &mut inconsistent,
        &singleton_budget,
    ));

    let mut consistent = vec![equal(0, 1), equal(1, 2), equal(2, 0)];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(prune_face_configuration_singleton_support(
        &mut consistent,
        &budget,
    ));
    assert!(consistent.iter().all(|domain| domain.len() == 2));

    let preserved = consistent.clone();
    let exhausted = WorkBudget::new(0);
    assert!(prune_face_configuration_singleton_support(
        &mut consistent,
        &exhausted,
    ));
    assert_eq!(consistent, preserved);
    assert!(exhausted.exhausted());
}

#[test]
fn incidence_face_singleton_support_tracks_multiword_configuration_masks() {
    let wide = (0..130)
        .map(|point| vec![(0, [point, point])])
        .collect::<Vec<_>>();
    let mut domains = vec![wide, vec![vec![(0, [129, 129])]]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(prune_face_configuration_singleton_support(
        &mut domains,
        &budget,
    ));
    assert_eq!(domains[0], vec![vec![(0, [129, 129])]]);
    assert_eq!(domains[1], vec![vec![(0, [129, 129])]]);
}

#[test]
fn ordered_face_support_prunes_edge_pairs_to_complete_configurations() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: None,
    };
    let domains = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
    ])];
    let mut choices = vec![vec![[0, 1], [0, 2], [3, 4]], vec![[0, 1], [0, 2]]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(prune_ordered_face_endpoint_support(
        &domains,
        &mut choices,
        &budget,
    ));
    assert_eq!(choices, vec![vec![[0, 1], [0, 2]], vec![[0, 1], [0, 2]]]);

    let mut unpruned = vec![vec![[0, 1], [0, 2], [3, 4]], vec![[0, 1], [0, 2]]];
    let exhausted = WorkBudget::new(0);
    assert!(prune_ordered_face_endpoint_support(
        &domains,
        &mut unpruned,
        &exhausted,
    ));
    assert_eq!(
        unpruned,
        vec![vec![[0, 1], [0, 2], [3, 4]], vec![[0, 1], [0, 2]]]
    );
    assert!(exhausted.exhausted());
}

#[test]
fn incidence_forced_face_chain_does_not_consume_branch_budget() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let choices = vec![vec![[0, 0]], vec![[1, 1]]];
    let edge_faces = [[0, 0], [1, 1]];
    let face_edges = vec![vec![0], vec![1]];
    let assignments = vec![
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0)]],
        }]),
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1)]],
        }]),
    ];
    let budget = WorkBudget::new(1);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true, true],
        edges: &[0, 1],
        constraints: Vec::new(),
        assignment: vec![None, None],
        degrees: vec![BTreeMap::new(), BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    search.search();

    assert!(!search.outcome.is_closed());
    assert_eq!(search.solutions, vec![vec![(0, [0, 0]), (1, [1, 1])]]);
}

#[test]
fn incidence_forced_face_configuration_closes_its_frontier_atomically() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    let edge_faces = [[0, 0], [0, 0]];
    let face_edges = vec![vec![0, 1]];
    let assignments = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
    ])];
    let budget = WorkBudget::new(1);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true, true],
        edges: &[0, 1],
        constraints: vec![(0, 0), (0, 1)],
        assignment: vec![None, None],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    search.search();

    assert!(!search.outcome.is_closed());
    assert_eq!(search.solutions, vec![vec![(0, [0, 1]), (1, [0, 1])]]);
}

#[test]
fn incidence_candidate_uses_a_separate_global_quotient_validation_budget() {
    let choices = vec![vec![[0, 0]]];
    let edge_faces = [[0, 0]];
    let face_edges = vec![vec![0]];
    let assignments = vec![MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 1,
                exact_uses: vec![(
                    MeshBoundaryEdgeCandidate {
                        edge: 0,
                        start: 0,
                        end: 0,
                        reversed: Some(false),
                    },
                    1,
                )],
            }],
            missing_edges: Vec::new(),
        },
    )];
    let budget = WorkBudget::new(0);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true],
        edges: &[0],
        constraints: vec![(0, 0)],
        assignment: vec![None],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert!(search.candidate_fits(0, [0, 0]));
    assert!(!budget.exhausted());
    search.adjust(0, [0, 0], true);
    search.assignment[0] = Some([0, 0]);
    assert!(search.ordered_faces_feasible([0]));
    assert!(!budget.exhausted());
}

#[test]
fn incidence_selection_validates_only_its_affected_faces() {
    let choices = vec![vec![[0, 0]], vec![[0, 1]]];
    let edge_faces = [[0, 0], [1, 1]];
    let face_edges = vec![vec![0], vec![1]];
    let assignments = vec![
        MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0]),
        MeshFaceBoundaryDomain::UnorderedFullCycle(vec![1]),
    ];
    let budget = WorkBudget::new(1_000);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true, false],
        edges: &[0],
        constraints: vec![(0, 0)],
        assignment: vec![Some([0, 0]), Some([0, 1])],
        degrees: sparse_degrees(&[&[2, 0], &[1, 1]]),
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashSet::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        outcome: SearchOutcome::Open,
        stopped: false,
    };

    assert!(search.ordered_faces_feasible([0]));
    assert!(!search.ordered_faces_feasible([1]));
}
