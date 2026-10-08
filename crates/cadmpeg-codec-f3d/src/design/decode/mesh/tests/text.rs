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
                    |ctx| {
                        let mut storage = ctx.reserve_scoped(0, "test texture storage")?;
                        super::super::parse_mesh_texture_table_record(
                            ctx,
                            &mut storage,
                            &graph.bytes,
                            frame,
                        )
                        .map(|_| ())
                    },
                );
                assert!(matches!(error, CodecError::ResourceLimit(failure)
                    if failure.dimension == ResourceDimension::RetainedBytes && failure.operation == "retain F3D mesh texture GUID"));
            }
        }
    }
}
