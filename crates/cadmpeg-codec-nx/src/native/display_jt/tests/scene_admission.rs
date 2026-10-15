// SPDX-License-Identifier: Apache-2.0

#[test]
fn scene_node_paths_resolve_a_root_shape_node() {
    let compressed = crate::native::display_jt::DisplayJtCompressedElement {
        id: "element".into(),
        segment: "scene".into(),
        segment_type: 0,
        ordinal: 0,
        object_type_id: [0; 16].into(),
        object_base_type: 0,
        object_id: 7,
        body_byte_len: 0,
        body_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
        inflated_offset: 0,
        source_offset: 0,
    };
    let base = crate::native::display_jt::DisplayJtBaseNodeData {
        id: "base".into(),
        element: compressed.id.clone(),
        object_type_id: [0; 16].into(),
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

    let paths = crate::test_support::with_decode_context(|ctx| {
        crate::native::display_jt::display_jt_node_paths(ctx, "scene", 7, &inputs)
    })
    .expect("scene node paths decode")
    .expect("complete scene node path");
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].node_path, [7]);
}

#[test]
fn jt_node_path_refuses_nesting_without_erasing_resource_error() {
    use std::collections::{BTreeMap, BTreeSet};

    let base = crate::native::display_jt::DisplayJtBaseNodeData {
        id: "base".into(),
        element: "scene".into(),
        object_type_id: [0; 16].into(),
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

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_recursion_depth = 0;
        },
        |ctx| {
            let error = crate::native::display_jt::resolve_display_jt_node_paths(
                ctx,
                7,
                &lookup,
                &mut BTreeSet::new(),
                &cadmpeg_core::decode::WorkBudget::new(
                    crate::native::display_jt::MAX_JT_NODE_PATH_VISITS,
                ),
            )
            .err()
            .expect("node path exceeds the nesting limit");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
                        && limit.operation == "resolve JT node path"
            ));
        },
    );
}

#[test]
fn jt_node_path_skips_after_the_visit_bound() {
    use std::collections::{BTreeMap, BTreeSet};

    let base = crate::native::display_jt::DisplayJtBaseNodeData {
        id: "base".into(),
        element: "scene".into(),
        object_type_id: [0; 16].into(),
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

    crate::test_support::with_decode_context(|ctx| {
        assert!(crate::native::display_jt::resolve_display_jt_node_paths(
            ctx,
            7,
            &lookup,
            &mut BTreeSet::new(),
            &cadmpeg_core::decode::WorkBudget::new(0),
        )
        .expect("an exhausted visit bound skips the node")
        .is_none());
    });
}
