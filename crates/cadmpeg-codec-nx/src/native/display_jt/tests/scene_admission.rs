// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::native::display_jt::{
    DisplayJtBaseNodeData, DisplayJtCompressedElement, DisplayJtTessellationInputs, JtSceneGraph,
    JtTessellationIndex,
};

/// Resolves the paths of node 7 in a one-node scene under an adjusted policy.
fn scene_node_paths_under(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<usize, CodecError> {
    let compressed = DisplayJtCompressedElement {
        id: "element".into(),
        segment: "scene".into(),
        segment_type: 0,
        ordinal: 0,
        object_type_id: [0; 16],
        object_base_type: 0,
        object_id: 7,
        body_byte_len: 0,
        body_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
        inflated_offset: 0,
        source_offset: 0,
    };
    let base = DisplayJtBaseNodeData {
        id: "base".into(),
        element: compressed.id.clone(),
        object_type_id: [0; 16],
        object_id: 7,
        version: 1,
        flags: 0,
        attribute_object_ids: Vec::new(),
        family_data_byte_len: 0,
        family_data_sha256: "00".repeat(32).try_into().expect("valid digest"),
        source_offset: 0,
    };
    let inputs = DisplayJtTessellationInputs {
        meshes: &[],
        coordinates: &[],
        normals: &[],
        colors: &[],
        texture_coordinates: &[],
        vertex_flags: &[],
        vertex_headers: &[],
        coordinate_headers: &[],
        shape_elements: &[],
        bindings: &[],
        shape_nodes: &[],
        base_nodes: &[base],
        group_nodes: &[],
        instance_nodes: &[],
        transforms: &[],
        materials: &[],
        compressed_elements: &[compressed],
    };
    crate::test_support::with_decode_context_over(&[], configure, |ctx| {
        scene_node_paths(ctx, &inputs)
    })
}

fn scene_node_paths(
    ctx: &DecodeContext<'_>,
    inputs: &DisplayJtTessellationInputs<'_>,
) -> Result<usize, CodecError> {
    let index = JtTessellationIndex::new(ctx, inputs)?;
    let graph = JtSceneGraph::new(ctx, "scene", inputs, &index)?.expect("complete scene graph");
    let (paths, _storage) = graph.node_paths(ctx, 7)?.expect("resolved node paths");
    Ok(paths.len())
}

fn refusal_dimension(result: Result<usize, CodecError>) -> ResourceDimension {
    match result {
        Err(CodecError::ResourceLimit(limit)) => limit.dimension,
        other => panic!("expected a resource refusal, got {other:?}"),
    }
}

#[test]
fn scene_node_paths_refuse_collection_limit() {
    assert_eq!(
        refusal_dimension(scene_node_paths_under(|policy| {
            policy.limits.max_collection_items = 0;
        })),
        ResourceDimension::CollectionItems
    );
}

#[test]
fn scene_node_paths_refuse_scoped_limit() {
    assert_eq!(
        refusal_dimension(scene_node_paths_under(|policy| {
            policy.limits.max_materialized_bytes = 0;
        })),
        ResourceDimension::MaterializedBytes
    );
}

#[test]
fn scene_node_paths_hold_temporary_paths_outside_retained_storage() {
    let paths = scene_node_paths_under(|policy| policy.limits.max_retained_bytes = 0)
        .expect("scene graph and paths are scoped");
    assert_eq!(paths, 1);
}

#[test]
fn scene_node_paths_refuse_work_limit() {
    assert_eq!(
        refusal_dimension(scene_node_paths_under(|policy| {
            policy.limits.max_work_units = 0;
        })),
        ResourceDimension::WorkUnits
    );
}

#[test]
fn jt_node_path_refuses_nesting_without_erasing_resource_error() {
    let error = scene_node_paths_under(|policy| policy.limits.max_recursion_depth = 0)
        .expect_err("node path exceeds the nesting limit");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RecursionDepth
                && limit.operation == "resolve JT node path"
    ));
}
