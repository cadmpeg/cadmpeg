// SPDX-License-Identifier: Apache-2.0
//! Metadata decode allocation boundaries.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn metadata_unknown_stream_slots_refuse_at_collection_limit() {
    let scan = crate::decode::Scan {
        container: crate::container::Container {
            data: Vec::new().into(),
            physical_size: 0,
            layout: crate::container::test_modern_layout(0x06),
            entries: Vec::new(),
            fastload_table: None,
            indexed_section_layouts: std::sync::OnceLock::new(),
            om_section_cache: std::sync::OnceLock::new(),
        },
        streams: vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Partition,
                schema: None,
            },
        }],
    };
    let (dialects, _) = crate::test_support::with_decode_context(|ctx| crate::dialect::classify_layers(ctx, &scan))
            .unwrap()
            .into_report_parts();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, root) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let error = super::super::build_metadata_ir(&ctx, root, &scan, &dialects)
        .expect_err("one unknown stream needs one collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "nx metadata unknown streams"
    ));
}
