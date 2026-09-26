// SPDX-License-Identifier: Apache-2.0
//! Connected shell component admission.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::FaceId;

#[test]
fn connected_face_neighbors_refuse_collection_limit() {
    let faces = [
        FaceId::try_from("step:data:face#1").expect("test face id"),
        FaceId::try_from("step:data:face#2").expect("test face id"),
    ];
    let arena = DecodeArena::new();
    let (service_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let components =
        super::super::connected_face_components(&faces, &[], &[], &BTreeMap::new(), &service_ctx)
            .expect("service admits two face neighbors");
    assert_eq!(components, vec![vec![0], vec![1]]);

    let mut limited = DecodePolicy::service();
    limited.limits.max_collection_items = 1;
    let (limited_ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &limited).expect("limited context");
    let error =
        super::super::connected_face_components(&faces, &[], &[], &BTreeMap::new(), &limited_ctx)
            .expect_err("two face neighbors exceed one collection item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "STEP connected-face neighbors"
    ));
}
