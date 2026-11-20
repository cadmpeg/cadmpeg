// SPDX-License-Identifier: Apache-2.0
use crate::records::feature::assembly_features::{
    DERIVED_INSTANCE_RELATION_TYPE_GUID, OCCURRENCE_PLACEMENT_TYPE_GUID,
};

const COMPONENT_INSERT_TYPE_GUID: &str = "428B5C6E-8F2C-42D7-83B7-2C09BD91F83E";
const COMPANION_TYPE_GUID: &str = "6CCF41D5-40BE-48ED-A834-18F3EAED6C57";
const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";

fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

fn typed_reference(bytes: &mut Vec<u8>, target: u32, type_guid: &str) {
    bytes.push(1);
    bytes.extend_from_slice(&u64::from(target).to_le_bytes());
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.extend_from_slice(&[0, 0]);
}

/// The 290-byte typed scope prefix through its null GUID.
fn typed_scope_prefix(relation: u32, identity: u64, matrix: [[f64; 4]; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    header(&mut bytes, *b"289", 30);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&identity.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    typed_reference(&mut bytes, relation, DERIVED_INSTANCE_RELATION_TYPE_GUID);
    bytes.extend_from_slice(&[1, 0]);
    for value in matrix.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend(crate::bytes::lp_utf16_bytes(NULL_GUID).unwrap());
    bytes
}

/// A typed 177-byte relation, its paired header, and the scope header.
fn typed_relation(carrier_type: &str, scope_type: &str, scope: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    header(&mut bytes, *b"351", 20);
    bytes.extend_from_slice(&[0; 10]);
    typed_reference(&mut bytes, 10, carrier_type);
    bytes.extend_from_slice(&[0, 0]);
    typed_reference(&mut bytes, 21, COMPANION_TYPE_GUID);
    bytes.push(0);
    typed_reference(&mut bytes, scope, scope_type);
    header(&mut bytes, *b"266", 20);
    bytes
}

#[test]
fn typed_insert_scope_reads_identity_relation_and_transform() {
    let matrix = [
        [0.0, 0.0, 1.0, 2.5],
        [0.0, 1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0, -3.5],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let bytes = typed_scope_prefix(20, 17, matrix);
    assert_eq!(bytes.len(), 290);
    assert_eq!(
        super::super::exact_typed_matrix_insert_scope(&bytes, 0, 20),
        Some((matrix.try_into().unwrap(), 86, 17))
    );
    assert!(super::super::exact_typed_matrix_insert_scope(&bytes, 0, 21).is_none());
    for offset in [11, 29, 33, 42, 82, 84, 85, 214] {
        let mut changed = bytes.clone();
        changed[offset] ^= 2;
        assert!(
            super::super::exact_typed_matrix_insert_scope(&changed, 0, 20).is_none(),
            "byte {offset}"
        );
    }
    let mut other_type = bytes.clone();
    other_type[46..82].copy_from_slice(COMPANION_TYPE_GUID.as_bytes());
    assert!(super::super::exact_typed_matrix_insert_scope(&other_type, 0, 20).is_none());
    for end in 0..bytes.len() {
        assert!(super::super::exact_typed_matrix_insert_scope(&bytes[..end], 0, 20).is_none());
    }
}

#[test]
fn typed_insert_relation_names_the_carrier_and_the_scope() {
    let relation = typed_relation(
        OCCURRENCE_PLACEMENT_TYPE_GUID,
        COMPONENT_INSERT_TYPE_GUID,
        30,
    );
    let scope_at = 177 + 11;
    assert_eq!(relation.len(), scope_at);
    let mut bytes = relation.clone();
    header(&mut bytes, *b"289", 30);
    assert_eq!(
        super::super::typed_component_insert_relation(&bytes, 0, scope_at, 30),
        Some(10)
    );
    assert!(super::super::typed_component_insert_relation(&bytes, 0, scope_at, 31).is_none());
    assert!(super::super::typed_component_insert_relation(&bytes, scope_at, 0, 30).is_none());
    for offset in [11, 72, 73, 125] {
        let mut changed = bytes.clone();
        changed[offset] = 1;
        assert!(
            super::super::typed_component_insert_relation(&changed, 0, scope_at, 30).is_none(),
            "byte {offset}"
        );
    }
    for (carrier_type, scope_type) in [
        (COMPANION_TYPE_GUID, COMPONENT_INSERT_TYPE_GUID),
        (OCCURRENCE_PLACEMENT_TYPE_GUID, COMPANION_TYPE_GUID),
    ] {
        let mut other = typed_relation(carrier_type, scope_type, 30);
        header(&mut other, *b"289", 30);
        assert!(super::super::typed_component_insert_relation(&other, 0, scope_at, 30).is_none());
    }
}
