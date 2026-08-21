// SPDX-License-Identifier: Apache-2.0

use super::*;

fn label(ordinal: u32, object_indices: [Option<u32>; 4]) -> FeatureOperationLabel {
    FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: "EXTRUDE".to_string(),
        object_indices,
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        stable_identity: None,
        source_offset: u64::from(ordinal),
    }
}

#[test]
fn operation_header_identity_witness_survives_reordering() {
    let mut original = vec![
        label(0, [Some(55), Some(56), None, None]),
        label(1, [None; 4]),
        label(2, [Some(61), None, None, None]),
    ];
    assign_operation_header_identities(&mut original);
    let identity = original[0]
        .stable_identity
        .clone()
        .expect("unique non-null header tuple has an identity witness");
    assert_eq!(
        identity,
        "nx:feature-history:operation-header-identity#55-56-null-null"
    );
    assert!(original[1].stable_identity.is_none());

    let mut reordered = vec![
        original[2].clone(),
        original[0].clone(),
        original[1].clone(),
    ];
    for label in &mut reordered {
        label.stable_identity = None;
    }
    assign_operation_header_identities(&mut reordered);
    assert_eq!(
        reordered[1].stable_identity.as_deref(),
        Some(identity.as_str())
    );
    assert!(reordered[2].stable_identity.is_none());
}

#[test]
fn operation_header_identity_rejects_duplicate_tuples() {
    let mut labels = vec![
        label(0, [Some(55), Some(56), None, None]),
        label(1, [Some(55), Some(56), None, None]),
    ];
    assign_operation_header_identities(&mut labels);
    assert!(labels.iter().all(|label| label.stable_identity.is_none()));
}
