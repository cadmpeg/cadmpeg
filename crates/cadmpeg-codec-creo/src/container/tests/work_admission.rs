// SPDX-License-Identifier: Apache-2.0
use super::super::{read_array_count, structural_feature_ids, Section};
use cadmpeg_core::CodecError;

#[test]
fn parent_feature_arrays_accept_wide_feature_identities() {
    let bytes = b"parent_feats\0\xf8\x03\x7f\xbf\xff\xc0\x40\0\xf7\x10";
    let sections = [
        Section::scan("VisibGeom".into(), 0, bytes.len(), None, bytes).expect("bounded section"),
    ];
    let ids = structural_feature_ids(&sections, &[], &[]).expect("complete parent roster");
    assert_eq!(ids, [127, 16383, 16384].into_iter().collect());
}

#[test]
fn parent_feature_search_over_many_empty_arrays_finds_no_ids() {
    let bytes = b"parent_feats\0\xf8\x01\0".repeat(1000);
    let sections = [
        Section::scan("VisibGeom".into(), 0, bytes.len(), None, &bytes).expect("bounded section"),
    ];
    assert!(structural_feature_ids(&sections, &[], &[])
        .expect("linear scan")
        .is_empty());
}

#[test]
fn parent_feature_zero_ids_are_ignored() {
    let bytes = b"parent_feats\0\xf8\x03\0\0\0";
    let section =
        Section::scan("VisibGeom".into(), 0, bytes.len(), None, bytes).expect("bounded section");
    let sections = [section];
    let ids = structural_feature_ids(&sections, &[], &[]).expect("zero ids are complete");
    assert!(ids.is_empty());
}

#[test]
fn geometry_census_rejects_truncated_structural_counts() {
    for bytes in [
        b"srf_array\0\xf8".as_slice(),
        b"srf_array\0\xf8\x81".as_slice(),
    ] {
        let error = read_array_count(bytes, b"srf_array").expect_err("count is incomplete");
        assert!(matches!(error, CodecError::Malformed(_)));
    }
    assert_eq!(
        read_array_count(b"srf_array\0\xf8\0", b"srf_array").expect("complete zero count"),
        Some(0)
    );
}

#[test]
fn parent_feature_arrays_reject_truncated_counts_and_entries() {
    for bytes in [
        b"parent_feats\0\xf8".as_slice(),
        b"parent_feats\0\xf8\x81".as_slice(),
        b"parent_feats\0\xf8\x01\x81".as_slice(),
    ] {
        let sections =
            [Section::scan("VisibGeom".into(), 0, bytes.len(), None, bytes).expect("section")];
        let error = structural_feature_ids(&sections, &[], &[]).expect_err("array is incomplete");
        assert!(matches!(error, CodecError::Malformed(_)));
    }
}
