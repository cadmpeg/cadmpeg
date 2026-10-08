// SPDX-License-Identifier: Apache-2.0

use super::super::parse_mesh_design_records;
use super::{synthetic_mesh_graph, synthetic_mesh_graph_with_body_count};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

fn mesh_push_refusal(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    operation: &str,
    skip: usize,
) -> CodecError {
    crate::test_support::resource_refusal_at(
        ResourceDimension::CollectionItems,
        operation,
        skip,
        |ctx| {
            let mut asset_for_filename = |filename: &str| {
                let entry = format!("Synthetic/Textures/{filename}");
                Ok((entry.clone(), crate::ids::neutral_asset_id(&entry)))
            };
            parse_mesh_design_records(
                ctx,
                bytes,
                meta,
                "Synthetic/BulkStream.dat",
                &mut asset_for_filename,
            )
            .map(|_| ())
        },
    )
}

#[test]
fn mesh_output_pushes_refuse_each_collection_item() {
    let no_textures = synthetic_mesh_graph(false);
    crate::test_support::with_decode_context(|ctx| {
        let mut asset_for_filename = |filename: &str| {
            let entry = format!("Synthetic/Textures/{filename}");
            Ok((entry.clone(), crate::ids::neutral_asset_id(&entry)))
        };
        let design = parse_mesh_design_records(
            ctx,
            &no_textures.bytes,
            &no_textures.meta,
            "Synthetic/BulkStream.dat",
            &mut asset_for_filename,
        )
        .expect("valid mesh graph");
        let [feature] = design.as_slice() else {
            panic!("one mesh feature");
        };
        assert_eq!(feature.bodies().len(), 1);
        assert!(feature.texture_table.resources().is_empty());
    });

    for operation in ["f3d mesh feature bodies", "f3d mesh graph features"] {
        let refusal = mesh_push_refusal(&no_textures.bytes, &no_textures.meta, operation, 0);
        assert!(matches!(
            refusal,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
                    && limit.additional == 1
        ));
    }

    let two_bodies = synthetic_mesh_graph_with_body_count(false, 2);
    let second_body_refusal = mesh_push_refusal(
        &two_bodies.bytes,
        &two_bodies.meta,
        "f3d mesh feature bodies",
        1,
    );
    assert!(matches!(
        second_body_refusal,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d mesh feature bodies"
                && limit.additional == 1
    ));

    let textured = synthetic_mesh_graph(true);
    crate::test_support::with_decode_context(|ctx| {
        let mut asset_for_filename = |filename: &str| {
            let entry = format!("Synthetic/Textures/{filename}");
            Ok((entry.clone(), crate::ids::neutral_asset_id(&entry)))
        };
        let design = parse_mesh_design_records(
            ctx,
            &textured.bytes,
            &textured.meta,
            "Synthetic/BulkStream.dat",
            &mut asset_for_filename,
        )
        .expect("valid textured mesh graph");
        let [feature] = design.as_slice() else {
            panic!("one textured mesh feature");
        };
        assert_eq!(feature.texture_table.resources().len(), 2);
    });
    for skip in 0..2 {
        let refusal = mesh_push_refusal(
            &textured.bytes,
            &textured.meta,
            "f3d mesh texture resources",
            skip,
        );
        assert!(matches!(
            refusal,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "f3d mesh texture resources"
                    && limit.additional == 1
        ));
    }
}
