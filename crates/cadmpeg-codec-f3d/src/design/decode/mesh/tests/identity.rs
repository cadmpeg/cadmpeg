// SPDX-License-Identifier: Apache-2.0

use super::super::parse_mesh_collection_record;
use super::{synthetic_mesh_graph, typed_primary_frames, MESH_COLLECTION_TYPE_GUID};
use cadmpeg_core::CodecError;

#[test]
fn mesh_indexed_class_tag_copy_refuses_retained_bytes() {
    use cadmpeg_core::decode::ResourceDimension;

    let graph = synthetic_mesh_graph(false);
    let frames = typed_primary_frames(
        &graph.bytes,
        &graph.meta,
        MESH_COLLECTION_TYPE_GUID,
        "mesh-collection",
    )
    .unwrap();
    let [frame] = frames.as_slice() else {
        panic!("one mesh collection frame");
    };
    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::RetainedBytes,
        "copy F3D mesh indexed class tag",
        0,
        |ctx| parse_mesh_collection_record(ctx, &graph.bytes, &graph.meta, *frame).map(|_| ()),
    );
    assert!(matches!(
        refusal,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "copy F3D mesh indexed class tag"
                && limit.additional == 3
    ));
}
