// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use super::{
    no_texture_asset, put_reference, sole_typed_frame, synthetic_mesh_graph_with_body_count,
    MESH_FEATURE_SCOPE_TYPE_GUID,
};
use super::super::parse_mesh_design_records;

#[test]
fn mesh_diagnostic_scope_body_copy_refuses_each_resource_limit() {
    let mut graph = synthetic_mesh_graph_with_body_count(false, 2);
    let start = sole_typed_frame(&graph, MESH_FEATURE_SCOPE_TYPE_GUID).start;
    put_reference(&mut graph.bytes, start + 36, 104);
    for (dimension, additional) in [
        (ResourceDimension::WorkUnits, 10),
        (ResourceDimension::CollectionItems, 2),
        (ResourceDimension::RetainedBytes, 8),
    ] {
        let refusal = crate::test_support::resource_refusal_at(
            dimension,
            "f3d mesh diagnostic scope bodies",
            0,
            |ctx| {
                let mut no_asset = no_texture_asset;
                parse_mesh_design_records(
                    ctx,
                    &graph.bytes,
                    &graph.meta,
                    "Synthetic/BulkStream.dat",
                    &mut no_asset,
                )
                .map(|_| ())
            },
        );
        assert!(matches!(
            refusal,
            CodecError::ResourceLimit(limit)
                if limit.dimension == dimension
                    && limit.operation == "f3d mesh diagnostic scope bodies"
                    && limit.additional == additional
        ));
    }
}
