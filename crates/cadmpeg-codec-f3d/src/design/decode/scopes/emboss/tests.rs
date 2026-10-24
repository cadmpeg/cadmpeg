// SPDX-License-Identifier: Apache-2.0

use super::exact_emboss_construction;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::test_support::lp_utf16;

fn reference(bytes: &mut Vec<u8>, index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&u64::from(index).to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
}

fn fixture(start: usize) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = vec![0; start];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"900");
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for index in [60, 72, 63] {
        reference(&mut bytes, index);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&17u64.to_le_bytes());
    bytes.extend_from_slice(&23u64.to_le_bytes());
    for index in [69, 48, 66] {
        reference(&mut bytes, index);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 41);
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 48);
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    assert_eq!(bytes.len() - start, 235);
    bytes.resize(start + 430, 0);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design1/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Emboss,
        40,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64::try_from(start).unwrap();
            draft.class_tag = "900".to_owned().try_into().unwrap();
            draft.paired_class_tag = "901".to_owned().try_into().unwrap();
            draft.frame_length = 430;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = draft.byte_offset + 235;
            draft.reference_members =
                ReferenceRun::unlocated(vec![41, 44, 48, 51, 60, 63, 66, 69, 72]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn emboss_prefix_retains_control_words_and_locates_all_owner_and_group_links() {
    for start in [0, 16] {
        let (bytes, scope) = fixture(start);
        let decoded = exact_emboss_construction(&bytes, &scope).unwrap();
        assert_eq!(decoded.control_words, [1, 0]);
        assert_eq!(decoded.native_pair, [17, 23]);
        assert_eq!(decoded.depth_owner.value, 60);
        assert_eq!(decoded.auxiliary_group.value, 72);
        assert_eq!(decoded.horizontal_owner.value, 63);
        assert_eq!(decoded.rotation_owner.value, 69);
        assert_eq!(decoded.face_group.value, 48);
        assert_eq!(decoded.vertical_owner.value, 66);
        assert_eq!(decoded.profile_group.value, 41);
        assert_eq!(
            decoded.depth_owner.offset,
            u64::try_from(start).unwrap() + 29
        );
        assert_eq!(
            decoded.profile_group.offset,
            u64::try_from(start).unwrap() + 119
        );
        assert!(decoded.links_match(&scope));
    }
}

#[test]
fn emboss_prefix_refuses_invalid_counts_references_zero_runs_and_guid() {
    let (bytes, scope) = fixture(0);
    for offset in [
        0, 4, 7, 11, 28, 29, 33, 37, 61, 114, 129, 141, 146, 156, 160, 232,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 2;
        assert!(
            exact_emboss_construction(&changed, &scope).is_none(),
            "offset {offset}"
        );
    }
    for length in 0..bytes.len() {
        assert!(exact_emboss_construction(&bytes[..length], &scope).is_none());
    }
}

#[test]
fn emboss_prefix_keeps_unassigned_control_values_native() {
    let (mut bytes, scope) = fixture(0);
    bytes[20..24].copy_from_slice(&7u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&9u32.to_le_bytes());
    let decoded = exact_emboss_construction(&bytes, &scope).unwrap();
    assert_eq!(decoded.control_words, [7, 9]);
}

#[test]
fn emboss_prefix_refuses_reference_table_reordering() {
    let (bytes, mut scope) = fixture(0);
    scope
        .try_edit(|draft| {
            draft.reference_members =
                ReferenceRun::unlocated(vec![41, 44, 48, 51, 63, 60, 66, 69, 72]);
            draft.layout_fixture_references();
        })
        .unwrap();
    assert!(exact_emboss_construction(&bytes, &scope).is_none());
}
