// SPDX-License-Identifier: Apache-2.0
//! selection search tests.

use super::{
    possible_face_choices, possible_face_equations, repeated_domain, Arc, EdgeBoundaryLayout,
    EdgeRow, HashSet, MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment, MeshQuotient,
    MeshSelectionSearch, RefCell, SearchOutcome, StandardTopologyDraft, WorkBudget,
};
use crate::solve::mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS;

#[test]
fn remaining_merge_capacity_respects_mutually_exclusive_orientations() {
    catia_test_context!(ctx);
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
                end: 0,
                reversed: None,
            },
        ]],
    };
    let assignments = vec![vec![assignment]];
    let equations = possible_face_equations(&assignments);
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_choices: possible_face_choices(&assignments, &equations),
        possible_face_equations: equations,
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0, 1]), 4));

    assert_eq!(
        search
            .remaining_equation_merge_capacity(&mut quotient)
            .expect("count the possible remaining equation merges"),
        Some(2)
    );
}

#[test]
fn remaining_equations_must_connect_equal_singleton_domains() {
    catia_test_context!(ctx);
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: Some(false),
        }]],
    }]];
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient::new(vec![
        Arc::new(HashSet::from([0])),
        Arc::new(HashSet::from([1])),
        Arc::new(HashSet::from([0])),
        Arc::new(HashSet::from([2])),
    ]);

    assert_eq!(
        search
            .remaining_equation_merge_capacity(&mut quotient)
            .expect("count the possible remaining equation merges"),
        None
    );
}

#[test]
fn remaining_equation_components_require_a_coordinate_matching() {
    catia_test_context!(ctx);
    let assignments = Vec::new();
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work: Vec::new(),
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: Vec::new(),
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient::new(vec![
        Arc::new(HashSet::from([0, 1])),
        Arc::new(HashSet::from([0, 1])),
        Arc::new(HashSet::from([0, 1])),
        Arc::new(HashSet::from([2, 3])),
    ]);

    assert_eq!(
        search
            .remaining_equation_merge_capacity(&mut quotient)
            .expect("count the possible remaining equation merges"),
        None
    );
}

#[test]
fn coordinate_matching_reserves_unavoidable_roots_per_component() {
    catia_test_context!(ctx);
    let assignments = vec![Vec::new()];
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: vec![vec![[0, 1], [1, 2]]],
        possible_face_choices: vec![vec![vec![[0, 1]], vec![[1, 2]]]],
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[[0.0, 0.0, 0.0]; 3],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient::new(vec![
        Arc::new(HashSet::from([0])),
        Arc::new(HashSet::from([0])),
        Arc::new(HashSet::from([0])),
        Arc::new(HashSet::from([1, 2])),
    ]);

    assert_eq!(
        search
            .remaining_equation_merge_capacity(&mut quotient)
            .expect("count the possible remaining equation merges"),
        None
    );
}

#[test]
fn completed_mesh_search_continues_to_check_uniqueness() {
    catia_test_context!(ctx);
    let assignments = Vec::new();
    let edge_candidates = Vec::new();
    let edge_rows = Vec::new();
    let vertex_points = Vec::new();
    let search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work: Vec::new(),
        edge_candidates: &edge_candidates,
        edge_rows: &edge_rows,
        vertex_points: &vertex_points,
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: Vec::new(),
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Solved((
            StandardTopologyDraft {
                faces: Vec::new(),
                edge_rows: Vec::new(),
                vertex_points: Vec::new(),
                logical_vertex_count: 0,
            },
            Vec::new(),
        )),
        face_equation_cache: RefCell::default(),
    };

    assert!(!search.should_stop());
}

#[test]
fn mesh_selection_declines_when_its_work_budget_is_exhausted() {
    catia_test_context!(ctx);
    let mut search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &[],
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work: Vec::new(),
        edge_candidates: &[],
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: Vec::new(),
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient::new(Vec::new());

    search
        .search_with_limit(&quotient, 0)
        .expect("search the mesh selection");

    assert!(matches!(search.outcome, SearchOutcome::Exhausted));
}

#[test]
fn mesh_selection_finishes_the_active_face_component_first() {
    const UNRELATED_FACE_COUNT: usize = 1_000;
    catia_test_context!(ctx);
    let use_edge = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let selected_assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![use_edge(0)]],
    };
    let mut assignments = vec![vec![selected_assignment]];
    assignments.extend((0..UNRELATED_FACE_COUNT).map(|index| {
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_edge(index + 2)]],
        }]
    }));
    assignments.push(vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![use_edge(0), use_edge(1)]],
    }]);
    let face_count = assignments.len();
    let edge_count = UNRELATED_FACE_COUNT + 2;
    let mut selected = vec![None; face_count];
    selected[0] = Some((0, vec![vec![false]]));
    let mut edge_candidates = vec![vec![[0, 0]]; edge_count];
    edge_candidates[1] = vec![[1, 1]];
    let mut domains = Vec::with_capacity(edge_count * 2);
    for candidates in &edge_candidates {
        let domain = Arc::new(candidates.iter().flatten().copied().collect::<HashSet<_>>());
        domains.push(domain.clone());
        domains.push(domain);
    }
    let mut search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: vec![Vec::new(); face_count],
        possible_face_choices: vec![Vec::new(); face_count],
        face_work: vec![Some(1); face_count],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[[0.0; 3], [1.0, 0.0, 0.0]],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected,
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient::new(domains);
    let budget = WorkBudget::new(5);
    let propagation_budget = WorkBudget::new(0);

    search
        .search_from_state(&quotient, true, &budget, &propagation_budget)
        .expect("search from state");

    assert!(!search.outcome.is_closed());
    assert!(matches!(search.outcome, SearchOutcome::Open));
}

#[test]
fn forced_face_selection_does_not_exhaust_the_work_budget() {
    catia_test_context!(ctx);
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: Some(false),
        }]],
    }]];
    let edge_candidates = vec![vec![[0, 0]]];
    let edge_rows = vec![{
        assert!(EdgeRow::new(1, vec![0], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
        EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row")
    }];
    let mut search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &edge_rows,
        vertex_points: &[[0.0, 0.0, 0.0]],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), 2));

    search.search(&quotient).expect("search the mesh selection");

    assert!(!search.outcome.is_closed());
}

#[test]
fn overmerged_face_options_do_not_exhaust_the_work_budget() {
    catia_test_context!(ctx);
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
                end: 0,
                reversed: None,
            },
        ]],
    }]];
    let edge_candidates = vec![Vec::new(); 2];
    let edge_rows = vec![
        {
            assert!(EdgeRow::new(1, vec![0], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        };
        2
    ];
    let mut search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &edge_rows,
        vertex_points: &[[0.0, 0.0, 0.0]; 3],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient::new(repeated_domain(HashSet::from([0, 1, 2]), 4));

    search.search(&quotient).expect("search the mesh selection");

    assert!(!search.outcome.is_closed());
    assert!(matches!(search.outcome, SearchOutcome::Open));
}

#[test]
fn mesh_selection_merges_corner_equations_common_to_every_option() {
    catia_test_context!(ctx);
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: Some(false),
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 2,
                reversed: Some(false),
            },
            MeshBoundaryEdgeCandidate {
                edge: 2,
                start: 2,
                end: 3,
                reversed: Some(false),
            },
        ]],
    };
    let assignments = vec![vec![assignment]];
    let candidates = vec![vec![], vec![], vec![]];
    let search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient =
        MeshQuotient::new((0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect());

    assert!(search
        .propagate_forced_face_equations_from(
            &mut quotient,
            None,
            &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
        .expect("propagate the forced face equations"));
    assert_eq!(
        quotient.find(1).expect("find the component root"),
        quotient.find(2).expect("find the component root")
    );
    assert_eq!(
        quotient.find(3).expect("find the component root"),
        quotient.find(4).expect("find the component root")
    );
    assert_eq!(
        quotient.find(5).expect("find the component root"),
        quotient.find(0).expect("find the component root")
    );
    assert_eq!(quotient.root_count().expect("count the component roots"), 3);
}

#[test]
fn mesh_selection_merges_equations_common_to_every_assignment() {
    catia_test_context!(ctx);
    let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: edge + 1,
        reversed: Some(reversed),
    };
    let assignments = vec![vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0, false), use_(1, false), use_(2, false)]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0, false), use_(1, false), use_(2, true)]],
        },
    ]];
    let candidates = vec![vec![], vec![], vec![]];
    let search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(2)],
        edge_candidates: &candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient =
        MeshQuotient::new((0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect());

    assert!(search
        .propagate_forced_face_equations_from(
            &mut quotient,
            None,
            &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
        .expect("propagate the forced face equations"));
    assert_eq!(
        quotient.find(1).expect("find the component root"),
        quotient.find(2).expect("find the component root")
    );
    assert_eq!(quotient.root_count().expect("count the component roots"), 5);
}

#[test]
fn mesh_selection_common_equations_ignore_infeasible_assignments() {
    catia_test_context!(ctx);
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: edge + 1,
        reversed: Some(false),
    };
    let assignments = vec![vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(2)]],
        },
    ]];
    let candidates = vec![vec![]; 3];
    let search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(2)],
        edge_candidates: &candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient::new(
        [1, 0, 0, 1, 2, 2]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
    );

    assert!(search
        .propagate_forced_face_equations_from(
            &mut quotient,
            None,
            &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
        .expect("propagate the forced face equations"));
    assert_eq!(
        quotient.find(1).expect("find the component root"),
        quotient.find(2).expect("find the component root")
    );
    assert_eq!(
        quotient.find(3).expect("find the component root"),
        quotient.find(0).expect("find the component root")
    );
    assert_eq!(quotient.root_count().expect("count the component roots"), 4);
}

#[test]
fn mesh_selection_propagates_closed_ports_without_enumerating_directions() {
    catia_test_context!(ctx);
    let boundary = (0..13)
        .map(|edge| MeshBoundaryEdgeCandidate {
            edge,
            start: edge,
            end: (edge + 1) % 13,
            reversed: None,
        })
        .collect();
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![boundary],
    }]];
    let candidates = vec![vec![]; 13];
    let search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient::new((0..26).map(|_| Arc::new((0..13).collect())).collect());
    for edge in 0..13 {
        quotient
            .merge(edge * 2, edge * 2 + 1)
            .expect("merge the components")
            .expect("closed port");
    }

    assert_eq!(
        quotient.root_count().expect("count the component roots"),
        13
    );
    assert!(search
        .propagate_forced_face_equations_from(
            &mut quotient,
            None,
            &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
        .expect("propagate the forced face equations"));
    assert_eq!(quotient.root_count().expect("count the component roots"), 1);
}

#[test]
fn single_selected_face_search_reaches_a_solved_outcome() {
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::CodecError;
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: Some(false),
        }]],
    }]];
    let edge_rows = vec![{
        assert!(EdgeRow::new(1, vec![0], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
        EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row")
    }];
    let edge_candidates = vec![vec![[0, 0]]];
    let vertex_points = [[0.0; 3]];
    let domain = Arc::new(HashSet::from([0]));
    let quotient = MeshQuotient::new(vec![domain.clone(), domain]);
    let run = |ctx: &DecodeContext<'_>| -> Result<SearchOutcome<(StandardTopologyDraft, Vec<usize>)>, CodecError> {
        let mut search = MeshSelectionSearch {
            ctx,
            assignments: &assignments,
            possible_face_equations: Vec::new(),
            possible_face_choices: Vec::new(),
            face_work: vec![Some(1)],
            edge_candidates: &edge_candidates,
            edge_rows: &edge_rows,
            vertex_points: &vertex_points,
            candidate_gauge: None,
            port_identities: None,
            fixed_face_directions: Vec::new(),
            fixed_edge_orientations: Vec::new(),
            edge_has_fixed_direction: Vec::new(),
            selected: vec![Some((0, vec![vec![false]]))],
            visited_states: HashSet::new(),
            outcome: SearchOutcome::Open,
            face_equation_cache: RefCell::default(),
        };
        let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
        search.search_state(&quotient, true, &budget, &budget)?;
        Ok(search.outcome)
    };
    catia_test_context!(service_ctx);
    assert!(matches!(
        run(&service_ctx).expect("search state"),
        SearchOutcome::Solved(_)
    ));
}
