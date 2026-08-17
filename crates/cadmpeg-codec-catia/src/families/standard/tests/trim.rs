use super::*;

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
fn trim_record_layout_uses_the_complete_handle_span_as_its_count_bound() {
    let handle_count = 500_001u32;
    let strip_length = handle_count - 3;
    let mut bytes = vec![0x01, 0x43, 0x01, 0x01, 0xff];
    bytes.extend_from_slice(&handle_count.to_le_bytes());
    bytes.push(0xff);
    bytes.extend_from_slice(&strip_length.to_le_bytes());
    bytes.extend(std::iter::repeat_n(0, handle_count as usize));

    let layout = parse_trim_record_layout(&bytes, 0, 1).expect("complete handle span");
    assert_eq!(layout.stored_count, handle_count as usize);
    assert_eq!(layout.end, bytes.len());
}

#[test]
fn trim_record_rejects_invalid_present_frame_vector() {
    let mut bytes = vec![0x01, 0x49, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00];
    for value in [2.0f32, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 10, 0, 11, 0, 12]);

    assert!(parse_trim_record_layout(&bytes, 0, 2).is_none());
    assert!(parse_trim_record(&bytes, 0, 2).is_none());
    assert!(parse_trim_chain(&bytes, bytes.len(), 1, 2).is_none());

    let mut non_finite = bytes[..8].to_vec();
    for value in [f32::NAN, 0.0, 1.0] {
        non_finite.extend_from_slice(&value.to_le_bytes());
    }
    non_finite.extend_from_slice(&[0, 10, 0, 11, 0, 12]);
    assert!(parse_trim_record_layout(&non_finite, 0, 2).is_none());
}

#[test]
fn trim_record_accepts_binary32_round_trip_frame_vector() {
    let mut bytes = vec![0x01, 0x49, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00];
    for value in [0.577_350_26f32, 0.577_350_26, 0.577_350_26] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 10, 0, 11, 0, 12]);

    let record = parse_trim_record(&bytes, 0, 2).expect("binary32 unit vector");
    assert!(record.frame_vector.is_some());
}

#[test]
fn trim_record_rejects_frame_vector_outside_binary32_round_trip_bound() {
    let mut bytes = vec![0x01, 0x49, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00];
    for value in [1.00001f32, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 10, 0, 11, 0, 12]);

    assert!(parse_trim_record_layout(&bytes, 0, 2).is_none());
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
fn two_handle_standard_rows_select_u8_complete_boundary_layout() {
    let mut bytes = vec![
        0x01, 0x01, 0x02, // two kind-1 rows
        0x02, 0x02, 0x02, 0x00, // edge 2 -> 0
        0x02, 0x02, 0x00, 0x01, // edge 0 -> 1
    ];
    bytes.extend_from_slice(&EDGE_DELIMITER);
    bytes.extend_from_slice(&[0x01, 0x06, 0x00]);

    let (rows, vertex_header) = parse_edge_tables_at(&bytes, 0).expect("u8 edge rows");
    assert_eq!(rows[0].handles, [2, 0]);
    assert_eq!(rows[1].handles, [0, 1]);
    assert!(rows
        .iter()
        .all(|row| row.boundary_layout == EdgeBoundaryLayout::CompleteBoundaryRun));
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
fn mesh_candidate_comparison_preserves_same_class_edge_row_interchange() {
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

    assert!(!mesh_candidates_equivalent(&left, &right));
}

#[test]
fn mesh_candidate_comparison_collapses_unbound_observable_edge_gauge() {
    let edge_rows = vec![
        EdgeRow {
            kind: 2,
            handles: vec![10, 11],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        },
        EdgeRow {
            kind: 2,
            handles: vec![20, 21],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        },
    ];
    let topology = |swapped: bool| StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary {
                coedges: if swapped {
                    vec![
                        CoedgeUse {
                            edge_row: 1,
                            reversed: false,
                            start_vertex: 0,
                            end_vertex: 1,
                        },
                        CoedgeUse {
                            edge_row: 0,
                            reversed: false,
                            start_vertex: 1,
                            end_vertex: 2,
                        },
                    ]
                } else {
                    vec![
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
                            end_vertex: 2,
                        },
                    ]
                },
            }],
        }],
        edge_rows: edge_rows.clone(),
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        logical_vertex_count: 3,
    };
    let left = (topology(false), vec![0, 1, 2]);
    let right = (topology(true), vec![0, 1, 2]);
    let edge_classes = [7, 7];
    let edge_candidates = vec![vec![[0, 1], [1, 2]], vec![[0, 1], [1, 2]]];
    let edge_identity_evidence = [false, false];

    assert!(!mesh_candidates_equivalent(&left, &right));
    assert!(mesh_candidates_equivalent_with_gauge(
        &left,
        &right,
        &edge_classes,
        &edge_candidates,
        &edge_identity_evidence,
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
fn standard_face_population_accepts_flagged_fbb_rows() {
    let row = [0xb0, 0x04, 0x04, 0xff, 0x99, 0x1f, 0x1a, 0xd1];
    assert_eq!(standard_face_count(&row.repeat(6)), Some(6));
}

#[test]
fn standard_face_population_rejects_equal_largest_fbb_runs() {
    let row = [0x30, 0x04, 0x04, 0xff, 0xff, 0xff, 0xd2, 0xd2];
    let mut bytes = row.repeat(2);
    bytes.push(0);
    bytes.extend_from_slice(&row.repeat(2));

    assert_eq!(standard_face_count(&bytes), None);
}

#[test]
fn standard_face_population_withholds_multiple_complete_fbb_groups() {
    let mut bytes = crate::test_support::standard_quad_topology_stream();
    bytes.extend(crate::test_support::standard_quad_topology_stream());

    let groups = standard_fbb_groups(&bytes);
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| {
        group.face_count == 1
            && group.topology.face_count() == 1
            && group.topology.edge_rows().len() == 4
    }));
    assert_eq!(standard_face_count(&bytes), None);
    assert!(crate::families::standard::fbb::parse_standard(&bytes).is_none());
}

#[test]
fn standard_helpers_share_the_source_closed_face_population() {
    let mut bytes = crate::test_support::standard_quad_topology_stream();
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xaa, 0xbb, 0xcc, 0xdd]);
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0x11, 0x22, 0x33, 0x44]);

    assert_eq!(standard_face_count(&bytes), Some(1));
    assert_eq!(standard_edge_count(&bytes), Some(4));
    assert_eq!(
        crate::solve::missing_edge::standard_edge_rows(&bytes)
            .expect("selected edge table")
            .len(),
        4
    );
    assert_eq!(
        crate::families::standard::fbb::parse_standard(&bytes)
            .expect("selected topology")
            .face_count(),
        1
    );
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
fn allocation_program_rejects_an_unconsumed_trim_packet() {
    let trims = [trim(0x4a, [0, 1, 2, 3]), trim(0x41, [4, 5, 6, 7])];
    assert!(motif_port_points(&trims, 4).is_none());
}
