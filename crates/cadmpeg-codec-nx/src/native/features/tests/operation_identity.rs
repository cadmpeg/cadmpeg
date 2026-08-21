// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::{composed_feature_history_payload, prt_with_named_payloads};
use std::collections::BTreeMap;

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
    let block_identities = BTreeMap::from([
        (55, Some("block-55".to_string())),
        (56, Some("block-56".to_string())),
        (61, Some("block-61".to_string())),
    ]);
    let mut original = vec![
        label(0, [Some(55), Some(56), None, None]),
        label(1, [None; 4]),
        label(2, [Some(61), None, None, None]),
    ];
    assign_operation_header_identities(&mut original, &block_identities);
    let identity = original[0]
        .stable_identity
        .clone()
        .expect("unique non-null header tuple has an identity witness");
    assert_eq!(
        identity,
        "nx:feature-history:operation-header-identity#content:block-55-block-56-null-null"
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
    assign_operation_header_identities(&mut reordered, &block_identities);
    assert_eq!(
        reordered[1].stable_identity.as_deref(),
        Some(identity.as_str())
    );
    assert!(reordered[2].stable_identity.is_none());
}

#[test]
fn operation_header_identity_rejects_duplicate_tuples() {
    let block_identities = BTreeMap::from([
        (55, Some("block-55".to_string())),
        (56, Some("block-56".to_string())),
    ]);
    let mut labels = vec![
        label(0, [Some(55), Some(56), None, None]),
        label(1, [Some(55), Some(56), None, None]),
    ];
    assign_operation_header_identities(&mut labels, &block_identities);
    assert!(labels.iter().all(|label| label.stable_identity.is_none()));
}

#[test]
fn operation_header_identity_survives_offset_store_insertion() {
    let first_payload = composed_feature_history_payload(
        &[(&[1, 2, 0xff, 0xff], "EXTRUDE", Vec::new())],
        &[b"alpha".as_slice(), b"beta".as_slice()],
    );
    let second_payload = composed_feature_history_payload(
        &[(&[2, 3, 0xff, 0xff], "EXTRUDE", Vec::new())],
        &[
            b"inserted".as_slice(),
            b"alpha".as_slice(),
            b"beta".as_slice(),
        ],
    );
    let first = crate::container::scan_bytes(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        first_payload,
    )]))
    .expect("first synthetic container");
    let second = crate::container::scan_bytes(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        second_payload,
    )]))
    .expect("second synthetic container");

    let first_labels = super::feature_operation_labels(&first);
    let second_labels = super::feature_operation_labels(&second);
    assert_eq!(
        first_labels[0].object_indices,
        [Some(1), Some(2), None, None]
    );
    assert_eq!(
        second_labels[0].object_indices,
        [Some(2), Some(3), None, None]
    );
    assert_eq!(
        first_labels[0].stable_identity,
        second_labels[0].stable_identity
    );

    let first_records = super::feature_operation_records(&first);
    let second_records = super::feature_operation_records(&second);
    assert_eq!(
        first_records[0].stable_identity,
        second_records[0].stable_identity
    );
}

#[test]
fn operation_header_identity_requires_unique_resolved_blocks() {
    let block_identities = BTreeMap::from([(55, None), (56, Some("block-56".to_string()))]);
    let mut labels = vec![label(0, [Some(55), Some(56), None, None])];
    assign_operation_header_identities(&mut labels, &block_identities);
    assert!(labels[0].stable_identity.is_none());
}
