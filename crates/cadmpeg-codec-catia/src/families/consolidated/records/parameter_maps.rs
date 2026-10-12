// SPDX-License-Identifier: Apache-2.0
//! Affine-map invariants of eight-scalar class-23 definitions.

use super::consolidated_edge_definition_data;

fn payload(operands: [u8; 2], values: [f64; 8]) -> Vec<u8> {
    let mut bytes = vec![0x82, operands[0], operands[1], 0x0a, 0x87, 0x0d];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn class23_affine_map_rejects_contradictory_intervals_scale_and_slots() {
    const TOLERANCE: f64 = 1.0e-12;
    let values = [4.0, 8.0, TOLERANCE, 1.0, 3.0, 2.0, 2.0, TOLERANCE];
    assert!(consolidated_edge_definition_data(0x23, &payload([5, 9], values)).is_some());
    for (index, value) in [
        (0, 5.0),
        (1, 9.0),
        (2, 0.0),
        (3, 3.0),
        (4, 1.0),
        (5, -2.0),
        (6, 0.0),
    ] {
        let mut invalid = values;
        invalid[index] = value;
        if index == 2 {
            invalid[7] = value;
        }
        assert!(
            consolidated_edge_definition_data(0x23, &payload([5, 9], invalid)).is_none(),
            "lane {index}"
        );
    }
    assert!(consolidated_edge_definition_data(0x23, &payload([5, 5], values)).is_none());
    assert!(consolidated_edge_definition_data(0x23, &payload([9, 5], values)).is_none());
    let mut wrong_fixed_operand = payload([5, 9], values);
    wrong_fixed_operand[4] = 0x88;
    assert!(consolidated_edge_definition_data(0x23, &wrong_fixed_operand).is_none());
    let identity = [1.0, 3.0, TOLERANCE, 1.0, 3.0, 1.0, 0.0, TOLERANCE];
    assert!(consolidated_edge_definition_data(0x23, &payload([9, 5], identity)).is_some());
}

#[test]
fn class23_slots_reject_other_local_slot_pairs() {
    for pair in [[0, 1], [1, 1], [2, 2], [1, 3]] {
        assert!(serde_json::from_value::<super::Class23Slots>(serde_json::json!(pair)).is_err());
    }
    for pair in [[1, 2], [2, 1]] {
        let slots: super::Class23Slots =
            serde_json::from_value(serde_json::json!(pair)).expect("known local slots");
        assert_eq!(
            serde_json::to_value(slots).expect("serialize local slots"),
            serde_json::json!(pair)
        );
    }
}
