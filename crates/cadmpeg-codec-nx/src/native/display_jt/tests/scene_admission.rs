// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::native::display_jt::{
    DisplayJtBaseNodeData, DisplayJtCompressedElement, DisplayJtTessellationInputs, JtSceneGraph,
    JtTessellationIndex,
};

/// Resolves the paths of node 7 in a one-node scene under an adjusted policy,
/// or walks the cap in `walk` until its named path operation refuses.
fn scene_node_paths_under(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    walk: Option<(ResourceDimension, &str)>,
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
    if let Some((dimension, operation)) = walk {
        return Err(crate::test_support::resource_refusal_at(
            &[],
            dimension,
            operation,
            |ctx| scene_node_paths(ctx, &inputs),
        ));
    }
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

#[test]
fn scene_node_paths_refuse_collection_limit() {
    drop(scene_node_paths_under(
        |_| {},
        Some((ResourceDimension::CollectionItems, "nx JT root path state")),
    ));
}

#[test]
fn scene_node_paths_refuse_scoped_limit() {
    drop(scene_node_paths_under(
        |_| {},
        Some((
            ResourceDimension::MaterializedBytes,
            "nx JT node path nodes",
        )),
    ));
}

#[test]
fn scene_node_paths_hold_temporary_paths_outside_retained_storage() {
    let paths = scene_node_paths_under(|policy| policy.limits.max_retained_bytes = 0, None)
        .expect("scene graph and paths are scoped");
    assert_eq!(paths, 1);
}

#[test]
fn scene_node_paths_refuse_work_limit() {
    drop(scene_node_paths_under(
        |_| {},
        Some((ResourceDimension::WorkUnits, "resolve JT path states")),
    ));
}

#[test]
fn jt_node_path_refuses_nesting_without_erasing_resource_error() {
    let error = scene_node_paths_under(|policy| policy.limits.max_recursion_depth = 0, None)
        .expect_err("node path exceeds the nesting limit");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RecursionDepth
                && limit.operation == "resolve JT node path"
    ));
}

fn scene_element(type_id: [u8; 16], base_type: u8, object_id: u32, body: &[u8]) -> Vec<u8> {
    let mut element = Vec::new();
    element.extend_from_slice(&u32::try_from(21 + body.len()).unwrap().to_le_bytes());
    element.extend_from_slice(&type_id);
    element.push(base_type);
    element.extend_from_slice(&object_id.to_le_bytes());
    element.extend_from_slice(body);
    element
}

#[test]
fn scene_node_families_reject_elements_independently() {
    use crate::container::{Container, DirEntry, DirEntryBody, Region};
    use crate::native::display_jt::{
        DisplayJtCompression, DisplayJtDocument, DisplayJtSegment, JtVersionField,
    };

    const INSTANCE: [u8; 16] = [
        0x2a, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    // An instance element with a shape-node base type: the instance family
    // rejects the segment, the base family still reads the element.
    let mut instance = Vec::new();
    instance.extend_from_slice(&1_u16.to_le_bytes());
    instance.extend_from_slice(&0x20_u32.to_le_bytes());
    instance.extend_from_slice(&1_u32.to_le_bytes());
    instance.extend_from_slice(&7_u32.to_le_bytes());
    instance.extend_from_slice(&1_u16.to_le_bytes());
    instance.extend_from_slice(&9_u32.to_le_bytes());
    let mut group = Vec::new();
    group.extend_from_slice(&1_u16.to_le_bytes());
    group.extend_from_slice(&0_u32.to_le_bytes());
    group.extend_from_slice(&0_u32.to_le_bytes());
    group.extend_from_slice(&1_u16.to_le_bytes());
    group.extend_from_slice(&2_u32.to_le_bytes());
    group.extend_from_slice(&7_u32.to_le_bytes());
    group.extend_from_slice(&9_u32.to_le_bytes());
    let mut inflated = scene_element(INSTANCE, 2, 1, &instance);
    inflated.extend_from_slice(&scene_element([0x11; 16], 1, 2, &group));
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);

    let compressed = crate::test_support::test_bytes::zlib_compress_at_level(&inflated, 1);
    let mut data = vec![0; 33];
    data.extend_from_slice(&compressed);
    let compression: DisplayJtCompression = serde_json::from_value(serde_json::json!({
        "flag": 2,
        "compressed_data_byte_len": compressed.len() + 1,
        "algorithm": 2,
        "compressed_byte_len": compressed.len(),
        "inflated_sha256": cadmpeg_ir::hash::sha256_hex(&inflated)
    }))
    .unwrap();
    let segment = DisplayJtSegment {
        id: "nx:jt:segment#0".into(),
        document: "nx:jt:document#0".into(),
        toc_entry: "nx:jt:toc-entry#0".into(),
        segment_id: [0; 16],
        segment_type: 1,
        segment_byte_len: u32::try_from(data.len()).unwrap(),
        payload_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&data[24..]),
        compression: Some(compression),
        source_offset: 0,
    };
    let version = match JtVersionField::new(
        format!("{:<80}", "Version 9.4 JT"),
        Ok::<_, std::convert::Infallible>,
        |text, _| Ok(text.parse()),
    ) {
        Ok(version) => version.unwrap(),
        Err(error) => match error {},
    };
    let document = DisplayJtDocument {
        id: "nx:jt:document#0".into(),
        index_row: "nx:jt:row#0".into(),
        version,
        toc_offset: 0,
        lsg_segment_id: [0; 16],
        toc_entries: Vec::new(),
        physical_byte_len: 0,
        source_offset: 0,
    };
    let container = Container {
        data: data.as_slice().into(),
        physical_size: cadmpeg_core::decode::u64_from_index(data.len()),
        layout: crate::container::test_modern_layout(6),
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".into(),
            region: Region::Footer,
            body: DirEntryBody::File {
                offset: 0,
                len: cadmpeg_core::decode::u64_from_index(data.len()),
            },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };

    let nodes = crate::test_support::with_decode_context(|ctx| {
        crate::native::display_jt::display_jt_scene_nodes(
            ctx,
            &container,
            std::slice::from_ref(&segment),
            std::slice::from_ref(&document),
        )
    })
    .unwrap();
    assert_eq!(nodes.base_nodes.len(), 2);
    assert_eq!(nodes.group_nodes.len(), 1);
    assert!(nodes.instance_nodes.is_empty());
    assert_eq!(nodes.group_nodes[0].child_object_ids, [7, 9]);
}
