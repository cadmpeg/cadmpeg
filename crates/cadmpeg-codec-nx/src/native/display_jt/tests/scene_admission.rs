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
        crate::native::display_jt::display_jt_node_paths(
            ctx,
            "scene",
            7,
            &inputs,
            &cadmpeg_core::decode::WorkBudget::new(
                crate::native::display_jt::MAX_JT_NODE_PATH_VISITS,
            ),
        )
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

#[test]
fn jt_tessellations_report_the_visit_bound_as_a_scan_loss() {
    use super::super::{
        DisplayJtBaseNodeData, DisplayJtCompressedElement, DisplayJtCompressedElementWire,
        DisplayJtGroupNodeData, DisplayJtPolygonMesh, DisplayJtPolygonMeshWire,
        DisplayJtShapeLodBinding, DisplayJtShapeLodElement, DisplayJtTriStripShapeNode,
        DisplayJtVertexCoordinateArrayHeader, DisplayJtVertexCoordinates, JtArea, JtBounds,
        JtVertexVersion, QuantizedRange,
    };

    fn base(id: String, object_id: u32) -> DisplayJtBaseNodeData {
        DisplayJtBaseNodeData {
            id,
            element: "scene-element".into(),
            object_type_id: [0; 16].into(),
            object_id,
            version: 1,
            flags: 0,
            attribute_object_ids: Vec::new(),
            family_data_byte_len: 0,
            family_data_sha256: "00".repeat(32).try_into().expect("valid digest"),
            source_offset: 0,
        }
    }

    // Two group nodes per level, each the parent of both nodes one level
    // below, give the shape node 2^levels instance paths.
    fn tessellate(levels: u32) -> (usize, Vec<cadmpeg_ir::report::loss::LossNote>) {
        let mesh = DisplayJtPolygonMesh::try_from(DisplayJtPolygonMeshWire {
            id: "mesh".into(),
            topology: "topology".into(),
            coordinate_header: "coordinate-header".into(),
            polygons: vec![vec![0, 1, 2]],
            vertex_attribute_indices: vec![vec![None; 3]],
            polygon_groups: vec![0],
            polygon_flags: vec![0],
            source_offset: 0,
        })
        .expect("valid mesh");
        let coordinates = DisplayJtVertexCoordinates {
            id: "coordinates".into(),
            header: "coordinate-header".into(),
            points_m: vec![super::finite([0.0; 3]); 3],
            coordinate_hash: 0,
            byte_len: 4,
            source_offset: 0,
        };
        let header = DisplayJtVertexCoordinateArrayHeader {
            id: "coordinate-header".into(),
            element: "shape-element".into(),
            unique_vertex_count: 3,
            component_count: 3,
            component_ranges: [QuantizedRange::ZERO; 3],
            component_quantization_bits: [0; 3].into(),
            compressed_components_byte_len: 4,
            compressed_components_sha256: "00".repeat(32).try_into().expect("valid digest"),
            source_offset: 0,
        };
        let shape_element = DisplayJtShapeLodElement {
            id: "shape-element".into(),
            segment: "shape-segment".into(),
            ordinal: 0,
            object_type_id: [0; 16].into(),
            object_id: 7,
            body_byte_len: 0,
            body_sha256: "00".repeat(32).try_into().expect("valid digest"),
            source_offset: 0,
        };
        let binding = DisplayJtShapeLodBinding {
            id: "binding".into(),
            scene_segment: "scene-segment".into(),
            table_version: 1,
            shape_node_object_id: 9,
            key_object_id: 1,
            key: "JT_LLPROP_SHAPEIMPL".into(),
            value_object_id: 2,
            state_flags: 0,
            property_version: 1,
            shape_segment: "shape-segment".into(),
            payload_object_id: 7,
            reserved_value: 1,
            source_offset: 0,
        };
        let shape_node = DisplayJtTriStripShapeNode {
            id: "shape-node".into(),
            base_node: "base-9".into(),
            object_id: 9,
            reserved_bounds: JtBounds::try_from([[0.0; 3]; 2]).expect("valid bounds"),
            untransformed_bounds: JtBounds::try_from([[0.0; 3]; 2]).expect("valid bounds"),
            area: JtArea::new(0.0).expect("valid area"),
            vertex_count_range: [0, 0],
            node_count_range: [0, 0],
            polygon_count_range: [0, 0],
            memory_byte_len: 0,
            compression_level: 0.0.try_into().expect("valid level"),
            vertex_version: JtVertexVersion::One,
            vertex_bindings: 2,
            vertex_quantization_bits: 0,
            normal_quantization_factor: 0,
            texture_quantization_bits: 0,
            color_quantization_bits: 0,
            source_offset: 0,
        };
        let compressed: DisplayJtCompressedElement = DisplayJtCompressedElementWire {
            id: "scene-element".into(),
            segment: "scene-segment".into(),
            segment_type: 1,
            ordinal: 0,
            object_type_id: [0; 16].into(),
            object_base_type: 2,
            object_id: 9,
            body_byte_len: 0,
            body_sha256: "00".repeat(32).try_into().expect("valid digest"),
            inflated_offset: 0,
            source_offset: 0,
        }
        .try_into()
        .expect("valid element");
        let mut base_nodes = vec![base("base-9".into(), 9)];
        let mut group_nodes = Vec::new();
        let mut children = vec![9];
        for level in 0..levels {
            let pair = [100 + 2 * level, 101 + 2 * level];
            for object_id in pair {
                base_nodes.push(base(format!("base-{object_id}"), object_id));
                group_nodes.push(DisplayJtGroupNodeData {
                    id: format!("group-{object_id}"),
                    base_node: format!("base-{object_id}"),
                    object_id,
                    version: 1,
                    child_object_ids: children.clone(),
                    family_data_byte_len: 0,
                    family_data_sha256: "00".repeat(32).try_into().expect("valid digest"),
                    source_offset: 0,
                });
            }
            children = pair.to_vec();
        }
        let inputs = crate::native::display_jt::DisplayJtTessellationInputs {
            meshes: std::slice::from_ref(&mesh),
            coordinates: std::slice::from_ref(&coordinates),
            normals: &[],
            colors: &[],
            texture_coordinates: &[],
            vertex_flags: &[],
            vertex_headers: &[],
            coordinate_headers: std::slice::from_ref(&header),
            shape_elements: std::slice::from_ref(&shape_element),
            bindings: std::slice::from_ref(&binding),
            shape_nodes: std::slice::from_ref(&shape_node),
            base_nodes: &base_nodes,
            group_nodes: &group_nodes,
            instance_nodes: &[],
            transforms: &[],
            materials: &[],
            compressed_elements: std::slice::from_ref(&compressed),
        };
        let mut losses = Vec::new();
        let tessellations = crate::test_support::with_decode_context(|ctx| {
            crate::native::display_jt::display_jt_tessellations(ctx, &inputs, &mut losses)
        })
        .expect("a bounded path scan is not an error");
        (tessellations.len(), losses)
    }

    let (_, losses) = tessellate(2);
    assert!(losses.is_empty());

    let (tessellations, losses) = tessellate(16);
    assert_eq!(tessellations, 0);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::NxLossCode::SourceScanBounded.kind()
    );
}
