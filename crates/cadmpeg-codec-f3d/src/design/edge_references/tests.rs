// SPDX-License-Identifier: Apache-2.0

use super::edge_operand_reference_edge_sets;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use std::num::NonZeroU32;

fn operand() -> DesignEdgeOperand {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:edge-operand#10", "scope_record_index": 1,
        "scope_reference_ordinal": 0, "record_index": 10, "byte_offset": 0,
        "class_tag": "297", "paired_byte_offset": 16, "paired_class_tag": "259",
        "recipe_record_index": 13, "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:test:recipe", "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "", "recipe_references": [],
        "recipe_program_offset": 0, "recipe_program": [],
        "changed_boundary_edge_slots": [], "deleted_boundary_edge_slots": [],
        "next_record_index": 14, "next_byte_offset": 160,
    }))
    .unwrap()
}

#[test]
fn selected_edge_reference_source_preserves_invalid_ordinals_and_exact_length() {
    let mut operand = operand();
    operand.terminal_reference_edge_slots = vec![vec![17], vec![18, 19]];
    operand.local_topology_references = Some(
        [u32::MAX, 2, 1, 2]
            .map(|value| NonZeroU32::new(value).unwrap())
            .to_vec(),
    );
    let mut source = edge_operand_reference_edge_sets(&operand);
    assert_eq!(source.len(), 4);
    assert_eq!(source.next(), Some(None));
    assert_eq!(source.len(), 3);
    assert_eq!(
        source.collect::<Vec<_>>(),
        vec![Some(&[18, 19][..]), Some(&[17][..]), Some(&[18, 19][..])]
    );
}

#[test]
fn selected_edge_reference_source_explicit_empty_selection_stays_empty() {
    let mut operand = operand();
    operand.terminal_reference_edge_slots = vec![vec![17]];
    operand.local_topology_references = Some(Vec::new());
    assert_eq!(edge_operand_reference_edge_sets(&operand).len(), 0);
}

#[test]
fn selected_edge_reference_source_native_context_precedes_terminal_source() {
    let mut operand = operand();
    operand.terminal_reference_edge_slots = vec![vec![99]];
    operand.recipe_reference_contexts = vec![serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0, "result_faces": [], "result_shared_edge_slots": [],
        "preceding_faces": [], "shared_edge_slots": [], "changed_shared_edge_slots": [],
        "changed_reference_edge_slots": [17, 18],
    }))
    .unwrap()];
    assert_eq!(
        edge_operand_reference_edge_sets(&operand).collect::<Vec<_>>(),
        vec![Some(&[17, 18][..])]
    );
}
