// SPDX-License-Identifier: Apache-2.0
use super::{parameter_positions, records};
use crate::test_support::{
    test_b2::{b2_chamfer_contact_stream, b2_chamfer_stream},
    with_service_context,
};

#[test]
fn both_scalar_modes_require_positive_lengths_and_acute_angles() {
    let bytes = b2_chamfer_stream();
    assert_eq!(with_service_context(|_ctx| records(&bytes)).len(), 1);
    for scalar in [0.0, -1.0, std::f64::consts::FRAC_PI_2] {
        let mut invalid = bytes.clone();
        let at = invalid.len() - 8;
        invalid[at..].copy_from_slice(&scalar.to_le_bytes());
        assert!(with_service_context(|_ctx| records(&invalid)).is_empty());
    }
    let mut lengths = bytes.clone();
    let at = lengths.len() - 17;
    lengths[at] = 0x0d;
    let at = lengths.len() - 8;
    lengths[at..].copy_from_slice(&5.0_f64.to_le_bytes());
    assert_eq!(with_service_context(|_ctx| records(&lengths)).len(), 1);
}

#[test]
fn every_control_has_a_closed_domain() {
    let bytes = b2_chamfer_stream();
    for control in 0..6 {
        let mut invalid = bytes.clone();
        let at = invalid.len() - 22 + control;
        invalid[at] = 0xff;
        assert!(with_service_context(|_ctx| records(&invalid)).is_empty());
    }
}

#[test]
fn contact_ownership_requires_context_and_closed_connectors() {
    let bytes = b2_chamfer_contact_stream();
    let inventory = crate::wire::records::consolidated_records(&bytes);
    with_service_context(|_ctx| {
        let construction = records(&bytes).remove(0);
        assert_eq!(
            parameter_positions(&bytes, &inventory, &construction).len(),
            4
        );
        let mut unrelated = bytes.clone();
        let third = inventory[inventory.len() - 2].byte_offset();
        unrelated[third + 5] = 0x15;
        assert_eq!(
            parameter_positions(&unrelated, &inventory, &construction).len(),
            2
        );
        let mut wrong_rectangle = bytes.clone();
        wrong_rectangle[third + 7..third + 15].copy_from_slice(&8.0_f64.to_le_bytes());
        assert_eq!(
            parameter_positions(&wrong_rectangle, &inventory, &construction).len(),
            2
        );
        assert!(parameter_positions(&bytes, &inventory[1..], &construction).is_empty());
    });
}

#[test]
fn compact_parser_accepts_complete_nested_syntax() {
    let bytes = b2_chamfer_stream();
    let mut nested = vec![0xa5, 0x03, 0x7f];
    nested.extend_from_slice(
        &u32::try_from(bytes.len())
            .expect("valid bounded chamfer fixture")
            .to_le_bytes(),
    );
    nested.push(5);
    nested.extend(bytes);
    assert_eq!(with_service_context(|_ctx| records(&nested))[0].pos, 8);
}
