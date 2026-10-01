// SPDX-License-Identifier: Apache-2.0
use super::super::{read_array_count, structural_feature_ids, Section};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn parent_feature_zero_ids_refuse_search_and_entry_work() {
    let bytes = b"parent_feats\0\xf8\x03\0\0\0";
    let section =
        Section::scan("VisibGeom".into(), 0, bytes.len(), None, bytes).expect("bounded section");
    let sections = [section];
    let ids = crate::test_support::assert_work_boundaries(
        &[
            "creo structural feature sections",
            "creo parent-feature search",
            "creo parent-feature entries",
        ],
        |ctx| structural_feature_ids(ctx, &sections, &[], &[]),
    );
    assert!(ids.is_empty());
}

#[test]
fn geometry_census_charges_each_namespace_pass() {
    let region = [0; 64];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        read_array_count(&ctx, &region, b"srf_array").expect("first scan"),
        None
    );
    let error =
        read_array_count(&ctx, &region, b"crv_array").expect_err("independent scan needs work");
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "creo geometry census search");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn geometry_census_rejects_truncated_structural_counts() {
    for bytes in [
        b"srf_array\0\xf8".as_slice(),
        b"srf_array\0\xf8\x81".as_slice(),
    ] {
        let error =
            crate::decode::with_test_decode_ctx(|ctx| read_array_count(ctx, bytes, b"srf_array"))
                .expect_err("count is incomplete");
        assert!(matches!(error, CodecError::Malformed(_)));
    }
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| read_array_count(
            ctx,
            b"srf_array\0\xf8\0",
            b"srf_array"
        ))
        .expect("complete zero count"),
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
        let error = crate::decode::with_test_decode_ctx(|ctx| {
            structural_feature_ids(ctx, &sections, &[], &[])
        })
        .expect_err("array is incomplete");
        assert!(matches!(error, CodecError::Malformed(_)));
    }
}
