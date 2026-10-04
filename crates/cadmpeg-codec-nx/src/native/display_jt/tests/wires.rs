// SPDX-License-Identifier: Apache-2.0
//! Byte identity and native admission for JT scene and mesh wires.

use super::super::{
    DisplayJtMaterialAttribute, DisplayJtMaterialAttributeWire, DisplayJtPartitionNode,
    DisplayJtPartitionNodeWire, DisplayJtPolygonMesh, DisplayJtPolygonMeshWire,
    DisplayJtTriStripShapeNode, DisplayJtTriStripShapeNodeWire,
};
use serde_json::{json, Value};

fn polygon_mesh_wire() -> Value {
    json!({
        "id": "nx:jt:polygon-mesh#0",
        "topology": "nx:jt:topology#0",
        "coordinate_header": "nx:jt:coordinate-header#0",
        "polygons": [[1, 2, 3], [4, 5]],
        "vertex_attribute_indices": [[null, 7, null], [8, 9]],
        "polygon_groups": [0, -1], "polygon_flags": [1, 2],
        "source_offset": 20
    })
}

fn tri_strip_wire() -> Value {
    json!({
        "id": "nx:jt:tri-strip-shape-node#0",
        "base_node": "nx:jt:base-node#0", "object_id": 4,
        "reserved_bounds": [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]],
        "untransformed_bounds": [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]],
        "area": 2.5, "vertex_count_range": [1, 3],
        "node_count_range": [0, 1], "polygon_count_range": [0, 2],
        "memory_byte_len": 32, "compression_level": 0.5,
        "vertex_version": 2, "vertex_bindings": 3,
        "vertex_quantization_bits": 7, "normal_quantization_factor": 2,
        "texture_quantization_bits": 8, "color_quantization_bits": 9,
        "version_2_vertex_bindings": 5, "source_offset": 24
    })
}

fn material_wire() -> Value {
    json!({
        "id": "nx:jt:material-attribute#0",
        "element": "nx:jt:compressed-element#0", "object_id": 4,
        "state_flags": 1, "field_inhibit_flags": 2,
        "version": 2, "data_flags": 3,
        "ambient": [0.0, 0.25, 0.5, 1.0],
        "diffuse": [0.0, 0.25, 0.5, 1.0],
        "specular": [0.0, 0.25, 0.5, 1.0],
        "emission": [0.0, 0.25, 0.5, 1.0],
        "shininess": 10.0, "reflectivity": 0.5,
        "source_offset": 24
    })
}

fn partition_wire() -> Value {
    json!({
        "id": "nx:jt:partition-node#0",
        "base_node": "nx:jt:base-node#0", "object_id": 5,
        "group_version": 1, "child_object_ids": [1, 2],
        "partition_flags": 1, "file_name_code_units": [78, 88, 937],
        "file_name": "NXΩ",
        "transformed_bounds": [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]],
        "area": 2.5, "vertex_count_range": [1, 3],
        "node_count_range": [0, 1], "polygon_count_range": [0, 2],
        "untransformed_bounds": [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]],
        "reserved_bounds": null, "source_offset": 24
    })
}

#[test]
fn polygon_mesh_borrowed_wire_matches_owned_bytes() {
    let record: DisplayJtPolygonMesh = serde_json::from_value(polygon_mesh_wire()).unwrap();
    let owned = DisplayJtPolygonMeshWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn polygon_mesh_native_limit_refuses_before_wire_columns() {
    let wire = polygon_mesh_wire();
    let record: DisplayJtPolygonMesh = serde_json::from_value(wire.clone()).unwrap();
    cadmpeg_test_support::native_serialization::assert_native_limit(&record, wire);
}

#[test]
fn tri_strip_borrowed_wire_matches_owned_bytes() {
    let record: DisplayJtTriStripShapeNode = serde_json::from_value(tri_strip_wire()).unwrap();
    let owned = DisplayJtTriStripShapeNodeWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn tri_strip_native_limit_refuses_before_wire_strings() {
    let wire = tri_strip_wire();
    let record: DisplayJtTriStripShapeNode = serde_json::from_value(wire.clone()).unwrap();
    cadmpeg_test_support::native_serialization::assert_native_limit(&record, wire);
}

#[test]
fn material_borrowed_wire_matches_owned_bytes() {
    let record: DisplayJtMaterialAttribute = serde_json::from_value(material_wire()).unwrap();
    let owned = DisplayJtMaterialAttributeWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn material_native_limit_refuses_before_wire_strings() {
    let wire = material_wire();
    let record: DisplayJtMaterialAttribute = serde_json::from_value(wire.clone()).unwrap();
    cadmpeg_test_support::native_serialization::assert_native_limit(&record, wire);
}

#[test]
fn partition_borrowed_wire_matches_owned_bytes() {
    let record: DisplayJtPartitionNode = serde_json::from_value(partition_wire()).unwrap();
    let owned = DisplayJtPartitionNodeWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn partition_native_limit_refuses_before_utf16_copy() {
    let wire = partition_wire();
    let record: DisplayJtPartitionNode = serde_json::from_value(wire.clone()).unwrap();
    cadmpeg_test_support::native_serialization::assert_native_limit(&record, wire);
}
