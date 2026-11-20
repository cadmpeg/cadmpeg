// SPDX-License-Identifier: Apache-2.0

use super::super::profile_matches_entry_candidates;
use cadmpeg_ir::sketches::{SketchEntityId, SketchEntityUse, SketchId};

fn sketch() -> SketchId {
    crate::ids::neutral_sketch_id(&super::placement())
}

fn id(sketch: &SketchId, record: u32) -> SketchEntityId {
    crate::ids::neutral_sketch_record_id(sketch, record)
}

fn uses(sketch: &SketchId, records: &[u32]) -> Vec<SketchEntityUse> {
    records
        .iter()
        .map(|record| SketchEntityUse {
            entity: id(sketch, *record),
            reversed: false,
        })
        .collect()
}

/// With one curve per entry the loop holds exactly those curves.
#[test]
fn single_candidates_require_the_exact_loop() {
    let sketch = sketch();
    let entries = vec![vec![id(&sketch, 1)], vec![id(&sketch, 2)]];
    assert!(profile_matches_entry_candidates(
        &uses(&sketch, &[2, 1]),
        &entries
    ));
    assert!(!profile_matches_entry_candidates(
        &uses(&sketch, &[1]),
        &entries
    ));
    assert!(!profile_matches_entry_candidates(
        &uses(&sketch, &[1, 2, 3]),
        &entries
    ));
}

/// An identity pair that names split pieces of one curve selects every piece
/// that the loop holds, and at least one of them.
#[test]
fn split_curve_entry_names_the_pieces_the_loop_holds() {
    let sketch = sketch();
    let entries = vec![
        vec![id(&sketch, 1)],
        vec![id(&sketch, 10), id(&sketch, 11), id(&sketch, 12)],
    ];
    assert!(profile_matches_entry_candidates(
        &uses(&sketch, &[1, 10]),
        &entries
    ));
    assert!(profile_matches_entry_candidates(
        &uses(&sketch, &[10, 1, 12]),
        &entries
    ));
    assert!(!profile_matches_entry_candidates(
        &uses(&sketch, &[1]),
        &entries
    ));
    assert!(!profile_matches_entry_candidates(
        &uses(&sketch, &[10, 11]),
        &entries
    ));
    assert!(!profile_matches_entry_candidates(
        &uses(&sketch, &[1, 10, 13]),
        &entries
    ));
}
