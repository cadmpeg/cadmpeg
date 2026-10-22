// SPDX-License-Identifier: Apache-2.0

use super::{exact_assembly_operand_path, exact_assembly_operand_path_envelope};
use crate::records::feature::assembly::DesignAssemblyOperandPathLink;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

const GUID: &str = "11111111-1111-1111-1111-111111111111";

fn path_link() -> DesignAssemblyOperandPathLink {
    DesignAssemblyOperandPathLink {
        locator_reference_offset: 0,
        locator_record_index: 64,
        locator_class_tag: "304".to_owned().try_into().unwrap(),
        locator_byte_offset: 0,
        locator_scope_reference_offset: 0,
        wrapper_record_index: 66,
        wrapper_reference_offset: 0,
        wrapper_class_tag: "382".to_owned().try_into().unwrap(),
        wrapper_byte_offset: 0,
        path_reference_offset: 0,
    }
}

fn path_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"329");
    bytes.extend_from_slice(&65_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    for unit in GUID.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[test]
fn assembly_path_keeps_one_occurrence() {
    let bytes = path_bytes();
    let path = exact_assembly_operand_path(&bytes, 0, 65, bytes.len(), path_link(), None)
        .unwrap()
        .unwrap();
    assert_eq!(path.occurrence_guids().len(), 1);
}

fn write_reference(bytes: &mut [u8], at: usize, index: u32) {
    bytes[at] = 1;
    bytes[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
}

fn envelope_bytes(scope_record_index: u32) -> Vec<u8> {
    let mut locator = vec![0; 190];
    locator[0..4].copy_from_slice(&3_u32.to_le_bytes());
    locator[4..7].copy_from_slice(b"304");
    locator[7..11].copy_from_slice(&64_u32.to_le_bytes());
    write_reference(&mut locator, 21, 164);
    for ordinal in 0..16 {
        let value = if ordinal % 5 == 0 { 1.0_f64 } else { 0.0 };
        locator[33 + ordinal * 8..41 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
    }
    write_reference(&mut locator, 162, scope_record_index);
    write_reference(&mut locator, 173, 66);
    locator[184..188].copy_from_slice(&2_u32.to_le_bytes());
    locator.extend_from_slice(&path_bytes());

    let mut wrapper = vec![0; 37];
    wrapper[0..4].copy_from_slice(&3_u32.to_le_bytes());
    wrapper[4..7].copy_from_slice(b"382");
    wrapper[7..11].copy_from_slice(&66_u32.to_le_bytes());
    wrapper[21] = 1;
    wrapper[22..26].copy_from_slice(&1_u32.to_le_bytes());
    write_reference(&mut wrapper, 26, 65);
    locator.extend_from_slice(&wrapper);
    locator.extend_from_slice(&3_u32.to_le_bytes());
    locator.extend_from_slice(b"396");
    locator.extend_from_slice(&67_u32.to_le_bytes());
    locator
}

#[test]
fn assembly_path_span_keeps_its_record_index() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        10,
    );
    let bytes = envelope_bytes(scope.record_index);
    let path = exact_assembly_operand_path_envelope(
        &bytes,
        &scope,
        &std::collections::HashMap::new(),
        64,
        0,
        0,
    )
    .unwrap()
    .unwrap();
    assert_eq!(path.record_index, 65);
}

fn push_guid(bytes: &mut Vec<u8>, guid: &str) {
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    for unit in guid.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
}

/// An identity-qualified counted path record whose dynamic class tag is
/// `class_tag`: one occurrence GUID, then the four identity GUIDs.
fn identity_qualified_path_bytes(class_tag: [u8; 3], identities: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&65_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    push_guid(&mut bytes, GUID);
    if identities {
        push_guid(&mut bytes, "22222222-2222-2222-2222-222222222222");
        push_guid(&mut bytes, "33333333-3333-3333-3333-333333333333");
        bytes.extend_from_slice(&2_u64.to_le_bytes());
        push_guid(&mut bytes, "44444444-4444-4444-4444-444444444444");
        push_guid(&mut bytes, "55555555-5555-5555-5555-555555555555");
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
    }
    bytes
}

#[test]
fn typed_path_record_reads_whatever_its_class_tag() {
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID as TYPED;
    let bytes = identity_qualified_path_bytes(*b"384", true);
    let read = |type_guid| {
        exact_assembly_operand_path(&bytes, 0, 65, bytes.len(), path_link(), type_guid).unwrap()
    };
    assert!(
        read(None).is_none(),
        "class 384 is not an admitted path class"
    );
    let path = read(Some(TYPED)).expect("typed path");
    assert!(path.admitted_by_type());
    assert_eq!(path.occurrence_guids().len(), 1);
    assert_eq!(path.identity_guids().len(), 4);

    // A typed path stores its identity suffix.
    let bare = identity_qualified_path_bytes(*b"384", false);
    assert!(
        exact_assembly_operand_path(&bare, 0, 65, bare.len(), path_link(), Some(TYPED))
            .unwrap()
            .is_none()
    );

    // An admitted class keeps its own reading and is not marked typed.
    let listed = identity_qualified_path_bytes(*b"390", true);
    let path = exact_assembly_operand_path(&listed, 0, 65, listed.len(), path_link(), None)
        .unwrap()
        .expect("class 390 path");
    assert!(!path.admitted_by_type());
}

/// A wide locator (record 64) with or without its transform, then the path
/// (65), its wrapper (66) and the next header.
fn wide_envelope_bytes(scope_record_index: u32, transform: bool) -> Vec<u8> {
    let mut locator = vec![0; 68];
    locator[0..4].copy_from_slice(&3_u32.to_le_bytes());
    locator[4..7].copy_from_slice(b"337");
    locator[7..11].copy_from_slice(&64_u32.to_le_bytes());
    locator[39..43].copy_from_slice(&3_u32.to_le_bytes());
    write_reference(&mut locator, 56, 164);
    if transform {
        for ordinal in 0..16 {
            let value = if ordinal % 5 == 0 { 1.0_f64 } else { 0.0 };
            locator.extend_from_slice(&value.to_le_bytes());
        }
    } else {
        locator[67] = 1;
    }
    let backlink = locator.len() + 1;
    locator.resize(backlink + 28, 0);
    write_reference(&mut locator, backlink, scope_record_index);
    write_reference(&mut locator, backlink + 11, 66);
    locator[backlink + 22..backlink + 26].copy_from_slice(&2_u32.to_le_bytes());
    locator.extend_from_slice(&path_bytes());

    let mut wrapper = vec![0; 37];
    wrapper[0..4].copy_from_slice(&3_u32.to_le_bytes());
    wrapper[4..7].copy_from_slice(b"341");
    wrapper[7..11].copy_from_slice(&66_u32.to_le_bytes());
    wrapper[21] = 1;
    wrapper[22..26].copy_from_slice(&1_u32.to_le_bytes());
    write_reference(&mut wrapper, 26, 65);
    locator.extend_from_slice(&wrapper);
    locator.extend_from_slice(&3_u32.to_le_bytes());
    locator.extend_from_slice(b"396");
    locator.extend_from_slice(&67_u32.to_le_bytes());
    locator
}

#[test]
fn wide_scope_locator_is_admitted_by_its_record_type() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 945;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let typed = std::collections::HashMap::from([(64, (super::WIDE_PATH_LOCATOR_TYPE_GUID, 5))]);
    for transform in [true, false] {
        let bytes = wide_envelope_bytes(scope.record_index, transform);
        let locator_length = if transform { 225 } else { 97 };
        assert_eq!(bytes.len(), locator_length + path_bytes().len() + 37 + 11);
        let path = exact_assembly_operand_path_envelope(&bytes, &scope, &typed, 64, 0, 0)
            .unwrap()
            .expect("wide locator");
        assert_eq!(path.record_index, 65);
        let untyped = std::collections::HashMap::new();
        assert!(
            exact_assembly_operand_path_envelope(&bytes, &scope, &untyped, 64, 0, 0)
                .unwrap()
                .is_none()
        );
    }
}

/// A typed path names a local occurrence by its decimal occurrence key.
#[test]
fn typed_path_reads_a_decimal_occurrence_key() {
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID as TYPED;
    let guid_path = identity_qualified_path_bytes(*b"366", true);
    // Replace the 36-unit occurrence GUID at offset 25 with the key "34".
    let mut bytes = guid_path[..25].to_vec();
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for unit in "34".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&guid_path[25 + 4 + 72..]);
    let path = exact_assembly_operand_path(&bytes, 0, 65, bytes.len(), path_link(), Some(TYPED))
        .unwrap()
        .expect("typed path with a decimal key");
    assert_eq!(path.occurrence_guids()[0].value.as_str(), "34");
    assert_eq!(path.identity_guids().len(), 4);
}

/// A typed path's identity GUIDs can be followed by a zero word, a pair count
/// and that many pairs of occurrence identities.
#[test]
fn typed_path_reads_its_occurrence_identity_pairs() {
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID as TYPED;
    let plain = identity_qualified_path_bytes(*b"366", true);
    let mut bytes = plain[..plain.len() - 8].to_vec();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for unit in "43".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    push_guid(&mut bytes, "66666666-6666-6666-6666-666666666666");
    let path = exact_assembly_operand_path(&bytes, 0, 65, bytes.len(), path_link(), Some(TYPED))
        .unwrap()
        .expect("typed path with an identity pair");
    let pairs = path.paired_occurrence_keys();
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].value.as_str(), "43");
    assert_eq!(
        pairs[1].value.as_str(),
        "66666666-6666-6666-6666-666666666666"
    );
    assert_eq!(path.identity_guids().len(), 4);

    // The plain zero tail has no pairs; a pair without its `u32 2` is refused.
    let plain = exact_assembly_operand_path(&plain, 0, 65, plain.len(), path_link(), Some(TYPED))
        .unwrap()
        .expect("typed path without pairs");
    assert!(plain.paired_occurrence_keys().is_empty());
    // A path its class admits, not its type, has no pairs.
    let mut untyped = bytes.clone();
    untyped[4..7].copy_from_slice(b"390");
    assert!(
        exact_assembly_operand_path(&untyped, 0, 65, untyped.len(), path_link(), None)
            .unwrap()
            .is_none()
    );
    let marker = bytes.len() - 4 - 72 - 8 - 4;
    bytes[marker] = 3;
    assert!(
        exact_assembly_operand_path(&bytes, 0, 65, bytes.len(), path_link(), Some(TYPED))
            .unwrap()
            .is_none()
    );
}
