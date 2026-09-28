// SPDX-License-Identifier: Apache-2.0

use crate::native::features::offset_data_block_bytes_for_section;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn offset_block_view_refusal(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let control = crate::om::EntityRecord {
        offset: 5,
        bytes: &[0xaa],
    };
    let column = crate::om::EntityRecord {
        offset: 6,
        bytes: &[0xbb],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    let mut reservation = ctx.reserve_scoped(0, "NX offset block view storage")
        .expect("empty reservation");
    let mut blocks = BTreeMap::new();
    offset_data_block_bytes_for_section(
        &ctx,
        &mut reservation,
        &mut blocks,
        3,
        100,
        &control,
        &[column],
    )
    .unwrap_err()
}

#[test]
fn offset_block_view_refuses_collection_limit() {
    let error = offset_block_view_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "NX offset block view entries"));
}

#[test]
fn offset_block_view_refuses_scoped_limit() {
    let error = offset_block_view_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "NX offset block view storage"));
}

#[test]
fn offset_block_view_refuses_work_limit() {
    let error = offset_block_view_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index NX offset block view"));
}
