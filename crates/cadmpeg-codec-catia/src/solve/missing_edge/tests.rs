use super::*;

fn row(handles: &[u32]) -> EdgeRow {
    EdgeRow {
        kind: 2,
        handles: handles.to_vec(),
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }
}

fn handles(values: &[u32]) -> HashSet<u32> {
    values.iter().copied().collect()
}

fn raw_visualization_table(mode: u8, triples: &[[f32; 3]]) -> Vec<u8> {
    let mut bytes = INDEXED_VISUALIZATION_POINT_MARKER.to_vec();
    let count = u32::try_from(triples.len()).expect("synthetic table count");
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.push(0xff);
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, mode]);
    for triple in triples {
        for coordinate in triple {
            bytes.extend_from_slice(&coordinate.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn raw_visualization_points_bind_terminal_handles_by_direct_index() {
    let points = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let bytes = raw_visualization_table(1, &[points[0], points[1], points[0]]);
    let rows = [row(&[0, 40, 1]), row(&[1, 41, 2])];

    assert_eq!(
        visualization_endpoint_pairs(&bytes, &rows, &points),
        Some(vec![[0, 1], [1, 0]])
    );
}

#[test]
fn compressed_visualization_points_reuse_coordinate_prefixes() {
    let points = [[1.0, 2.0, 3.0], [1.0, 2.0, 4.0], [1.0, 5.0, 6.0]];
    let mut bytes = INDEXED_VISUALIZATION_POINT_MARKER.to_vec();
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.push(0xff);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(&[0xe4, 0xff]);
    bytes.extend_from_slice(&6u32.to_le_bytes());
    for scalar in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0] {
        bytes.extend_from_slice(&scalar.to_le_bytes());
    }
    let rows = [row(&[0, 20, 2]), row(&[2, 21, 3])];

    assert_eq!(
        visualization_endpoint_pairs(&bytes, &rows, &points),
        Some(vec![[0, 1], [1, 2]])
    );
}

#[test]
fn compressed_visualization_points_require_initial_xyz_and_exact_scalar_count() {
    let points = [[1.0, 2.0, 3.0], [1.0, 2.0, 4.0]];
    let rows = [row(&[0, 1])];
    let mut bytes = INDEXED_VISUALIZATION_POINT_MARKER.to_vec();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.push(0xff);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x08, 0xff]);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    for scalar in [1.0f32, 2.0, 3.0, 4.0] {
        bytes.extend_from_slice(&scalar.to_le_bytes());
    }

    let scalar_count_at = INDEXED_VISUALIZATION_POINT_HEADER_LEN + 2;
    let mut missing_scalar = bytes.clone();
    missing_scalar[scalar_count_at..scalar_count_at + 4].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(
        visualization_endpoint_pairs(&missing_scalar, &rows, &points),
        None
    );

    bytes[19] |= 1;
    assert_eq!(visualization_endpoint_pairs(&bytes, &rows, &points), None);

    let mut missing_delimiter = bytes;
    missing_delimiter[20] = 0;
    assert_eq!(
        visualization_endpoint_pairs(&missing_delimiter, &rows, &points),
        None
    );
}

#[test]
fn visualization_points_abstain_for_other_modes_or_incomplete_coverage() {
    let points = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let rows = [row(&[0, 1])];

    assert_eq!(
        visualization_endpoint_pairs(&raw_visualization_table(2, &points), &rows, &points,),
        None
    );
    assert_eq!(
        visualization_endpoint_pairs(
            &raw_visualization_table(1, &[points[0], points[0]]),
            &rows,
            &points,
        ),
        None
    );

    let mut missing_secondary_lead = raw_visualization_table(1, &points);
    missing_secondary_lead[10] = 0;
    assert_eq!(
        visualization_endpoint_pairs(&missing_secondary_lead, &rows, &points),
        None
    );
}

#[test]
fn repeated_long_row_selects_one_majority_sharing_face() {
    let rows = vec![row(&[10, 11, 12, 13, 14])];
    let faces = vec![
        handles(&[10, 11, 12, 13, 14, 90]),
        handles(&[10, 11, 12, 80]),
        handles(&[10, 14, 70]),
    ];

    let candidates = repeated_edge_face_handle_candidates_from_sets(&rows, &faces, &[[0, 0]])
        .expect("complete owning-face containment");

    assert_eq!(candidates, vec![vec![1]]);
}

#[test]
fn repeated_long_row_abstains_when_majority_sharing_is_not_unique() {
    let rows = vec![row(&[10, 11, 12, 13])];
    let faces = vec![
        handles(&[10, 11, 12, 13]),
        handles(&[10, 11, 12]),
        handles(&[11, 12, 13]),
    ];

    let candidates = repeated_edge_face_handle_candidates_from_sets(&rows, &faces, &[[0, 0]])
        .expect("complete owning-face containment");

    assert_eq!(candidates, vec![Vec::<usize>::new()]);
}

#[test]
fn repeated_short_row_retains_every_complete_handle_sharing_face() {
    let rows = vec![row(&[10, 11])];
    let faces = vec![
        handles(&[10, 11, 90]),
        handles(&[10, 11, 80]),
        handles(&[10, 11, 70]),
        handles(&[10, 60]),
    ];

    let candidates = repeated_edge_face_handle_candidates_from_sets(&rows, &faces, &[[0, 0]])
        .expect("complete owning-face containment");

    assert_eq!(candidates, vec![vec![1, 2]]);
}

#[test]
fn repeated_handle_selector_requires_file_wide_owning_face_containment() {
    let rows = vec![row(&[10, 11]), row(&[20, 21])];
    let faces = vec![handles(&[10, 11, 20]), handles(&[20, 21])];

    assert!(
        repeated_edge_face_handle_candidates_from_sets(&rows, &faces, &[[0, 0], [0, 1]],).is_none()
    );
}

#[test]
fn handle_face_candidates_do_not_reopen_resolved_incidence() {
    let edge_faces = [[0, 2], [1, 1], [3, 3]];
    let mut allowed = [vec![2], vec![2, 4], Vec::new()];
    let handles = [vec![2], vec![4, 5], vec![5]];

    refine_repeated_edge_face_candidates(&edge_faces, &mut allowed, &handles)
        .expect("aligned face domains");

    assert!(allowed[0].is_empty());
    assert_eq!(allowed[1], vec![4]);
    assert_eq!(allowed[2], vec![5]);
}
