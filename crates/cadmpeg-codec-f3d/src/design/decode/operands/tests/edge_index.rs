// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::operands::{decode_edge_operands, insert_edge_member_index};
use crate::records::decal::DesignRecordHeader;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn edge_operand_header_and_offset_indices_refuse_collection_limits() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned()).unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        for (limit, operation) in [
            (0, "f3d edge operand header index"),
            (1, "f3d edge operand offset stream"),
            (2, "f3d edge operand stream offset"),
        ] {
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                decode_edge_operands(&ctx, scan, &[], &[], std::slice::from_ref(&header), &[]),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
    });
}

#[test]
fn edge_operand_member_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut indices = std::collections::HashSet::new();
    assert!(matches!(
        insert_edge_member_index(&ctx, &mut indices, 7),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d edge operand member index"
    ));
}
