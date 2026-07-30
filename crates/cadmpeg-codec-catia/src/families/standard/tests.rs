//! Behavioral tests for standard B-rep topology solvers and parsers.

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use cadmpeg_ir::topology::BodyKind;

use crate::families::standard::fbb::{
    parse_edge_tables_at, parse_edge_tables_scoped_at, parse_fbb_edge_tables_width,
    parse_trim_chain, parse_trim_record, parse_trim_record_layout, parse_vertex_table,
    prune_edge_candidates_by_port_domains, standard_face_count, EDGE_DELIMITER,
};
use crate::families::standard::topology::{
    complete_duplicate_face_slots, reconstruct_incidence, solve_boundary_orientation_constraints,
    Boundary, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopology, StandardTopology, TrimRecord,
};
use crate::solve::incidence::{
    compact_boundary_domain_viable, prune_face_configuration_support,
    reconstruct_incidence_candidates,
};
use crate::solve::matching::unique_coordinate_bijection;
use crate::solve::mesh_quotient::{
    canonical_edge_class_assignment, canonicalize_mesh_vertex_labels,
    deduplicate_mesh_quotient_assignments, initial_mesh_quotient, mesh_assignment_can_merge,
    mesh_assignment_endpoint_cycles_viable, mesh_candidates_equivalent,
    mesh_candidates_equivalent_with_edge_classes, mesh_edge_points_compatible,
    mesh_face_endpoint_configurations, possible_face_choices, possible_face_choices_with_limit,
    possible_face_equations, prune_mesh_endpoint_pair_support,
    prune_mesh_endpoint_pair_support_with_limit, MeshConstraintBudget,
    MeshPartialEndpointConstraint, MeshQuotient, MeshSelectionSearch,
    MAX_FACE_EQUATION_CACHE_ENTRIES, MAX_MESH_CONSTRAINT_OPERATIONS,
};
use crate::solve::missing_edge::{
    bind_edge_port_candidates, bounded_endpoint_cycle_orders, bounded_oriented_trail_orders,
    face_endpoint_candidates_close, motif_port_points, propagate_edge_port_points,
    propagate_partial_edge_port_points, resolve_edge_faces_from_runs, same_unordered_pair,
    unique_duplicate_face_assignment, MeshBoundaryEdgeCandidate, MeshEdgeRun,
    MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
};
use crate::solve::UnionFind;

fn repeated_domain(domain: HashSet<usize>, count: usize) -> Vec<Arc<HashSet<usize>>> {
    let domain = Arc::new(domain);
    vec![domain; count]
}

fn sparse_degrees(faces: &[&[u8]]) -> Vec<BTreeMap<usize, u8>> {
    faces
        .iter()
        .map(|degrees| {
            degrees
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(point, degree)| (degree != 0).then_some((point, degree)))
                .collect()
        })
        .collect()
}

fn triangle_packet(handles: [u16; 3]) -> Vec<u8> {
    let mut bytes = vec![0x01, 0x41, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00];
    for handle in handles {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes
}

#[test]
fn trim_chain_requires_exact_packet_count_and_boundary_landing() {
    let incidental = triangle_packet([90, 91, 92]);
    let first = triangle_packet([0, 1, 2]);
    let second = triangle_packet([3, 4, 5]);
    let mut bytes = incidental;
    bytes.push(0);
    bytes.extend_from_slice(&first);
    bytes.extend_from_slice(&second);

    let records = parse_trim_chain(&bytes, bytes.len(), 2, 2).expect("exact chain");
    assert_eq!(records[0].handles, [0, 1, 2]);
    assert_eq!(records[1].handles, [3, 4, 5]);
    assert_eq!(records[0].independent_count, 1);
    assert!(records[0].strip_lengths.is_empty());
    assert!(records[0].fan_lengths.is_empty());
    assert!(parse_trim_chain(&bytes, bytes.len(), 2, 3).is_none());
}

#[test]
fn endpoint_trail_ordering_stops_when_its_result_limit_is_exceeded() {
    let trails = (0..10).map(|edge| vec![edge]).collect::<Vec<_>>();
    assert!(bounded_oriented_trail_orders(&trails, 16).is_none());
    assert_eq!(
        bounded_oriented_trail_orders(&[vec![0], vec![1]], 2),
        Some(vec![vec![0, 1], vec![1, 0]])
    );
}

#[test]
fn endpoint_cycle_ordering_quotients_rotation_and_reversal() {
    let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];
    assert_eq!(
        bounded_endpoint_cycle_orders(&[2, 0, 1], &candidates, 4),
        Some(vec![vec![0, 1, 2]])
    );
}

#[test]
fn endpoint_cycle_ordering_stops_at_its_result_limit() {
    let candidates = vec![vec![[0, 0]]; 8];
    assert!(bounded_endpoint_cycle_orders(&(0..8).collect::<Vec<_>>(), &candidates, 16).is_none());
}

#[test]
fn trim_record_layout_indexes_extent_without_materializing_triangles() {
    let bytes = triangle_packet([10, 11, 12]);
    let layout = parse_trim_record_layout(&bytes, 0, 2).expect("trim packet layout");
    assert_eq!(layout.handle_offset, 8);
    assert_eq!(layout.stored_count, 3);
    assert_eq!(layout.end, bytes.len());

    let record = parse_trim_record(&bytes, 0, 2).expect("materialized trim packet");
    assert_eq!(record.triangles, [[10, 11, 12]]);
}

#[test]
fn forced_trim_chain_has_no_recursive_depth_limit() {
    const RECORD_COUNT: usize = 10_000;
    let packet = triangle_packet([0, 0, 0]);
    let bytes = packet.repeat(RECORD_COUNT);

    let records =
        parse_trim_chain(&bytes, bytes.len(), RECORD_COUNT, 2).expect("forced trim packet chain");

    assert_eq!(records.len(), RECORD_COUNT);
    assert!(records.iter().all(|record| record.handles == [0, 0, 0]));
}

#[test]
fn trim_packet_retains_primitive_partition_lengths() {
    let mut bytes = vec![
        0x01, 0x47, 0x01, 0x01, 0x01, 0xff, 0x0a, 0x00, 0x00, 0x00, 0x03, 0x04,
    ];
    for handle in 0u16..10 {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    let [record] = parse_trim_chain(&bytes, bytes.len(), 1, 2)
        .expect("mixed primitive packet")
        .try_into()
        .expect("one packet");
    assert_eq!(record.independent_count, 1);
    assert_eq!(record.strip_lengths, [3]);
    assert_eq!(record.fan_lengths, [4]);
}

#[test]
fn standard_edge_row_arity_uses_widened_count_form() {
    let mut bytes = Vec::new();
    for (kind, handles) in [(1, [10u16, 11]), (2, [20, 21])] {
        bytes.extend_from_slice(&[0x01, kind, 1, 0x02, 0xff]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for handle in handles {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        bytes.extend_from_slice(&EDGE_DELIMITER);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 0]);

    let (rows, vertex_header) = parse_edge_tables_at(&bytes, 0).expect("widened row arity");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].handles, vec![10, 11]);
    assert_eq!(rows[1].handles, vec![20, 21]);
    assert_eq!(vertex_header, bytes.len() - 3);
}

#[test]
fn coordinate_rows_canonicalize_logical_vertex_labels() {
    let topology = |start_vertex, end_vertex| StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary {
                coedges: vec![CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex,
                    end_vertex,
                }],
            }],
        }],
        edge_rows: vec![EdgeRow {
            kind: 1,
            handles: vec![0, 1],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };

    let left_candidate = (topology(0, 1), vec![1, 0]);
    let right_candidate = (topology(1, 0), vec![0, 1]);
    assert_ne!(left_candidate, right_candidate);
    assert!(mesh_candidates_equivalent(
        &left_candidate,
        &right_candidate
    ));
    let left = canonicalize_mesh_vertex_labels(left_candidate.0, &left_candidate.1);
    let right = canonicalize_mesh_vertex_labels(right_candidate.0, &right_candidate.1);

    assert_eq!(left, right);
    assert_eq!(left.expect("canonical topology").1, vec![0, 1]);

    let forward = canonicalize_mesh_vertex_labels(topology(0, 1), &[0, 1]);
    let mut reversed = topology(0, 1);
    reversed.faces[0].boundaries[0].coedges[0].reversed = true;
    let reversed = canonicalize_mesh_vertex_labels(reversed, &[0, 1]);
    assert_eq!(forward, reversed);
}

#[test]
fn mesh_candidate_comparison_ignores_boundary_cycle_start() {
    let mut topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary {
                coedges: vec![
                    CoedgeUse {
                        edge_row: 0,
                        reversed: false,
                        start_vertex: 0,
                        end_vertex: 1,
                    },
                    CoedgeUse {
                        edge_row: 1,
                        reversed: false,
                        start_vertex: 1,
                        end_vertex: 0,
                    },
                ],
            }],
        }],
        edge_rows: vec![
            EdgeRow {
                kind: 1,
                handles: vec![0, 1],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
            EdgeRow {
                kind: 1,
                handles: vec![1, 0],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
        ],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };
    let left = (topology.clone(), vec![0, 1]);
    topology.faces[0].boundaries[0].coedges.rotate_left(1);
    let right = (topology, vec![0, 1]);

    assert_ne!(left, right);
    assert!(mesh_candidates_equivalent(&left, &right));
}

#[test]
fn mesh_candidate_comparison_ignores_boundary_direction_and_order() {
    let boundary = |edges: &[(usize, usize, usize)]| Boundary {
        coedges: edges
            .iter()
            .map(|&(edge_row, start_vertex, end_vertex)| CoedgeUse {
                edge_row,
                reversed: false,
                start_vertex,
                end_vertex,
            })
            .collect(),
    };
    let edge_rows = (0..4)
        .map(|edge| EdgeRow {
            kind: 1,
            handles: vec![edge, edge + 1],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let left_topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![
                boundary(&[(0, 0, 1), (1, 1, 0)]),
                boundary(&[(2, 2, 3), (3, 3, 2)]),
            ],
        }],
        edge_rows,
        vertex_points: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
        ],
        logical_vertex_count: 4,
    };
    let mut right_topology = left_topology.clone();
    right_topology.faces[0].boundaries.reverse();
    for boundary in &mut right_topology.faces[0].boundaries {
        boundary.coedges.reverse();
        for coedge in &mut boundary.coedges {
            coedge.reversed = !coedge.reversed;
            std::mem::swap(&mut coedge.start_vertex, &mut coedge.end_vertex);
        }
    }
    let left = (left_topology, vec![0, 1, 2, 3]);
    let right = (right_topology, vec![0, 1, 2, 3]);

    assert_ne!(left, right);
    assert!(mesh_candidates_equivalent(&left, &right));
}

#[test]
fn mesh_candidate_comparison_canonicalizes_equivalent_edge_allocations() {
    let edge_rows = vec![
        EdgeRow {
            kind: 2,
            handles: vec![10, 11],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        },
        EdgeRow {
            kind: 2,
            handles: vec![10, 12],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        },
    ];
    let topology = |swapped: bool| StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![
                Boundary {
                    coedges: vec![CoedgeUse {
                        edge_row: usize::from(swapped),
                        reversed: false,
                        start_vertex: 0,
                        end_vertex: 1,
                    }],
                },
                Boundary {
                    coedges: vec![CoedgeUse {
                        edge_row: usize::from(!swapped),
                        reversed: false,
                        start_vertex: 1,
                        end_vertex: 2,
                    }],
                },
            ],
        }],
        edge_rows: edge_rows.clone(),
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        logical_vertex_count: 3,
    };
    let left = (topology(false), vec![0, 1, 2]);
    let right = (topology(true), vec![0, 1, 2]);

    assert!(mesh_candidates_equivalent_with_edge_classes(
        &left,
        &right,
        &[7, 7]
    ));
    assert!(!mesh_candidates_equivalent_with_edge_classes(
        &left,
        &right,
        &[7, 8]
    ));
    assert!(!mesh_candidates_equivalent_with_edge_classes(
        &left,
        &right,
        &[7]
    ));
}

#[test]
fn mesh_candidate_comparison_rejects_two_invalid_candidates() {
    let invalid = (
        StandardTopology {
            faces: Vec::new(),
            edge_rows: Vec::new(),
            vertex_points: Vec::new(),
            logical_vertex_count: 0,
        },
        vec![0],
    );

    assert!(!mesh_candidates_equivalent(&invalid, &invalid));
    assert!(!mesh_candidates_equivalent_with_edge_classes(
        &invalid,
        &invalid,
        &[]
    ));
}

#[test]
fn equivalent_edge_classes_require_monotone_endpoint_assignments() {
    let classes = [4, 4, 9];
    let choices = vec![
        vec![[0, 2], [0, 1]],
        vec![[0, 1], [0, 2]],
        vec![[3, 4], [3, 5]],
    ];

    assert_eq!(
        canonical_edge_class_assignment(&classes, &choices, &[Some([2, 0]), Some([0, 2]), None]),
        Some(vec![true, true, false])
    );
    assert_eq!(
        canonical_edge_class_assignment(&classes, &choices, &[Some([0, 2]), Some([1, 0]), None]),
        None
    );
    assert!(
        canonical_edge_class_assignment(&classes, &choices, &[None, Some([0, 1]), None]).is_some()
    );
}

#[test]
fn standard_face_population_ignores_shorter_fbb_marker_runs() {
    let row = [0x30, 0x04, 0x04, 0xff, 0xff, 0xff, 0xd2, 0xd2];
    let mut bytes = row.to_vec();
    bytes.push(0);
    bytes.extend_from_slice(&row);
    bytes.extend_from_slice(&row);
    bytes.extend_from_slice(&row);

    assert_eq!(standard_face_count(&bytes), Some(3));
}

#[test]
fn standard_face_population_rejects_equal_largest_fbb_runs() {
    let row = [0x30, 0x04, 0x04, 0xff, 0xff, 0xff, 0xd2, 0xd2];
    let mut bytes = row.repeat(2);
    bytes.push(0);
    bytes.extend_from_slice(&row.repeat(2));

    assert_eq!(standard_face_count(&bytes), None);
}

fn trim(kind: u8, handles: [u32; 4]) -> TrimRecord {
    TrimRecord {
        triangles: Vec::new(),
        frame_vector: None,
        handles: handles.to_vec(),
        independent_count: 0,
        strip_lengths: vec![handles.len()],
        fan_lengths: Vec::new(),
        kind,
    }
}

#[test]
fn allocation_program_replays_seed_tooth_and_transition() {
    let trims = [
        trim(0x4a, [0, 1, 2, 3]),
        trim(0x4a, [10, 11, 12, 13]),
        trim(0x4a, [20, 21, 22, 23]),
        trim(0x42, [30, 31, 32, 33]),
        trim(0x4a, [40, 41, 30, 31]),
        trim(0x42, [50, 51, 40, 41]),
        trim(0x4a, [60, 61, 62, 63]),
    ];
    let points = motif_port_points(&trims, 20).expect("complete motif allocation");
    let order = [
        20, 21, 2, 3, 0, 1, 22, 23, 32, 33, 30, 31, 40, 41, 50, 51, 60, 61, 62, 63,
    ];
    for (index, handle) in order.into_iter().enumerate() {
        assert_eq!(points[&handle], index);
    }
}

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
    let topology =
        reconstruct_incidence_candidates(&rows, &points, &edge_faces, &candidates, None, 4)
            .expect("unique face-closing endpoint assignment");
    assert_eq!(topology.edge_vertices().expect("edge vertices")[0], [0, 1]);

    let ports = [[11, 10], [11, 12], [10, 12], [13, 10], [11, 13], [13, 12]];
    let topology =
        reconstruct_incidence_candidates(&rows, &points, &edge_faces, &candidates, Some(&ports), 4)
            .expect("unique face-closing assignment with deferred port orientation");
    assert_eq!(topology.edge_vertices().expect("edge vertices")[0], [1, 0]);
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
    let budget = MeshConstraintBudget::new(2);
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
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        mesh_quotient: None,
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    assert!(!search.candidate_fits(0, [0, 1]));
    assert!(search.candidate_fits(0, [0, 2]));
}

#[test]
fn incidence_component_requires_degree_support_to_fit_every_incident_face() {
    let choices = vec![vec![[0, 1]], vec![[1, 2]]];
    let edge_faces = [[0, 0], [0, 1]];
    let face_edges = vec![vec![0, 1], vec![1]];
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        mesh_quotient: None,
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    assert!(!search.candidate_fits(0, [0, 1]));
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
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        mesh_quotient: None,
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    search.search();

    assert!(!search.exhausted);
    assert_eq!(search.solutions.len(), 1);
}

#[test]
fn incidence_component_schedules_partial_constraint_variables_first() {
    let choices = vec![vec![[0, 1], [0, 2]], vec![[3, 4], [3, 5], [4, 5]]];
    let edge_faces = [[0, 0], [0, 0]];
    let face_edges = vec![vec![0, 1]];
    let edges = [0, 1];
    let active_edges = [false, true];
    let valid = |_: &[Option<[usize; 2]>]| true;
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        mesh_quotient: None,
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
            assignment_predecessors: None,
            valid: &valid,
        }),
        dead_states: HashSet::new(),
        budget: &budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
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
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        mesh_quotient: None,
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
            assignment_predecessors: Some(&assignment_predecessors),
            valid: &valid,
        }),
        dead_states: HashSet::new(),
        budget: &budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
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
    assert_eq!(
        independent.branch_options(None),
        Some(vec![(2, [3, 4]), (2, [3, 5])])
    );
}

#[test]
fn incidence_component_declines_when_its_work_budget_is_exhausted() {
    let choices = vec![vec![[0, 0]]];
    let edge_faces = [[0, 0]];
    let face_edges = vec![vec![0]];
    let edges = [0];
    let budget = MeshConstraintBudget::new(0);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: None,
        mesh_quotient: None,
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    search.search();

    assert!(search.exhausted);
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
    let budget = MeshConstraintBudget::new(0);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        mesh_quotient: None,
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    assert_eq!(search.face_configuration_options(), None);
    assert!(!budget.exhausted.get());
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
    let search_budget = MeshConstraintBudget::new(16);
    let propagation_budget = MeshConstraintBudget::new(0);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        mesh_quotient: None,
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    search.search();

    assert!(!search.exhausted);
    assert_eq!(search.solutions, vec![vec![(0, [0, 0])]]);
    assert!(propagation_budget.exhausted.get());
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
    let budget = MeshConstraintBudget::new(4);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        mesh_quotient: None,
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    assert_eq!(
        search.face_configuration_options(),
        Some(vec![vec![(0, [0, 0])]])
    );
    assert!(!budget.exhausted.get());
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
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        mesh_quotient: None,
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    assert_eq!(
        search.face_configuration_options(),
        Some(vec![vec![(1, [10, 11]), (2, [10, 11])]])
    );
}

#[test]
fn incidence_face_configuration_support_retains_shared_edge_correlations() {
    let correlated = vec![
        vec![(0, [0, 1]), (1, [2, 3])],
        vec![(0, [4, 5]), (1, [6, 7])],
    ];
    let mut domains = vec![correlated.clone(), vec![vec![(0, [0, 1]), (1, [2, 3])]]];
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(prune_face_configuration_support(&mut domains, &budget));
    assert_eq!(domains[0], vec![correlated[0].clone()]);

    let mut incompatible = vec![correlated, vec![vec![(0, [0, 1]), (1, [6, 7])]]];
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(!prune_face_configuration_support(
        &mut incompatible,
        &budget
    ));

    let preserved = incompatible.clone();
    let budget = MeshConstraintBudget::new(0);
    assert!(prune_face_configuration_support(&mut incompatible, &budget));
    assert_eq!(incompatible, preserved);
    assert!(budget.exhausted.get());
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
    let budget = MeshConstraintBudget::new(1);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        mesh_quotient: None,
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    search.search();

    assert!(!search.exhausted);
    assert_eq!(search.solutions, vec![vec![(0, [0, 0]), (1, [1, 1])]]);
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
    let quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: vec![vec![0], vec![1]],
    };
    let budget = MeshConstraintBudget::new(0);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        mesh_quotient: Some(&quotient),
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    assert!(search.candidate_fits(0, [0, 0]));
    assert!(!budget.exhausted.get());
    search.adjust(0, [0, 0], true);
    search.assignment[0] = Some([0, 0]);
    assert!(search.ordered_faces_feasible([0]));
    assert!(!budget.exhausted.get());
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
    let quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([0, 1])),
        ],
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let budget = MeshConstraintBudget::new(1_000);
    let propagation_budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let search = crate::solve::incidence::IncidenceComponentSearch {
        choices: &choices,
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        mesh_quotient: Some(&quotient),
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
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        exhausted: false,
        stopped: false,
    };

    assert!(search.ordered_faces_feasible([0]));
    assert!(!search.ordered_faces_feasible([1]));
}

#[test]
fn incidence_components_join_only_through_shared_face_vertices() {
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[1, 3], [2, 3]],
        vec![[4, 5], [4, 6]],
        vec![[7, 8]],
    ];
    let edge_faces = [[0, 0], [0, 0], [0, 0], [0, 0]];
    assert_eq!(
        crate::solve::incidence::incidence_choice_components(&choices, &edge_faces, None, None),
        vec![vec![0, 1], vec![2]]
    );
}

#[test]
fn incidence_component_preflight_ignores_disjoint_unassigned_cycles_on_the_same_face() {
    let choices = vec![vec![[0, 0], [1, 1]], vec![[2, 2], [3, 3]]];
    let edge_faces = [[0, 0], [0, 0]];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        1,
        4,
        None,
        None,
        None,
        &|_| true,
    )
    .expect("independent same-face cycle solutions");

    assert_eq!(solutions.len(), 4);
}

#[test]
fn incidence_component_composition_does_not_allocate_the_declared_point_product() {
    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &[vec![[usize::MAX - 1, usize::MAX - 1]]],
        &[[0, 0]],
        1,
        usize::MAX,
        None,
        None,
        None,
        &|_| true,
    )
    .expect("sparse component degree state");

    assert_eq!(solutions, vec![vec![[usize::MAX - 1, usize::MAX - 1]]]);
}

#[test]
fn incidence_component_preflight_retains_fixed_chain_frontiers() {
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[1, 3]],
        vec![[3, 4], [3, 5]],
        vec![[4, 0]],
    ];
    let edge_faces = [[0, 0]; 4];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        1,
        6,
        None,
        None,
        None,
        &|_| true,
    )
    .expect("fixed-chain component frontier");

    assert_eq!(solutions, vec![vec![[0, 1], [1, 3], [3, 4], [4, 0]]]);
}

#[test]
fn partial_incidence_constraint_joins_every_component_it_can_couple() {
    let components = vec![vec![0, 2], vec![1], vec![3, 5], vec![4]];
    let active = [true, false, false, true, false, false];

    assert_eq!(
        crate::solve::incidence::join_partial_constraint_components(components, &active, None),
        vec![vec![0, 2, 3, 5], vec![1], vec![4]],
    );
}

#[test]
fn partial_incidence_predecessors_join_only_their_class_components() {
    let components = vec![vec![0, 2], vec![1], vec![3, 5], vec![4]];
    let predecessors = [None, None, None, Some(0), None, None];

    assert_eq!(
        crate::solve::incidence::join_partial_constraint_components(
            components,
            &[false; 6],
            Some(&predecessors),
        ),
        vec![vec![0, 2, 3, 5], vec![1], vec![4]],
    );
}

#[test]
fn incidence_components_order_by_endpoint_branch_width() {
    let choices = vec![
        vec![[0, 0], [1, 1]],
        vec![[2, 2], [3, 3], [4, 4]],
        vec![[5, 5], [6, 6]],
        vec![[7, 7]],
    ];
    let mut components = vec![vec![0, 2], vec![1], vec![3]];

    crate::solve::incidence::order_incidence_components_by_branch_width(&mut components, &choices)
        .expect("valid component edges");

    assert_eq!(components, vec![vec![3], vec![1], vec![0, 2]]);
}

#[test]
fn incidence_components_keep_fixed_face_boundaries_independent() {
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[1, 2], [1, 3]],
        vec![[4, 5], [4, 6]],
        vec![[5, 6], [5, 7]],
    ];
    let edge_faces = [[0, 0]; 4];
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let fixed = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)], vec![use_(2), use_(3)]],
        },
    ])];
    assert_eq!(
        crate::solve::incidence::incidence_choice_components(
            &choices,
            &edge_faces,
            Some(&fixed),
            None,
        ),
        vec![vec![0, 1], vec![2, 3]]
    );

    let alternatives = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)], vec![use_(2), use_(3)]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(2)], vec![use_(1), use_(3)]],
        },
    ])];
    assert_eq!(
        crate::solve::incidence::incidence_choice_components(
            &choices,
            &edge_faces,
            Some(&alternatives),
            None,
        ),
        vec![vec![0, 1, 2, 3]]
    );
}

#[test]
fn incidence_components_include_overlapping_quotient_domains() {
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[3, 4], [3, 5]],
        vec![[6, 7], [6, 8]],
    ];
    let edge_faces = [[0, 0], [1, 1], [2, 2]];
    let quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: [
            HashSet::from([0, 1]),
            HashSet::from([0, 2]),
            HashSet::from([1, 3]),
            HashSet::from([3, 4]),
            HashSet::from([6, 7]),
            HashSet::from([6, 8]),
        ]
        .map(Arc::new)
        .to_vec(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        crate::solve::incidence::incidence_choice_components(
            &choices,
            &edge_faces,
            None,
            Some(&quotient)
        ),
        vec![vec![0, 1], vec![2]]
    );
}

#[test]
fn incidence_components_solve_coupled_face_vertex_closures() {
    let a = vec![[0, 2], [0, 12], [2, 12]];
    let b = vec![[1, 3], [1, 1969], [3, 1969]];
    let c = vec![
        [0, 1],
        [0, 2],
        [0, 3],
        [0, 12],
        [0, 1969],
        [1, 2],
        [1, 3],
        [1, 12],
        [1, 1969],
        [2, 3],
        [2, 12],
        [2, 1969],
        [3, 12],
        [3, 1969],
        [12, 1969],
    ];
    let choices = vec![
        a.clone(),
        b.clone(),
        a,
        b,
        c,
        vec![[2, 3]],
        vec![[2, 12]],
        vec![[12, 1969]],
        vec![[3, 1969]],
    ];
    let edge_faces = [
        [1, 0],
        [3, 0],
        [2, 1],
        [2, 3],
        [2, 0],
        [0, 0],
        [1, 1],
        [2, 2],
        [3, 3],
    ];
    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        4,
        1970,
        None,
        None,
        None,
        &|_| true,
    )
    .expect("component closure solution");
    assert!(solutions
        .iter()
        .any(|solution| { solution[..5] == [[0, 2], [1, 3], [0, 12], [1, 1969], [0, 1]] }));
}

#[test]
fn incidence_components_reject_degree_cycles_in_the_wrong_edge_order() {
    let choices = vec![
        vec![[0, 1]],
        vec![[1, 2], [2, 3]],
        vec![[2, 3], [1, 2]],
        vec![[3, 0]],
    ];
    let edge_faces = [[0, 0]; 4];
    let mesh_assignments = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![(0..4)
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: None,
                })
                .collect()],
        },
    ])];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        1,
        4,
        Some(&mesh_assignments),
        None,
        None,
        &|_| true,
    )
    .expect("ordered component solution");

    assert_eq!(solutions.len(), 1);
    assert_eq!(solutions[0], [[0, 1], [1, 2], [2, 3], [3, 0]]);
}

#[test]
fn incidence_unordered_full_cycle_rejects_disconnected_degree_cycles() {
    let choices = vec![
        vec![[0, 1]],
        vec![[0, 1], [1, 2]],
        vec![[2, 3]],
        vec![[2, 3], [0, 3]],
    ];
    let edge_faces = [[0, 0]; 4];
    let domains = vec![MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2, 3])];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        1,
        4,
        Some(&domains),
        None,
        None,
        &|_| true,
    )
    .expect("connected cycle solution");

    assert_eq!(solutions, vec![vec![[0, 1], [1, 2], [2, 3], [0, 3]]]);
}

#[test]
fn deferred_boundary_enforces_anchored_gap_capacities() {
    let domain = crate::solve::missing_edge::MeshDeferredFaceBoundary {
        cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
            length: 6,
            exact_uses: vec![
                (
                    MeshBoundaryEdgeCandidate {
                        edge: 0,
                        start: 0,
                        end: 1,
                        reversed: Some(false),
                    },
                    1,
                ),
                (
                    MeshBoundaryEdgeCandidate {
                        edge: 3,
                        start: 3,
                        end: 4,
                        reversed: Some(false),
                    },
                    1,
                ),
            ],
        }],
        missing_edges: vec![1, 2, 4, 5],
    };
    let valid = [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5], [0, 5]];
    let overfilled_first_gap = [[0, 1], [1, 2], [2, 3], [4, 5], [3, 4], [0, 5]];

    assert!(crate::solve::incidence::deferred_boundary_closes(
        &domain, &valid
    ));
    let assignment = crate::solve::incidence::deferred_boundary_assignment(&domain, &valid)
        .expect("materialized deferred boundary");
    assert_eq!(
        assignment.boundaries[0]
            .iter()
            .map(|use_| (use_.edge, use_.reversed))
            .collect::<Vec<_>>(),
        vec![
            (0, Some(false)),
            (1, None),
            (2, None),
            (3, Some(false)),
            (4, None),
            (5, None),
        ]
    );
    assert!(!crate::solve::incidence::deferred_boundary_closes(
        &domain,
        &overfilled_first_gap
    ));
}

#[test]
fn deferred_anchored_runs_propagate_forced_adjacencies() {
    let use_ = |edge, start| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end: (start + 1) % 2,
        reversed: Some(false),
    };
    let domains = [MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 2,
                exact_uses: vec![(use_(0, 0), 1), (use_(1, 1), 1)],
            }],
            missing_edges: Vec::new(),
        },
    )];
    let candidates = vec![vec![[0, 1]], vec![[0, 1]]];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 2, &[[0, 1], [2, 3]])
            .expect("initial quotient");
    let budget = crate::solve::mesh_quotient::MeshConstraintBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("forced deferred quotient");

    assert_eq!(quotient.union.find(0), quotient.union.find(3));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
}

#[test]
fn deferred_gap_search_propagates_quotient_forced_edge_order() {
    let use_ = |edge, start| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end: (start + 1) % 4,
        reversed: Some(false),
    };
    let domains = [MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 4,
                exact_uses: vec![(use_(0, 0), 1), (use_(1, 2), 1)],
            }],
            missing_edges: vec![2, 3],
        },
    )];
    let candidates = vec![vec![[0, 1]], vec![[2, 3]], vec![[1, 2]], vec![[0, 3]]];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(8),
        domains: (0..8)
            .map(|node| Arc::new(HashSet::from([[0, 1, 2, 3, 1, 2, 3, 0][node]])))
            .collect(),
        members: (0..8).map(|node| vec![node]).collect(),
    };
    let budget = crate::solve::mesh_quotient::MeshConstraintBudget::new(10_000);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("deferred gap quotient");

    assert_eq!(quotient.union.find(1), quotient.union.find(4));
    assert_eq!(quotient.union.find(5), quotient.union.find(2));
    assert_eq!(quotient.union.find(3), quotient.union.find(6));
    assert_eq!(quotient.union.find(7), quotient.union.find(0));
}

#[test]
fn ordered_structural_equations_propagate_without_direction_enumeration() {
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
            ]],
        },
    ])];
    let candidates = vec![vec![[0, 0]], vec![[0, 0]]];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    quotient.merge(0, 1).expect("first closed edge");
    quotient.merge(2, 3).expect("second closed edge");
    let budget = crate::solve::mesh_quotient::MeshConstraintBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("structural quotient");

    assert_eq!(quotient.union.find(0), quotient.union.find(2));
}

#[test]
fn ordered_face_options_preflight_exact_signature_work() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0)]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1)]],
        },
    ])];
    let candidates = vec![Vec::new(), Vec::new()];
    let broad = Arc::new((0..100).collect::<HashSet<_>>());
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![broad; 4],
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let budget = crate::solve::mesh_quotient::MeshConstraintBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("bounded common quotient propagation");

    assert_eq!(quotient.root_count(), 4);
    assert!(!budget.exhausted.get());
}

#[test]
fn ordered_cycle_support_propagates_domain_forced_directions() {
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
            ]],
        },
    ])];
    let candidates = vec![vec![[0, 1]], vec![[0, 1]]];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([1])),
            Arc::new(HashSet::from([1])),
            Arc::new(HashSet::from([0])),
        ],
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let budget = crate::solve::mesh_quotient::MeshConstraintBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("supported cycle quotient");

    assert_eq!(quotient.union.find(0), quotient.union.find(3));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
}

#[test]
fn ordered_components_retain_unknown_edges_in_the_abstract_quotient() {
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 0,
                    reversed: Some(false),
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 0,
                    reversed: Some(false),
                },
            ]],
        },
    ])];
    let candidates = vec![Vec::new(), Vec::new()];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0, 1]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    crate::solve::mesh_quotient::propagate_common_boundary_components(
        &domains,
        &candidates,
        &mut quotient,
    )
    .expect("ordered component quotient");

    assert_eq!(quotient.union.find(0), quotient.union.find(3));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
}

#[test]
fn unordered_components_close_cycles_in_the_abstract_quotient() {
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![2, 0, 1])];
    let candidates = vec![Vec::new(); 3];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: [0, 1, 1, 2, 2, 0]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    crate::solve::mesh_quotient::propagate_common_boundary_components(
        &domains,
        &candidates,
        &mut quotient,
    )
    .expect("unordered component quotient");

    assert_eq!(quotient.union.find(1), quotient.union.find(2));
    assert_eq!(quotient.union.find(3), quotient.union.find(4));
    assert_eq!(quotient.union.find(5), quotient.union.find(0));
}

#[test]
fn compact_unordered_boundary_rejects_partial_subtours() {
    let domain = MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2, 3, 4]);

    assert!(!compact_boundary_domain_viable(
        &domain,
        &[Some([0, 1]), Some([1, 2]), Some([2, 0]), None, None],
        None,
    ));
    assert!(compact_boundary_domain_viable(
        &domain,
        &[Some([0, 1]), Some([1, 2]), Some([2, 3]), None, None],
        None,
    ));
    assert!(compact_boundary_domain_viable(
        &domain,
        &[
            Some([0, 1]),
            Some([1, 2]),
            Some([2, 3]),
            Some([3, 4]),
            Some([4, 0]),
        ],
        None,
    ));
}

#[test]
fn unordered_component_enumeration_is_atomic_at_its_state_limit() {
    let quotient = MeshQuotient {
        union: UnionFind::new(16),
        domains: repeated_domain(HashSet::from([0]), 16),
        members: (0..16).map(|node| vec![node]).collect(),
    };
    let budget = crate::solve::mesh_quotient::MeshConstraintBudget::new(10_000);

    assert!(
        crate::solve::mesh_quotient::bounded_unordered_cycle_assignments(
            &(0..8).collect::<Vec<_>>(),
            &quotient,
            16,
            &budget,
        )
        .is_none()
    );
}

#[test]
fn deferred_components_select_gap_orders_in_the_abstract_quotient() {
    let use_ = |edge, start| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end: (start + 1) % 4,
        reversed: Some(false),
    };
    let domains = [MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 4,
                exact_uses: vec![(use_(0, 0), 1), (use_(1, 2), 1)],
            }],
            missing_edges: vec![2, 3],
        },
    )];
    let candidates = vec![Vec::new(); 4];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(8),
        domains: (0..8)
            .map(|node| Arc::new(HashSet::from([[0, 1, 2, 3, 1, 2, 3, 0][node]])))
            .collect(),
        members: (0..8).map(|node| vec![node]).collect(),
    };

    crate::solve::mesh_quotient::propagate_common_boundary_components(
        &domains,
        &candidates,
        &mut quotient,
    )
    .expect("deferred component quotient");

    assert_eq!(quotient.union.find(1), quotient.union.find(4));
    assert_eq!(quotient.union.find(5), quotient.union.find(2));
    assert_eq!(quotient.union.find(3), quotient.union.find(6));
    assert_eq!(quotient.union.find(7), quotient.union.find(0));
}

#[test]
fn deferred_faces_share_one_endpoint_quotient() {
    let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: (edge + 1) % 2,
        reversed: Some(reversed),
    };
    let domain = |second_reversed| {
        MeshFaceBoundaryDomain::DeferredValidation(
            crate::solve::missing_edge::MeshDeferredFaceBoundary {
                cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                    length: 2,
                    exact_uses: vec![(use_(0, false), 1), (use_(1, second_reversed), 1)],
                }],
                missing_edges: Vec::new(),
            },
        )
    };
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    let quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&choices, 2, &[[0, 1], [2, 3]])
            .expect("initial quotient");
    let budget = crate::solve::mesh_quotient::MeshConstraintBudget::new(10_000);

    assert!(
        crate::solve::incidence::compact_boundary_domains_jointly_viable(
            &[domain(false), domain(false)],
            &choices,
            &[Some([0, 1]), Some([0, 1])],
            None,
            &quotient,
            &budget,
        )
    );
    assert!(
        !crate::solve::incidence::compact_boundary_domains_jointly_viable(
            &[domain(false), domain(true)],
            &choices,
            &[Some([0, 1]), Some([0, 1])],
            None,
            &quotient,
            &budget,
        )
    );
}

#[test]
fn compact_faces_share_one_physical_edge_direction_gauge() {
    let choices = vec![
        vec![[0, 1]],
        vec![[1, 2]],
        vec![[0, 2]],
        vec![[0, 3]],
        vec![[1, 3]],
    ];
    let assignment = choices
        .iter()
        .map(|choices| Some(choices[0]))
        .collect::<Vec<_>>();
    let domains = [
        MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2]),
        MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 3, 4]),
    ];
    let quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &choices,
        4,
        &[[0, 1], [2, 3], [4, 5], [6, 7], [8, 9]],
    )
    .expect("initial quotient");
    let budget = crate::solve::mesh_quotient::MeshConstraintBudget::new(10_000);

    assert!(
        crate::solve::incidence::compact_boundary_domains_jointly_viable(
            &domains,
            &choices,
            &assignment,
            None,
            &quotient,
            &budget,
        )
    );
}

#[test]
fn compact_face_quotient_states_accumulate_across_calls() {
    let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: (edge + 1) % 2,
        reversed: Some(reversed),
    };
    let domain = |second_reversed| {
        MeshFaceBoundaryDomain::DeferredValidation(
            crate::solve::missing_edge::MeshDeferredFaceBoundary {
                cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                    length: 2,
                    exact_uses: vec![(use_(0, false), 1), (use_(1, second_reversed), 1)],
                }],
                missing_edges: Vec::new(),
            },
        )
    };
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    let assignment = [Some([0, 1]), Some([0, 1])];
    let quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&choices, 2, &[[0, 1], [2, 3]])
            .expect("initial quotient");
    let budget = crate::solve::mesh_quotient::MeshConstraintBudget::new(10_000);
    let first = domain(false);
    let conflicting = domain(true);
    let initial = vec![(quotient.clone(), HashSet::new())];

    let first_states = crate::solve::incidence::advance_compact_boundary_domains(
        [&first],
        &choices,
        &assignment,
        None,
        initial.clone(),
        &budget,
    )
    .expect("first face quotient");
    assert!(crate::solve::incidence::advance_compact_boundary_domains(
        [&conflicting],
        &choices,
        &assignment,
        None,
        initial,
        &budget,
    )
    .is_some());
    assert!(crate::solve::incidence::advance_compact_boundary_domains(
        [&conflicting],
        &choices,
        &assignment,
        None,
        first_states,
        &budget,
    )
    .is_none());
}

#[test]
fn incidence_components_filter_complete_solutions_during_search() {
    let choices = vec![
        vec![[0, 1]],
        vec![[1, 2], [2, 3]],
        vec![[2, 3], [1, 2]],
        vec![[3, 0]],
    ];
    let edge_faces = [[0, 0]; 4];
    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        1,
        4,
        None,
        None,
        None,
        &|pairs| pairs[1] == [2, 3],
    )
    .expect("filtered component solution");

    assert_eq!(solutions, vec![vec![[0, 1], [2, 3], [1, 2], [3, 0]]]);
}

#[test]
fn incidence_components_apply_monotone_partial_constraints_before_solution_limits() {
    let choices = vec![
        (0..300).map(|point| [point, point]).collect::<Vec<_>>(),
        (300..600).map(|point| [point, point]).collect::<Vec<_>>(),
    ];
    let edge_faces = [[0, 0], [1, 1]];
    let partial = |assignment: &[Option<[usize; 2]>]| {
        assignment[0].is_none_or(|pair| pair == [0, 0])
            && assignment[1].is_none_or(|pair| pair == [300, 300])
    };
    let active_edges = [true, true];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        2,
        600,
        None,
        None,
        Some(MeshPartialEndpointConstraint {
            active_edges: &active_edges,
            coupled_edges: &active_edges,
            assignment_predecessors: None,
            valid: &partial,
        }),
        &|_| true,
    )
    .expect("partially constrained component solutions");

    assert_eq!(solutions, vec![vec![[0, 0], [300, 300]]]);
}

#[test]
fn incidence_components_reuse_independent_solution_domains() {
    use std::ops::ControlFlow;

    const COMPONENT_COUNT: usize = 15;
    let choices = (0..COMPONENT_COUNT)
        .map(|component| {
            let first = component * 2;
            vec![[first, first], [first + 1, first + 1]]
        })
        .collect::<Vec<_>>();
    let edge_faces = (0..COMPONENT_COUNT)
        .map(|face| [face, face])
        .collect::<Vec<_>>();
    let mut visited = 0usize;

    let outcome = crate::solve::incidence::visit_component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        COMPONENT_COUNT,
        COMPONENT_COUNT * 2,
        None,
        None,
        None,
        &|_| true,
        &mut |_| {
            visited += 1;
            ControlFlow::Continue(())
        },
    );

    assert_eq!(
        outcome,
        crate::solve::incidence::IncidenceSolve::Solved(1 << COMPONENT_COUNT)
    );
    assert_eq!(visited, 1 << COMPONENT_COUNT);
}

#[test]
fn incidence_components_include_fixed_incidence_chains() {
    let choices = vec![vec![[0, 0], [0, 1]], vec![[2, 2], [2, 3]], vec![[0, 2]]];
    let components =
        crate::solve::incidence::incidence_choice_components(&choices, &[[0, 0]; 3], None, None);

    assert_eq!(components, vec![vec![0, 1]]);
}

#[test]
fn incidence_components_preflight_independent_unsatisfiable_domains() {
    use std::ops::ControlFlow;

    const BROAD_COMPONENT_COUNT: usize = 15;
    let mut choices = (0..BROAD_COMPONENT_COUNT)
        .map(|component| {
            let first = component * 2;
            vec![[first, first], [first + 1, first + 1]]
        })
        .collect::<Vec<_>>();
    choices.push(vec![[30, 30], [31, 31], [32, 32], [33, 33]]);
    let edge_faces = (0..choices.len())
        .map(|face| [face, face])
        .collect::<Vec<_>>();
    let constrained_edge = choices.len() - 1;
    let active_edges = (0..choices.len())
        .map(|edge| edge == constrained_edge)
        .collect::<Vec<_>>();
    let partial = |assignment: &[Option<[usize; 2]>]| assignment[constrained_edge].is_none();
    let mut visited = false;

    let outcome = crate::solve::incidence::visit_component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        choices.len(),
        34,
        None,
        None,
        Some(MeshPartialEndpointConstraint {
            active_edges: &active_edges,
            coupled_edges: &active_edges,
            assignment_predecessors: None,
            valid: &partial,
        }),
        &|_| true,
        &mut |_| {
            visited = true;
            ControlFlow::Continue(())
        },
    );

    assert_eq!(
        outcome,
        crate::solve::incidence::IncidenceSolve::Rejected(
            crate::solve::incidence::IncidenceRejection::ComponentDomain
        )
    );
    assert!(!visited);
}

#[test]
fn incidence_components_discard_quotient_impossible_complete_solutions() {
    let choices = vec![vec![[0, 0], [1, 1]], vec![[1, 1]]];
    let edge_faces = [[0, 0], [1, 1]];
    let mut union = UnionFind::new(4);
    union.union(0, 1);
    union.union(2, 3);
    let quotient = MeshQuotient {
        union,
        domains: (0..4).map(|_| Arc::new(HashSet::from([0, 1]))).collect(),
        members: vec![vec![0, 1], Vec::new(), vec![2, 3], Vec::new()],
    };

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        2,
        2,
        None,
        Some(&quotient),
        None,
        &|_| true,
    )
    .expect("globally assignable component solution");

    assert_eq!(solutions, vec![vec![[0, 0], [1, 1]]]);
}

#[test]
fn incidence_components_preflight_quotient_impossible_domains() {
    use std::ops::ControlFlow;

    const BROAD_COMPONENT_COUNT: usize = 15;
    let mut choices = (0..BROAD_COMPONENT_COUNT)
        .map(|component| {
            let first = component * 2;
            vec![[first, first], [first + 1, first + 1]]
        })
        .collect::<Vec<_>>();
    choices.extend([vec![[30, 30], [31, 31]], vec![[30, 30]], vec![[31, 31]]]);
    let edge_faces = (0..choices.len())
        .map(|face| [face, face])
        .collect::<Vec<_>>();
    let mut union = UnionFind::new(choices.len() * 2);
    let mut domains = Vec::with_capacity(choices.len() * 2);
    let mut members = (0..choices.len() * 2)
        .map(|node| vec![node])
        .collect::<Vec<_>>();
    for edge in 0..choices.len() {
        union.union(edge * 2, edge * 2 + 1);
        members[edge * 2].push(edge * 2 + 1);
        members[edge * 2 + 1].clear();
        let points = if edge < BROAD_COMPONENT_COUNT {
            HashSet::from([edge * 2, edge * 2 + 1])
        } else {
            HashSet::from([30, 31])
        };
        let points = Arc::new(points);
        domains.extend([points.clone(), points]);
    }
    let quotient = MeshQuotient {
        union,
        domains,
        members,
    };
    let mut visited = false;

    let outcome = crate::solve::incidence::visit_component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        choices.len(),
        32,
        None,
        Some(&quotient),
        None,
        &|_| true,
        &mut |_| {
            visited = true;
            ControlFlow::Continue(())
        },
    );

    assert_eq!(
        outcome,
        crate::solve::incidence::IncidenceSolve::Rejected(
            crate::solve::incidence::IncidenceRejection::ComponentDomain
        )
    );
    assert!(!visited);
}

#[test]
fn fixed_incidence_assignments_must_satisfy_the_mesh_quotient() {
    let choices = vec![vec![[0, 0]], vec![[0, 0]]];
    let edge_faces = [[0, 0], [1, 1]];
    let quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: (0..4).map(|_| Arc::new(HashSet::from([0]))).collect(),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        crate::solve::incidence::component_incidence_pair_solution_outcome(
            &choices,
            &edge_faces,
            2,
            1,
            None,
            Some(&quotient),
            None,
            &|_| true,
        ),
        crate::solve::incidence::IncidenceSolve::Rejected(
            crate::solve::incidence::IncidenceRejection::FixedAssignment
        )
    );
}

#[test]
fn incidence_outcome_distinguishes_exhaustion_from_rejection() {
    use crate::solve::incidence::{component_incidence_pair_solution_outcome, IncidenceSolve};

    let choices = vec![(0..300).map(|point| [point, point]).collect::<Vec<_>>()];
    assert_eq!(
        component_incidence_pair_solution_outcome(
            &choices,
            &[[0, 0]],
            1,
            300,
            None,
            None,
            None,
            &|_| true,
        ),
        IncidenceSolve::Exhausted
    );
    assert_eq!(
        component_incidence_pair_solution_outcome(
            &[Vec::new()],
            &[[0, 0]],
            1,
            1,
            None,
            None,
            None,
            &|_| true,
        ),
        IncidenceSolve::Rejected(crate::solve::incidence::IncidenceRejection::FixedAssignment)
    );
}

#[test]
fn incidence_component_products_stream_until_the_consumer_stops() {
    use crate::solve::incidence::{visit_component_incidence_pair_solutions, IncidenceSolve};
    use std::ops::ControlFlow;

    let choices = (0..9)
        .map(|edge| vec![[edge * 2, edge * 2], [edge * 2 + 1, edge * 2 + 1]])
        .collect::<Vec<_>>();
    let edge_faces = (0..9).map(|face| [face, face]).collect::<Vec<_>>();
    let mut visited = 0usize;

    let outcome = visit_component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        9,
        18,
        None,
        None,
        None,
        &|_| true,
        &mut |_| {
            visited += 1;
            if visited == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    );

    assert_eq!(outcome, IncidenceSolve::Solved(2));
    assert_eq!(visited, 2);
}

#[test]
fn incidence_component_prefix_can_prove_the_consumer_result_before_exhaustion() {
    use crate::solve::incidence::{visit_component_incidence_pair_solutions, IncidenceSolve};
    use std::cell::Cell;
    use std::ops::ControlFlow;

    let choices = vec![(0..300).map(|point| [point, point]).collect::<Vec<_>>()];
    let mut visited = 0usize;
    let validated = Cell::new(0usize);
    let outcome = visit_component_incidence_pair_solutions(
        &choices,
        &[[0, 0]],
        1,
        300,
        None,
        None,
        None,
        &|_| {
            validated.set(validated.get() + 1);
            true
        },
        &mut |_| {
            visited += 1;
            if visited == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    );

    assert_eq!(outcome, IncidenceSolve::Solved(2));
    assert_eq!(visited, 2);
    assert_eq!(validated.get(), 2);
}

#[test]
fn quotient_assignments_ignore_span_allocation_with_identical_edge_order() {
    let use_ = |edge, start, end| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end,
        reversed: None,
    };
    let mut faces = vec![vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                use_(0, 0, 1),
                use_(1, 1, 2),
                use_(2, 2, 3),
                use_(3, 3, 4),
            ]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                use_(0, 0, 2),
                use_(1, 2, 3),
                use_(2, 3, 4),
                use_(3, 4, 5),
            ]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                use_(3, 0, 1),
                use_(2, 1, 2),
                use_(1, 2, 3),
                use_(0, 3, 4),
            ]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                use_(0, 0, 1),
                use_(2, 1, 2),
                use_(1, 2, 3),
                use_(3, 3, 4),
            ]],
        },
    ]];
    deduplicate_mesh_quotient_assignments(&mut faces);
    assert_eq!(faces[0].len(), 2);
    assert_eq!(faces[0][0].boundaries[0][0].edge, 0);
    assert_eq!(faces[0][1].boundaries[0][1].edge, 2);
}

#[test]
fn mesh_option_enumeration_does_not_scan_fixed_direction_gauges() {
    const EDGE_COUNT: usize = 10;
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..EDGE_COUNT)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: edge,
                end: (edge + 1) % EDGE_COUNT,
                reversed: None,
            })
            .collect()],
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(EDGE_COUNT * 2),
        domains: repeated_domain(HashSet::from([0]), EDGE_COUNT * 2),
        members: (0..EDGE_COUNT * 2).map(|node| vec![node]).collect(),
    };
    let candidates = vec![vec![[0, 0]]; EDGE_COUNT];
    let budget = MeshConstraintBudget::new(30);

    let options = quotient.assignment_options_limited(
        &assignment,
        &candidates,
        &HashSet::new(),
        2,
        Some(&budget),
    );

    assert_eq!(options.len(), 1);
    assert_eq!(options[0].0, vec![vec![false; EDGE_COUNT]]);
    assert!(!budget.exhausted.get());
}

#[test]
fn mesh_option_enumeration_preserves_asymmetric_endpoint_directions() {
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
    let quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0], vec![1], vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    let options = quotient.assignment_options_limited(
        &assignment,
        &[vec![[0, 1]], vec![[0, 1]]],
        &HashSet::new(),
        4,
        None,
    );

    assert!(options
        .iter()
        .any(|(directions, _)| directions == &[vec![false, true]]));
}

#[test]
fn quotient_merge_preserves_physical_edge_pair_correlation() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0], vec![0, 1], vec![0], vec![2]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    quotient.merge(1, 2).expect("nonempty port intersection");
    assert!(!quotient.edge_domains_viable(&[vec![[0, 1]], vec![[0, 2]]]));
}

#[test]
fn quotient_clones_share_unconstrained_point_domains() {
    let all = Arc::new((0..1_000).collect::<HashSet<_>>());
    let quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![all.clone(), all.clone(), all.clone(), all.clone()],
        members: (0..4).map(|node| vec![node]).collect(),
    };

    let clone = quotient.clone();
    assert!(Arc::ptr_eq(&quotient.domains[0], &clone.domains[0]));
    assert!(Arc::ptr_eq(&quotient.domains[0], &quotient.domains[3]));
}

#[test]
fn port_quotient_completes_only_supported_unknown_edge_pairs() {
    let candidates = [vec![[0, 1]], Vec::new(), vec![[2, 3]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        5,
        &[[10, 11], [10, 12], [12, 13]],
    )
    .expect("initial quotient");
    let completed = crate::solve::mesh_quotient::complete_mesh_endpoint_candidates_from_quotient(
        &candidates,
        &mut quotient,
        16,
        32,
    )
    .expect("bounded quotient completion");

    assert_eq!(completed[0], vec![[0, 1]]);
    assert_eq!(completed[1], vec![[0, 2], [0, 3], [1, 2], [1, 3]]);
    assert_eq!(completed[2], vec![[2, 3]]);
}

#[test]
fn coordinate_root_fixpoint_removes_unsupported_edge_pairs() {
    let candidates = [
        vec![[0, 1], [0, 2]],
        vec![[0, 1]],
        vec![[0, 1]],
        vec![[2, 2]],
    ];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        3,
        &[[10, 11], [10, 12], [13, 11], [14, 14]],
    )
    .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(3, &candidates, None)
        .expect("coordinate root domains");

    assert_eq!(
        domains.edge_candidates(),
        &[vec![[0, 1]], vec![[0, 1]], vec![[0, 1]], vec![[2, 2]]]
    );
}

#[test]
fn selected_edge_pair_propagates_through_shared_coordinate_roots() {
    let candidates = [vec![[0, 1], [0, 2]], vec![[1, 3], [2, 3]], vec![[2, 3]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        4,
        &[[10, 11], [11, 12], [13, 14]],
    )
    .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(4, &candidates, None)
        .expect("coordinate root domains")
        .refine_edge_candidate_arc(0, [0, 1], None)
        .expect("selected edge pair");

    assert!(domains.supports_edge_candidate(1, [1, 3]));
    assert!(!domains.supports_edge_candidate(1, [2, 3]));
}

#[test]
fn port_quotient_declines_unbounded_unknown_edge_pairs() {
    let candidates = [Vec::new()];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 100, &[[10, 11]])
            .expect("initial quotient");
    assert!(
        crate::solve::mesh_quotient::complete_mesh_endpoint_candidates_from_quotient(
            &candidates,
            &mut quotient,
            1_000,
            1_000,
        )
        .is_none()
    );
}

#[test]
fn coordinate_root_domains_keep_unknown_edge_pairs_implicit() {
    let candidates = [Vec::new()];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 2, &[[10, 11]])
            .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(2, &candidates, None)
        .expect("implicit coordinate domains");

    assert!(domains.edge_candidates()[0].is_empty());
    assert_eq!(domains.edge_candidate_points(0), Some(vec![0, 1]));
    assert_eq!(
        domains
            .implicit_edge_candidates(0, Some(0))
            .expect("implicit candidates")
            .collect::<Vec<_>>(),
        vec![[0, 1]]
    );
    assert!(domains.refine_edge_candidate_arc(0, [0, 1], None).is_some());
    assert!(domains.refine_edge_candidate_arc(0, [0, 0], None).is_none());
}

#[test]
fn required_implicit_coordinate_pairs_scale_with_root_domains_not_their_product() {
    let candidates = [Vec::new(), Vec::new()];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 4, &[[10, 11], [12, 13]])
            .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(4, &candidates, None)
        .expect("implicit coordinate domains");
    let implicit = domains
        .implicit_edge_candidates(0, Some(1))
        .expect("required implicit candidates");

    assert_eq!(implicit.width_upper_bound(), 3);
    assert_eq!(implicit.collect::<Vec<_>>(), vec![[0, 1], [1, 2], [1, 3]]);

    let mut visited = Vec::new();
    assert_eq!(
        domains.any_implicit_edge_candidate_with_point(0, 1, |pair| {
            visited.push(pair);
            pair == [1, 3]
        }),
        Some(true)
    );
    assert_eq!(visited, vec![[0, 1], [1, 2], [1, 3]]);
}

#[test]
fn coordinate_domain_preparation_scales_with_constraint_graph_work() {
    let candidates = vec![Vec::new(); 100];
    let ports = (0..100)
        .map(|edge| [(edge * 2) as u32, (edge * 2 + 1) as u32])
        .collect::<Vec<_>>();
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 200, &ports)
        .expect("initial quotient");

    assert!(
        quotient
            .coordinate_domain_preparation_limit(200, &candidates)
            .expect("preparation limit")
            > crate::solve::mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS
    );
}

#[test]
fn incidence_search_consumes_implicit_coordinate_root_pairs() {
    use crate::solve::incidence::{component_incidence_pair_solution_outcome, IncidenceSolve};

    let candidates = vec![Vec::new(); 3];
    let quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        3,
        &[[10, 11], [11, 12], [12, 10]],
    )
    .expect("cycle quotient");
    let outcome = component_incidence_pair_solution_outcome(
        &candidates,
        &[[0, 0]; 3],
        1,
        3,
        None,
        Some(&quotient),
        None,
        &|_| true,
    );

    assert!(matches!(
        outcome,
        IncidenceSolve::Solved(_) | IncidenceSolve::Ambiguous
    ));
}

#[test]
fn ordered_face_equations_narrow_unknown_edge_roots_before_pair_completion() {
    let edge_candidates = vec![vec![[0, 1]], Vec::new(), vec![[0, 2]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &edge_candidates,
        3,
        &[[10, 11], [12, 13], [14, 15]],
    )
    .expect("initial quotient");
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![(0..3)
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: Some(false),
                })
                .collect()],
        },
    ])];
    let budget = crate::solve::mesh_quotient::MeshConstraintBudget::new(10_000);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &edge_candidates,
        &mut quotient,
        &budget,
    )
    .expect("common face equations");
    let completed = crate::solve::mesh_quotient::complete_mesh_endpoint_candidates_from_quotient(
        &edge_candidates,
        &mut quotient,
        16,
        32,
    )
    .expect("completed edge domain");

    assert_eq!(completed[1], vec![[1, 2]]);
}

#[test]
fn quotient_pair_domains_propagate_through_shared_components() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0, 1], vec![2], vec![0, 1], vec![3, 4]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let root = quotient.merge(0, 2).expect("shared endpoint component");

    assert!(quotient.edge_domains_viable(&[vec![[0, 2]], vec![[0, 3], [1, 4]],]));
    assert_eq!(*quotient.domains[root], HashSet::from([0]));
    assert_eq!(
        *quotient.domains[quotient.union.find(3)],
        HashSet::from([3])
    );
}

#[test]
fn quotient_assignment_requires_one_consistent_closed_orientation() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0], vec![1], vec![2], vec![3]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
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
        ]],
    };
    assert!(!quotient.assignment_has_option(&assignment, &[vec![], vec![]], None));
    Arc::make_mut(&mut quotient.domains[2]).insert(1);
    assert!(!quotient.assignment_has_option(&assignment, &[vec![], vec![]], None));
    Arc::make_mut(&mut quotient.domains[3]).insert(0);
    assert!(quotient.assignment_has_option(&assignment, &[vec![], vec![]], None));
}

#[test]
fn quotient_assignment_declines_when_its_work_budget_is_exhausted() {
    let quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: (0..2).map(|node| vec![node]).collect(),
    };
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let budget = MeshConstraintBudget::new(0);

    assert!(!quotient.assignment_has_option(&assignment, &[vec![[0, 0]]], Some(&budget),));
    assert!(budget.exhausted.get());
}

#[test]
fn face_choice_materialization_declines_when_its_work_budget_is_exhausted() {
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: Some(false),
        }]],
    }]];
    let equations = possible_face_equations(&assignments);

    assert!(possible_face_choices_with_limit(&assignments, &equations, 0).is_none());
}

#[test]
fn fixed_boundary_option_has_no_recursive_depth_limit() {
    const EDGE_COUNT: usize = 10_000;
    let quotient = MeshQuotient {
        union: UnionFind::new(EDGE_COUNT * 2),
        domains: repeated_domain(HashSet::from([0]), EDGE_COUNT * 2),
        members: (0..EDGE_COUNT * 2).map(|node| vec![node]).collect(),
    };
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..EDGE_COUNT)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: edge,
                end: (edge + 1) % EDGE_COUNT,
                reversed: Some(false),
            })
            .collect()],
    };
    let candidates = vec![vec![[0, 0]]; EDGE_COUNT];

    assert!(quotient.assignment_has_option(&assignment, &candidates, None));
}

#[test]
fn quotient_options_reject_an_interior_pair_contradiction() {
    let quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: [vec![0], vec![1, 2], vec![2], vec![3], vec![0, 3], vec![0]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..6).map(|node| vec![node]).collect(),
    };
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
                end: 3,
                reversed: None,
            },
        ]],
    };
    let candidates = [vec![[0, 1]], vec![[2, 3]], vec![[0, 3]]];

    let options = quotient.assignment_options(&assignment, &candidates);

    assert!(!options
        .iter()
        .any(|(directions, _)| directions == &[vec![false, false, false]]));
    let unrestricted = [Vec::new(), Vec::new(), Vec::new()];
    let options = quotient.assignment_options(&assignment, &unrestricted);
    let limited =
        quotient.assignment_options_limited(&assignment, &unrestricted, &HashSet::new(), 1, None);
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].0, options[0].0);
    let unique = quotient.assignment_options_limited(
        &assignment,
        &unrestricted,
        &HashSet::new(),
        4_096,
        None,
    );
    assert!(unique
        .iter()
        .all(|option| options.iter().any(|candidate| candidate.0 == option.0)));
}

#[test]
fn quotient_options_decline_when_their_work_budget_is_exhausted() {
    let quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: (0..2).map(|node| vec![node]).collect(),
    };
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let budget = MeshConstraintBudget::new(0);

    let options = quotient.assignment_options_limited(
        &assignment,
        &[vec![[0, 0]]],
        &HashSet::new(),
        1,
        Some(&budget),
    );

    assert!(options.is_empty());
    assert!(budget.exhausted.get());
}

#[test]
fn quotient_point_assignment_preserves_endpoint_pair_relations() {
    let quotient = || MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0, 1], vec![2], vec![0, 1], vec![3]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    assert!(quotient()
        .point_assignment(4, &[vec![], vec![]], None)
        .is_none());
    assert!(quotient().point_assignment_exists(4, &[vec![], vec![]], None));

    let assignment = quotient()
        .point_assignment(4, &[vec![[0, 2]], vec![[1, 3]]], None)
        .expect("edge-pair relations determine the coordinate bijection");
    assert_eq!(assignment[&0], 0);
    assert_eq!(assignment[&1], 2);
    assert_eq!(assignment[&2], 1);
    assert_eq!(assignment[&3], 3);
}

#[test]
fn quotient_point_existence_declines_when_its_work_budget_is_exhausted() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0, 1]), 2),
        members: (0..2).map(|node| vec![node]).collect(),
    };
    let budget = MeshConstraintBudget::new(0);

    assert!(!quotient.point_assignment_exists(2, &[vec![]], Some(&budget)));
    assert!(budget.exhausted.get());
}

#[test]
fn point_assignment_handles_deep_augmenting_paths_iteratively() {
    const ROOT_COUNT: usize = 10_000;
    let mut domains = (0..ROOT_COUNT - 1)
        .map(|root| Arc::new(HashSet::from([root, root + 1])))
        .collect::<Vec<_>>();
    domains.push(Arc::new(HashSet::from([0])));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(ROOT_COUNT),
        domains,
        members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
    };

    let assignment = quotient
        .point_assignment(ROOT_COUNT, &[], None)
        .expect("forced coordinate bijection");

    assert_eq!(assignment.len(), ROOT_COUNT);
    assert_eq!(assignment[&(ROOT_COUNT - 1)], 0);
    assert!((0..ROOT_COUNT - 1).all(|root| assignment[&root] == root + 1));
}

#[test]
fn quotient_point_existence_rejects_an_all_different_conflict() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: vec![Arc::new(HashSet::from([0])), Arc::new(HashSet::from([0]))],
        members: vec![vec![0], vec![1]],
    };

    assert!(!quotient.point_assignment_exists(2, &[vec![]], None));
}

#[test]
fn quotient_point_existence_can_become_viable_after_a_root_merge() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([1])),
            Arc::new(HashSet::from([2])),
        ],
        members: vec![vec![0], vec![1], vec![2], vec![3]],
    };

    assert!(!quotient.point_assignment_exists(3, &[vec![], vec![]], None));
    quotient.merge(0, 1).expect("compatible roots merge");
    assert!(quotient.point_assignment_exists(3, &[vec![], vec![]], None));
}

#[test]
fn radial_orientation_solves_each_face_boundary_independently() {
    let rows = (0..18)
        .map(|edge| EdgeRow {
            kind: 1,
            handles: vec![edge * 2, edge * 2 + 1],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        })
        .collect();
    let points = (0..12).map(|point| [f64::from(point), 0.0, 0.0]).collect();
    let edge_faces = [
        [8, 2],
        [8, 3],
        [4, 0],
        [7, 0],
        [4, 1],
        [7, 1],
        [2, 4],
        [3, 4],
        [7, 6],
        [7, 5],
        [8, 6],
        [8, 5],
        [1, 0],
        [1, 0],
        [3, 2],
        [3, 2],
        [6, 5],
        [6, 5],
    ];
    let edge_points = [
        [0, 1],
        [0, 1],
        [2, 4],
        [3, 5],
        [2, 4],
        [3, 5],
        [6, 7],
        [6, 7],
        [8, 9],
        [8, 9],
        [10, 11],
        [10, 11],
        [2, 3],
        [4, 5],
        [0, 6],
        [1, 7],
        [8, 10],
        [9, 11],
    ];
    let topology = reconstruct_incidence(rows, points, &edge_faces, &edge_points, 9)
        .expect("orientable multi-boundary shell");
    assert_eq!(topology.body_kinds(&[9]), Some(vec![BodyKind::Solid]));
    assert_eq!(topology.body_kinds(&[4, 5]), None);
    assert_eq!(topology.faces()[4].boundaries.len(), 2);
    let mut uses = vec![Vec::new(); 18];
    for face in topology.faces() {
        for boundary in &face.boundaries {
            for coedge in &boundary.coedges {
                uses[coedge.edge_row].push(coedge.reversed);
            }
        }
    }
    assert!(uses
        .iter()
        .all(|senses| senses == &[false, true] || senses == &[true, false]));
}

#[test]
fn open_standard_edge_incidence_classifies_a_sheet_body() {
    let mut topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary {
                coedges: vec![CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }],
            }],
        }],
        edge_rows: vec![
            EdgeRow {
                kind: 1,
                handles: vec![0, 1],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
            EdgeRow {
                kind: 1,
                handles: vec![2, 3],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
        ],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };

    assert_eq!(topology.body_kinds(&[1]), None);
    topology.edge_rows.pop();
    assert_eq!(topology.body_kinds(&[1]), Some(vec![BodyKind::Sheet]));
}

#[test]
fn solid_body_cycles_orient_independently_from_an_open_sheet_body() {
    let use_ = |edge_row| CoedgeUse {
        edge_row,
        reversed: false,
        start_vertex: edge_row,
        end_vertex: 1 - edge_row,
    };
    let mut topology = StandardTopology {
        faces: vec![
            FaceTopology {
                boundaries: vec![Boundary {
                    coedges: vec![use_(0), use_(1)],
                }],
            },
            FaceTopology {
                boundaries: vec![Boundary {
                    coedges: vec![use_(0), use_(1)],
                }],
            },
            FaceTopology {
                boundaries: vec![Boundary {
                    coedges: vec![CoedgeUse {
                        edge_row: 2,
                        reversed: false,
                        start_vertex: 0,
                        end_vertex: 1,
                    }],
                }],
            },
        ],
        edge_rows: (0..3)
            .map(|edge| EdgeRow {
                kind: 1,
                handles: vec![edge * 2, edge * 2 + 1],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            })
            .collect(),
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };

    assert_eq!(
        topology.body_kinds(&[2, 1]),
        Some(vec![BodyKind::Solid, BodyKind::Sheet])
    );
    assert_eq!(topology.body_kinds(&[3]), Some(vec![BodyKind::General]));
    assert_eq!(topology.face_components(), vec![vec![0, 1], vec![2]]);
    topology
        .orient_solid_body_cycles(&[2, 1])
        .expect("closed group orientation");

    for edge in 0..2 {
        assert_ne!(
            topology.faces[0].boundaries[0].coedges[edge].reversed,
            topology.faces[1].boundaries[0].coedges[1 - edge].reversed,
        );
    }
    assert!(!topology.faces[2].boundaries[0].coedges[0].reversed);
}

#[test]
fn mesh_selection_rejects_an_odd_boundary_orientation_cycle() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let assignments = vec![
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(2)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1), use_(2)]],
        }],
    ];
    let mut search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1); 3],
        edge_candidates: &[],
        edge_rows: &[],
        vertex_points: &[],
        selected: vec![
            Some((0, vec![vec![false, false]])),
            Some((0, vec![vec![false, false]])),
            Some((0, vec![vec![false, false]])),
        ],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };

    assert!(!search.selected_orientable());
    search.selected[2] = Some((0, vec![vec![false, true]]));
    assert!(search.selected_orientable());
}

#[test]
fn partial_boundary_orientation_constraints_reject_an_odd_parity_cycle() {
    let mut edge_uses = HashMap::from([
        (0, vec![(0, false), (1, false)]),
        (1, vec![(1, false), (2, false)]),
        (2, vec![(2, false), (0, false)]),
        (3, vec![(3, false)]),
    ]);

    assert!(solve_boundary_orientation_constraints(4, &edge_uses, false).is_none());
    edge_uses.get_mut(&2).expect("third paired edge")[1].1 = true;
    assert!(solve_boundary_orientation_constraints(4, &edge_uses, false).is_some());
    assert!(solve_boundary_orientation_constraints(4, &edge_uses, true).is_none());
}

#[test]
fn partial_face_orientability_rejects_an_odd_open_path_cycle() {
    let edge_faces = [[0, 1], [1, 2], [2, 0]];
    let face_edges = vec![vec![0, 2], vec![0, 1], vec![1, 2]];
    let budget = MeshConstraintBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let complete = [Some([0, 1]), Some([1, 2]), Some([0, 2])];

    assert!(!crate::solve::incidence::partial_face_orientability_viable(
        &complete,
        &edge_faces,
        &face_edges,
        &budget,
    ));
    let partial = [Some([0, 1]), Some([1, 2]), None];
    assert!(crate::solve::incidence::partial_face_orientability_viable(
        &partial,
        &edge_faces,
        &face_edges,
        &budget,
    ));
    assert!(!budget.exhausted.get());
}

#[test]
fn mesh_selection_rejects_a_branch_with_no_orientable_remaining_face() {
    let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed,
    };
    let assignments = vec![
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0, None), use_(2, None)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0, None), use_(1, None)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1, Some(false)), use_(2, Some(false))]],
        }],
    ];
    let edge_candidates = vec![Vec::new(); 3];
    let mut search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1); 3],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        selected: vec![
            Some((0, vec![vec![false, false]])),
            Some((0, vec![vec![false, false]])),
            None,
        ],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    assert!(!search.fixed_remaining_faces_are_orientable());
    search.selected[1] = Some((0, vec![vec![false, true]]));
    assert!(search.fixed_remaining_faces_are_orientable());
}

#[test]
fn mesh_selection_checks_all_fixed_remaining_faces_together() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: Some(false),
    };
    let assignments = vec![
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(2), use_(0)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1), use_(2)]],
        }],
    ];
    let edge_candidates = vec![Vec::new(); 3];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1); 3],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        selected: vec![Some((0, vec![vec![false, false]])), None, None],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };

    assert!(!search.fixed_remaining_faces_are_orientable());
}

#[test]
fn partial_mesh_selection_survives_optional_deduction_exhaustion() {
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
    let edge_candidates = vec![vec![[0, 1]], vec![[0, 1]]];
    let edge_rows = vec![
        EdgeRow {
            kind: 1,
            handles: vec![0, 1],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        };
        2
    ];
    let vertex_points = vec![[0.0; 3], [1.0, 0.0, 0.0]];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &edge_rows,
        vertex_points: &vertex_points,
        selected: vec![None],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient =
        initial_mesh_quotient(&edge_candidates, 2, &[[0, 1], [2, 3]]).expect("initial quotient");
    quotient.merge(1, 2).expect("selected face corner");
    let propagation_budget = MeshConstraintBudget::new(0);
    let changed_edges = HashSet::from([0]);

    let mut prepared = search
        .prepare_selected_branch(&quotient, &changed_edges, &propagation_budget)
        .expect("partial quotient remains viable");

    assert_eq!(prepared.root_count(), 3);
    assert!(propagation_budget.exhausted.get());
}

#[test]
fn mesh_assignment_distinguishes_quotient_work_from_direction_only_work() {
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
                end: 0,
                reversed: Some(false),
            },
        ]],
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0, 1]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert!(mesh_assignment_can_merge(&assignment, &mut quotient));
    quotient.merge(1, 2).expect("first boundary corner");
    quotient.merge(3, 0).expect("second boundary corner");
    assert!(!mesh_assignment_can_merge(&assignment, &mut quotient));
}

#[test]
fn remaining_merge_capacity_counts_distinct_quotient_equations() {
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
                end: 0,
                reversed: Some(false),
            },
        ]],
    };
    let assignments = vec![vec![assignment.clone()], vec![assignment]];
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1); 2],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        selected: vec![None; 2],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0, 1]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        Some(2)
    );
    quotient.merge(1, 2).expect("first repeated equation");
    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        Some(1)
    );
}

#[test]
fn remaining_merge_capacity_respects_mutually_exclusive_orientations() {
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
        assignments: &assignments,
        possible_face_choices: possible_face_choices(&assignments, &equations),
        possible_face_equations: equations,
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        selected: vec![None],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0, 1]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        Some(2)
    );
}

#[test]
fn remaining_equations_must_connect_equal_singleton_domains() {
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
        selected: vec![None],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([1])),
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([2])),
        ],
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        None
    );
}

#[test]
fn remaining_equation_components_require_a_coordinate_matching() {
    let assignments = Vec::new();
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work: Vec::new(),
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        selected: Vec::new(),
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([2, 3])),
        ],
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        None
    );
}

#[test]
fn coordinate_matching_reserves_unavoidable_roots_per_component() {
    let assignments = vec![Vec::new()];
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: vec![vec![[0, 1], [1, 2]]],
        possible_face_choices: vec![vec![vec![[0, 1]], vec![[1, 2]]]],
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[[0.0, 0.0, 0.0]; 3],
        selected: vec![None],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([1, 2])),
        ],
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        None
    );
}

#[test]
fn completed_mesh_search_continues_to_check_uniqueness() {
    let assignments = Vec::new();
    let edge_candidates = Vec::new();
    let edge_rows = Vec::new();
    let vertex_points = Vec::new();
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work: Vec::new(),
        edge_candidates: &edge_candidates,
        edge_rows: &edge_rows,
        vertex_points: &vertex_points,
        selected: Vec::new(),
        states: 512,
        solution: Some((
            StandardTopology {
                faces: Vec::new(),
                edge_rows: Vec::new(),
                vertex_points: Vec::new(),
                logical_vertex_count: 0,
            },
            Vec::new(),
        )),
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };

    assert!(!search.should_stop());
}

#[test]
fn mesh_selection_declines_when_its_work_budget_is_exhausted() {
    let mut search = MeshSelectionSearch {
        assignments: &[],
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work: Vec::new(),
        edge_candidates: &[],
        edge_rows: &[],
        vertex_points: &[],
        selected: Vec::new(),
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(0),
        domains: Vec::new(),
        members: Vec::new(),
    };

    search.search_with_limit(&quotient, 0);

    assert!(search.exhausted);
    assert!(search.solution.is_none());
}

#[test]
fn mesh_selection_finishes_the_active_face_component_first() {
    const UNRELATED_FACE_COUNT: usize = 1_000;
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
        assignments: &assignments,
        possible_face_equations: vec![Vec::new(); face_count],
        possible_face_choices: vec![Vec::new(); face_count],
        face_work: vec![Some(1); face_count],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[[0.0; 3], [1.0, 0.0, 0.0]],
        selected,
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(edge_count * 2),
        domains,
        members: (0..edge_count * 2).map(|node| vec![node]).collect(),
    };
    let budget = MeshConstraintBudget::new(5);
    let propagation_budget = MeshConstraintBudget::new(0);

    search.search_from_state(&quotient, true, &budget, &propagation_budget);

    assert!(!search.exhausted);
    assert!(search.solution.is_none());
}

#[test]
fn forced_face_selection_does_not_consume_the_branch_budget() {
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: Some(false),
        }]],
    }]];
    let edge_candidates = vec![vec![[0, 0]]];
    let edge_rows = vec![EdgeRow {
        kind: 1,
        handles: vec![0],
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    let mut search = MeshSelectionSearch {
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
        selected: vec![None],
        states: 512,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: (0..2).map(|node| vec![node]).collect(),
    };

    search.search(&quotient);

    assert!(!search.exhausted);
    assert_eq!(search.states, 512);
}

#[test]
fn overmerged_face_options_do_not_consume_the_branch_budget() {
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
        EdgeRow {
            kind: 1,
            handles: vec![0],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        };
        2
    ];
    let mut search = MeshSelectionSearch {
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
        selected: vec![None],
        states: 512,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0, 1, 2]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    search.search(&quotient);

    assert!(!search.exhausted);
    assert_eq!(search.states, 512);
    assert!(search.solution.is_none());
}

#[test]
fn mesh_selection_merges_corner_equations_common_to_every_option() {
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
        selected: vec![None],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: (0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
    assert_eq!(quotient.union.find(3), quotient.union.find(4));
    assert_eq!(quotient.union.find(5), quotient.union.find(0));
    assert_eq!(quotient.root_count(), 3);
}

#[test]
fn mesh_selection_merges_equations_common_to_every_assignment() {
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
        selected: vec![None],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: (0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
    assert_eq!(quotient.root_count(), 5);
}

#[test]
fn mesh_selection_common_equations_ignore_infeasible_assignments() {
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
        selected: vec![None],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: [1, 0, 0, 1, 2, 2]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
    assert_eq!(quotient.union.find(3), quotient.union.find(0));
    assert_eq!(quotient.root_count(), 4);
}

#[test]
fn mesh_selection_propagates_closed_ports_without_enumerating_directions() {
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
        selected: vec![None],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(26),
        domains: (0..26).map(|_| Arc::new((0..13).collect())).collect(),
        members: (0..26).map(|node| vec![node]).collect(),
    };
    for edge in 0..13 {
        quotient.merge(edge * 2, edge * 2 + 1).expect("closed port");
    }

    assert_eq!(quotient.root_count(), 13);
    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(quotient.root_count(), 1);
}

#[test]
fn face_equation_cache_ignores_unrelated_quotient_components() {
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
    let candidates = vec![vec![]; 3];
    let search = MeshSelectionSearch {
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
        selected: vec![None],
        states: 0,
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: (0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
    quotient.merge(4, 5).expect("unrelated component merge");
    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
    quotient
        .merge(0, 4)
        .expect("component joined to a face port");
    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(search.face_equation_cache.borrow().len(), 2);
    {
        let mut cache = search.face_equation_cache.borrow_mut();
        for key in 1..=MAX_FACE_EQUATION_CACHE_ENTRIES {
            cache.insert((key, Vec::new()), Vec::new());
        }
    }
    quotient.merge(1, 2).expect("new face-component merge");
    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
}

#[test]
fn endpoint_ports_propagate_resolved_pairs_to_unresolved_edges() {
    let ports = [[10, 11], [11, 12], [12, 13], [13, 10]];
    let pairs = [Some([0, 1]), Some([1, 2]), None, Some([3, 0])];
    assert_eq!(
        propagate_edge_port_points(&ports, &pairs),
        Some(vec![Some([0, 1]), Some([1, 2]), Some([2, 3]), Some([3, 0]),])
    );
}

#[test]
fn partial_endpoint_ports_propagate_known_components_only() {
    let ports = [
        Some([10, 11]),
        Some([11, 12]),
        None,
        Some([12, 13]),
        Some([13, 10]),
    ];
    let pairs = [Some([0, 1]), Some([1, 2]), Some([8, 9]), None, Some([3, 0])];

    assert_eq!(
        propagate_partial_edge_port_points(&ports, &pairs),
        Some(vec![
            Some([0, 1]),
            Some([1, 2]),
            Some([8, 9]),
            Some([2, 3]),
            Some([3, 0]),
        ])
    );
}

#[test]
fn unbound_native_edge_pair_must_be_unique_in_the_geometric_domain() {
    use crate::families::standard::decode::unique_unbound_native_endpoint_pair;

    assert_eq!(
        unique_unbound_native_endpoint_pair(&[2, 4, 7], &[[7, 2], [2, 7], [1, 4]]),
        Some([2, 7])
    );
    assert_eq!(
        unique_unbound_native_endpoint_pair(&[2, 4, 7], &[[7, 2], [2, 4]]),
        None
    );
    assert_eq!(
        unique_unbound_native_endpoint_pair(&[2, 4, 7], &[[1, 4], [2, 9]]),
        None
    );
}

#[test]
fn endpoint_port_propagation_requires_a_point_bijection() {
    assert_eq!(
        propagate_edge_port_points(&[[10, 11]], &[Some([0, 1])]),
        Some(vec![Some([0, 1])])
    );
    assert_eq!(
        propagate_edge_port_points(&[[10, 11], [10, 12]], &[Some([0, 1]), Some([0, 1])]),
        None
    );
    assert_eq!(
        propagate_edge_port_points(&[[10, 11]], &[Some([0, 0])]),
        None
    );
}

#[test]
fn endpoint_port_propagation_closes_equal_port_edges() {
    let ports = [[10, 11], [11, 12], [10, 10]];
    let pairs = [Some([0, 1]), Some([1, 2]), None];

    assert_eq!(
        propagate_edge_port_points(&ports, &pairs),
        Some(vec![Some([0, 1]), Some([1, 2]), Some([0, 0])])
    );
}

#[test]
fn equal_endpoint_ports_produce_closed_edge_candidates() {
    let ports = [[10, 10], [10, 11]];
    let candidates = [vec![[0, 0], [1, 1], [2, 2]], vec![[1, 3], [2, 4]]];
    assert_eq!(
        prune_edge_candidates_by_port_domains(&ports, &candidates),
        Some(vec![vec![[1, 1], [2, 2]], vec![[1, 3], [2, 4]]])
    );
    assert_eq!(
        prune_edge_candidates_by_port_domains(&[[10, 10]], &[vec![[0, 1], [0, 2]]]),
        None
    );
}

#[test]
fn endpoint_port_domains_propagate_pair_correlation_to_a_fixpoint() {
    let ports = [[10, 11], [11, 12], [12, 13]];
    let candidates = [vec![[0, 1], [2, 3]], vec![[1, 4], [3, 5]], vec![[4, 6]]];

    assert_eq!(
        prune_edge_candidates_by_port_domains(&ports, &candidates),
        Some(vec![vec![[0, 1]], vec![[1, 4]], vec![[4, 6]]])
    );
}

#[test]
fn mesh_endpoint_validation_accepts_equal_points_only_for_closed_ports() {
    assert!(mesh_edge_points_compatible(true, &[[2, 2]], [2, 2]));
    assert!(!mesh_edge_points_compatible(false, &[[2, 2]], [2, 2]));
    assert!(!mesh_edge_points_compatible(true, &[[1, 1]], [2, 2]));
}

#[test]
fn quotient_merges_roots_forced_to_one_coordinate_identity() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [0, 1, 0, 2]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert!(quotient.merge_singleton_coordinate_roots(&[Vec::new(), Vec::new()]));
    assert_eq!(quotient.root_count(), 3);
    assert_eq!(quotient.union.find(0), quotient.union.find(2));
}

#[test]
fn singleton_coordinate_root_merges_are_batched() {
    const ROOT_COUNT: usize = 10_000;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(ROOT_COUNT),
        domains: repeated_domain(HashSet::from([0]), ROOT_COUNT),
        members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
    };
    let candidates = vec![Vec::new(); ROOT_COUNT / 2];

    assert!(quotient.merge_singleton_coordinate_roots(&candidates));
    assert_eq!(quotient.root_count(), 1);
}

#[test]
fn quotient_closes_coordinate_roots_forced_by_joint_edge_pairs() {
    let all = Arc::new(HashSet::from([0, 1, 2]));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: vec![all.clone(); 6],
        members: (0..6).map(|node| vec![node]).collect(),
    };
    quotient.merge(1, 2).expect("shared first corner");
    quotient.merge(3, 4).expect("shared second corner");
    let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];

    let assignment = quotient
        .close_coordinate_roots(3, &candidates, None)
        .expect("unique joint coordinate closure");

    assert_eq!(quotient.root_count(), 3);
    assert_eq!(quotient.union.find(0), quotient.union.find(5));
    assert_eq!(assignment[&quotient.union.find(0)], 0);
    assert_eq!(assignment[&quotient.union.find(1)], 1);
    assert_eq!(assignment[&quotient.union.find(3)], 2);
    for node in 0..6 {
        let root = quotient.union.find(node);
        assert_eq!(quotient.domains[root].len(), 1);
        assert_eq!(quotient.domains[root].iter().next(), assignment.get(&root));
    }
}

#[test]
fn quotient_coordinate_closure_declines_when_its_work_budget_is_exhausted() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: (0..2).map(|node| vec![node]).collect(),
    };
    let budget = MeshConstraintBudget::new(0);

    assert!(quotient
        .close_coordinate_roots(1, &[vec![]], Some(&budget))
        .is_none());
    assert!(budget.exhausted.get());
}

#[test]
fn quotient_coordinate_closure_does_not_rescan_assigned_roots() {
    const ROOT_COUNT: usize = 100;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(ROOT_COUNT),
        domains: repeated_domain(HashSet::from([0]), ROOT_COUNT),
        members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
    };
    let budget = MeshConstraintBudget::new(2 * ROOT_COUNT + 1);

    let assignment = quotient
        .close_coordinate_roots(1, &[], Some(&budget))
        .expect("forced coordinate closure");

    assert_eq!(quotient.root_count(), 1);
    assert_eq!(assignment.values().copied().collect::<Vec<_>>(), [0]);
    assert!(!budget.exhausted.get());
}

#[test]
fn quotient_incidence_closure_updates_face_degrees_incrementally() {
    const EDGE_COUNT: usize = 64;
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2 * EDGE_COUNT),
        domains: (0..EDGE_COUNT)
            .flat_map(|edge| [singleton(edge), singleton((edge + 1) % EDGE_COUNT)])
            .collect(),
        members: (0..2 * EDGE_COUNT).map(|node| vec![node]).collect(),
    };
    for edge in 0..EDGE_COUNT {
        quotient
            .merge(edge * 2 + 1, ((edge + 1) % EDGE_COUNT) * 2)
            .expect("adjacent boundary ports share one coordinate root");
    }
    let edge_candidates = vec![Vec::new(); EDGE_COUNT];
    let edge_faces = vec![[0, 0]; EDGE_COUNT];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(
        (0..EDGE_COUNT).collect(),
    )];
    let budget = MeshConstraintBudget::new(45_000);

    assert!(quotient
        .close_coordinate_roots_for_incidence_with_budget(
            EDGE_COUNT,
            &edge_candidates,
            &edge_faces,
            1,
            &domains,
            Some(&budget),
        )
        .is_some());
    assert!(!budget.exhausted.get());
}

#[test]
fn quotient_coordinate_closure_enforces_sparse_endpoint_membership_before_search() {
    const EDGE_COUNT: usize = 50;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(EDGE_COUNT * 2),
        domains: repeated_domain(HashSet::from([0, 1]), EDGE_COUNT * 2),
        members: (0..EDGE_COUNT * 2).map(|node| vec![node]).collect(),
    };
    let candidates = (0..EDGE_COUNT)
        .map(|edge| vec![[edge % 2, edge % 2]])
        .collect::<Vec<_>>();
    let budget = MeshConstraintBudget::new(1_000);

    let assignment = quotient
        .close_coordinate_roots(2, &candidates, Some(&budget))
        .expect("arc-consistent coordinate closure");

    assert_eq!(quotient.root_count(), 2);
    assert_eq!(
        assignment.values().copied().collect::<HashSet<_>>(),
        HashSet::from([0, 1])
    );
    assert!(!budget.exhausted.get());
}

#[test]
fn quotient_coordinate_closure_propagates_edge_arc_consistency_to_a_fixpoint() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: repeated_domain(HashSet::from([0, 1]), 6),
        members: (0..6).map(|node| vec![node]).collect(),
    };
    quotient.merge(1, 2).expect("shared relation root");
    let candidates = vec![vec![[0, 0], [1, 1]], vec![[0, 0]], vec![[1, 1]]];
    let budget = MeshConstraintBudget::new(1_000);

    let assignment = quotient
        .close_coordinate_roots(2, &candidates, Some(&budget))
        .expect("arc-consistent coordinate closure");

    assert_eq!(quotient.root_count(), 2);
    assert_eq!(assignment[&quotient.union.find(0)], 0);
    assert_eq!(assignment[&quotient.union.find(4)], 1);
    assert!(!budget.exhausted.get());
}

#[test]
fn quotient_coordinate_closure_forces_the_only_root_supporting_a_point() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0, 1], vec![0], vec![0, 1, 2], vec![0]]
            .into_iter()
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .collect(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let budget = MeshConstraintBudget::new(100);

    let assignment = quotient
        .close_coordinate_roots(3, &[Vec::new(), Vec::new()], Some(&budget))
        .expect("point-support-forced coordinate closure");

    assert_eq!(quotient.root_count(), 3);
    assert_eq!(assignment[&quotient.union.find(0)], 1);
    assert_eq!(assignment[&quotient.union.find(1)], 0);
    assert_eq!(assignment[&quotient.union.find(2)], 2);
    assert!(!budget.exhausted.get());
}

#[test]
fn quotient_coordinate_closure_rejects_a_coordinate_support_hall_conflict() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0, 1, 2, 3], vec![0, 1, 2, 3], vec![3], vec![3]]
            .into_iter()
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .collect(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let budget = MeshConstraintBudget::new(1_000);

    assert!(quotient
        .close_coordinate_roots(4, &[Vec::new(), Vec::new()], Some(&budget))
        .is_none());
    assert!(!budget.exhausted.get());
}

#[test]
fn coordinate_support_matching_exposes_essential_and_unsupported_hall_edges() {
    let supports = [vec![0, 1], vec![0, 1], vec![0, 1, 2], vec![2, 3]];
    let matching = crate::solve::matching::distinct_domain_matching_with_budget(
        supports.iter().map(Vec::as_slice),
        4,
        None,
        None,
    )
    .expect("coordinate support matching");

    assert_eq!(matching[2], 2);
    assert!(
        crate::solve::matching::distinct_domain_matching_with_budget(
            supports.iter().map(Vec::as_slice),
            4,
            None,
            Some(crate::solve::matching::MatchingEdgeConstraint::Exclude(
                2,
                matching[2]
            )),
        )
        .is_none()
    );

    let partitioned = [vec![0, 1, 2], vec![0, 1], vec![2, 3], vec![2, 3]];
    assert!(
        crate::solve::matching::distinct_domain_matching_with_budget(
            partitioned.iter().map(Vec::as_slice),
            4,
            None,
            Some(crate::solve::matching::MatchingEdgeConstraint::Require(
                0, 2
            )),
        )
        .is_none()
    );
    assert!(
        crate::solve::matching::distinct_domain_matching_with_budget(
            partitioned.iter().map(Vec::as_slice),
            4,
            None,
            Some(crate::solve::matching::MatchingEdgeConstraint::Require(
                0, 0
            )),
        )
        .is_some()
    );
}

#[test]
fn quotient_coordinate_closure_enforces_complete_face_degrees() {
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut open = MeshQuotient {
        union: UnionFind::new(4),
        domains: [singleton(0), singleton(1), singleton(0), singleton(2)].into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let candidates = vec![Vec::new(); 2];
    let edge_faces = [[0, 0]; 2];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1])];
    let budget = MeshConstraintBudget::new(1_000);
    open.merge(0, 2).expect("shared endpoint");

    assert!(open
        .close_coordinate_roots_for_incidence_with_budget(
            3,
            &candidates,
            &edge_faces,
            1,
            &domains,
            Some(&budget),
        )
        .is_none());

    let mut closed = MeshQuotient {
        union: UnionFind::new(6),
        domains: [
            singleton(0),
            singleton(1),
            singleton(1),
            singleton(2),
            singleton(2),
            singleton(0),
        ]
        .into(),
        members: (0..6).map(|node| vec![node]).collect(),
    };
    let candidates = vec![Vec::new(); 3];
    let edge_faces = [[0, 0]; 3];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];

    assert!(closed
        .close_coordinate_roots_for_incidence_with_budget(
            3,
            &candidates,
            &edge_faces,
            1,
            &domains,
            Some(&MeshConstraintBudget::new(1_000)),
        )
        .is_some());
}

#[test]
fn quotient_coordinate_closure_rejects_sealed_unordered_subcycles() {
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(8),
        domains: [
            singleton(0),
            singleton(1),
            singleton(1),
            singleton(0),
            Arc::new(HashSet::from([0, 2])),
            singleton(3),
            singleton(3),
            singleton(2),
        ]
        .into(),
        members: (0..8).map(|node| vec![node]).collect(),
    };
    let candidates = vec![vec![[0, 1]], vec![[0, 1]], vec![[2, 3]], vec![[2, 3]]];
    let edge_faces = [[0, 0]; 4];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2, 3])];
    let budget = MeshConstraintBudget::new(10_000);

    assert!(quotient
        .close_coordinate_roots_for_incidence_with_budget(
            4,
            &candidates,
            &edge_faces,
            1,
            &domains,
            Some(&budget),
        )
        .is_none());
    assert!(!budget.exhausted.get());
}

#[test]
fn quotient_coordinate_closure_enforces_ordered_face_cycles() {
    let singleton = |point| Arc::new(HashSet::from([point]));
    let quotient = || MeshQuotient {
        union: UnionFind::new(8),
        domains: [
            singleton(0),
            singleton(1),
            singleton(2),
            singleton(3),
            singleton(1),
            singleton(2),
            singleton(3),
            singleton(0),
        ]
        .into(),
        members: (0..8).map(|node| vec![node]).collect(),
    };
    let candidates = vec![Vec::new(); 4];
    let edge_faces = [[0, 0]; 4];
    let domain = |order: [usize; 4]| {
        [MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![order
                    .into_iter()
                    .map(|edge| MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: None,
                    })
                    .collect()],
            },
        ])]
    };

    assert!(quotient()
        .close_coordinate_roots_for_incidence_with_budget(
            4,
            &candidates,
            &edge_faces,
            1,
            &domain([0, 1, 2, 3]),
            Some(&MeshConstraintBudget::new(10_000)),
        )
        .is_none());
    assert!(quotient()
        .close_coordinate_roots_for_incidence_with_budget(
            4,
            &candidates,
            &edge_faces,
            1,
            &domain([0, 2, 1, 3]),
            Some(&MeshConstraintBudget::new(10_000)),
        )
        .is_some());

    let fixed = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![[0, 2, 1, 3]
                .into_iter()
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: Some(edge == 2),
                })
                .collect()],
        },
    ])];
    assert!(quotient()
        .close_coordinate_roots_for_incidence_with_budget(
            4,
            &candidates,
            &edge_faces,
            1,
            &fixed,
            Some(&MeshConstraintBudget::new(10_000)),
        )
        .is_none());
}

#[test]
fn quotient_closes_independent_coordinate_components_with_local_budgets() {
    const COMPONENT_COUNT: usize = 100;
    let point_count = COMPONENT_COUNT * 3;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(COMPONENT_COUNT * 6),
        domains: (0..COMPONENT_COUNT)
            .flat_map(|component| {
                let points = Arc::new((component * 3..component * 3 + 3).collect::<HashSet<_>>());
                std::iter::repeat_n(points, 6)
            })
            .collect(),
        members: (0..COMPONENT_COUNT * 6).map(|node| vec![node]).collect(),
    };
    let mut candidates = Vec::new();
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        let point = component * 3;
        quotient
            .merge(node + 1, node + 2)
            .expect("shared first corner");
        quotient
            .merge(node + 3, node + 4)
            .expect("shared second corner");
        candidates.extend([
            vec![[point, point + 1]],
            vec![[point + 1, point + 2]],
            vec![[point, point + 2]],
        ]);
    }

    let assignment = quotient
        .close_coordinate_roots(point_count, &candidates, None)
        .expect("independent coordinate closures");

    assert_eq!(quotient.root_count(), point_count);
    assert_eq!(assignment.len(), point_count);
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        assert_eq!(quotient.union.find(node), quotient.union.find(node + 5));
    }
}

#[test]
fn quotient_counts_global_face_incidence_once_across_coordinate_components() {
    const COMPONENT_COUNT: usize = 40;
    let point_count = COMPONENT_COUNT * 3;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(COMPONENT_COUNT * 6),
        domains: (0..COMPONENT_COUNT)
            .flat_map(|component| {
                let points = Arc::new((component * 3..component * 3 + 3).collect::<HashSet<_>>());
                std::iter::repeat_n(points, 6)
            })
            .collect(),
        members: (0..COMPONENT_COUNT * 6).map(|node| vec![node]).collect(),
    };
    let mut candidates = Vec::new();
    let mut edge_faces = Vec::new();
    let mut domains = Vec::new();
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        let point = component * 3;
        let first_edge = candidates.len();
        quotient
            .merge(node + 1, node + 2)
            .expect("shared first corner");
        quotient
            .merge(node + 3, node + 4)
            .expect("shared second corner");
        candidates.extend([
            vec![[point, point + 1]],
            vec![[point + 1, point + 2]],
            vec![[point, point + 2]],
        ]);
        edge_faces.extend([[component, component]; 3]);
        domains.push(MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![(first_edge..first_edge + 3)
                    .map(|edge| MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: None,
                    })
                    .collect()],
            },
        ]));
    }
    let budget = MeshConstraintBudget::new(20_000);

    assert!(quotient
        .close_coordinate_roots_for_incidence_with_budget(
            point_count,
            &candidates,
            &edge_faces,
            COMPONENT_COUNT,
            &domains,
            Some(&budget),
        )
        .is_some());
    assert!(!budget.exhausted.get());
}

#[test]
fn quotient_closure_does_not_budget_forced_component_depth() {
    const ROOT_COUNT: usize = 10_000;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(ROOT_COUNT),
        domains: repeated_domain(HashSet::from([0]), ROOT_COUNT),
        members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
    };
    let candidates = vec![vec![[0, 0]]; ROOT_COUNT / 2];

    let assignment = quotient
        .close_coordinate_roots(1, &candidates, None)
        .expect("forced coordinate component");

    assert_eq!(quotient.root_count(), 1);
    assert_eq!(assignment.values().copied().collect::<Vec<_>>(), [0]);
}

#[test]
fn quotient_does_not_guess_an_ambiguous_coordinate_closure() {
    let all = Arc::new(HashSet::from([0, 1]));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![all.clone(); 4],
        members: (0..4).map(|node| vec![node]).collect(),
    };
    quotient.merge(1, 2).expect("shared middle corner");

    assert!(quotient
        .close_coordinate_roots(2, &[vec![[0, 1]], vec![[0, 1]]], None)
        .is_none());
    assert_eq!(quotient.root_count(), 3);
}

#[test]
fn quotient_closure_requires_every_coordinate_row_in_a_domain() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    quotient.merge(1, 2).expect("shared endpoint");

    assert!(quotient
        .close_coordinate_roots(2, &[vec![[0, 0]], vec![[0, 0]]], None)
        .is_none());
    assert_eq!(quotient.root_count(), 3);
}

#[test]
fn quotient_accepts_diagonal_domain_for_closed_edge() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: vec![Arc::new(HashSet::from([2])), Arc::new(HashSet::from([2]))],
        members: vec![vec![0], vec![1]],
    };
    quotient.merge(0, 1).expect("closed endpoint merge");
    assert!(quotient.edge_domains_viable(&[vec![[2, 2]]]));
    assert!(!quotient.edge_domains_viable(&[vec![[1, 2]]]));
}

#[test]
fn quotient_point_assignment_accepts_a_closed_diagonal_edge() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: vec![vec![0], vec![1]],
    };
    let root = quotient.merge(0, 1).expect("closed endpoint merge");

    assert_eq!(
        quotient.point_assignment(1, &[vec![[0, 0]]], None),
        Some(HashMap::from([(root, 0)]))
    );
}

#[test]
fn quotient_retains_diagonal_pairs_until_ports_are_merged() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: vec![
            Arc::new(HashSet::from([1, 2])),
            Arc::new(HashSet::from([1, 2])),
        ],
        members: vec![vec![0], vec![1]],
    };

    assert!(quotient.edge_domains_viable(&[vec![[2, 2]]]));
    assert_eq!(
        quotient.domains,
        vec![Arc::new(HashSet::from([2])), Arc::new(HashSet::from([2]))]
    );
    quotient.merge(0, 1).expect("closed endpoint merge");
    assert!(quotient.edge_domains_viable(&[vec![[2, 2]]]));
}

#[test]
fn closed_edge_is_a_single_coedge_boundary_on_each_incident_face() {
    let topology = reconstruct_incidence(
        vec![EdgeRow {
            kind: 0,
            handles: vec![7, 7],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }],
        vec![[1.0, 0.0, 0.0]],
        &[[0, 1]],
        &[[0, 0]],
        2,
    )
    .expect("closed radial edge");
    assert!(topology
        .faces()
        .iter()
        .all(|face| face.boundaries.len() == 1 && face.boundaries[0].coedges.len() == 1));
    assert_ne!(
        topology.faces()[0].boundaries[0].coedges[0].reversed,
        topology.faces()[1].boundaries[0].coedges[0].reversed
    );
}

#[test]
fn duplicate_face_reference_slot_is_completed_by_face_closure() {
    let rows = (0..3)
        .map(|handle| EdgeRow {
            kind: 0,
            handles: vec![handle],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let faces = complete_duplicate_face_slots(
        &rows,
        &[[0, 1], [0, 1], [0, 0]],
        &[[0, 1], [1, 2], [2, 0]],
        2,
        None,
        Some(&[]),
    )
    .expect("unique face-closing slot assignment");

    assert_eq!(faces, vec![[0, 1], [0, 1], [0, 1]]);
}

#[test]
fn duplicate_face_completion_keeps_sparse_endpoint_identities() {
    let rows = (0..3)
        .map(|handle| EdgeRow {
            kind: 0,
            handles: vec![handle],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let faces = complete_duplicate_face_slots(
        &rows,
        &[[0, 1], [0, 1], [0, 0]],
        &[
            [usize::MAX, usize::MAX - 1],
            [usize::MAX - 1, 0],
            [0, usize::MAX],
        ],
        2,
        None,
        Some(&[]),
    )
    .expect("sparse endpoint identities");

    assert_eq!(faces, vec![[0, 1], [0, 1], [0, 1]]);
}

#[test]
fn vertex_table_rejects_unbacked_extended_count_before_allocation() {
    assert!(parse_vertex_table(&[0x01, 0x06, 0xff, 0xff, 0xff, 0xff, 0xff], 0).is_none());
}

#[test]
fn vertex_table_rejects_an_overflowing_start_offset() {
    assert!(parse_vertex_table(&[], usize::MAX).is_none());
}

#[test]
fn mesh_assignment_endpoint_cycles_reject_crossed_edge_order() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: None,
    };
    let assignment = |edges: &[usize]| MeshFaceBoundaryAssignment {
        boundaries: vec![edges.iter().copied().map(use_).collect()],
    };
    let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]];

    assert!(mesh_assignment_endpoint_cycles_viable(
        &assignment(&[0, 1, 2, 3]),
        &candidates,
    ));
    assert!(!mesh_assignment_endpoint_cycles_viable(
        &assignment(&[0, 2, 1, 3]),
        &candidates,
    ));
}

#[test]
fn mesh_face_endpoint_configurations_preserve_pair_correlation() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..4)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 0,
                reversed: None,
            })
            .collect()],
    };
    let candidates = vec![
        vec![[0, 1]],
        vec![[1, 2], [2, 3]],
        vec![[2, 3], [1, 2]],
        vec![[3, 0]],
    ];
    let budget = MeshConstraintBudget::new(4_096);
    let configurations = mesh_face_endpoint_configurations(
        std::slice::from_ref(&assignment),
        &candidates,
        &[None; 4],
        &budget,
    )
    .expect("bounded face configurations");

    assert_eq!(
        configurations,
        vec![vec![(0, [0, 1]), (1, [1, 2]), (2, [2, 3]), (3, [0, 3])]],
    );

    let exhausted = MeshConstraintBudget::new(1);
    assert!(
        mesh_face_endpoint_configurations(&[assignment], &candidates, &[None; 4], &exhausted)
            .is_none()
    );
    assert!(exhausted.exhausted.get());
}

#[test]
fn mesh_assignment_endpoint_cycles_preserve_unconstrained_boundaries() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 0,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 0,
                end: 0,
                reversed: None,
            },
        ]],
    };
    assert!(mesh_assignment_endpoint_cycles_viable(
        &assignment,
        &[vec![[0, 1]], Vec::new()],
    ));
}

#[test]
fn mesh_endpoint_pair_support_propagates_across_incident_faces() {
    let assignment = |edges: &[usize]| MeshFaceBoundaryAssignment {
        boundaries: vec![edges
            .iter()
            .copied()
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 0,
                reversed: None,
            })
            .collect()],
    };
    let mut assignments = vec![
        vec![assignment(&[0, 1, 2])],
        vec![assignment(&[0, 3, 4]), assignment(&[0, 5, 6])],
    ];
    let mut candidates = vec![
        vec![[0, 1], [0, 3]],
        vec![[1, 2]],
        vec![[2, 0]],
        vec![[1, 4]],
        vec![[4, 0]],
        vec![[3, 5]],
        vec![[5, 0]],
    ];

    assert!(prune_mesh_endpoint_pair_support(
        &mut assignments,
        &mut candidates,
    ));
    assert_eq!(candidates[0], vec![[0, 1]]);
    assert_eq!(assignments[1], vec![assignment(&[0, 3, 4])]);
}

#[test]
fn mesh_endpoint_pair_support_does_not_treat_budget_exhaustion_as_a_contradiction() {
    let mut assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    }]];
    let mut candidates = vec![vec![[0, 0]]];

    assert!(prune_mesh_endpoint_pair_support_with_limit(
        &mut assignments,
        &mut candidates,
        0,
    ));
}

#[test]
fn independent_duplicate_face_slots_have_one_canonical_search_order() {
    let rows = (0..12)
        .map(|edge| EdgeRow {
            kind: 1,
            handles: vec![edge],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let serialized = vec![[0, 0]; rows.len()];
    let points = (0..rows.len())
        .map(|point| [point, point])
        .collect::<Vec<_>>();

    let completed = complete_duplicate_face_slots(&rows, &serialized, &points, 2, None, None)
        .expect("independent closed edges have one face completion");
    assert_eq!(completed, vec![[0, 1]; rows.len()]);
}

#[test]
fn duplicate_face_slot_requires_one_joint_carrier_and_mesh_assignment() {
    let serialized = [[0, 0], [0, 1], [1, 1]];
    let allowed = [vec![1, 2], Vec::new(), vec![0, 2]];
    let resolved = unique_duplicate_face_assignment(&serialized, &allowed, 3, |faces| {
        faces == [[0, 2], [0, 1], [1, 0]]
    })
    .expect("one complete assignment");
    assert_eq!(resolved, [[0, 2], [0, 1], [1, 0]]);

    assert!(unique_duplicate_face_assignment(&serialized, &allowed, 3, |_| true).is_none());
    assert!(unique_duplicate_face_assignment(
        &serialized,
        &[vec![3], Vec::new(), vec![0]],
        3,
        |_| true,
    )
    .is_none());
}

#[test]
fn duplicate_face_slots_do_not_budget_forced_assignments() {
    const EDGE_COUNT: usize = 5_000;
    let serialized = vec![[0, 0]; EDGE_COUNT];
    let allowed = vec![vec![1, 1]; EDGE_COUNT];

    let resolved = unique_duplicate_face_assignment(&serialized, &allowed, 2, |_| true)
        .expect("forced duplicate-face assignments");

    assert_eq!(resolved, vec![[0, 1]; EDGE_COUNT]);
}

#[test]
fn face_endpoint_candidates_require_one_closed_local_cycle() {
    let faces = [[0, 1], [0, 2], [0, 3]];
    assert!(face_endpoint_candidates_close(
        &faces,
        &[vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]],
        0,
    ));
    assert!(!face_endpoint_candidates_close(
        &faces,
        &[vec![[0, 1]], vec![[1, 2]], vec![[3, 4]]],
        0,
    ));
}

#[test]
fn face_endpoint_candidates_do_not_budget_fixed_cycle_size() {
    const EDGE_COUNT: usize = 65_537;
    let faces = vec![[0, 0]; EDGE_COUNT];
    let candidates = (0..EDGE_COUNT)
        .map(|edge| vec![[edge, (edge + 1) % EDGE_COUNT]])
        .collect::<Vec<_>>();

    assert!(face_endpoint_candidates_close(&faces, &candidates, 0));
}

#[test]
fn counted_edge_arities_are_bounded_by_remaining_bytes() {
    let oversized_row = [0x01, 0x01, 0x01, 0x02, 0xff, 0xff, 0xff, 0xff, 0xff];
    assert!(parse_edge_tables_scoped_at(&oversized_row, 0).is_none());
    assert!(parse_fbb_edge_tables_width(&oversized_row, 0, 3).is_none());
}

#[test]
fn trim_primitive_counts_are_bounded_by_remaining_bytes() {
    let oversized_primitives = [
        0x01, 0x46, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x00,
        0x00,
    ];
    assert!(parse_trim_record(&oversized_primitives, 0, 2).is_none());
}

#[test]
fn duplicate_face_completion_rejects_out_of_range_faces() {
    let rows = vec![EdgeRow {
        kind: 0,
        handles: vec![0, 1],
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    assert!(complete_duplicate_face_slots(&rows, &[[0, 2]], &[[0, 1]], 2, None, None,).is_none());
}

#[test]
fn exact_mesh_occurrences_complete_duplicate_face_slot() {
    let run = |edge, face| MeshEdgeRun {
        edge,
        face,
        cycle: 0,
        start: 0,
        segment_count: 1,
        reversed: false,
    };
    let faces = resolve_edge_faces_from_runs(
        &[[1, 1], [2, 2], [3, 4]],
        &[run(0, 1), run(0, 5), run(1, 2), run(2, 3), run(2, 4)],
    )
    .expect("consistent exact face occurrences");

    assert_eq!(faces, vec![[1, 5], [2, 2], [3, 4]]);
}

#[test]
fn equivalent_edge_rows_share_one_incidence_assignment_gauge() {
    let rows = vec![
        EdgeRow {
            kind: 0,
            handles: vec![0],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 0,
            handles: vec![1],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 0,
            handles: vec![2, 3],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 0,
            handles: vec![4, 5],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
    ];
    let faces = complete_duplicate_face_slots(
        &rows,
        &[[0, 1], [0, 1], [2, 2], [2, 2]],
        &[[0, 1], [1, 2], [2, 0], [0, 2]],
        3,
        Some(&[0, 1, 2, 2]),
        None,
    )
    .expect("one assignment modulo equivalent edge rows");

    let mut assigned = [faces[2][1], faces[3][1]];
    assigned.sort_unstable();
    assert_eq!(assigned, [0, 1]);
}

#[test]
fn endpoint_ports_reject_contradictory_pair_constraints() {
    let ports = [[10, 11], [11, 12], [12, 10]];
    let pairs = [Some([0, 1]), Some([1, 2]), Some([0, 3])];
    assert_eq!(propagate_edge_port_points(&ports, &pairs), None);
}

#[test]
fn native_edge_identities_bind_ambiguous_coordinate_pairs() {
    let ports = [[10, 11], [12, 13], [10, 12], [11, 13]];
    let candidates = [vec![[0, 1]], vec![[2, 3]], vec![[0, 2]], vec![[1, 3]]];
    assert_eq!(
        bind_edge_port_candidates(&ports, &candidates),
        Some(vec![[0, 1], [2, 3], [0, 2], [1, 3]])
    );
}

#[test]
fn native_edge_identities_reject_multiple_coordinate_bijections() {
    let ports = [[10, 11]];
    let candidates = [vec![[0, 1], [2, 3]]];
    assert_eq!(bind_edge_port_candidates(&ports, &candidates), None);
}

#[test]
fn native_edge_identities_preserve_endpoint_equality() {
    assert_eq!(
        bind_edge_port_candidates(&[[10, 11]], &[vec![[0, 0]]]),
        None
    );
    assert_eq!(
        bind_edge_port_candidates(&[[10, 10]], &[vec![[0, 1]]]),
        None
    );
    assert_eq!(
        bind_edge_port_candidates(&[[10, 10]], &[vec![[0, 0]]]),
        Some(vec![[0, 0]])
    );
}

#[test]
fn native_edge_identities_bind_independent_components_with_local_budgets() {
    const COMPONENT_COUNT: usize = 100;
    let ports = (0..COMPONENT_COUNT)
        .map(|component| {
            let port = u32::try_from(component * 2).expect("bounded port identity");
            [port, port + 1]
        })
        .collect::<Vec<_>>();
    let candidates = (0..COMPONENT_COUNT)
        .map(|component| vec![[component * 2, component * 2 + 1]])
        .collect::<Vec<_>>();

    let solution =
        bind_edge_port_candidates(&ports, &candidates).expect("independent port components");

    assert_eq!(solution.len(), COMPONENT_COUNT);
    assert!(solution
        .iter()
        .zip(&candidates)
        .all(|(pair, candidates)| same_unordered_pair(*pair, candidates[0])));
}

#[test]
fn native_edge_identities_do_not_charge_forced_chain_depth() {
    const EDGE_COUNT: usize = 10_000;
    let ports = (0..EDGE_COUNT)
        .map(|edge| {
            let port = u32::try_from(edge).expect("bounded port identity");
            [port, port + 1]
        })
        .collect::<Vec<_>>();
    let candidates = (0..EDGE_COUNT)
        .map(|edge| vec![[edge, edge + 1]])
        .collect::<Vec<_>>();

    let solution =
        bind_edge_port_candidates(&ports, &candidates).expect("forced connected port chain");

    assert_eq!(
        solution,
        candidates.into_iter().flatten().collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_coordinate_rows_have_one_geometric_bijection() {
    let domains = [HashSet::from([0, 1]), HashSet::from([0, 1])];
    assert_eq!(
        unique_coordinate_bijection(&domains, &[[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]]),
        Some(vec![0, 1])
    );
}

#[test]
fn forced_coordinate_bijection_has_no_recursive_depth_limit() {
    const POINT_COUNT: usize = 10_000;
    let domains = (0..POINT_COUNT)
        .map(|point| HashSet::from([point]))
        .collect::<Vec<_>>();
    let points = (0..POINT_COUNT)
        .map(|point| {
            [
                f64::from(u32::try_from(point).expect("bounded point index")),
                0.0,
                0.0,
            ]
        })
        .collect::<Vec<_>>();

    assert_eq!(
        unique_coordinate_bijection(&domains, &points),
        Some((0..POINT_COUNT).collect())
    );
}

#[test]
fn coordinate_bijection_respects_duplicate_class_capacity() {
    let domains = [
        HashSet::from([0, 2]),
        HashSet::from([0, 1]),
        HashSet::from([0, 1]),
    ];
    let points = [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];

    assert_eq!(
        unique_coordinate_bijection(&domains, &points),
        Some(vec![2, 0, 1])
    );
}

#[test]
fn distinct_coordinate_bijections_remain_ambiguous() {
    let domains = [HashSet::from([0, 1]), HashSet::from([0, 1])];
    assert_eq!(
        unique_coordinate_bijection(&domains, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
        None
    );
}

/// Record-decoder tests migrated from the crate-level `tests` module.
mod record_decoders {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::collections::{HashMap, HashSet};

    use crate::families::standard::records::{StandardFaceBounds, StandardSurfaceRecord};
    use crate::tests::{
        a8_freeform_curve_stream, a8_surface_stream, append_b5_record, b5_closed_triangle_stream,
        le_f32, le_f64, standard_quad_topology_stream,
    };

    #[test]
    fn standard_torus_major_sign_selects_the_axis_hemisphere() {
        let mut bytes = vec![0x00, 0x33, 0x38];
        for value in [0.0_f32, 0.0, 7.0, 0.0, 0.0, -20.0, 5.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        let surface = crate::families::standard::records::decode_curved(
            &bytes,
            &crate::families::standard::records::SurfacePrefix {
                pos: 0,
                target: 0,
                kind: 0x38,
            },
        )
        .expect("signed torus carrier");
        let SurfaceGeometry::Torus {
            axis,
            major_radius,
            minor_radius,
            ..
        } = surface
        else {
            panic!("torus geometry");
        };
        assert_eq!(axis, Vector3::new(0.0, 0.0, -1.0));
        assert_eq!(major_radius, 20.0);
        assert_eq!(minor_radius, 5.0);
    }

    #[test]
    fn standard_analytic_carriers_have_no_model_size_cutoff() {
        for (kind, values, expected_radius) in [
            (0x35, vec![0.0_f32, 0.0, 0.0, 2_000_000.0], 2_000_000.0),
            (
                0x33,
                vec![0.0_f32, 0.0, 0.0, 0.0, 0.0, 2_000_000.0],
                2_000_000.0,
            ),
        ] {
            let mut bytes = vec![0x00, 0x33, kind];
            for value in values {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            let surface = crate::families::standard::records::decode_curved(
                &bytes,
                &crate::families::standard::records::SurfacePrefix {
                    pos: 0,
                    target: 0,
                    kind,
                },
            )
            .expect("large analytic carrier");
            let (SurfaceGeometry::Sphere { radius, .. } | SurfaceGeometry::Cylinder { radius, .. }) =
                surface
            else {
                panic!("expected sphere or cylinder");
            };
            assert_eq!(radius, expected_radius);
        }

        let mut bytes = vec![0x00, 0x33, 0x38];
        for value in [0.0_f32, 0.0, 0.0, 0.0, 0.0, 2_000_000.0, 1_500_000.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        assert!(matches!(
            crate::families::standard::records::decode_curved(
                &bytes,
                &crate::families::standard::records::SurfacePrefix {
                    pos: 0,
                    target: 0,
                    kind: 0x38,
                },
            ),
            Some(SurfaceGeometry::Torus {
                major_radius: 2_000_000.0,
                minor_radius: 1_500_000.0,
                ..
            })
        ));
    }

    #[test]
    fn standard_f32_frames_canonicalize_to_orthonormal_ir() {
        let component = (0.5_f64 + 4.0e-6).sqrt() as f32;
        let mut bytes = vec![0x00, 0x33, 0x33];
        for value in [0.0_f32, 0.0, 0.0, component, component, 5.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        let surface = crate::families::standard::records::decode_curved(
            &bytes,
            &crate::families::standard::records::SurfacePrefix {
                pos: 0,
                target: 0,
                kind: 0x33,
            },
        )
        .expect("near-unit cylinder carrier");
        let SurfaceGeometry::Cylinder {
            axis,
            ref_direction,
            ..
        } = surface
        else {
            panic!("cylinder geometry");
        };
        assert!((axis.norm() - 1.0).abs() < 1.0e-12);
        assert!((ref_direction.norm() - 1.0).abs() < 1.0e-12);
        assert!(axis.dot(ref_direction).abs() < 1.0e-12);

        let plane = crate::families::standard::records::decode_plane(
            &crate::families::standard::records::PlaneParams {
                target: 0,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 0.999_999),
            },
        )
        .expect("near-unit plane carrier");
        let SurfaceGeometry::Plane { normal, u_axis, .. } = plane else {
            panic!("plane geometry");
        };
        assert!((normal.norm() - 1.0).abs() < 1.0e-12);
        assert!((u_axis.norm() - 1.0).abs() < 1.0e-12);
        assert!(normal.dot(u_axis).abs() < 1.0e-12);
        assert!(crate::families::standard::records::decode_plane(
            &crate::families::standard::records::PlaneParams {
                target: 0,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 0.0),
            },
        )
        .is_none());
    }

    #[test]
    fn standard_topology_recovers_a_quad_boundary_and_port_vertices() {
        let topology =
            crate::families::standard::fbb::parse_standard(&standard_quad_topology_stream())
                .expect("valid standard topology");

        assert_eq!(topology.face_count(), 1);
        assert_eq!(topology.edge_rows().len(), 4);
        assert_eq!(topology.vertex_points().len(), 4);
        let boundary = &topology.faces()[0].boundaries[0];
        assert_eq!(boundary.coedges.len(), 4);
        assert_eq!(
            boundary
                .coedges
                .iter()
                .map(|use_| use_.edge_row)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert!(boundary.coedges.iter().all(|use_| !use_.reversed));
        assert_eq!(topology.logical_vertex_count(), 4);
    }

    #[test]
    fn standard_counted_vertex_table_excludes_incidental_markers() {
        let mut bytes = standard_quad_topology_stream();
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        bytes.extend_from_slice(&le_f32(10.0));
        bytes.extend_from_slice(&le_f32(20.0));
        bytes.extend_from_slice(&le_f32(30.0));

        assert_eq!(
            crate::families::standard::fbb::standard_vertex_points(&bytes)
                .expect("required invariant")
                .len(),
            4
        );
    }

    #[test]
    fn standard_topology_accepts_delimiters_between_counted_edge_tables() {
        let mut bytes = standard_quad_topology_stream();
        let header = bytes
            .windows(3)
            .position(|window| window == [0x01, 0x01, 0x04])
            .expect("edge table header");
        bytes[header + 2] = 2;
        let second_table = header + 3 + 2 * 8;
        bytes.splice(
            second_table..second_table,
            [
                0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02,
            ],
        );

        let topology =
            crate::families::standard::fbb::parse_standard(&bytes).expect("two edge tables");
        assert_eq!(
            topology
                .edge_rows()
                .iter()
                .map(|row| row.kind)
                .collect::<Vec<_>>(),
            vec![1, 1, 2, 2]
        );
        assert_eq!(
            crate::solve::missing_edge::standard_edge_rows(&bytes)
                .expect("edge rows")
                .iter()
                .map(|row| row.kind)
                .collect::<Vec<_>>(),
            vec![1, 1, 2, 2]
        );
    }

    #[test]
    fn fbb_topology_reads_u24_mesh_and_edge_handles() {
        let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 10, 0, 0, 0, 10];
        for handle in [
            1u32, 0x01_0010, 0x01_0011, 0x01_0012, 0x01_0013, 0x01_0014, 0x01_0015, 0x01_0016,
            0x01_0017, 0x01_0010,
        ] {
            bytes.extend_from_slice(&handle.to_be_bytes()[1..]);
        }
        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
        for (kind, rows) in [
            (
                1,
                [
                    [0x01_0010u32, 0x01_0011],
                    [0x01_0011, 0x01_0012],
                    [0x01_0012, 0x01_0013],
                    [0x01_0013, 0x01_0014],
                ],
            ),
            (
                2,
                [
                    [0x01_0014u32, 0x01_0015],
                    [0x01_0015, 0x01_0016],
                    [0x01_0016, 0x01_0017],
                    [0x01_0017, 0x01_0010],
                ],
            ),
        ] {
            bytes.extend_from_slice(&[0x01, kind, 4]);
            for row in rows {
                bytes.extend_from_slice(&[0x02, 2]);
                for handle in row {
                    bytes.extend_from_slice(&handle.to_be_bytes()[1..]);
                }
            }
            bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
        }
        bytes.extend_from_slice(&[0x01, 0x06, 4]);
        for index in 0..4 {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [index as f32, 0.0, 0.0] {
                bytes.extend_from_slice(&le_f32(value));
            }
        }

        let topology =
            crate::families::standard::topology::parse_fbb(&bytes).expect("valid FBB topology");
        assert_eq!(topology.edge_rows()[0].handles, vec![0x01_0010, 0x01_0011]);
        assert_eq!(topology.faces()[0].boundaries[0].coedges.len(), 8);
        assert_eq!(topology.logical_vertex_count(), 8);
        assert_eq!(topology.vertex_points().len(), 4);
        let table_ports = crate::solve::missing_edge::standard_edge_port_identities(&bytes)
            .expect("scoped FBB ports");
        assert_eq!(table_ports[0][1], table_ports[1][0]);
        assert_eq!(table_ports[1][1], table_ports[2][0]);
        assert_eq!(table_ports[2][1], table_ports[3][0]);
        assert_ne!(table_ports[3][1], table_ports[4][0]);
        assert_eq!(table_ports[4][1], table_ports[5][0]);
        assert_eq!(table_ports[5][1], table_ports[6][0]);
        assert_eq!(table_ports[6][1], table_ports[7][0]);
        let native_ports = [
            [100, 101],
            [101, 102],
            [102, 103],
            [103, 100],
            [100, 101],
            [101, 102],
            [102, 103],
            [103, 100],
        ];
        let quotient = crate::families::standard::topology::parse_fbb_with_native_vertices(
            &bytes,
            &native_ports,
        )
        .expect("native endpoint quotient");
        assert_eq!(quotient.logical_vertex_count(), 4);
        assert_eq!(
            quotient.edge_vertices().expect("edge vertices"),
            native_ports.map(|pair| pair
                .map(|identity| usize::try_from(identity - 100).expect("required invariant")))
        );
        assert_eq!(
            quotient
                .bind_vertex_points(&[
                    [0, 1],
                    [1, 2],
                    [2, 3],
                    [3, 0],
                    [0, 1],
                    [1, 2],
                    [2, 3],
                    [3, 0],
                ])
                .expect("coordinate binding"),
            vec![0, 1, 2, 3]
        );
        let runs =
            crate::solve::missing_edge::standard_mesh_edge_runs(&bytes).expect("u24 edge runs");
        assert_eq!(runs.len(), 8);
        assert!(runs.iter().all(|run| run.segment_count == 1));
        assert_eq!(
            crate::families::standard::fbb::standard_vertex_points(&bytes)
                .expect("required invariant")
                .len(),
            4
        );
    }

    #[test]
    fn fbb_topology_reads_u16_mesh_and_edge_handles() {
        let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 6, 0, 0, 0, 6];
        for handle in [1u16, 0x1010, 0x1011, 0x1012, 0x1013, 0x1010] {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
        for (kind, rows) in [
            (1, [[0x1010u16, 0x1011], [0x1011, 0x1012]]),
            (2, [[0x1012u16, 0x1013], [0x1013, 0x1010]]),
        ] {
            bytes.extend_from_slice(&[0x01, kind, 2]);
            for row in rows {
                bytes.extend_from_slice(&[0x02, 2]);
                for handle in row {
                    bytes.extend_from_slice(&handle.to_be_bytes());
                }
            }
            bytes.extend_from_slice(&[0x10, 0xa4, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
        }
        bytes.extend_from_slice(&[0x01, 0x06, 4]);
        for index in 0..4 {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [index as f32, 0.0, 0.0] {
                bytes.extend_from_slice(&le_f32(value));
            }
        }

        let topology =
            crate::families::standard::topology::parse_fbb(&bytes).expect("valid u16 FBB topology");
        assert_eq!(topology.edge_rows()[0].handles, vec![0x1010, 0x1011]);
        assert_eq!(topology.faces()[0].boundaries[0].coedges.len(), 4);
        assert_eq!(topology.vertex_points().len(), 4);
    }

    #[test]
    fn fbb_topology_reads_u8_mesh_and_edge_handles() {
        let mut bytes = vec![0x01, 0x49, 0x02, 0xff, 6, 0, 0, 0];
        for value in [0.0f32, 0.0, 1.0] {
            bytes.extend_from_slice(&le_f32(value));
        }
        bytes.extend_from_slice(&[0x10, 0x11, 0x12, 0x10, 0x12, 0x13]);
        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
        for (kind, rows) in [
            (1, [[0x10u8, 0x11], [0x11, 0x12]]),
            (2, [[0x12u8, 0x13], [0x13, 0x10]]),
        ] {
            bytes.extend_from_slice(&[0x01, kind, 2]);
            for row in rows {
                bytes.extend_from_slice(&[0x02, 2]);
                bytes.extend_from_slice(&row);
            }
            bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
        }
        bytes.extend_from_slice(&[0x01, 0x06, 4]);
        for index in 0..4 {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [index as f32, 0.0, 0.0] {
                bytes.extend_from_slice(&le_f32(value));
            }
        }

        let topology =
            crate::families::standard::topology::parse_fbb(&bytes).expect("valid u8 FBB topology");
        assert_eq!(topology.edge_rows()[0].handles, vec![0x10, 0x11]);
        assert_eq!(topology.faces()[0].boundaries[0].coedges.len(), 4);
        assert_eq!(topology.vertex_points().len(), 4);
        assert_eq!(
            crate::families::standard::fbb::standard_face_frame_vectors(&bytes),
            [Some([0.0, 0.0, 1.0])]
        );
    }

    #[test]
    fn fbb_topology_requires_one_u16_delimiter_family() {
        let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 6, 0, 0, 0, 6];
        for handle in [1u16, 0x1010, 0x1011, 0x1012, 0x1013, 0x1010] {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
        for (kind, family, rows) in [
            (1, 0x94, [[0x1010u16, 0x1011], [0x1011, 0x1012]]),
            (2, 0xa4, [[0x1012u16, 0x1013], [0x1013, 0x1010]]),
        ] {
            bytes.extend_from_slice(&[0x01, kind, 2]);
            for row in rows {
                bytes.extend_from_slice(&[0x02, 2]);
                for handle in row {
                    bytes.extend_from_slice(&handle.to_be_bytes());
                }
            }
            bytes.extend_from_slice(&[0x10, family, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
        }
        bytes.extend_from_slice(&[0x01, 0x06, 0]);

        assert!(crate::families::standard::topology::parse_fbb(&bytes).is_none());
    }

    #[test]
    fn standard_vertex_roster_preserves_native_identity_order() {
        let mut bytes = vec![0x54, 0x02, 0x00, 0x00, 0, 0, 0, 0xff];
        for identity in [0x01_0203u32, 0x01_0206, 0x01_0209] {
            bytes.push(0x54);
            bytes.extend_from_slice(&identity.to_le_bytes()[..3]);
            bytes.extend_from_slice(&[0, 0, 0]);
        }
        bytes.extend_from_slice(&[0x54, 0x01, 0x00, 0x00, 0, 0, 0]);

        assert_eq!(
            crate::families::standard::records::standard_vertex_roster(&bytes, 3),
            Some(vec![0x01_0203, 0x01_0206, 0x01_0209])
        );
    }

    #[test]
    fn standard_topology_matches_edge_interiors_and_collapses_endpoint_ports() {
        let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 11, 0, 0, 0, 11];
        for handle in [1u16, 10, 11, 12, 13, 14, 15, 16, 17, 18, 10] {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
        bytes.extend_from_slice(&[0x01, 0x01, 3]);
        for row in [
            [101u16, 12, 11, 100],
            [101, 14, 15, 102],
            [102, 17, 18, 100],
        ] {
            bytes.extend_from_slice(&[0x02, 4]);
            for handle in row {
                bytes.extend_from_slice(&handle.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x01, 0x06, 3]);
        for index in 0..3 {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [index as f32, 0.0, 0.0] {
                bytes.extend_from_slice(&le_f32(value));
            }
        }

        let topology =
            crate::families::standard::fbb::parse_standard(&bytes).expect("interior-run topology");
        let coedges = &topology.faces()[0].boundaries[0].coedges;
        assert_eq!(
            coedges.iter().map(|use_| use_.edge_row).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(coedges[0].reversed);
        assert_eq!(topology.logical_vertex_count(), 3);
    }

    #[test]
    fn standard_legacy_two_strip_packet_recovers_two_face_boundaries() {
        let mut bytes = vec![0x01, 0x42, 0x02, 0xff, 12, 0, 0, 0, 6, 6];
        for handle in [10u16, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 25] {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
        bytes.extend_from_slice(&[0x01, 0x01, 6]);
        for row in [
            [100u16, 11, 101],
            [101, 15, 102],
            [102, 12, 100],
            [200, 21, 201],
            [201, 25, 202],
            [202, 22, 200],
        ] {
            bytes.extend_from_slice(&[0x02, 3]);
            for handle in row {
                bytes.extend_from_slice(&handle.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x01, 0x06, 6]);
        for index in 0..6 {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [index as f32, 0.0, 0.0] {
                bytes.extend_from_slice(&le_f32(value));
            }
        }

        let topology =
            crate::families::standard::fbb::parse_standard(&bytes).expect("legacy B=2 packet");
        assert_eq!(topology.faces()[0].boundaries.len(), 2);
        assert!(topology.faces()[0]
            .boundaries
            .iter()
            .all(|boundary| boundary.coedges.len() == 3));
        assert_eq!(topology.logical_vertex_count(), 6);
    }

    #[test]
    fn standard_curve_support_table_recovers_leading_spline_and_widened_faces() {
        let mut bytes = vec![0x60, 1, 2, 3, 0, 0, 0, 0xff];
        bytes.extend_from_slice(&260u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&[0x60, 4, 5, 6, 0, 2, 0, 0x33, 0x36, 0xff]);
        bytes.extend_from_slice(&260u32.to_le_bytes());
        bytes.push(2);

        let rows = crate::families::standard::records::standard_curve_supports(&bytes, 300);
        assert_eq!(rows.len(), 2);
        assert!(matches!(
            rows[0].geometry,
            crate::families::standard::records::StandardCurveGeometry::Bspline
        ));
        assert!(matches!(
            rows[1].geometry,
            crate::families::standard::records::StandardCurveGeometry::Line
        ));
        assert_eq!(rows[0].faces, [260, 1]);
        assert_eq!(rows[1].faces, [260, 2]);
    }

    #[test]
    fn topology_binds_logical_vertices_from_exact_edge_endpoint_pairs() {
        let topology =
            crate::families::standard::fbb::parse_standard(&standard_quad_topology_stream())
                .expect("quad topology");
        let assignment = topology
            .bind_vertex_points(&[[0, 1], [1, 2], [2, 3], [3, 0]])
            .expect("unique point assignment");

        assert_eq!(assignment, vec![0, 1, 2, 3]);
    }

    #[test]
    fn standard_circle_parser_rejects_non_support_marker() {
        let mut bytes = vec![0x61, 0, 0, 0, 0, 0x12, 0, 0x33, 0x37];
        bytes.extend_from_slice(&[0; 18]);
        assert!(crate::families::standard::records::standard_circles(&bytes, 1).is_empty());
    }

    #[test]
    fn standard_circle_parser_has_no_model_size_cutoff() {
        let mut bytes = vec![0x60, 1, 2, 3, 0x00, 0x12, 0x00, 0x33, 0x37];
        for value in [0.0_f32, 0.0, 0.0, 2_000_000.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&[0, 1]);
        assert_eq!(
            crate::families::standard::records::standard_circles(&bytes, 2)[0].radius,
            2_000_000.0
        );
    }

    #[test]
    fn standard_surface_roster_walks_freeform_and_analytic_records() {
        use crate::families::standard::records::StandardSurfaceRecord;

        let mut bytes = vec![0x34, 0x12, 0, 0, 0, 0];
        for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 2.0] {
            bytes.extend_from_slice(&le_f32(value));
        }
        bytes.push(0x01);
        let analytic = bytes.len();
        bytes.extend_from_slice(&[0x78, 0x56, 0, 0, 0x1a, 0, 0x33, 0x33]);
        bytes.resize(analytic + 72, 0);
        bytes.push(0xff);
        bytes.push(0x60);

        let records = crate::families::standard::records::standard_surface_records(&bytes, 2)
            .expect("surface roster");
        assert!(matches!(
            records[0],
            StandardSurfaceRecord::Freeform {
                pos: 0,
                tag: 0x1234,
                forward: true,
                ..
            }
        ));
        let StandardSurfaceRecord::Freeform { bounds, .. } = &records[0] else {
            unreachable!("freeform roster row")
        };
        assert_eq!(bounds.aabb_center, [0.0, 0.0, 0.0]);
        assert_eq!(bounds.aabb_half_extents, [1.0, 1.0, 1.0]);
        assert_eq!(bounds.sphere_center, [0.0, 0.0, 0.0]);
        assert_eq!(bounds.sphere_radius, 2.0);
        assert!(matches!(
            &records[1],
            StandardSurfaceRecord::Analytic(prefix)
                if prefix.pos == analytic + 5 && prefix.target == 0x5678 && prefix.kind == 0x33
        ));
    }

    #[test]
    fn plane_bounds_bind_normals_by_persistent_carrier_tag() {
        fn bounds_record(
            tag: u32,
            center: [f32; 3],
            half: [f32; 3],
            sphere: [f32; 3],
            radius: f32,
        ) -> Vec<u8> {
            let mut bytes = vec![0xff];
            bytes.extend_from_slice(&tag.to_le_bytes()[..3]);
            bytes.extend_from_slice(&[0x00, 0x02, 0x00, 0x33, 0x32]);
            for value in [
                center[0], center[1], center[2], half[0], half[1], half[2], sphere[0], sphere[1],
                sphere[2], radius,
            ] {
                bytes.extend_from_slice(&le_f32(value));
            }
            bytes
        }

        let mut bytes = bounds_record(0x0001_0203, [1.0, 2.0, 3.0], [1.0; 3], [1.0, 2.0, 3.0], 4.0);
        bytes.extend(bounds_record(
            0x0004_0506,
            [4.0, 5.0, 6.0],
            [1.0; 3],
            [4.0, 5.0, 6.0],
            4.0,
        ));
        bytes.extend(bounds_record(
            0x0007_0809,
            [0.0, 0.0, 50.0],
            [2.5, 2.5, 0.0],
            [5.2e-7, 1.6e-7, 50.0],
            2.5,
        ));
        let normals = HashMap::from([
            (0x0004_0506, [0.0, 1.0, 0.0]),
            (0x0001_0203, [1.0, 0.0, 0.0]),
            (0x0007_0809, [0.0, 0.0, 1.0]),
        ]);
        let planes = crate::families::standard::records::plane_params(&bytes, &normals);

        assert_eq!(planes.len(), 3);
        assert_eq!(planes[0].target, 0x0001_0203);
        assert_eq!(planes[0].normal, Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(planes[1].target, 0x0004_0506);
        assert_eq!(planes[1].normal, Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(planes[2].target, 0x0007_0809);
        assert_eq!(
            planes[2].origin,
            Point3::new(f64::from(5.2e-7f32), f64::from(1.6e-7f32), 50.0)
        );
    }

    #[test]
    fn analytic_surface_records_retain_trimmed_face_bounds_after_their_parameters() {
        for (kind, relative) in [(0x32, 3), (0x33, 27), (0x34, 27), (0x35, 19), (0x38, 31)] {
            let mut bytes = vec![0; 96];
            for (index, value) in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 1.0, 2.0, 3.0, 8.0]
                .into_iter()
                .enumerate()
            {
                let at = 5 + relative + 4 * index;
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            let record = StandardSurfaceRecord::Analytic(
                crate::families::standard::records::SurfacePrefix {
                    pos: 5,
                    target: 1,
                    kind,
                },
            );
            assert_eq!(
                crate::families::standard::records::standard_face_bounds(&bytes, &record),
                Some(StandardFaceBounds {
                    aabb_center: [1.0, 2.0, 3.0],
                    aabb_half_extents: [4.0, 5.0, 6.0],
                    sphere_center: [1.0, 2.0, 3.0],
                    sphere_radius: 8.0,
                })
            );
        }
        assert_eq!(
            crate::families::standard::records::standard_face_bounds(
                &[0; 96],
                &StandardSurfaceRecord::Analytic(
                    crate::families::standard::records::SurfacePrefix {
                        pos: 5,
                        target: 1,
                        kind: 0x34,
                    },
                ),
            ),
            None
        );
    }

    #[test]
    fn standard_face_witness_requires_an_analytic_marker() {
        let mut record = vec![0u8; 48];
        record[5..8].copy_from_slice(&[0x00, 0x33, 0x33]);
        for (index, value) in [1.0f32, 2.0, 3.0].into_iter().enumerate() {
            record[32 + index * 4..36 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            crate::families::standard::records::standard_face_witness(&record, 5),
            Some(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
        );
        record[6] = 0x32;
        assert!(crate::families::standard::records::standard_face_witness(&record, 5).is_none());
    }

    #[test]
    fn standard_curve_supports_begin_after_the_surface_roster() {
        let mut bytes = vec![
            0x60, 1, 0, 0, 0, 2, 0, 0x33, 0x36, 0, 0, // earlier valid-looking row
        ];
        bytes.extend_from_slice(&[0x34, 0x12, 0, 0, 0, 0]);
        for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 2.0] {
            bytes.extend_from_slice(&le_f32(value));
        }
        bytes.push(0x01);
        bytes.extend_from_slice(&[
            0x60, 2, 0, 0, 0, 2, 0, 0x33, 0x36, 0, 0, // roster-adjacent row
        ]);

        let rows = crate::families::standard::records::standard_curve_supports(&bytes, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tag, 2);
    }

    #[test]
    fn standard_freeform_tag_resolves_direct_and_face_carriers() {
        let mut stream = b5_closed_triangle_stream();
        let vertex_start = stream
            .windows(3)
            .position(|bytes| bytes == [0x05, 0x08, 0x01])
            .expect("required invariant");
        let mut unresolved_face = Vec::new();
        append_b5_record(
            &mut unresolved_face,
            0x5f,
            501,
            &[0x82, 0x18, 100, 0, 0x18, 231, 3, 0x05],
        );
        stream.splice(vertex_start..vertex_start, unresolved_face);
        let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
            [stream],
            &HashSet::from([100, 501]),
            &HashSet::new(),
        );
        assert!(matches!(
            evidence.surface_geometries.get(&100),
            Some(SurfaceGeometry::Plane { .. })
        ));
        assert!(matches!(
            evidence.surface_geometries.get(&501),
            Some(SurfaceGeometry::Plane { .. })
        ));
    }

    #[test]
    fn standard_freeform_tag_resolves_standalone_a8_carrier() {
        let mut stream = a8_surface_stream();
        stream[7..11].copy_from_slice(&100u32.to_le_bytes());
        append_b5_record(&mut stream, 0x2e, 501, &[0x18, 100, 0]);
        let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
            [stream],
            &HashSet::from([100, 501]),
            &HashSet::new(),
        );
        for tag in [100, 501] {
            assert!(matches!(
                evidence.surface_geometries.get(&tag),
                Some(SurfaceGeometry::Nurbs(surface))
                    if surface.u_degree == 2
                        && surface.v_degree == 2
                        && surface.u_count == 3
                        && surface.v_count == 3
            ));
        }
    }

    #[test]
    fn standard_freeform_tag_rejects_conflicting_standalone_a8_carriers() {
        let mut first = a8_surface_stream();
        first[7..11].copy_from_slice(&100u32.to_le_bytes());
        let mut second = first.clone();
        second[67..75].copy_from_slice(&1.0f64.to_le_bytes());
        first.extend(second);

        let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
            [first],
            &HashSet::from([100]),
            &HashSet::new(),
        );
        assert!(!evidence.surface_geometries.contains_key(&100));
    }

    #[test]
    fn standard_freeform_tag_collapses_repeated_standalone_a8_carrier() {
        let mut stream = a8_surface_stream();
        stream[7..11].copy_from_slice(&100u32.to_le_bytes());
        stream.extend(stream.clone());

        let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
            [stream],
            &HashSet::from([100]),
            &HashSet::new(),
        );
        assert!(matches!(
            evidence.surface_geometries.get(&100),
            Some(SurfaceGeometry::Nurbs(_))
        ));
    }

    #[test]
    fn standard_freeform_tag_resolves_standalone_a8_rolling_ball() {
        let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
            [a8_freeform_curve_stream()],
            &HashSet::from([0x1234_5678]),
            &HashSet::new(),
        );
        assert!(matches!(
            evidence.procedural_surfaces.get(&0x1234_5678),
            Some(
                crate::families::standard::decode::StandardSurfaceProcedure::RollingBall {
                    carrier_object_id: 0x1234_5678,
                    definition: cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet {
                        degree: 5,
                        ..
                    },
                }
            )
        ));
    }

    #[test]
    fn standard_object_evidence_rejects_cross_stream_edge_owner_conflicts() {
        let first = b5_closed_triangle_stream();
        let mut second = first.clone();
        let face = second
            .windows(3)
            .position(|bytes| bytes == [0xb5, 0x03, 0x5f])
            .expect("face record");
        second[face + 4..face + 8].copy_from_slice(&501u32.to_le_bytes());

        let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
            [first, second],
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(evidence.edge_owner_faces.is_empty());
    }

    #[test]
    fn standard_face_resolves_a_rolling_ball_result_carrier() {
        let mut stream = b5_closed_triangle_stream();
        let vertex_start = stream
            .windows(3)
            .position(|bytes| bytes == [0x05, 0x08, 0x01])
            .expect("vertex run");
        let mut records = a8_freeform_curve_stream();
        records[7..11].copy_from_slice(&110u32.to_le_bytes());
        let mut offset = vec![0x82, 0xee, 0xef];
        offset.extend_from_slice(&le_f64(-0.5));
        offset.push(0x19);
        for bound in [-2.0, 3.0, -4.0, 5.0] {
            offset.extend_from_slice(&le_f64(bound));
        }
        append_b5_record(&mut records, 0x30, 102, &offset);
        append_b5_record(&mut records, 0x5f, 501, &[0x82, 0xe6, 0x18, 231, 3, 0x05]);
        stream.splice(vertex_start..vertex_start, records);

        let evidence = crate::families::standard::decode::standard_object_evidence_from_streams(
            [stream],
            &HashSet::from([501]),
            &HashSet::new(),
        );
        assert!(!evidence.surface_geometries.contains_key(&501));
        assert!(matches!(
            evidence.procedural_surfaces.get(&501),
            Some(
                crate::families::standard::decode::StandardSurfaceProcedure::RollingBall {
                    carrier_object_id: 110,
                    definition: cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet { .. },
                }
            )
        ));
    }

    #[test]
    fn standard_duplicate_edge_face_uses_object_stream_owner_identity() {
        use crate::families::standard::records::{
            StandardCurveGeometry, StandardCurveSupport, StandardSurfaceRecord,
        };

        let mut edge_faces = vec![[0, 0]];
        let supports = vec![StandardCurveSupport {
            pos: 0,
            tag: 700,
            faces: [0, 0],
            geometry: StandardCurveGeometry::Bspline,
        }];
        let records = [10u32, 20]
            .into_iter()
            .map(|target| {
                StandardSurfaceRecord::Analytic(crate::families::standard::records::SurfacePrefix {
                    pos: 0,
                    target,
                    kind: 0x33,
                })
            })
            .collect::<Vec<_>>();
        crate::families::standard::decode::apply_standard_native_edge_faces(
            &mut edge_faces,
            &supports,
            &records,
            &HashMap::from([(700, HashSet::from([20, 900]))]),
        );
        assert_eq!(edge_faces, [[0, 1]]);

        let mut ambiguous = vec![[0, 0]];
        let mut repeated_records = records;
        repeated_records.push(StandardSurfaceRecord::Analytic(
            crate::families::standard::records::SurfacePrefix {
                pos: 0,
                target: 20,
                kind: 0x33,
            },
        ));
        crate::families::standard::decode::apply_standard_native_edge_faces(
            &mut ambiguous,
            &supports,
            &repeated_records,
            &HashMap::from([(700, HashSet::from([20]))]),
        );
        assert_eq!(ambiguous, [[0, 0]]);
    }

    #[test]
    fn standard_line_parser_reads_face_incidence() {
        let bytes = [0x60, 1, 2, 3, 0, 2, 0, 0x33, 0x36, 0, 1];
        let lines = crate::families::standard::records::standard_lines(&bytes, 2);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tag, 0x03_0201);
        assert_eq!(lines[0].faces, [0, 1]);
    }
}
