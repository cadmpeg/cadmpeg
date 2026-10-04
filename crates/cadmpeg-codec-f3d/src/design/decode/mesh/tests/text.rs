// SPDX-License-Identifier: Apache-2.0
use super::{
    sole_typed_frame, synthetic_mesh_graph, MESH_GUID_TYPE_GUID, MESH_TEXTURE_TABLE_TYPE_GUID,
};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

#[test]
fn mesh_guid_ownership_preserves_retained_refusals() {
    for textures in [false, true] {
        let graph = synthetic_mesh_graph(textures);
        let frame = sole_typed_frame(&graph, MESH_GUID_TYPE_GUID);
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::RetainedBytes,
            "retain F3D mesh GUID",
            0,
            |ctx| super::super::parse_mesh_guid_record(ctx, &graph.bytes, frame),
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "retain F3D mesh GUID"));
        if textures {
            {
                let frame = sole_typed_frame(&graph, MESH_TEXTURE_TABLE_TYPE_GUID);
                let error = crate::test_support::resource_refusal_at(
                    ResourceDimension::RetainedBytes,
                    "retain F3D mesh texture GUID",
                    0,
                    |ctx| super::super::parse_mesh_texture_table_record(ctx, &graph.bytes, frame),
                );
                assert!(matches!(error, CodecError::ResourceLimit(failure)
                    if failure.dimension == ResourceDimension::RetainedBytes && failure.operation == "retain F3D mesh texture GUID"));
            }
        }
    }
}

#[test]
fn mesh_indexed_class_tag_utf8_validation_refuses_work() {
    let mut record = Vec::new();
    record.extend_from_slice(&3u32.to_le_bytes());
    record.extend_from_slice(b"307");
    record.extend_from_slice(&42u32.to_le_bytes());
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "validate F3D mesh indexed class tag",
        0,
        |ctx| super::super::indexed_class_tag(ctx, &record, 0).map(|_| ()),
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "validate F3D mesh indexed class tag"
                && limit.additional == 3
    ));
}
