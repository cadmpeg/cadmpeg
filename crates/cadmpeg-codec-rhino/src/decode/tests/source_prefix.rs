// SPDX-License-Identifier: Apache-2.0

use super::{object_record, scan_with_objects, ArchiveVersion, DecodeContext, POINT_CLASS};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn object_candidate_index_visits_first_source_before_large_suffix() {
    const SOURCE_COUNT: usize = 1_025;
    let objects = (0..SOURCE_COUNT)
        .map(|_| object_record(ArchiveVersion::V5, 1, POINT_CLASS))
        .collect::<Vec<_>>();
    let scan = scan_with_objects(&objects);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Bulk admission of all source records exceeds this cap. One visited
    // record and its fixed-width UUID lookup fit before the first map slot.
    policy.limits.max_work_units = 1_024;
    policy.limits.max_collection_items = 0;
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
            .expect("source bytes fit the root policy");
    let error = match DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root)) {
        Err(error) => error,
        Ok(_) => panic!("the first candidate map slot must refuse"),
    };
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("candidate admission must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
    assert_eq!(refusal.operation, "Rhino object candidate keys");
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == refusal
    ));
}
