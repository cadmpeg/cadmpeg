// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;
use super::{
    compact_feature_reference, exact_legacy_mirror_scope_count, exact_legacy_mirror_scope_tolerance,
};
use crate::design::decode::sketch::IndexedRecordOffsets;

fn indexed_header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) -> usize {
    let start = bytes.len();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    start
}

fn utf16(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(
        &u32::try_from(value.encode_utf16().count())
            .expect("test GUID length fits u32")
            .to_le_bytes(),
    );
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
}

#[test]
fn compact_mirror_reference_uses_the_identity_record_lane() {
    let record_index = 40;
    let reference = 17_u32;
    let mut bytes = Vec::new();
    let start = indexed_header(&mut bytes, *b"320", record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&(record_index + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    utf16(&mut bytes, "dfa12ed5-41e3-47c2-947d-286843e235df");
    utf16(&mut bytes, "15afb570-2968-417f-8485-96c81b2d332f");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"259", record_index);
    indexed_header(&mut bytes, *b"306", record_index + 1);
    indexed_header(&mut bytes, *b"291", record_index + 2);
    let identity = indexed_header(&mut bytes, *b"428", record_index + 3);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&reference.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"457", record_index + 4);
    let header = DesignRecordHeader {
        id: String::new(),
        record_index,
        class_tag: "320".into(),
        byte_offset: start as u64,
    };

    assert_eq!(
        compact_feature_reference(&bytes, &header),
        Some((reference, (identity + 21) as u64))
    );
    bytes[identity + 20] = 1;
    assert_eq!(compact_feature_reference(&bytes, &header), None);
}

#[test]
fn class_413_mirror_scope_decodes_inline_tolerance() {
    let mut bytes = vec![0; 89];
    let mut scope =
        DesignParameterScope::empty("scope", crate::records::DesignFeatureKind::Mirror, 10);
    scope.class_tag = "413".into();
    scope.paired_class_tag = "262".into();
    scope.kind_offset = 0;
    scope.previous_history_state_id_offset = Some(43);
    scope.frame_length = 89;
    scope.paired_byte_offset = 89;
    bytes[47..51].copy_from_slice(&89_u32.to_le_bytes());
    bytes[51..59].copy_from_slice(&0.25_f64.to_le_bytes());
    bytes[59..63].copy_from_slice(&89_u32.to_le_bytes());
    bytes[63] = 1;
    bytes[64..68].copy_from_slice(&12_u32.to_le_bytes());
    bytes[76] = 1;
    bytes[77..81].copy_from_slice(&11_u32.to_le_bytes());
    let (value, offset, carrier) =
        exact_legacy_mirror_scope_tolerance(&bytes, &scope).expect("class-413 tolerance");
    assert_eq!(value, 0.25);
    assert_eq!(offset, 51);
    assert_eq!(carrier.first_reference, 12);
    assert_eq!(carrier.second_reference, 11);
}

#[test]
fn class_369_mirror_scope_decodes_inline_tolerance() {
    let mut bytes = vec![0; 89];
    let mut scope =
        DesignParameterScope::empty("scope", crate::records::DesignFeatureKind::Mirror, 10);
    scope.class_tag = "369".into();
    scope.paired_class_tag = "261".into();
    scope.kind_offset = 0;
    scope.previous_history_state_id_offset = Some(43);
    scope.frame_length = 89;
    scope.paired_byte_offset = 89;
    bytes[47..51].copy_from_slice(&89_u32.to_le_bytes());
    bytes[51..59].copy_from_slice(&0.25_f64.to_le_bytes());
    bytes[59..63].copy_from_slice(&89_u32.to_le_bytes());
    bytes[63] = 1;
    bytes[64..68].copy_from_slice(&12_u32.to_le_bytes());
    bytes[76] = 1;
    bytes[77..81].copy_from_slice(&11_u32.to_le_bytes());
    let (value, offset, carrier) =
        exact_legacy_mirror_scope_tolerance(&bytes, &scope).expect("class-369 tolerance");
    assert_eq!(value, 0.25);
    assert_eq!(offset, 51);
    assert_eq!(carrier.marker, 89);
    assert_eq!(carrier.repeated_marker_offset, Some(59));
    assert_eq!(carrier.first_reference, 12);
    assert_eq!(carrier.second_reference, 11);

    bytes[59..63].copy_from_slice(&90_u32.to_le_bytes());
    assert_eq!(exact_legacy_mirror_scope_tolerance(&bytes, &scope), None);
}

#[test]
fn class_391_mirror_scope_decodes_inline_tolerance() {
    let mut bytes = vec![0; 88];
    let mut scope =
        DesignParameterScope::empty("scope", crate::records::DesignFeatureKind::Mirror, 10);
    scope.class_tag = "391".into();
    scope.paired_class_tag = "261".into();
    scope.kind_offset = 0;
    scope.previous_history_state_id_offset = Some(42);
    scope.frame_length = 88;
    scope.paired_byte_offset = 88;
    bytes[46..50].copy_from_slice(&94_u32.to_le_bytes());
    bytes[50..58].copy_from_slice(&0.25_f64.to_le_bytes());
    bytes[58..62].copy_from_slice(&94_u32.to_le_bytes());
    bytes[62] = 1;
    bytes[63..67].copy_from_slice(&12_u32.to_le_bytes());
    bytes[75] = 1;
    bytes[76..80].copy_from_slice(&11_u32.to_le_bytes());
    let (value, offset, carrier) =
        exact_legacy_mirror_scope_tolerance(&bytes, &scope).expect("class-391 tolerance");
    assert_eq!(value, 0.25);
    assert_eq!(offset, 50);
    assert_eq!(carrier.marker, 94);
    assert_eq!(carrier.repeated_marker_offset, Some(58));
    assert_eq!(carrier.first_reference, 12);
    assert_eq!(carrier.second_reference, 11);

    bytes[58..62].copy_from_slice(&95_u32.to_le_bytes());
    assert_eq!(exact_legacy_mirror_scope_tolerance(&bytes, &scope), None);
}

#[test]
fn class_440_mirror_scope_decodes_inline_tolerance() {
    let mut bytes = vec![0; 89];
    let mut scope =
        DesignParameterScope::empty("scope", crate::records::DesignFeatureKind::Mirror, 10);
    scope.class_tag = "440".into();
    scope.paired_class_tag = "258".into();
    scope.kind_offset = 0;
    scope.previous_history_state_id_offset = Some(43);
    scope.frame_length = 89;
    scope.paired_byte_offset = 89;
    bytes[47..51].copy_from_slice(&100_u32.to_le_bytes());
    bytes[51..59].copy_from_slice(&0.25_f64.to_le_bytes());
    bytes[59..63].copy_from_slice(&100_u32.to_le_bytes());
    bytes[63] = 1;
    bytes[64..68].copy_from_slice(&12_u32.to_le_bytes());
    bytes[76] = 1;
    bytes[77..81].copy_from_slice(&11_u32.to_le_bytes());
    let (value, offset, carrier) =
        exact_legacy_mirror_scope_tolerance(&bytes, &scope).expect("class-440 tolerance");
    assert_eq!(value, 0.25);
    assert_eq!(offset, 51);
    assert_eq!(carrier.marker, 100);
    assert_eq!(carrier.first_reference, 12);
    assert_eq!(carrier.second_reference, 11);
}

#[test]
fn class_441_mirror_scope_decodes_the_unrepeated_inline_tolerance() {
    let mut bytes = vec![0; 84];
    let mut scope =
        DesignParameterScope::empty("scope", crate::records::DesignFeatureKind::Mirror, 10);
    scope.class_tag = "441".into();
    scope.paired_class_tag = "267".into();
    scope.kind_offset = 0;
    scope.previous_history_state_id_offset = Some(42);
    scope.frame_length = 84;
    scope.paired_byte_offset = 84;
    bytes[46..50].copy_from_slice(&61_u32.to_le_bytes());
    bytes[50..58].copy_from_slice(&0.125_f64.to_le_bytes());
    bytes[58] = 1;
    bytes[59..63].copy_from_slice(&12_u32.to_le_bytes());
    bytes[71] = 1;
    bytes[72..76].copy_from_slice(&11_u32.to_le_bytes());
    let (value, offset, carrier) =
        exact_legacy_mirror_scope_tolerance(&bytes, &scope).expect("class-441 tolerance");
    assert_eq!(value, 0.125);
    assert_eq!(offset, 50);
    assert_eq!(carrier.marker, 61);
    assert_eq!(carrier.repeated_marker_offset, None);
    assert_eq!(carrier.first_reference, 12);
    assert_eq!(carrier.second_reference, 11);
}

#[test]
fn class_441_mirror_scope_decodes_the_inline_count_owner() {
    let scope_record_index: u32 = 65;
    let count_record_index: u32 = 80;
    let mut bytes = vec![0; 99];
    bytes[0..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"426");
    bytes[7..11].copy_from_slice(&count_record_index.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
    bytes[24] = 1;
    bytes[25..29].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[35..39].copy_from_slice(&0_u32.to_le_bytes());
    bytes[40..44].copy_from_slice(&2_u32.to_le_bytes());
    bytes[44] = 1;
    bytes[45..49].copy_from_slice(&(count_record_index + 2).to_le_bytes());
    bytes[55..59].copy_from_slice(&1_u32.to_le_bytes());
    bytes[63] = 1;
    bytes[64..68].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[76] = 1;
    bytes[77..81].copy_from_slice(&(count_record_index + 1).to_le_bytes());
    bytes[88] = 1;
    bytes[89..93].copy_from_slice(&scope_record_index.to_le_bytes());
    indexed_header(&mut bytes, *b"267", count_record_index);

    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::DesignFeatureKind::Mirror,
        scope_record_index,
    );
    scope.class_tag = "441".into();
    scope.paired_class_tag = "267".into();
    scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![1, 2, 3, count_record_index]);
    let records = IndexedRecordOffsets::build(&bytes);

    assert_eq!(
        exact_legacy_mirror_scope_count(&bytes, &records, &scope),
        Some((2, count_record_index, 40))
    );
}
