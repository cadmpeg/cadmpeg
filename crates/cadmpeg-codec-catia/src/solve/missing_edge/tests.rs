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
