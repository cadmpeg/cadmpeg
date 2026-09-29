// SPDX-License-Identifier: Apache-2.0


fn scene_node_path_limit_error(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let compressed = crate::native::display_jt::DisplayJtCompressedElement {
        id: "element".into(),
        segment: "scene".into(),
        segment_type: 0,
        ordinal: 0,
        object_type_id: [0; 16],
        object_base_type: 0,
        object_id: 7,
        body_byte_len: 0,
        body_sha256: crate::native::display_jt::Sha256Hex::digest(&[]),
        inflated_offset: 0,
        source_offset: 0,
    };
    let base = crate::native::display_jt::DisplayJtBaseNodeData {
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
    let inputs = crate::native::display_jt::DisplayJtTessellationInputs {
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    crate::native::display_jt::display_jt_node_paths(&ctx, "scene", 7, &inputs)
        .err()
        .expect("scene node path limit refusal")
}

#[test]
fn scene_node_paths_refuse_collection_limit() {
    let error = scene_node_path_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn scene_node_paths_refuse_scoped_limit() {
    let error = scene_node_path_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn scene_node_paths_refuse_retained_limit() {
    let error = scene_node_path_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn scene_node_paths_refuse_work_limit() {
    let error = scene_node_path_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn jt_node_path_refuses_nesting_without_erasing_resource_error() {
    use std::collections::{BTreeMap, BTreeSet};

    let base = crate::native::display_jt::DisplayJtBaseNodeData {
        id: "base".into(),
        element: "scene".into(),
        object_type_id: [0; 16],
        object_id: 7,
        version: 1,
        flags: 0,
        attribute_object_ids: Vec::new(),
        family_data_byte_len: 0,
        family_data_sha256: "00".repeat(32).try_into().expect("valid digest"),
        source_offset: 0,
    };
    let mut by_object = BTreeMap::new();
    by_object.insert(7, &base);
    let parents = BTreeMap::new();
    let instance_ids = BTreeMap::new();
    let lookup = crate::native::display_jt::JtPathLookup {
        by_object: &by_object,
        parents: &parents,
        instance_ids: &instance_ids,
        transforms: &[],
        materials: &[],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    let error = crate::native::display_jt::resolve_display_jt_node_paths(
        &ctx,
        7,
        &lookup,
        &mut BTreeSet::new(),
        &mut ctx
            .reserve_scoped(0, "test JT visiting nodes")
            .expect("empty visiting reservation"),
    )
    .err()
    .expect("node path exceeds the nesting limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
                && limit.operation == "resolve JT node path"
    ));
}

