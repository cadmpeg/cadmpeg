// SPDX-License-Identifier: Apache-2.0

use super::super::parse_mesh_design_records;
use super::{
    no_texture_asset, put_reference, sole_typed_frame, synthetic_mesh_graph_with_body_count,
    MESH_FEATURE_SCOPE_TYPE_GUID,
};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

/// Each diagnostic list of a two-body collection whose scope repeats its
/// first body admits its items.
#[test]
fn mesh_scope_diagnostic_lists_refuse_collection_items() {
    let mut graph = synthetic_mesh_graph_with_body_count(false, 2);
    let start = sole_typed_frame(&graph, MESH_FEATURE_SCOPE_TYPE_GUID).start;
    put_reference(&mut graph.bytes, start + 36, 104);
    for operation in [
        "f3d mesh diagnostic collection bodies",
        "f3d mesh diagnostic scope bodies",
        "f3d mesh diagnostic body links",
    ] {
        let dimension = ResourceDimension::CollectionItems;
        let refusal = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            let mut no_asset = no_texture_asset;
            parse_mesh_design_records(
                ctx,
                &graph.bytes,
                &graph.meta,
                "Synthetic/BulkStream.dat",
                &mut no_asset,
            )
            .map(|_| ())
        });
        assert!(matches!(
            refusal,
            CodecError::ResourceLimit(limit)
                if limit.dimension == dimension
                    && limit.operation == operation
                    && limit.additional == 1
        ));
    }
    crate::test_support::with_decode_context(|ctx| {
        let mut no_asset = no_texture_asset;
        let error = parse_mesh_design_records(
            ctx,
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_asset,
        )
        .expect_err("disagreeing scope body list");
        assert!(matches!(
            error,
            CodecError::Malformed(message)
                if message.ends_with(
                    "collection 100 bodies [104, 117], scope Some(109) bodies Some([104, 104]), \
                     body links [(104, 109, 100), (117, 109, 100)]"
                )
        ));
    });
}
